use std::collections::HashMap;
use std::io;

use matrix_sdk::Client;

use super::core::{
    PendingBrowserLogin, WorkerState, abort_sync_task, abort_verification_tasks, shutdown_state,
};
use super::{
    EventSender, InitialSnapshot, RoomSnapshot, emit_error, emit_event, emit_invalid_state,
    emit_matrix_error,
};
use crate::api::{
    CoreConfig, Event, LoginMethod, LoginRequest, Operation, RestoreSessionRequest, SyncStopReason,
};
use crate::loopback::{self, LoopbackListener, LoopbackRedirect};
use crate::matrix::{self, BackupServerStatusMode, HomeserverSpec, PausedSendRooms};

pub(super) async fn handle_login(
    config: &CoreConfig,
    event_tx: &EventSender,
    state: &mut WorkerState,
    request: LoginRequest,
) -> bool {
    if !matches!(state, WorkerState::NoClient) {
        emit_invalid_state(
            event_tx,
            Operation::Login,
            "login requires an idle core; logout first",
        );
        return false;
    }

    let client = match matrix::build_client(config, HomeserverSpec::Discover(&request.server)).await
    {
        Ok(client) => client,
        Err(err) => {
            emit_matrix_error(event_tx, Operation::Login, err);
            return false;
        }
    };

    // Before the session exists: signing in starts E2EE.
    matrix::register_event_handlers(&client, event_tx.clone());

    match request.method {
        LoginMethod::Password { username, password } => {
            match matrix::login(&client, &username, &password, &request.device_display_name).await {
                Ok(session) => {
                    let signed_in = Event::LoggedIn {
                        session,
                        homeserver_url: client.homeserver().to_string(),
                    };
                    enter_session(
                        client,
                        signed_in,
                        Operation::Login,
                        event_tx,
                        state,
                        request.start_sync,
                    )
                    .await;
                }
                Err(err) => emit_matrix_error(event_tx, Operation::Login, err),
            }
        }
        LoginMethod::Browser { user_id_hint } => {
            start_browser_login(
                client,
                &request.server,
                user_id_hint.as_deref(),
                &request.device_display_name,
                event_tx,
                state,
                request.start_sync,
            )
            .await;
        }
    }
    false
}

/// Opens the loopback listener, prepares the homeserver's sign-in page
/// and waits for the browser to come back from it.
pub(super) async fn start_browser_login(
    client: Client,
    server: &str,
    user_id_hint: Option<&str>,
    device_display_name: &str,
    event_tx: &EventSender,
    state: &mut WorkerState,
    start_sync: bool,
) {
    let listener = match LoopbackListener::bind().await {
        Ok(listener) => listener,
        Err(err) => {
            emit_error(
                event_tx,
                Operation::Login,
                None,
                format!("failed to listen for the browser on 127.0.0.1: {err}"),
            );
            return;
        }
    };
    let sign_in = match matrix::start_browser_login(
        &client,
        server,
        listener.redirect_uri(),
        user_id_hint,
        device_display_name,
    )
    .await
    {
        Ok(sign_in) => sign_in,
        Err(err) => {
            emit_matrix_error(event_tx, Operation::Login, err);
            return;
        }
    };
    let redirect = tokio::task::spawn_local(listener.next_redirect());
    emit_event(event_tx, Event::SignInUrl { url: sign_in.url });
    *state = WorkerState::AwaitingBrowser(Box::new(PendingBrowserLogin {
        client,
        flow: sign_in.flow,
        redirect,
        start_sync,
    }));
}

/// The browser came back: completes the sign-in, tells the browser how it
/// went, and brings the session online.
pub(super) async fn finish_browser_login(
    event_tx: &EventSender,
    state: &mut WorkerState,
    redirect: io::Result<LoopbackRedirect>,
) {
    let WorkerState::AwaitingBrowser(pending) = std::mem::replace(state, WorkerState::NoClient)
    else {
        unreachable!("only a browser sign-in waits for a redirect");
    };
    let PendingBrowserLogin {
        client,
        flow,
        redirect: _,
        start_sync,
    } = *pending;
    let LoopbackRedirect { mut stream, query } = match redirect {
        Ok(redirect) => redirect,
        Err(err) => {
            emit_error(
                event_tx,
                Operation::Login,
                None,
                format!("failed to receive the browser's sign-in: {err}"),
            );
            return;
        }
    };
    let signed_in = matrix::finish_browser_login(&client, flow, &query).await;
    let page = match &signed_in {
        Ok(session) => format!(
            "Signed in to mirx as {}. You can close this page.",
            session.user_id
        ),
        Err(err) => format!("mirx could not sign in: {err}. Return to mirx and try again."),
    };
    // The page is a courtesy: a browser that went away undoes nothing.
    let _ = loopback::respond(&mut stream, "200 OK", &loopback::result_page(&page)).await;
    drop(stream);
    match signed_in {
        Ok(session) => {
            let signed_in = Event::LoggedIn {
                session,
                homeserver_url: client.homeserver().to_string(),
            };
            enter_session(
                client,
                signed_in,
                Operation::Login,
                event_tx,
                state,
                start_sync,
            )
            .await;
        }
        Err(err) => emit_matrix_error(event_tx, Operation::Login, err),
    }
}

pub(super) async fn handle_cancel_login(event_tx: &EventSender, state: &mut WorkerState) -> bool {
    if !matches!(state, WorkerState::AwaitingBrowser(_)) {
        emit_invalid_state(event_tx, Operation::Login, "no sign-in to cancel");
        return false;
    }
    // Closes the listener's port and drops the half-signed-in client.
    shutdown_state(state).await;
    emit_event(event_tx, Event::LoginCancelled);
    false
}

pub(super) async fn handle_restore_session(
    config: &CoreConfig,
    event_tx: &EventSender,
    state: &mut WorkerState,
    request: RestoreSessionRequest,
) -> bool {
    if !matches!(state, WorkerState::NoClient) {
        emit_invalid_state(
            event_tx,
            Operation::RestoreSession,
            "session restore requires an idle core; logout first",
        );
        return false;
    }

    let client =
        match matrix::build_client(config, HomeserverSpec::Url(&request.homeserver_url)).await {
            Ok(client) => client,
            Err(err) => {
                emit_matrix_error(event_tx, Operation::RestoreSession, err);
                return false;
            }
        };

    matrix::register_event_handlers(&client, event_tx.clone());

    if let Err(err) = matrix::restore_session(&client, request.session).await {
        emit_matrix_error(event_tx, Operation::RestoreSession, err);
        return false;
    }

    enter_session(
        client,
        Event::RestoredSession,
        Operation::RestoreSession,
        event_tx,
        state,
        request.start_sync,
    )
    .await;
    false
}

/// Brings a signed-in client online: the initial sync and the first room
/// snapshot, announced by `signed_in` (`LoggedIn` or `RestoredSession`),
/// the update tasks and the E2EE status. Initial sync failures are errors
/// of `operation`; the session stands either way, and the first sync task
/// response emits the snapshot instead.
pub(super) async fn enter_session(
    client: Client,
    signed_in: Event,
    operation: Operation,
    event_tx: &EventSender,
    state: &mut WorkerState,
    start_sync: bool,
) {
    let current_user_id = client
        .user_id()
        .expect("a signed-in client knows its user")
        .to_string();
    client
        .send_queue()
        .respawn_tasks_for_rooms_with_unsent_requests()
        .await;
    let session_update_task = matrix::start_session_update_task(client.clone(), event_tx.clone());
    let paused_sends = PausedSendRooms::default();
    let send_queue_update_task = matrix::start_send_queue_update_task(
        client.clone(),
        current_user_id,
        event_tx.clone(),
        paused_sends.clone(),
    );
    // Before the first sync: the event cache counts unread messages, here
    // those of the initial sync, and does so in its own task, so the
    // read-state task must already be listening when it is done.
    client
        .event_cache()
        .subscribe()
        .expect("the session's client is alive");
    let read_state_updates = client.room_info_notable_update_receiver();
    let snapshot = match matrix::initial_room_sync(&client).await {
        Ok(()) => RoomSnapshot::read(&client).await,
        Err(err) => Err(err),
    };
    emit_event(event_tx, signed_in);
    // Offline, or the server is down: the first sync that gets through
    // emits the snapshot.
    let initial_snapshot = InitialSnapshot::pending(match snapshot {
        Ok(snapshot) => {
            snapshot.emit(event_tx);
            false
        }
        Err(err) => {
            emit_matrix_error(event_tx, operation, err);
            true
        }
    });
    let room_read_state_update_task = matrix::start_room_read_state_update_task(
        client.clone(),
        event_tx.clone(),
        read_state_updates,
    );
    let room_list_update_task =
        matrix::start_room_list_update_task(client.clone(), event_tx.clone());
    // Subscribed before the first summary, so no change falls between.
    let e2ee_session_state_task =
        matrix::start_e2ee_session_state_task(client.clone(), event_tx.clone());
    match matrix::summarize_e2ee_status(&client, BackupServerStatusMode::Fetch).await {
        Ok(status) => emit_event(event_tx, Event::E2eeStatus(status)),
        Err(err) => emit_matrix_error(event_tx, Operation::E2eeStatus, err),
    }
    let e2ee_recovery_state_task =
        matrix::start_e2ee_recovery_state_task(client.clone(), event_tx.clone());
    let e2ee_backup_state_task =
        matrix::start_e2ee_backup_state_task(client.clone(), event_tx.clone());
    let room_identity_task = matrix::start_room_identity_task(client.clone(), event_tx.clone());

    *state = WorkerState::Ready {
        client,
        initial_snapshot,
        paused_sends,
        sync_task: None,
        session_update_task,
        send_queue_update_task,
        room_list_update_task,
        room_read_state_update_task,
        e2ee_recovery_state_task,
        e2ee_backup_state_task,
        e2ee_session_state_task,
        room_identity_task,
        verification_tasks: HashMap::new(),
        identity_reset: None,
    };

    if start_sync {
        handle_start_sync(event_tx, state);
    }
}

pub(super) fn handle_start_sync(event_tx: &EventSender, state: &mut WorkerState) -> bool {
    let WorkerState::Ready {
        client,
        initial_snapshot,
        paused_sends,
        sync_task,
        ..
    } = state
    else {
        emit_invalid_state(
            event_tx,
            Operation::StartSync,
            "start sync requires an authenticated session",
        );
        return false;
    };

    if sync_task
        .as_ref()
        .is_some_and(|existing_task| !existing_task.is_finished())
    {
        emit_invalid_state(
            event_tx,
            Operation::StartSync,
            "continuous sync is already running",
        );
        return false;
    }

    *sync_task = Some(matrix::start_sync_task(
        client.clone(),
        event_tx.clone(),
        initial_snapshot.clone(),
        paused_sends.clone(),
    ));
    emit_event(event_tx, Event::SyncStarted);
    false
}

/// Stops continuous sync and keeps the session: mIRC's Disconnect, not a
/// logout.
pub(super) fn handle_stop_sync(event_tx: &EventSender, state: &mut WorkerState) -> bool {
    let WorkerState::Ready { sync_task, .. } = state else {
        emit_invalid_state(
            event_tx,
            Operation::StopSync,
            "stop sync requires an authenticated session",
        );
        return false;
    };

    if !sync_task.as_ref().is_some_and(|task| !task.is_finished()) {
        emit_invalid_state(event_tx, Operation::StopSync, "sync is not running");
        return false;
    }

    abort_sync_task(sync_task);
    emit_event(event_tx, Event::SyncStopped(SyncStopReason::Requested));
    false
}

pub(super) async fn handle_logout(event_tx: &EventSender, state: &mut WorkerState) -> bool {
    let WorkerState::Ready {
        client,
        initial_snapshot: _,
        paused_sends: _,
        sync_task,
        session_update_task,
        send_queue_update_task,
        room_list_update_task,
        room_read_state_update_task,
        e2ee_recovery_state_task,
        e2ee_backup_state_task,
        e2ee_session_state_task,
        room_identity_task,
        verification_tasks,
        identity_reset,
    } = state
    else {
        emit_invalid_state(
            event_tx,
            Operation::Logout,
            "logout requires an authenticated session",
        );
        return false;
    };

    abort_sync_task(sync_task);
    session_update_task.abort();
    send_queue_update_task.abort();
    room_list_update_task.abort();
    room_read_state_update_task.abort();
    e2ee_recovery_state_task.abort();
    e2ee_backup_state_task.abort();
    e2ee_session_state_task.abort();
    room_identity_task.abort();
    abort_verification_tasks(verification_tasks);
    if let Some(pending) = identity_reset.take() {
        pending.task.abort();
    }

    if let Err(err) = matrix::logout(client.clone()).await {
        emit_matrix_error(event_tx, Operation::Logout, err);
        return false;
    }

    *state = WorkerState::NoClient;
    emit_event(event_tx, Event::LoggedOut);
    false
}
