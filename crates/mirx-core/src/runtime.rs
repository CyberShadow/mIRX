use std::cell::Cell;
use std::collections::HashMap;
use std::error::Error;
use std::fmt::{self, Display, Formatter};
use std::io;
use std::rc::Rc;
use std::sync::mpsc::{self, Receiver};
use std::thread::{self, JoinHandle};

use matrix_sdk::Client;
use matrix_sdk::encryption::verification::VerificationRequest as MatrixVerificationRequest;
use tokio::runtime::Builder;
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel};
use tokio::task::{JoinHandle as TokioJoinHandle, LocalSet};

use crate::api::{
    AcceptRoomInviteRequest, BackfillRoomHistoryRequest, BanRoomMemberRequest, Command, CoreConfig,
    CoreErrorEvent, CreateDirectRoomRequest, CreateRoomRequest, E2eeRecoveryOperation,
    E2eeRecoverySetupSucceeded, E2eeRecoverySucceeded, EditMessageRequest, Event, HighlightWord,
    InviteRoomMemberRequest, JoinRoomRequest, KickRoomMemberRequest, LeaveRoomRequest, LoginMethod,
    LoginRequest, MarkRoomReadRequest, Operation, RecoverE2eeRequest, RedactEventRequest,
    RejectRoomInviteRequest, RequestVerificationRequest, RestoreSessionRequest, RoomJoined,
    RoomMembers, RoomMembership, RoomReadState, RoomSummary, SendReactionRequest,
    SetDisplayNameRequest, SetRoomMemberPowerLevelRequest, SetRoomNotificationModeRequest,
    SetRoomTopicRequest, SetupE2eeRecoveryRequest, SyncStopReason, UnbanRoomMemberRequest,
    VerificationFlowId,
};
use crate::loopback::{self, LoopbackListener, LoopbackRedirect};
use crate::matrix::{
    self, BackupServerStatusMode, BrowserFlow, HomeserverSpec, MatrixCoreError, PausedSendRooms,
};

pub(crate) type EventSender = mpsc::Sender<Event>;

#[derive(Debug, Clone)]
pub struct CommandSender {
    sender: UnboundedSender<Command>,
}

impl CommandSender {
    pub fn new(sender: UnboundedSender<Command>) -> Self {
        Self { sender }
    }

    pub fn send(&self, command: Command) -> Result<(), SendCommandError> {
        self.sender.send(command).map_err(|_| SendCommandError)
    }
}

#[derive(Debug)]
pub struct CoreHandle {
    command_sender: CommandSender,
    events: Receiver<Event>,
    worker: JoinHandle<()>,
}

impl CoreHandle {
    pub fn command_sender(&self) -> &CommandSender {
        &self.command_sender
    }

    pub fn events(&self) -> &Receiver<Event> {
        &self.events
    }

    pub fn join(self) -> thread::Result<()> {
        let Self {
            command_sender,
            events,
            worker,
        } = self;
        drop(command_sender);
        drop(events);
        worker.join()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SendCommandError;

impl Display for SendCommandError {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.write_str("mirx-core command channel is closed")
    }
}

impl Error for SendCommandError {}

#[derive(Debug)]
pub enum StartError {
    ThreadSpawn(std::io::Error),
    RuntimeInitialization(String),
    WorkerStopped,
}

impl Display for StartError {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::ThreadSpawn(err) => write!(f, "failed to spawn mirx-core thread: {err}"),
            Self::RuntimeInitialization(message) => {
                write!(f, "failed to initialize mirx-core runtime: {message}")
            }
            Self::WorkerStopped => f.write_str("mirx-core worker stopped during startup"),
        }
    }
}

impl Error for StartError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::ThreadSpawn(err) => Some(err),
            Self::RuntimeInitialization(_) | Self::WorkerStopped => None,
        }
    }
}

enum WorkerState {
    NoClient,
    /// A browser sign-in waits for the browser to come back to the
    /// loopback listener.
    AwaitingBrowser(Box<PendingBrowserLogin>),
    Ready {
        client: Client,
        /// Given to every sync task, which emits it if it is pending.
        initial_snapshot: InitialSnapshot,
        /// Shared by the send queue's update task and every sync task.
        paused_sends: PausedSendRooms,
        sync_task: Option<TokioJoinHandle<()>>,
        session_update_task: TokioJoinHandle<()>,
        send_queue_update_task: TokioJoinHandle<()>,
        room_list_update_task: TokioJoinHandle<()>,
        room_read_state_update_task: TokioJoinHandle<()>,
        e2ee_recovery_state_task: TokioJoinHandle<()>,
        e2ee_backup_state_task: TokioJoinHandle<()>,
        verification_tasks: HashMap<(String, String), TokioJoinHandle<()>>,
    },
}

struct PendingBrowserLogin {
    client: Client,
    flow: BrowserFlow,
    /// The listener, answering until the browser's request for the
    /// redirect URI; it closes its port when it ends.
    redirect: TokioJoinHandle<io::Result<LoopbackRedirect>>,
    start_sync: bool,
}

/// What the worker does next.
#[expect(
    clippy::large_enum_variant,
    reason = "a value of one loop iteration, never stored"
)]
enum Next {
    Command(Option<Command>),
    Redirect(io::Result<LoopbackRedirect>),
}

pub fn start(config: CoreConfig) -> Result<CoreHandle, StartError> {
    let (command_tx, command_rx) = unbounded_channel();
    let (event_tx, event_rx) = mpsc::channel();
    let (startup_tx, startup_rx) = mpsc::channel();

    let worker = thread::Builder::new()
        .name("mirx-core".to_owned())
        .spawn(move || {
            let runtime = match Builder::new_current_thread().enable_all().build() {
                Ok(runtime) => runtime,
                Err(err) => {
                    let _ = startup_tx.send(Err(err.to_string()));
                    return;
                }
            };

            if startup_tx.send(Ok(())).is_err() {
                return;
            }

            let local = LocalSet::new();
            local.block_on(&runtime, run_worker(config, command_rx, event_tx));
        })
        .map_err(StartError::ThreadSpawn)?;

    match startup_rx.recv() {
        Ok(Ok(())) => Ok(CoreHandle {
            command_sender: CommandSender::new(command_tx),
            events: event_rx,
            worker,
        }),
        Ok(Err(message)) => {
            let _ = worker.join();
            Err(StartError::RuntimeInitialization(message))
        }
        Err(_) => {
            let _ = worker.join();
            Err(StartError::WorkerStopped)
        }
    }
}

async fn run_worker(
    config: CoreConfig,
    mut command_rx: UnboundedReceiver<Command>,
    event_tx: EventSender,
) {
    emit_event(&event_tx, Event::Started);

    let mut state = WorkerState::NoClient;

    loop {
        // A browser sign-in waits for its redirect and for commands at
        // once; both are cancel-safe. The listener task reads the
        // requests, so a command never cuts one short.
        let next = match &mut state {
            WorkerState::AwaitingBrowser(pending) => tokio::select! {
                command = command_rx.recv() => Next::Command(command),
                redirect = &mut pending.redirect => Next::Redirect(
                    redirect.unwrap_or_else(|err| std::panic::resume_unwind(err.into_panic())),
                ),
            },
            WorkerState::NoClient | WorkerState::Ready { .. } => {
                Next::Command(command_rx.recv().await)
            }
        };
        match next {
            Next::Command(None) => break,
            Next::Command(Some(command)) => {
                if handle_command(command, &config, &event_tx, &mut state).await {
                    break;
                }
            }
            Next::Redirect(redirect) => finish_browser_login(&event_tx, &mut state, redirect).await,
        }
    }

    shutdown_state(&mut state).await;
    emit_event(&event_tx, Event::Stopped);
}

async fn handle_command(
    command: Command,
    config: &CoreConfig,
    event_tx: &EventSender,
    state: &mut WorkerState,
) -> bool {
    match command {
        Command::Login(request) => handle_login(config, event_tx, state, request).await,
        Command::CancelLogin => handle_cancel_login(event_tx, state).await,
        Command::RestoreSession(request) => {
            handle_restore_session(config, event_tx, state, request).await
        }
        Command::StartSync => handle_start_sync(event_tx, state),
        Command::StopSync => handle_stop_sync(event_tx, state),
        Command::JoinRoom(request) => handle_join_room(event_tx, state, request).await,
        Command::AcceptRoomInvite(request) => {
            handle_accept_room_invite(event_tx, state, request).await
        }
        Command::CreateDirectRoom(request) => {
            handle_create_direct_room(event_tx, state, request).await
        }
        Command::CreateRoom(request) => handle_create_room(event_tx, state, request).await,
        Command::LeaveRoom(request) => handle_leave_room(event_tx, state, request).await,
        Command::KickRoomMember(request) => handle_kick_room_member(event_tx, state, request).await,
        Command::BanRoomMember(request) => handle_ban_room_member(event_tx, state, request).await,
        Command::UnbanRoomMember(request) => {
            handle_unban_room_member(event_tx, state, request).await
        }
        Command::InviteRoomMember(request) => {
            handle_invite_room_member(event_tx, state, request).await
        }
        Command::SetRoomMemberPowerLevel(request) => {
            handle_set_room_member_power_level(event_tx, state, request).await
        }
        Command::SetDisplayName(request) => handle_set_display_name(event_tx, state, request).await,
        Command::RejectRoomInvite(request) => {
            handle_reject_room_invite(event_tx, state, request).await
        }
        Command::SetRoomTopic(request) => handle_set_room_topic(event_tx, state, request).await,
        Command::SetRoomNotificationMode(request) => {
            handle_set_room_notification_mode(event_tx, state, request).await
        }
        Command::AddHighlightWord(word) => handle_add_highlight_word(event_tx, state, word).await,
        Command::RemoveHighlightWord(word) => {
            handle_remove_highlight_word(event_tx, state, word).await
        }
        Command::IgnoreUser(user_id) => handle_ignore_user(event_tx, state, user_id).await,
        Command::UnignoreUser(user_id) => handle_unignore_user(event_tx, state, user_id).await,
        Command::FetchUserProfile(user_id) => {
            handle_fetch_user_profile(event_tx, state, user_id).await
        }
        Command::ListUserDevices(user_id) => {
            handle_list_user_devices(event_tx, state, user_id).await
        }
        Command::SendMessage(request) => handle_send_message(event_tx, state, request).await,
        Command::SendFile(request) => handle_send_file(event_tx, state, request).await,
        Command::EditMessage(request) => handle_edit_message(event_tx, state, request).await,
        Command::SendReaction(request) => handle_send_reaction(event_tx, state, request).await,
        Command::RedactEvent(request) => handle_redact_event(event_tx, state, request).await,
        Command::MarkRoomRead(request) => handle_mark_room_read(event_tx, state, request).await,
        Command::BackfillRoomHistory(request) => {
            handle_backfill_room_history(event_tx, state, request).await
        }
        Command::FetchRoomMedia(request) => handle_fetch_room_media(event_tx, state, request).await,
        Command::FetchRoomEvent(request) => handle_fetch_room_event(event_tx, state, request).await,
        Command::ListPublicRooms(request) => {
            handle_list_public_rooms(event_tx, state, request).await
        }
        Command::RefreshE2eeStatus => handle_refresh_e2ee_status(event_tx, state).await,
        Command::SetupE2eeRecovery(request) => {
            handle_setup_e2ee_recovery(event_tx, state, &request).await
        }
        Command::RecoverE2ee(request) => handle_recover_e2ee(event_tx, state, &request).await,
        Command::RequestVerification(request) => {
            handle_request_verification(event_tx, state, request).await
        }
        Command::AcceptVerification(flow) => {
            handle_accept_verification(event_tx, state, flow).await
        }
        Command::CancelVerification(flow) => {
            handle_cancel_verification(event_tx, state, flow).await
        }
        Command::ConfirmVerificationSas(flow) => {
            handle_confirm_verification_sas(event_tx, state, flow).await
        }
        Command::RejectVerificationSas(flow) => {
            handle_reject_verification_sas(event_tx, state, flow).await
        }
        Command::Logout => handle_logout(event_tx, state).await,
        Command::Shutdown => true,
    }
}

async fn handle_login(
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
async fn start_browser_login(
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
async fn finish_browser_login(
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

async fn handle_cancel_login(event_tx: &EventSender, state: &mut WorkerState) -> bool {
    if !matches!(state, WorkerState::AwaitingBrowser(_)) {
        emit_invalid_state(event_tx, Operation::Login, "no sign-in to cancel");
        return false;
    }
    // Closes the listener's port and drops the half-signed-in client.
    shutdown_state(state).await;
    emit_event(event_tx, Event::LoginCancelled);
    false
}

async fn handle_restore_session(
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
async fn enter_session(
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
    match matrix::summarize_e2ee_status(&client, BackupServerStatusMode::Fetch).await {
        Ok(status) => emit_event(event_tx, Event::E2eeStatus(status)),
        Err(err) => emit_matrix_error(event_tx, Operation::E2eeStatus, err),
    }
    let e2ee_recovery_state_task =
        matrix::start_e2ee_recovery_state_task(client.clone(), event_tx.clone());
    let e2ee_backup_state_task =
        matrix::start_e2ee_backup_state_task(client.clone(), event_tx.clone());

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
        verification_tasks: HashMap::new(),
    };

    if start_sync {
        handle_start_sync(event_tx, state);
    }
}

fn handle_start_sync(event_tx: &EventSender, state: &mut WorkerState) -> bool {
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
fn handle_stop_sync(event_tx: &EventSender, state: &mut WorkerState) -> bool {
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

async fn handle_join_room(
    event_tx: &EventSender,
    state: &mut WorkerState,
    request: JoinRoomRequest,
) -> bool {
    let WorkerState::Ready { client, .. } = state else {
        emit_invalid_state(
            event_tx,
            Operation::JoinRoom,
            "join room requires an authenticated session",
        );
        return false;
    };

    match matrix::join_room(client, &request).await {
        Ok(room_id) => {
            emit_snapshot_then_room_joined(client, event_tx, Operation::JoinRoom, room_id.into())
                .await
        }
        Err(err) => emit_matrix_error(event_tx, Operation::JoinRoom, err),
    }

    false
}

async fn handle_accept_room_invite(
    event_tx: &EventSender,
    state: &mut WorkerState,
    request: AcceptRoomInviteRequest,
) -> bool {
    let WorkerState::Ready { client, .. } = state else {
        emit_room_invalid_state(
            event_tx,
            Operation::AcceptRoomInvite,
            &request.room_id,
            "accept room invitation requires an authenticated session",
        );
        return false;
    };

    match matrix::accept_room_invite(client, &request.room_id).await {
        Ok(()) => {
            emit_snapshot_then_room_joined(
                client,
                event_tx,
                Operation::AcceptRoomInvite,
                request.room_id,
            )
            .await
        }
        Err(err) => {
            emit_room_matrix_error(event_tx, Operation::AcceptRoomInvite, &request.room_id, err)
        }
    }

    false
}

async fn handle_create_direct_room(
    event_tx: &EventSender,
    state: &mut WorkerState,
    request: CreateDirectRoomRequest,
) -> bool {
    let WorkerState::Ready { client, .. } = state else {
        emit_invalid_state(
            event_tx,
            Operation::CreateDirectRoom,
            "create direct room requires an authenticated session",
        );
        return false;
    };

    match matrix::create_direct_room(client, &request.user_id).await {
        Ok(room_id) => {
            emit_snapshot_then_room_joined(
                client,
                event_tx,
                Operation::CreateDirectRoom,
                room_id.into(),
            )
            .await
        }
        Err(err) => emit_matrix_error(event_tx, Operation::CreateDirectRoom, err),
    }

    false
}

async fn handle_create_room(
    event_tx: &EventSender,
    state: &mut WorkerState,
    request: CreateRoomRequest,
) -> bool {
    let WorkerState::Ready { client, .. } = state else {
        emit_invalid_state(
            event_tx,
            Operation::CreateRoom,
            "create room requires an authenticated session",
        );
        return false;
    };

    match matrix::create_room(client, &request).await {
        Ok(room_id) => {
            emit_snapshot_then_room_joined(client, event_tx, Operation::CreateRoom, room_id.into())
                .await
        }
        Err(err) => emit_matrix_error(event_tx, Operation::CreateRoom, err),
    }

    false
}

/// Emits the room snapshot that lists a room just joined, then
/// `RoomJoined`, so the UI knows the room by the time it opens its window.
/// A failed snapshot is the operation's error about the room, and no
/// `RoomJoined` follows.
async fn emit_snapshot_then_room_joined(
    client: &Client,
    event_tx: &EventSender,
    operation: Operation,
    room_id: String,
) {
    match matrix::emit_room_snapshot(client, event_tx).await {
        Ok(()) => emit_event(event_tx, Event::RoomJoined(RoomJoined { room_id })),
        Err(err) => emit_room_matrix_error(event_tx, operation, &room_id, err),
    }
}

async fn handle_leave_room(
    event_tx: &EventSender,
    state: &mut WorkerState,
    request: LeaveRoomRequest,
) -> bool {
    let WorkerState::Ready { client, .. } = state else {
        emit_room_invalid_state(
            event_tx,
            Operation::LeaveRoom,
            &request.room_id,
            "leave room requires an authenticated session",
        );
        return false;
    };

    if let Err(err) = matrix::leave_joined_room(client, &request.room_id).await {
        emit_room_matrix_error(event_tx, Operation::LeaveRoom, &request.room_id, err);
        return false;
    }

    if let Err(err) = matrix::emit_room_snapshot(client, event_tx).await {
        emit_room_matrix_error(event_tx, Operation::LeaveRoom, &request.room_id, err);
    }

    false
}

async fn handle_kick_room_member(
    event_tx: &EventSender,
    state: &mut WorkerState,
    request: KickRoomMemberRequest,
) -> bool {
    let WorkerState::Ready { client, .. } = state else {
        emit_room_invalid_state(
            event_tx,
            Operation::KickRoomMember,
            &request.room_id,
            "kick room member requires an authenticated session",
        );
        return false;
    };

    if let Err(err) = matrix::kick_room_member(client, &request).await {
        emit_room_matrix_error(event_tx, Operation::KickRoomMember, &request.room_id, err);
        return false;
    }

    if let Err(err) =
        matrix::emit_joined_room_members_snapshot(client, event_tx, &request.room_id).await
    {
        emit_room_matrix_error(event_tx, Operation::KickRoomMember, &request.room_id, err);
    }

    false
}

async fn handle_ban_room_member(
    event_tx: &EventSender,
    state: &mut WorkerState,
    request: BanRoomMemberRequest,
) -> bool {
    let WorkerState::Ready { client, .. } = state else {
        emit_room_invalid_state(
            event_tx,
            Operation::BanRoomMember,
            &request.room_id,
            "ban room member requires an authenticated session",
        );
        return false;
    };

    if let Err(err) = matrix::ban_room_member(client, &request).await {
        emit_room_matrix_error(event_tx, Operation::BanRoomMember, &request.room_id, err);
        return false;
    }

    if let Err(err) =
        matrix::emit_joined_room_members_snapshot(client, event_tx, &request.room_id).await
    {
        emit_room_matrix_error(event_tx, Operation::BanRoomMember, &request.room_id, err);
    }

    false
}

async fn handle_unban_room_member(
    event_tx: &EventSender,
    state: &mut WorkerState,
    request: UnbanRoomMemberRequest,
) -> bool {
    let WorkerState::Ready { client, .. } = state else {
        emit_room_invalid_state(
            event_tx,
            Operation::UnbanRoomMember,
            &request.room_id,
            "unban room member requires an authenticated session",
        );
        return false;
    };

    if let Err(err) = matrix::unban_room_member(client, &request).await {
        emit_room_matrix_error(event_tx, Operation::UnbanRoomMember, &request.room_id, err);
        return false;
    }

    if let Err(err) =
        matrix::emit_joined_room_members_snapshot(client, event_tx, &request.room_id).await
    {
        emit_room_matrix_error(event_tx, Operation::UnbanRoomMember, &request.room_id, err);
    }

    false
}

async fn handle_invite_room_member(
    event_tx: &EventSender,
    state: &mut WorkerState,
    request: InviteRoomMemberRequest,
) -> bool {
    let WorkerState::Ready { client, .. } = state else {
        emit_room_invalid_state(
            event_tx,
            Operation::InviteRoomMember,
            &request.room_id,
            "invite room member requires an authenticated session",
        );
        return false;
    };

    if let Err(err) = matrix::invite_room_member(client, &request).await {
        emit_room_matrix_error(event_tx, Operation::InviteRoomMember, &request.room_id, err);
        return false;
    }

    if let Err(err) =
        matrix::emit_joined_room_members_snapshot(client, event_tx, &request.room_id).await
    {
        emit_room_matrix_error(event_tx, Operation::InviteRoomMember, &request.room_id, err);
    }

    false
}

async fn handle_set_room_member_power_level(
    event_tx: &EventSender,
    state: &mut WorkerState,
    request: SetRoomMemberPowerLevelRequest,
) -> bool {
    let WorkerState::Ready { client, .. } = state else {
        emit_room_invalid_state(
            event_tx,
            Operation::SetRoomMemberPowerLevel,
            &request.room_id,
            "set room member power level requires an authenticated session",
        );
        return false;
    };

    if let Err(err) = matrix::set_room_member_power_level(client, &request).await {
        emit_room_matrix_error(
            event_tx,
            Operation::SetRoomMemberPowerLevel,
            &request.room_id,
            err,
        );
        return false;
    }

    if let Err(err) =
        matrix::emit_joined_room_members_snapshot(client, event_tx, &request.room_id).await
    {
        emit_room_matrix_error(
            event_tx,
            Operation::SetRoomMemberPowerLevel,
            &request.room_id,
            err,
        );
    }

    false
}

async fn handle_set_display_name(
    event_tx: &EventSender,
    state: &mut WorkerState,
    request: SetDisplayNameRequest,
) -> bool {
    let WorkerState::Ready { client, .. } = state else {
        emit_invalid_state(
            event_tx,
            Operation::SetDisplayName,
            "set display name requires an authenticated session",
        );
        return false;
    };

    if let Err(err) = matrix::set_display_name(client, &request).await {
        emit_matrix_error(event_tx, Operation::SetDisplayName, err);
    }

    false
}

async fn handle_reject_room_invite(
    event_tx: &EventSender,
    state: &mut WorkerState,
    request: RejectRoomInviteRequest,
) -> bool {
    let WorkerState::Ready { client, .. } = state else {
        emit_room_invalid_state(
            event_tx,
            Operation::RejectRoomInvite,
            &request.room_id,
            "reject room invitation requires an authenticated session",
        );
        return false;
    };

    if let Err(err) = matrix::reject_room_invite(client, &request.room_id).await {
        emit_room_matrix_error(event_tx, Operation::RejectRoomInvite, &request.room_id, err);
        return false;
    }

    if let Err(err) = matrix::emit_room_snapshot(client, event_tx).await {
        emit_room_matrix_error(event_tx, Operation::RejectRoomInvite, &request.room_id, err);
    }

    false
}

async fn handle_send_message(
    event_tx: &EventSender,
    state: &mut WorkerState,
    request: crate::api::SendMessageRequest,
) -> bool {
    let WorkerState::Ready { client, .. } = state else {
        emit_room_invalid_state(
            event_tx,
            Operation::SendMessage,
            &request.room_id,
            "send message requires an authenticated session",
        );
        return false;
    };

    let room_id = request.room_id.clone();
    if let Err(err) = matrix::send_message(client, request).await {
        emit_room_matrix_error(event_tx, Operation::SendMessage, &room_id, err);
    }

    false
}

async fn handle_send_file(
    event_tx: &EventSender,
    state: &mut WorkerState,
    request: crate::api::SendFileRequest,
) -> bool {
    let WorkerState::Ready { client, .. } = state else {
        emit_room_invalid_state(
            event_tx,
            Operation::SendFile,
            &request.room_id,
            "send file requires an authenticated session",
        );
        return false;
    };

    let room_id = request.room_id.clone();
    if let Err(err) = matrix::send_file(client, request).await {
        emit_room_matrix_error(event_tx, Operation::SendFile, &room_id, err);
    }

    false
}

async fn handle_send_reaction(
    event_tx: &EventSender,
    state: &mut WorkerState,
    request: SendReactionRequest,
) -> bool {
    let WorkerState::Ready { client, .. } = state else {
        emit_room_invalid_state(
            event_tx,
            Operation::SendReaction,
            &request.room_id,
            "send reaction requires an authenticated session",
        );
        return false;
    };

    let room_id = request.room_id.clone();
    if let Err(err) = matrix::send_reaction(client, request).await {
        emit_room_matrix_error(event_tx, Operation::SendReaction, &room_id, err);
    }

    false
}

async fn handle_edit_message(
    event_tx: &EventSender,
    state: &mut WorkerState,
    request: EditMessageRequest,
) -> bool {
    let WorkerState::Ready { client, .. } = state else {
        emit_room_invalid_state(
            event_tx,
            Operation::EditMessage,
            &request.room_id,
            "edit message requires an authenticated session",
        );
        return false;
    };

    let room_id = request.room_id.clone();
    if let Err(err) = matrix::edit_message(client, request).await {
        emit_room_matrix_error(event_tx, Operation::EditMessage, &room_id, err);
    }

    false
}

async fn handle_redact_event(
    event_tx: &EventSender,
    state: &mut WorkerState,
    request: RedactEventRequest,
) -> bool {
    let WorkerState::Ready { client, .. } = state else {
        emit_room_invalid_state(
            event_tx,
            Operation::RedactEvent,
            &request.room_id,
            "redact event requires an authenticated session",
        );
        return false;
    };

    let room_id = request.room_id.clone();
    if let Err(err) = matrix::redact_event(client, request).await {
        emit_room_matrix_error(event_tx, Operation::RedactEvent, &room_id, err);
    }

    false
}

async fn handle_set_room_topic(
    event_tx: &EventSender,
    state: &mut WorkerState,
    request: SetRoomTopicRequest,
) -> bool {
    let WorkerState::Ready { client, .. } = state else {
        emit_room_invalid_state(
            event_tx,
            Operation::SetRoomTopic,
            &request.room_id,
            "set room topic requires an authenticated session",
        );
        return false;
    };

    let room_id = request.room_id.clone();
    if let Err(err) = matrix::set_room_topic(client, request).await {
        emit_room_matrix_error(event_tx, Operation::SetRoomTopic, &room_id, err);
    }

    false
}

async fn handle_set_room_notification_mode(
    event_tx: &EventSender,
    state: &mut WorkerState,
    request: SetRoomNotificationModeRequest,
) -> bool {
    let WorkerState::Ready { client, .. } = state else {
        emit_room_invalid_state(
            event_tx,
            Operation::SetRoomNotificationMode,
            &request.room_id,
            "set room notification mode requires an authenticated session",
        );
        return false;
    };

    let room_id = request.room_id.clone();
    if let Err(err) = matrix::set_room_notification_mode(client, request).await {
        emit_room_matrix_error(event_tx, Operation::SetRoomNotificationMode, &room_id, err);
    }

    false
}

async fn handle_add_highlight_word(
    event_tx: &EventSender,
    state: &mut WorkerState,
    word: String,
) -> bool {
    let WorkerState::Ready { client, .. } = state else {
        emit_invalid_state(
            event_tx,
            Operation::AddHighlightWord,
            "add highlight word requires an authenticated session",
        );
        return false;
    };

    if let Err(err) = matrix::add_highlight_word(client, &word).await {
        emit_matrix_error(event_tx, Operation::AddHighlightWord, err);
    }

    false
}

async fn handle_remove_highlight_word(
    event_tx: &EventSender,
    state: &mut WorkerState,
    word: String,
) -> bool {
    let WorkerState::Ready { client, .. } = state else {
        emit_invalid_state(
            event_tx,
            Operation::RemoveHighlightWord,
            "remove highlight word requires an authenticated session",
        );
        return false;
    };

    if let Err(err) = matrix::remove_highlight_word(client, &word).await {
        emit_matrix_error(event_tx, Operation::RemoveHighlightWord, err);
    }

    false
}

async fn handle_ignore_user(
    event_tx: &EventSender,
    state: &mut WorkerState,
    user_id: String,
) -> bool {
    let WorkerState::Ready { client, .. } = state else {
        emit_invalid_state(
            event_tx,
            Operation::IgnoreUser,
            "ignore user requires an authenticated session",
        );
        return false;
    };

    if let Err(err) = matrix::ignore_user(client, &user_id).await {
        emit_matrix_error(event_tx, Operation::IgnoreUser, err);
    }

    false
}

async fn handle_unignore_user(
    event_tx: &EventSender,
    state: &mut WorkerState,
    user_id: String,
) -> bool {
    let WorkerState::Ready { client, .. } = state else {
        emit_invalid_state(
            event_tx,
            Operation::UnignoreUser,
            "unignore user requires an authenticated session",
        );
        return false;
    };

    if let Err(err) = matrix::unignore_user(client, &user_id).await {
        emit_matrix_error(event_tx, Operation::UnignoreUser, err);
    }

    false
}

async fn handle_fetch_user_profile(
    event_tx: &EventSender,
    state: &mut WorkerState,
    user_id: String,
) -> bool {
    let WorkerState::Ready { client, .. } = state else {
        emit_invalid_state(
            event_tx,
            Operation::FetchUserProfile,
            "fetch user profile requires an authenticated session",
        );
        return false;
    };

    match matrix::fetch_user_profile(client, &user_id).await {
        Ok(profile) => emit_event(event_tx, Event::UserProfile(profile)),
        Err(err) => emit_matrix_error(event_tx, Operation::FetchUserProfile, err),
    }

    false
}

async fn handle_mark_room_read(
    event_tx: &EventSender,
    state: &mut WorkerState,
    request: MarkRoomReadRequest,
) -> bool {
    let WorkerState::Ready { client, .. } = state else {
        emit_room_invalid_state(
            event_tx,
            Operation::MarkRoomRead,
            &request.room_id,
            "mark room read requires an authenticated session",
        );
        return false;
    };

    if let Err(err) = matrix::mark_room_read(client, &request).await {
        emit_room_matrix_error(event_tx, Operation::MarkRoomRead, &request.room_id, err);
    }

    false
}

async fn handle_backfill_room_history(
    event_tx: &EventSender,
    state: &mut WorkerState,
    request: BackfillRoomHistoryRequest,
) -> bool {
    let WorkerState::Ready { client, .. } = state else {
        emit_room_invalid_state(
            event_tx,
            Operation::BackfillRoomHistory,
            &request.room_id,
            "backfill room history requires an authenticated session",
        );
        return false;
    };

    let room_id = request.room_id.clone();
    match matrix::backfill_room_history(client, request).await {
        Ok(backfill) => emit_event(event_tx, Event::RoomHistoryBackfill(backfill)),
        Err(err) => emit_room_matrix_error(event_tx, Operation::BackfillRoomHistory, &room_id, err),
    }

    false
}

async fn handle_fetch_room_media(
    event_tx: &EventSender,
    state: &mut WorkerState,
    request: crate::api::FetchRoomMediaRequest,
) -> bool {
    let WorkerState::Ready { client, .. } = state else {
        emit_room_invalid_state(
            event_tx,
            Operation::FetchRoomMedia,
            &request.room_id,
            "fetch room media requires an authenticated session",
        );
        return false;
    };

    let media = matrix::fetch_room_media(client, request).await;
    emit_event(event_tx, Event::RoomMedia(media));

    false
}

async fn handle_fetch_room_event(
    event_tx: &EventSender,
    state: &mut WorkerState,
    request: crate::api::FetchRoomEventRequest,
) -> bool {
    let WorkerState::Ready { client, .. } = state else {
        emit_room_invalid_state(
            event_tx,
            Operation::FetchRoomEvent,
            &request.room_id,
            "fetch room event requires an authenticated session",
        );
        return false;
    };

    let fetch = matrix::fetch_room_event(client, request).await;
    emit_event(event_tx, Event::RoomEvent(fetch));

    false
}

async fn handle_list_public_rooms(
    event_tx: &EventSender,
    state: &mut WorkerState,
    request: crate::api::ListPublicRoomsRequest,
) -> bool {
    let WorkerState::Ready { client, .. } = state else {
        emit_invalid_state(
            event_tx,
            Operation::ListPublicRooms,
            "list public rooms requires an authenticated session",
        );
        return false;
    };

    let page = matrix::list_public_rooms(client, request).await;
    emit_event(event_tx, Event::PublicRooms(page));

    false
}

async fn handle_refresh_e2ee_status(event_tx: &EventSender, state: &mut WorkerState) -> bool {
    let WorkerState::Ready { client, .. } = state else {
        emit_invalid_state(
            event_tx,
            Operation::E2eeStatus,
            "E2EE status requires an authenticated session",
        );
        return false;
    };

    match matrix::summarize_e2ee_status(client, BackupServerStatusMode::Fetch).await {
        Ok(status) => emit_event(event_tx, Event::E2eeStatus(status)),
        Err(err) => emit_matrix_error(event_tx, Operation::E2eeStatus, err),
    }

    false
}

async fn handle_recover_e2ee(
    event_tx: &EventSender,
    state: &mut WorkerState,
    request: &RecoverE2eeRequest,
) -> bool {
    let WorkerState::Ready { client, .. } = state else {
        emit_invalid_state(
            event_tx,
            Operation::RecoverE2ee,
            "E2EE recovery requires an authenticated session",
        );
        return false;
    };

    match matrix::recover_e2ee(client, request).await {
        Ok(status) => {
            emit_event(
                event_tx,
                Event::E2eeRecoverySucceeded(E2eeRecoverySucceeded {
                    operation: E2eeRecoveryOperation::Recover,
                }),
            );
            emit_event(event_tx, Event::E2eeStatus(status));
        }
        Err(err) => emit_matrix_error(event_tx, Operation::RecoverE2ee, err),
    }

    false
}

async fn handle_setup_e2ee_recovery(
    event_tx: &EventSender,
    state: &mut WorkerState,
    request: &SetupE2eeRecoveryRequest,
) -> bool {
    let WorkerState::Ready { client, .. } = state else {
        emit_invalid_state(
            event_tx,
            Operation::SetupE2eeRecovery,
            "E2EE recovery setup requires an authenticated session",
        );
        return false;
    };

    match matrix::setup_e2ee_recovery(client, request).await {
        Ok((recovery_key, status)) => {
            emit_event(
                event_tx,
                Event::E2eeRecoverySetupSucceeded(E2eeRecoverySetupSucceeded { recovery_key }),
            );
            emit_event(event_tx, Event::E2eeStatus(status));
        }
        Err(err) => emit_matrix_error(event_tx, Operation::SetupE2eeRecovery, err),
    }

    false
}

async fn handle_list_user_devices(
    event_tx: &EventSender,
    state: &mut WorkerState,
    user_id: String,
) -> bool {
    let WorkerState::Ready { client, .. } = state else {
        emit_invalid_state(
            event_tx,
            Operation::ListUserDevices,
            "listing devices requires an authenticated session",
        );
        return false;
    };

    match matrix::list_user_devices(client, &user_id).await {
        Ok(devices) => emit_event(event_tx, Event::UserDevices(devices)),
        Err(err) => emit_matrix_error(event_tx, Operation::ListUserDevices, err),
    }

    false
}

async fn handle_request_verification(
    event_tx: &EventSender,
    state: &mut WorkerState,
    request: RequestVerificationRequest,
) -> bool {
    let WorkerState::Ready {
        client,
        verification_tasks,
        ..
    } = state
    else {
        emit_invalid_state(
            event_tx,
            Operation::RequestVerification,
            "verification requires an authenticated session",
        );
        return false;
    };

    match matrix::request_verification(client, &request).await {
        Ok((announcement, flow_request)) => {
            let flow = VerificationFlowId {
                flow_id: announcement.flow_id.clone(),
                user_id: announcement.user_id.clone(),
            };
            emit_event(event_tx, Event::VerificationRequest(announcement));
            // An outgoing flow has no Accept to start the watcher, which
            // sees the other side's ready and starts SAS.
            watch_verification_request(event_tx, verification_tasks, flow, flow_request);
        }
        Err(err) => emit_matrix_error(event_tx, Operation::RequestVerification, err),
    }

    false
}

async fn handle_accept_verification(
    event_tx: &EventSender,
    state: &mut WorkerState,
    flow: VerificationFlowId,
) -> bool {
    let WorkerState::Ready { client, .. } = state else {
        emit_invalid_state(
            event_tx,
            Operation::Verification,
            "verification requires an authenticated session",
        );
        return false;
    };

    match matrix::accept_verification(client, &flow).await {
        Ok(event) => {
            emit_event(event_tx, Event::VerificationState(event));
            ensure_verification_watcher(event_tx, state, &flow).await;
        }
        Err(err) => emit_matrix_error(event_tx, Operation::Verification, err),
    }

    false
}

async fn handle_cancel_verification(
    event_tx: &EventSender,
    state: &mut WorkerState,
    flow: VerificationFlowId,
) -> bool {
    let WorkerState::Ready { client, .. } = state else {
        emit_invalid_state(
            event_tx,
            Operation::Verification,
            "verification requires an authenticated session",
        );
        return false;
    };

    match matrix::cancel_verification(client, &flow).await {
        Ok(event) => emit_event(event_tx, Event::VerificationState(event)),
        Err(err) => emit_matrix_error(event_tx, Operation::Verification, err),
    }

    false
}

async fn handle_confirm_verification_sas(
    event_tx: &EventSender,
    state: &mut WorkerState,
    flow: VerificationFlowId,
) -> bool {
    let WorkerState::Ready { client, .. } = state else {
        emit_invalid_state(
            event_tx,
            Operation::Verification,
            "verification requires an authenticated session",
        );
        return false;
    };

    match matrix::confirm_verification_sas(client, &flow).await {
        Ok(event) => {
            emit_event(event_tx, Event::VerificationState(event));
            ensure_verification_watcher(event_tx, state, &flow).await;
        }
        Err(err) => emit_matrix_error(event_tx, Operation::Verification, err),
    }

    false
}

async fn handle_reject_verification_sas(
    event_tx: &EventSender,
    state: &mut WorkerState,
    flow: VerificationFlowId,
) -> bool {
    let WorkerState::Ready { client, .. } = state else {
        emit_invalid_state(
            event_tx,
            Operation::Verification,
            "verification requires an authenticated session",
        );
        return false;
    };

    match matrix::reject_verification_sas(client, &flow).await {
        Ok(event) => emit_event(event_tx, Event::VerificationState(event)),
        Err(err) => emit_matrix_error(event_tx, Operation::Verification, err),
    }

    false
}

async fn handle_logout(event_tx: &EventSender, state: &mut WorkerState) -> bool {
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
        verification_tasks,
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
    abort_verification_tasks(verification_tasks);

    if let Err(err) = matrix::logout(client.clone()).await {
        emit_matrix_error(event_tx, Operation::Logout, err);
        return false;
    }

    *state = WorkerState::NoClient;
    emit_event(event_tx, Event::LoggedOut);
    false
}

/// Stops every worker task and waits until each is gone. The tasks own
/// `Client` clones, and the SDK store closes its connections on the runtime
/// when the last one drops. A task still queued when `run_worker` returns
/// would instead be dropped with the `LocalSet`, outside the runtime, where
/// that panics.
async fn shutdown_state(state: &mut WorkerState) {
    let tasks = match std::mem::replace(state, WorkerState::NoClient) {
        WorkerState::NoClient => return,
        // The listener's task does not own the client, which drops here,
        // on the runtime.
        WorkerState::AwaitingBrowser(pending) => {
            let PendingBrowserLogin { redirect, .. } = *pending;
            redirect.abort();
            if let Err(err) = redirect.await
                && err.is_panic()
            {
                std::panic::resume_unwind(err.into_panic());
            }
            return;
        }
        WorkerState::Ready {
            client: _,
            initial_snapshot: _,
            paused_sends: _,
            sync_task,
            session_update_task,
            send_queue_update_task,
            room_list_update_task,
            room_read_state_update_task,
            e2ee_recovery_state_task,
            e2ee_backup_state_task,
            verification_tasks,
        } => sync_task
            .into_iter()
            .chain([
                session_update_task,
                send_queue_update_task,
                room_list_update_task,
                room_read_state_update_task,
                e2ee_recovery_state_task,
                e2ee_backup_state_task,
            ])
            .chain(verification_tasks.into_values())
            .collect::<Vec<_>>(),
    };
    for task in &tasks {
        task.abort();
    }
    for task in tasks {
        if let Err(err) = task.await
            && err.is_panic()
        {
            std::panic::resume_unwind(err.into_panic());
        }
    }
}

fn abort_sync_task(sync_task: &mut Option<TokioJoinHandle<()>>) {
    if let Some(task) = sync_task.take() {
        task.abort();
    }
}

fn abort_verification_tasks(
    verification_tasks: &mut HashMap<(String, String), TokioJoinHandle<()>>,
) {
    for (_, task) in verification_tasks.drain() {
        task.abort();
    }
}

async fn ensure_verification_watcher(
    event_tx: &EventSender,
    state: &mut WorkerState,
    flow: &VerificationFlowId,
) {
    let WorkerState::Ready {
        client,
        verification_tasks,
        ..
    } = state
    else {
        return;
    };

    let key = verification_task_key(flow);
    if task_is_running(verification_tasks, &key) {
        return;
    }

    if let Ok(request) = matrix::lookup_verification_request(client, flow).await {
        watch_verification_request(event_tx, verification_tasks, flow.clone(), request);
        return;
    }
    let Ok(sas) = matrix::lookup_sas_verification(client, flow).await else {
        return;
    };
    let task = tokio::task::spawn_local(matrix::watch_sas_verification(
        sas,
        flow.clone(),
        event_tx.clone(),
    ));
    verification_tasks.insert(key, task);
}

/// Follows a request's flow until it ends: the watcher of its flow.
fn watch_verification_request(
    event_tx: &EventSender,
    verification_tasks: &mut HashMap<(String, String), TokioJoinHandle<()>>,
    flow: VerificationFlowId,
    request: MatrixVerificationRequest,
) {
    let key = verification_task_key(&flow);
    let task = tokio::task::spawn_local(matrix::watch_verification_request(
        request,
        flow,
        event_tx.clone(),
    ));
    verification_tasks.insert(key, task);
}

fn verification_task_key(flow: &VerificationFlowId) -> (String, String) {
    (flow.user_id.clone(), flow.flow_id.clone())
}

fn task_is_running(
    verification_tasks: &mut HashMap<(String, String), TokioJoinHandle<()>>,
    key: &(String, String),
) -> bool {
    if verification_tasks
        .get(key)
        .is_some_and(|existing_task| !existing_task.is_finished())
    {
        return true;
    }

    verification_tasks.remove(key);
    false
}

/// The account-wide lists the first room snapshot carries.
struct AccountLists {
    highlight_words: Result<Vec<HighlightWord>, MatrixCoreError>,
    ignored_users: Result<Vec<String>, MatrixCoreError>,
}

impl AccountLists {
    async fn read(client: &Client) -> Self {
        Self {
            highlight_words: matrix::highlight_words(client).await,
            ignored_users: matrix::ignored_users(client).await,
        }
    }
}

/// A session's first room snapshot: the room list, the account lists, and
/// every joined room's members and read state. Later changes come as they
/// happen.
struct RoomSnapshot {
    rooms: Vec<RoomSummary>,
    room_members: Vec<RoomMembers>,
    room_read_states: Vec<RoomReadState>,
    account_lists: AccountLists,
}

impl RoomSnapshot {
    /// After a successful sync: the store has the rooms.
    async fn read(client: &Client) -> Result<Self, MatrixCoreError> {
        Ok(Self {
            rooms: matrix::summarize_rooms(client).await?,
            room_members: matrix::summarize_joined_room_members(client).await?,
            room_read_states: matrix::summarize_joined_room_read_states(client),
            account_lists: AccountLists::read(client).await,
        })
    }

    fn emit(self, event_tx: &EventSender) {
        let Self {
            rooms,
            room_members,
            room_read_states,
            account_lists,
        } = self;
        let joined_room_ids = rooms
            .iter()
            .filter(|room| room.membership == RoomMembership::Join)
            .map(|room| room.room_id.clone())
            .collect::<std::collections::HashSet<_>>();

        emit_event(event_tx, Event::SyncedRooms(rooms));
        match account_lists.highlight_words {
            Ok(words) => emit_event(event_tx, Event::HighlightWords(words)),
            Err(err) => emit_matrix_error(event_tx, Operation::HighlightWords, err),
        }
        match account_lists.ignored_users {
            Ok(user_ids) => emit_event(event_tx, Event::IgnoredUsers(user_ids)),
            Err(err) => emit_matrix_error(event_tx, Operation::IgnoredUsers, err),
        }
        for members in room_members {
            if joined_room_ids.contains(&members.room_id) {
                emit_event(event_tx, Event::RoomMembers(members));
            }
        }
        for read_state in room_read_states {
            if joined_room_ids.contains(&read_state.room_id) {
                emit_event(event_tx, Event::RoomReadState(read_state));
            }
        }
    }
}

/// Whether the session's first room snapshot is still to come: its initial
/// sync, or reading the snapshot, failed. Shared by the session's sync
/// tasks, one at a time.
#[derive(Clone)]
pub(crate) struct InitialSnapshot(Rc<Cell<bool>>);

impl InitialSnapshot {
    fn pending(pending: bool) -> Self {
        Self(Rc::new(Cell::new(pending)))
    }

    /// At a sync task's first successful response: emits the snapshot if it
    /// is still to come. Failing to read it leaves it to the next task.
    pub(crate) async fn emit_if_pending(&self, client: &Client, event_tx: &EventSender) {
        if !self.0.get() {
            return;
        }
        match RoomSnapshot::read(client).await {
            Ok(snapshot) => {
                snapshot.emit(event_tx);
                self.0.set(false);
            }
            Err(err) => emit_matrix_error(event_tx, Operation::RoomList, err),
        }
    }
}

pub(crate) fn emit_event(event_tx: &EventSender, event: Event) {
    let _ = event_tx.send(event);
}

fn emit_error(
    event_tx: &EventSender,
    operation: Operation,
    room_id: Option<&str>,
    message: String,
) {
    emit_event(
        event_tx,
        Event::Error(CoreErrorEvent {
            operation,
            room_id: room_id.map(str::to_owned),
            message,
        }),
    );
}

/// An account-wide operation's refusal.
fn emit_invalid_state(event_tx: &EventSender, operation: Operation, message: &str) {
    emit_error(event_tx, operation, None, message.to_owned());
}

/// An account-wide operation's failure.
fn emit_matrix_error(event_tx: &EventSender, operation: Operation, err: MatrixCoreError) {
    emit_error(event_tx, operation, None, err.to_string());
}

/// The refusal of a request about a room.
fn emit_room_invalid_state(
    event_tx: &EventSender,
    operation: Operation,
    room_id: &str,
    message: &str,
) {
    emit_error(event_tx, operation, Some(room_id), message.to_owned());
}

/// The failure of a request about a room.
fn emit_room_matrix_error(
    event_tx: &EventSender,
    operation: Operation,
    room_id: &str,
    err: MatrixCoreError,
) {
    emit_error(event_tx, operation, Some(room_id), err.to_string());
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::sync::mpsc::{self, TryRecvError};
    use std::time::Duration;

    use tokio::sync::mpsc::unbounded_channel;

    use super::{
        AccountLists, EventSender, InitialSnapshot, PausedSendRooms, RoomSnapshot, WorkerState,
        handle_command, handle_login, handle_restore_session, handle_start_sync, handle_stop_sync,
        run_worker, shutdown_state,
    };
    use crate::api::CoreConfig;
    use crate::api::{
        AcceptRoomInviteRequest, BanRoomMemberRequest, Command, CreateDirectRoomRequest,
        CreateRoomRequest, CreateRoomVisibility, Event, HighlightWord, JoinRoomRequest,
        KickRoomMemberRequest, LoginMethod, LoginRequest, Operation, RestoreSessionRequest,
        RoomJoined, RoomMemberRank, RoomMemberSummary, RoomMembers, RoomMembership,
        RoomNotificationMode, RoomReadState, RoomSummary, SyncStopReason,
    };
    use crate::api::{
        RoomActivityKind, RoomActivityUpdate, RoomActivityUpdateKind, RoomActivityUpdateTarget,
        RoomMessagePresentationKind, SendMessageRequest,
    };
    use matrix_sdk::{
        ruma::{
            OwnedEventId, OwnedRoomId,
            events::{AnySyncStateEvent, room::member::RoomMemberEvent},
            serde::Raw,
        },
        test_utils::mocks::MatrixMockServer,
    };
    use matrix_sdk_test::{InvitedRoomBuilder, JoinedRoomBuilder};

    fn room_id() -> OwnedRoomId {
        OwnedRoomId::try_from("!room:example.org").expect("valid room id")
    }

    fn joined_members() -> Vec<Raw<RoomMemberEvent>> {
        [
            serde_json::json!({
                "type": "m.room.member", "state_key": "@example:localhost",
                "sender": "@example:localhost", "event_id": "$actor:localhost",
                "origin_server_ts": 1u64,
                "content": { "membership": "join", "displayname": "Actor" }
            }),
            serde_json::json!({
                "type": "m.room.member", "state_key": "@target:example.org",
                "sender": "@target:example.org", "event_id": "$target:example.org",
                "origin_server_ts": 2u64,
                "content": { "membership": "join", "displayname": "Target" }
            }),
        ]
        .into_iter()
        .map(|event| Raw::from_json_string(event.to_string()).expect("raw member event"))
        .collect()
    }

    fn joined_member_state() -> Vec<Raw<AnySyncStateEvent>> {
        [
            serde_json::json!({
                "type": "m.room.create", "state_key": "", "sender": "@example:localhost",
                "event_id": "$create:localhost", "origin_server_ts": 0u64,
                "content": { "creator": "@example:localhost" }
            }),
            serde_json::json!({
                "type": "m.room.member", "state_key": "@example:localhost",
                "sender": "@example:localhost", "event_id": "$actor:localhost",
                "origin_server_ts": 1u64,
                "content": { "membership": "join", "displayname": "Actor" }
            }),
            serde_json::json!({
                "type": "m.room.member", "state_key": "@target:example.org",
                "sender": "@target:example.org", "event_id": "$target:example.org",
                "origin_server_ts": 2u64,
                "content": { "membership": "join", "displayname": "Target" }
            }),
        ]
        .into_iter()
        .map(|event| Raw::from_json_string(event.to_string()).expect("raw state event"))
        .collect()
    }

    fn ready_state(client: matrix_sdk::Client) -> WorkerState {
        WorkerState::Ready {
            client,
            initial_snapshot: InitialSnapshot::pending(false),
            paused_sends: PausedSendRooms::default(),
            sync_task: None,
            room_list_update_task: tokio::task::spawn_local(async {}),
            room_read_state_update_task: tokio::task::spawn_local(async {}),
            e2ee_recovery_state_task: tokio::task::spawn_local(async {}),
            e2ee_backup_state_task: tokio::task::spawn_local(async {}),
            session_update_task: tokio::task::spawn_local(async {}),
            send_queue_update_task: tokio::task::spawn_local(async {}),
            verification_tasks: HashMap::new(),
        }
    }

    async fn assert_ready_moderation_success(command: Command, path: &str, mock_ban: bool) {
        let mock_server = MatrixMockServer::new().await;
        let client = mock_server.client_builder().build().await;
        mock_server
            .sync_room(
                &client,
                JoinedRoomBuilder::new(room_id().as_ref()).add_state_bulk(joined_member_state()),
            )
            .await;
        mock_server
            .mock_get_members()
            .ok(joined_members())
            .mock_once()
            .mount()
            .await;
        if mock_ban {
            mock_server.mock_ban_user().ok().expect(1).mount().await;
        } else {
            mock_server.mock_kick_user().ok().expect(1).mount().await;
        }

        let (event_tx, event_rx) = mpsc::channel::<Event>();
        let mut state = ready_state(client);
        let config = CoreConfig {
            profile_dir: std::env::temp_dir().join("mirx-core-runtime-test"),
            store_passphrase: "test-store-passphrase".to_owned(),
        };
        assert!(!handle_command(command, &config, &event_tx, &mut state).await);

        match event_rx
            .recv_timeout(Duration::from_secs(2))
            .expect("authoritative room members snapshot")
        {
            Event::RoomMembers(snapshot) => {
                assert_eq!(snapshot.room_id, "!room:example.org");
                assert!(
                    snapshot
                        .members
                        .iter()
                        .any(|member| member.user_id == "@target:example.org")
                );
            }
            other => panic!("expected room members snapshot, got {other:?}"),
        }
        assert_eq!(event_rx.try_recv(), Err(TryRecvError::Empty));
        assert_eq!(
            mock_server
                .received_requests()
                .await
                .expect("request recording enabled")
                .iter()
                .filter(|request| request.url.path().ends_with(path))
                .count(),
            1
        );
        shutdown_state(&mut state).await;
    }

    #[test]
    fn ready_kick_submits_once_then_emits_authoritative_members_snapshot() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("build tokio runtime");
        tokio::task::LocalSet::new().block_on(&runtime, async {
            assert_ready_moderation_success(
                Command::KickRoomMember(KickRoomMemberRequest {
                    room_id: "!room:example.org".to_owned(),
                    user_id: "@target:example.org".to_owned(),
                    reason: None,
                }),
                "/kick",
                false,
            )
            .await;
        });
    }

    #[test]
    fn ready_ban_submits_once_then_emits_authoritative_members_snapshot() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("build tokio runtime");
        tokio::task::LocalSet::new().block_on(&runtime, async {
            assert_ready_moderation_success(
                Command::BanRoomMember(BanRoomMemberRequest {
                    room_id: "!room:example.org".to_owned(),
                    user_id: "@target:example.org".to_owned(),
                    reason: None,
                }),
                "/ban",
                true,
            )
            .await;
        });
    }

    async fn assert_ready_moderation_rejection(command: Command, operation: Operation, path: &str) {
        let mock_server = MatrixMockServer::new().await;
        let client = mock_server.client_builder().build().await;
        let room = mock_server
            .sync_room(
                &client,
                JoinedRoomBuilder::new(room_id().as_ref()).add_state_bulk(joined_member_state()),
            )
            .await;
        assert!(
            room.members_no_sync(matrix_sdk::RoomMemberships::JOIN)
                .await
                .expect("cached members")
                .iter()
                .any(|member| member.user_id().as_str() == "@target:example.org")
        );

        let (event_tx, event_rx) = mpsc::channel::<Event>();
        let mut state = ready_state(client);
        let config = CoreConfig {
            profile_dir: std::env::temp_dir().join("mirx-core-runtime-test"),
            store_passphrase: "test-store-passphrase".to_owned(),
        };
        assert!(!handle_command(command, &config, &event_tx, &mut state).await);

        match event_rx
            .recv_timeout(Duration::from_secs(2))
            .expect("moderation rejection")
        {
            Event::Error(error) => assert_eq!(error.operation, operation),
            other => panic!("expected moderation error, got {other:?}"),
        }
        assert_eq!(event_rx.try_recv(), Err(TryRecvError::Empty));
        assert_eq!(
            mock_server
                .received_requests()
                .await
                .expect("request recording enabled")
                .iter()
                .filter(|request| request.url.path().ends_with(path))
                .count(),
            1
        );
        assert!(
            room.members_no_sync(matrix_sdk::RoomMemberships::JOIN)
                .await
                .expect("cached members after rejection")
                .iter()
                .any(|member| member.user_id().as_str() == "@target:example.org")
        );
        shutdown_state(&mut state).await;
    }

    #[test]
    fn ready_kick_rejection_emits_error_without_snapshot_or_optimistic_removal() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("build tokio runtime");
        tokio::task::LocalSet::new().block_on(&runtime, async {
            assert_ready_moderation_rejection(
                Command::KickRoomMember(KickRoomMemberRequest {
                    room_id: "!room:example.org".to_owned(),
                    user_id: "@target:example.org".to_owned(),
                    reason: None,
                }),
                Operation::KickRoomMember,
                "/kick",
            )
            .await;
        });
    }

    #[test]
    fn ready_ban_rejection_emits_error_without_snapshot_or_optimistic_removal() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("build tokio runtime");
        tokio::task::LocalSet::new().block_on(&runtime, async {
            assert_ready_moderation_rejection(
                Command::BanRoomMember(BanRoomMemberRequest {
                    room_id: "!room:example.org".to_owned(),
                    user_id: "@target:example.org".to_owned(),
                    reason: None,
                }),
                Operation::BanRoomMember,
                "/ban",
            )
            .await;
        });
    }

    fn test_config() -> CoreConfig {
        CoreConfig {
            profile_dir: std::env::temp_dir().join("mirx-core-runtime-test"),
            store_passphrase: "test-store-passphrase".to_owned(),
        }
    }

    /// Mounts `POST /join/<target>` answering with `room_id`; `target` is
    /// as it appears in the URL, percent-encoded.
    async fn mock_join(mock_server: &MatrixMockServer, target: &str, room_id: &str) {
        wiremock::Mock::given(wiremock::matchers::method("POST"))
            .and(wiremock::matchers::path(format!(
                "/_matrix/client/v3/join/{target}"
            )))
            .respond_with(
                wiremock::ResponseTemplate::new(200)
                    .set_body_json(serde_json::json!({ "room_id": room_id })),
            )
            .expect(1)
            .mount(mock_server.server())
            .await;
    }

    /// Runs `command` against a ready client and asserts that the room
    /// snapshot listing `room_id` comes first, then `RoomJoined`.
    async fn assert_join_emits_snapshot_then_room_joined(
        mock_server: &MatrixMockServer,
        client: matrix_sdk::Client,
        command: Command,
        room_id: &str,
    ) {
        // The snapshot reads each room's encryption state.
        mock_server
            .mock_room_state_encryption()
            .plain()
            .mount()
            .await;
        let (event_tx, event_rx) = mpsc::channel::<Event>();
        let mut state = ready_state(client);
        assert!(!handle_command(command, &test_config(), &event_tx, &mut state).await);

        let events = drain_events(&event_rx);
        match events.as_slice() {
            [Event::SyncedRooms(rooms), Event::RoomJoined(joined)] => {
                assert!(
                    rooms.iter().any(|room| room.room_id == room_id),
                    "the snapshot must list the joined room: {rooms:?}"
                );
                assert_eq!(
                    joined,
                    &RoomJoined {
                        room_id: room_id.to_owned()
                    }
                );
            }
            other => panic!("expected SyncedRooms then RoomJoined, got {other:?}"),
        }
        mock_server.server().verify().await;
        shutdown_state(&mut state).await;
    }

    #[test]
    fn join_room_emits_snapshot_then_room_joined_with_the_alias_room_id() {
        run_local(async {
            let mock_server = MatrixMockServer::new().await;
            let client = mock_server.client_builder().build().await;
            mock_join(&mock_server, "%23ops:example.org", "!ops:example.org").await;

            assert_join_emits_snapshot_then_room_joined(
                &mock_server,
                client,
                Command::JoinRoom(JoinRoomRequest {
                    room_or_alias: "#ops:example.org".to_owned(),
                    via: Vec::new(),
                }),
                "!ops:example.org",
            )
            .await;
        });
    }

    #[test]
    fn join_room_passes_via_servers() {
        run_local(async {
            let mock_server = MatrixMockServer::new().await;
            let client = mock_server.client_builder().build().await;
            wiremock::Mock::given(wiremock::matchers::method("POST"))
                .and(wiremock::matchers::path(
                    "/_matrix/client/v3/join/!ops:other.example",
                ))
                .and(wiremock::matchers::query_param("via", "other.example"))
                .respond_with(
                    wiremock::ResponseTemplate::new(200)
                        .set_body_json(serde_json::json!({ "room_id": "!ops:other.example" })),
                )
                .expect(1)
                .mount(mock_server.server())
                .await;

            assert_join_emits_snapshot_then_room_joined(
                &mock_server,
                client,
                Command::JoinRoom(JoinRoomRequest {
                    room_or_alias: "!ops:other.example".to_owned(),
                    via: vec!["other.example".to_owned()],
                }),
                "!ops:other.example",
            )
            .await;
        });
    }

    #[test]
    fn join_room_rejects_an_invalid_via_server_without_a_request() {
        run_local(async {
            let mock_server = MatrixMockServer::new().await;
            let client = mock_server.client_builder().build().await;
            let (event_tx, event_rx) = mpsc::channel::<Event>();
            let mut state = ready_state(client);
            let command = Command::JoinRoom(JoinRoomRequest {
                room_or_alias: "!ops:other.example".to_owned(),
                via: vec!["not a server".to_owned()],
            });
            assert!(!handle_command(command, &test_config(), &event_tx, &mut state).await);

            match drain_events(&event_rx).as_slice() {
                [Event::Error(error)] => {
                    assert_eq!(error.operation, Operation::JoinRoom);
                    assert!(
                        error
                            .message
                            .contains("invalid Matrix server name not a server"),
                        "{}",
                        error.message
                    );
                }
                other => panic!("expected one join error, got {other:?}"),
            }
            assert!(
                mock_server
                    .received_requests()
                    .await
                    .expect("request recording enabled")
                    .is_empty()
            );
            shutdown_state(&mut state).await;
        });
    }

    #[test]
    fn failed_join_room_emits_the_error_and_no_room_joined() {
        run_local(async {
            let mock_server = MatrixMockServer::new().await;
            let client = mock_server.client_builder().build().await;
            wiremock::Mock::given(wiremock::matchers::method("POST"))
                .and(wiremock::matchers::path(
                    "/_matrix/client/v3/join/%23gone:example.org",
                ))
                .respond_with(wiremock::ResponseTemplate::new(404).set_body_json(
                    serde_json::json!({ "errcode": "M_NOT_FOUND", "error": "No such room" }),
                ))
                .mount(mock_server.server())
                .await;
            let (event_tx, event_rx) = mpsc::channel::<Event>();
            let mut state = ready_state(client);
            let command = Command::JoinRoom(JoinRoomRequest {
                room_or_alias: "#gone:example.org".to_owned(),
                via: Vec::new(),
            });
            assert!(!handle_command(command, &test_config(), &event_tx, &mut state).await);

            match drain_events(&event_rx).as_slice() {
                [Event::Error(error)] => {
                    assert_eq!(error.operation, Operation::JoinRoom);
                    // A join may name an alias: it has no room yet.
                    assert_eq!(error.room_id, None);
                }
                other => panic!("expected one join error, got {other:?}"),
            }
            shutdown_state(&mut state).await;
        });
    }

    #[test]
    fn kick_failure_error_carries_the_room() {
        run_local(async {
            let mock_server = MatrixMockServer::new().await;
            let client = mock_server.client_builder().build().await;
            mock_server.sync_joined_room(&client, &room_id()).await;
            wiremock::Mock::given(wiremock::matchers::method("POST"))
                .and(wiremock::matchers::path_regex(
                    r"^/_matrix/client/v3/rooms/.*/kick$",
                ))
                .respond_with(wiremock::ResponseTemplate::new(403).set_body_json(
                    serde_json::json!({ "errcode": "M_FORBIDDEN", "error": "not an op" }),
                ))
                .expect(1)
                .mount(mock_server.server())
                .await;
            let (event_tx, event_rx) = mpsc::channel::<Event>();
            let mut state = ready_state(client);
            let command = Command::KickRoomMember(KickRoomMemberRequest {
                room_id: room_id().to_string(),
                user_id: "@bob:example.org".to_owned(),
                reason: None,
            });
            assert!(!handle_command(command, &test_config(), &event_tx, &mut state).await);

            match drain_events(&event_rx).as_slice() {
                [Event::Error(error)] => {
                    assert_eq!(error.operation, Operation::KickRoomMember);
                    assert_eq!(error.room_id.as_deref(), Some("!room:example.org"));
                    assert!(error.message.contains("not an op"), "{}", error.message);
                }
                other => panic!("expected one kick error, got {other:?}"),
            }
            shutdown_state(&mut state).await;
        });
    }

    #[test]
    fn create_direct_room_emits_snapshot_then_room_joined() {
        run_local(async {
            let mock_server = MatrixMockServer::new().await;
            let client = mock_server.client_builder().build().await;
            mock_server.mock_create_room().ok().expect(1).mount().await;

            assert_join_emits_snapshot_then_room_joined(
                &mock_server,
                client,
                Command::CreateDirectRoom(CreateDirectRoomRequest {
                    user_id: "@friend:example.org".to_owned(),
                }),
                "!room:example.org",
            )
            .await;
        });
    }

    #[test]
    fn create_room_emits_snapshot_then_room_joined() {
        run_local(async {
            let mock_server = MatrixMockServer::new().await;
            let client = mock_server.client_builder().build().await;
            mock_server.mock_create_room().ok().expect(1).mount().await;

            assert_join_emits_snapshot_then_room_joined(
                &mock_server,
                client,
                Command::CreateRoom(CreateRoomRequest {
                    name: "Team room".to_owned(),
                    visibility: CreateRoomVisibility::Private,
                }),
                "!room:example.org",
            )
            .await;
        });
    }

    #[test]
    fn accept_room_invite_emits_snapshot_then_room_joined() {
        run_local(async {
            let mock_server = MatrixMockServer::new().await;
            let client = mock_server.client_builder().build().await;
            mock_server
                .sync_room(&client, InvitedRoomBuilder::new(room_id().as_ref()))
                .await;
            mock_server
                .mock_room_join(room_id().as_ref())
                .ok()
                .expect(1)
                .mount()
                .await;

            assert_join_emits_snapshot_then_room_joined(
                &mock_server,
                client,
                Command::AcceptRoomInvite(AcceptRoomInviteRequest {
                    room_id: room_id().to_string(),
                }),
                room_id().as_str(),
            )
            .await;
        });
    }

    fn room_summary(room_id: &str, display_name: &str, membership: RoomMembership) -> RoomSummary {
        RoomSummary {
            room_id: room_id.to_owned(),
            display_name: display_name.to_owned(),
            topic: None,
            membership,
            encrypted: true,
            is_space: false,
            is_direct: false,
            direct_user_ids: Vec::new(),
            tags: Vec::new(),
            space_children: Vec::new(),
            notification_mode: match membership {
                RoomMembership::Join => Some(RoomNotificationMode::AllMessages),
                RoomMembership::Invite => None,
            },
            canonical_alias: None,
            alt_aliases: Vec::new(),
        }
    }

    fn room_read_state(
        room_id: &str,
        latest_active_event_id: &str,
        fully_read_event_id: &str,
    ) -> RoomReadState {
        RoomReadState {
            room_id: room_id.to_owned(),
            latest_active_event_id: Some(latest_active_event_id.to_owned()),
            fully_read_event_id: Some(fully_read_event_id.to_owned()),
            marked_unread: true,
            num_unread: 2,
            num_notifications: 1,
            num_mentions: 0,
        }
    }

    fn room_members(room_id: &str, members: &[(&str, Option<&str>)]) -> RoomMembers {
        RoomMembers {
            room_id: room_id.to_owned(),
            members: members
                .iter()
                .map(|(user_id, display_name)| RoomMemberSummary {
                    user_id: (*user_id).to_owned(),
                    display_name: display_name.map(str::to_owned),
                    display_name_ambiguous: false,
                    is_own_user: false,
                    rank: RoomMemberRank::Member,
                    can_kick: false,
                    can_ban: false,
                })
                .collect(),
        }
    }

    #[test]
    fn duplicate_start_sync_emits_typed_error_event() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("build tokio runtime");
        let local = tokio::task::LocalSet::new();

        local.block_on(&runtime, async {
            let client = matrix_sdk::Client::builder()
                .homeserver_url("http://localhost")
                .build()
                .await
                .expect("build matrix client");
            let existing_task = tokio::task::spawn_local(std::future::pending::<()>());
            let (event_tx, event_rx) = mpsc::channel::<Event>();
            let mut state = WorkerState::Ready {
                client,
                initial_snapshot: InitialSnapshot::pending(false),
                paused_sends: PausedSendRooms::default(),
                sync_task: Some(existing_task),
                room_list_update_task: tokio::task::spawn_local(std::future::pending::<()>()),
                room_read_state_update_task: tokio::task::spawn_local(std::future::pending::<()>()),
                e2ee_recovery_state_task: tokio::task::spawn_local(std::future::pending::<()>()),
                e2ee_backup_state_task: tokio::task::spawn_local(std::future::pending::<()>()),
                session_update_task: tokio::task::spawn_local(std::future::pending::<()>()),
                send_queue_update_task: tokio::task::spawn_local(std::future::pending::<()>()),
                verification_tasks: HashMap::new(),
            };

            assert!(!handle_start_sync(&event_tx as &EventSender, &mut state));

            match event_rx
                .recv_timeout(Duration::from_secs(2))
                .expect("duplicate sync error")
            {
                Event::Error(err) => {
                    assert_eq!(err.operation, Operation::StartSync);
                    assert_eq!(err.message, "continuous sync is already running");
                }
                other => panic!("expected duplicate sync error, got {other:?}"),
            }

            if let WorkerState::Ready {
                sync_task,
                room_list_update_task,
                room_read_state_update_task,
                e2ee_recovery_state_task,
                e2ee_backup_state_task,
                session_update_task,
                send_queue_update_task,
                ..
            } = &mut state
            {
                if let Some(task) = sync_task.take() {
                    task.abort();
                }
                room_list_update_task.abort();
                room_read_state_update_task.abort();
                e2ee_recovery_state_task.abort();
                e2ee_backup_state_task.abort();
                session_update_task.abort();
                send_queue_update_task.abort();
            }
        });
    }

    #[test]
    fn initial_room_members_and_read_states_follow_joined_synced_rooms() {
        let (event_tx, event_rx) = mpsc::channel::<Event>();
        let rooms = vec![
            room_summary("!ops:example.org", "Ops", RoomMembership::Join),
            room_summary("!eng:example.org", "Eng", RoomMembership::Join),
            room_summary("!invite:example.org", "Invite", RoomMembership::Invite),
        ];
        let room_members = vec![
            room_members(
                "!ops:example.org",
                &[
                    ("@alice:example.org", Some("Alice")),
                    ("@bob:example.org", None),
                ],
            ),
            room_members("!eng:example.org", &[("@carol:example.org", Some("Carol"))]),
            room_members(
                "!invite:example.org",
                &[("@dave:example.org", Some("Dave"))],
            ),
        ];
        let read_states = vec![
            room_read_state(
                "!ops:example.org",
                "$latest-ops:example.org",
                "$fully-read-ops:example.org",
            ),
            room_read_state(
                "!eng:example.org",
                "$latest-eng:example.org",
                "$fully-read-eng:example.org",
            ),
            room_read_state(
                "!invite:example.org",
                "$latest-invite:example.org",
                "$fully-read-invite:example.org",
            ),
        ];

        RoomSnapshot {
            rooms: rooms.clone(),
            room_members: room_members.clone(),
            room_read_states: read_states.clone(),
            account_lists: AccountLists {
                highlight_words: Ok(vec![HighlightWord {
                    word: "deploy".to_owned(),
                    highlights: true,
                }]),
                ignored_users: Ok(vec!["@spam:example.org".to_owned()]),
            },
        }
        .emit(&event_tx as &EventSender);

        assert_eq!(
            event_rx
                .recv_timeout(Duration::from_secs(2))
                .expect("synced rooms event"),
            Event::SyncedRooms(rooms)
        );
        assert_eq!(
            event_rx
                .recv_timeout(Duration::from_secs(2))
                .expect("highlight words event"),
            Event::HighlightWords(vec![HighlightWord {
                word: "deploy".to_owned(),
                highlights: true,
            }])
        );
        assert_eq!(
            event_rx
                .recv_timeout(Duration::from_secs(2))
                .expect("ignored users event"),
            Event::IgnoredUsers(vec!["@spam:example.org".to_owned()])
        );
        for members in room_members.into_iter().take(2) {
            assert_eq!(
                event_rx
                    .recv_timeout(Duration::from_secs(2))
                    .expect("room members event"),
                Event::RoomMembers(members)
            );
        }
        for read_state in read_states.into_iter().take(2) {
            assert_eq!(
                event_rx
                    .recv_timeout(Duration::from_secs(2))
                    .expect("room read state event"),
                Event::RoomReadState(read_state)
            );
        }
        assert_eq!(
            event_rx.try_recv(),
            Err(TryRecvError::Empty),
            "initial snapshot must only emit synced rooms, highlight words, room members, and room read states"
        );
    }

    #[test]
    fn shutdown_state_drops_every_task_inside_the_runtime() {
        /// Owned by a task like a `Client` clone: dropping it needs the
        /// runtime, as the SDK store's connection pool does.
        struct NeedsRuntimeOnDrop(std::rc::Rc<std::cell::Cell<bool>>);
        impl Drop for NeedsRuntimeOnDrop {
            fn drop(&mut self) {
                tokio::runtime::Handle::current();
                self.0.set(true);
            }
        }

        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("build tokio runtime");
        let local = tokio::task::LocalSet::new();
        let dropped = std::rc::Rc::new(std::cell::Cell::new(false));

        local.block_on(&runtime, async {
            let client = matrix_sdk::Client::builder()
                .homeserver_url("http://localhost")
                .build()
                .await
                .expect("build matrix client");
            let guard = NeedsRuntimeOnDrop(dropped.clone());
            let verification_task = tokio::task::spawn_local(async move {
                let _guard = guard;
                std::future::pending::<()>().await
            });
            let mut state = WorkerState::Ready {
                client,
                initial_snapshot: InitialSnapshot::pending(false),
                paused_sends: PausedSendRooms::default(),
                sync_task: None,
                room_list_update_task: tokio::task::spawn_local(std::future::pending::<()>()),
                room_read_state_update_task: tokio::task::spawn_local(std::future::pending::<()>()),
                e2ee_recovery_state_task: tokio::task::spawn_local(std::future::pending::<()>()),
                e2ee_backup_state_task: tokio::task::spawn_local(std::future::pending::<()>()),
                session_update_task: tokio::task::spawn_local(std::future::pending::<()>()),
                send_queue_update_task: tokio::task::spawn_local(std::future::pending::<()>()),
                verification_tasks: HashMap::from([(
                    ("@alice:example.org".to_owned(), "txn-123".to_owned()),
                    verification_task,
                )]),
            };

            shutdown_state(&mut state).await;

            assert!(matches!(state, WorkerState::NoClient));
            assert!(dropped.get(), "the task is gone when shutdown returns");
        });
        drop(local);
    }

    #[test]
    fn worker_stops_when_command_channel_closes() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("build tokio runtime");
        let local = tokio::task::LocalSet::new();

        local.block_on(&runtime, async {
            let (command_tx, command_rx) = unbounded_channel();
            let (event_tx, event_rx) = mpsc::channel::<Event>();
            let config = CoreConfig {
                profile_dir: std::env::temp_dir().join("mirx-core-runtime-test"),
                store_passphrase: "test-store-passphrase".to_owned(),
            };

            drop(command_tx);
            run_worker(config, command_rx, event_tx).await;

            assert_eq!(
                event_rx
                    .recv_timeout(Duration::from_secs(2))
                    .expect("started event"),
                Event::Started
            );
            assert_eq!(
                event_rx
                    .recv_timeout(Duration::from_secs(2))
                    .expect("stopped event"),
                Event::Stopped
            );
        });
    }

    fn run_local(test: impl std::future::Future<Output = ()>) {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("build tokio runtime");
        tokio::task::LocalSet::new().block_on(&runtime, test);
    }

    async fn offline_client() -> matrix_sdk::Client {
        matrix_sdk::Client::builder()
            .homeserver_url("http://localhost")
            .build()
            .await
            .expect("build matrix client")
    }

    fn drain_events(event_rx: &mpsc::Receiver<Event>) -> Vec<Event> {
        event_rx.try_iter().collect()
    }

    /// Waits for the next event while letting the `LocalSet`'s tasks run;
    /// a blocking `recv_timeout` would starve the sync task.
    async fn next_event(event_rx: &mpsc::Receiver<Event>) -> Event {
        let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
        loop {
            match event_rx.try_recv() {
                Ok(event) => return event,
                Err(TryRecvError::Empty) => {
                    assert!(
                        tokio::time::Instant::now() < deadline,
                        "timed out waiting for a core event"
                    );
                    tokio::time::sleep(Duration::from_millis(10)).await;
                }
                Err(TryRecvError::Disconnected) => panic!("event channel closed"),
            }
        }
    }

    fn sync_task_is_none(state: &WorkerState) -> bool {
        let WorkerState::Ready { sync_task, .. } = state else {
            panic!("expected a ready worker");
        };
        sync_task.is_none()
    }

    fn assert_stop_sync_error(event_rx: &mpsc::Receiver<Event>, message: &str) {
        match drain_events(event_rx).as_slice() {
            [Event::Error(err)] => {
                assert_eq!(err.operation, Operation::StopSync);
                assert_eq!(err.message, message);
            }
            other => panic!("expected one stop sync error, got {other:?}"),
        }
    }

    #[test]
    fn stop_sync_aborts_running_sync_and_emits_sync_stopped() {
        run_local(async {
            let (event_tx, event_rx) = mpsc::channel::<Event>();
            let mut state = ready_state(offline_client().await);
            let WorkerState::Ready { sync_task, .. } = &mut state else {
                unreachable!();
            };
            *sync_task = Some(tokio::task::spawn_local(std::future::pending::<()>()));

            assert!(!handle_stop_sync(&event_tx, &mut state));

            assert_eq!(
                drain_events(&event_rx),
                vec![Event::SyncStopped(SyncStopReason::Requested)]
            );
            assert!(sync_task_is_none(&state));
            shutdown_state(&mut state).await;
        });
    }

    #[test]
    fn stop_sync_without_running_sync_emits_typed_error() {
        run_local(async {
            let (event_tx, event_rx) = mpsc::channel::<Event>();
            let mut state = ready_state(offline_client().await);

            assert!(!handle_stop_sync(&event_tx, &mut state));
            assert_stop_sync_error(&event_rx, "sync is not running");

            let finished = tokio::task::spawn_local(async {});
            while !finished.is_finished() {
                tokio::task::yield_now().await;
            }
            let WorkerState::Ready { sync_task, .. } = &mut state else {
                unreachable!();
            };
            *sync_task = Some(finished);

            assert!(!handle_stop_sync(&event_tx, &mut state));
            assert_stop_sync_error(&event_rx, "sync is not running");
            shutdown_state(&mut state).await;
        });
    }

    #[test]
    fn stop_sync_without_session_emits_invalid_state() {
        let (event_tx, event_rx) = mpsc::channel::<Event>();
        let mut state = WorkerState::NoClient;

        assert!(!handle_stop_sync(&event_tx, &mut state));

        assert_stop_sync_error(&event_rx, "stop sync requires an authenticated session");
    }

    #[test]
    fn start_sync_after_stop_sync_restarts() {
        run_local(async {
            let (event_tx, event_rx) = mpsc::channel::<Event>();
            let mut state = ready_state(offline_client().await);

            assert!(!handle_start_sync(&event_tx, &mut state));
            assert!(!handle_stop_sync(&event_tx, &mut state));
            assert!(!handle_start_sync(&event_tx, &mut state));

            assert_eq!(
                drain_events(&event_rx),
                vec![
                    Event::SyncStarted,
                    Event::SyncStopped(SyncStopReason::Requested),
                    Event::SyncStarted,
                ]
            );
            assert!(!sync_task_is_none(&state));
            shutdown_state(&mut state).await;
        });
    }

    #[test]
    fn sync_with_an_unknown_token_stops_with_session_ended() {
        run_local(async {
            let mock_server = MatrixMockServer::new().await;
            // No refresh token: the SDK would try a refresh first.
            let client = mock_server.client_builder().build().await;
            mock_server
                .mock_sync()
                .error_unknown_token(false)
                .mount()
                .await;
            let (event_tx, event_rx) = mpsc::channel::<Event>();
            let mut state = ready_state(client);

            assert!(!handle_start_sync(&event_tx, &mut state));

            assert_eq!(next_event(&event_rx).await, Event::SyncStarted);
            match next_event(&event_rx).await {
                Event::SyncStopped(SyncStopReason::SessionEnded(message)) => {
                    assert!(message.contains("M_UNKNOWN_TOKEN"), "{message}");
                }
                other => panic!("expected a session-ended sync stop, got {other:?}"),
            }
            assert_eq!(drain_events(&event_rx), Vec::new());
            shutdown_state(&mut state).await;
        });
    }

    #[test]
    fn sync_without_a_server_stops_with_lost_and_never_connects() {
        run_local(async {
            let client =
                matrix_sdk::test_utils::client::MockClientBuilder::new(Some(&closed_port_url()))
                    .build()
                    .await;
            let (event_tx, event_rx) = mpsc::channel::<Event>();
            let mut state = ready_state(client);

            assert!(!handle_start_sync(&event_tx, &mut state));

            assert_eq!(next_event(&event_rx).await, Event::SyncStarted);
            match next_event(&event_rx).await {
                Event::SyncStopped(SyncStopReason::Lost(message)) => {
                    assert!(message.contains("error sending request"), "{message}");
                }
                other => panic!("expected a lost sync stop, got {other:?}"),
            }
            assert_eq!(drain_events(&event_rx), Vec::new());
            shutdown_state(&mut state).await;
        });
    }

    #[test]
    fn sync_with_another_server_error_stops_with_lost() {
        run_local(async {
            let mock_server = MatrixMockServer::new().await;
            let client = mock_server.client_builder().build().await;
            // A 4xx is not retried; a 5xx would be, for minutes.
            mock_server
                .mock_sync()
                .respond_with(wiremock::ResponseTemplate::new(400).set_body_json(
                    serde_json::json!({
                        "errcode": "M_UNKNOWN",
                        "error": "bad sync",
                    }),
                ))
                .mount()
                .await;
            let (event_tx, event_rx) = mpsc::channel::<Event>();
            let mut state = ready_state(client);

            assert!(!handle_start_sync(&event_tx, &mut state));

            assert_eq!(next_event(&event_rx).await, Event::SyncStarted);
            match next_event(&event_rx).await {
                Event::SyncStopped(SyncStopReason::Lost(message)) => {
                    assert!(message.contains("bad sync"), "{message}");
                }
                other => panic!("expected a lost sync stop, got {other:?}"),
            }
            assert_eq!(drain_events(&event_rx), Vec::new());
            shutdown_state(&mut state).await;
        });
    }

    #[test]
    fn sync_announces_the_first_response_once() {
        run_local(async {
            let mock_server = MatrixMockServer::new().await;
            let client = mock_server.client_builder().build().await;
            mock_server.mock_sync().ok(|_| {}).mount().await;
            let (event_tx, event_rx) = mpsc::channel::<Event>();
            let mut state = ready_state(client);

            assert!(!handle_start_sync(&event_tx, &mut state));

            assert_eq!(next_event(&event_rx).await, Event::SyncStarted);
            assert_eq!(next_event(&event_rx).await, Event::SyncConnected);
            wait_for_sync_requests(&mock_server, 3).await;
            assert_eq!(drain_events(&event_rx), Vec::new());
            shutdown_state(&mut state).await;
        });
    }

    /// A URL whose port refuses connections: the network is down.
    fn closed_port_url() -> String {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind a local port");
        let address = listener
            .local_addr()
            .expect("a bound listener has an address");
        drop(listener);
        format!("http://{address}")
    }

    async fn wait_for_sync_requests(mock_server: &MatrixMockServer, count: usize) {
        let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
        loop {
            let syncs = mock_server
                .server()
                .received_requests()
                .await
                .expect("the mock server records requests")
                .into_iter()
                .filter(|request| request.url.path().ends_with("/sync"))
                .count();
            if syncs >= count {
                return;
            }
            assert!(
                tokio::time::Instant::now() < deadline,
                "timed out waiting for {count} sync requests, saw {syncs}"
            );
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    }

    fn login_request(mock_server: &MatrixMockServer, start_sync: bool) -> LoginRequest {
        LoginRequest {
            server: mock_server.uri(),
            method: LoginMethod::Password {
                username: "example".to_owned(),
                password: "wordpass".to_owned(),
            },
            device_display_name: "mirx".to_owned(),
            start_sync,
        }
    }

    async fn mock_login_server() -> MatrixMockServer {
        let mock_server = MatrixMockServer::new().await;
        mock_server.mock_versions().ok().mount().await;
        mock_server.mock_login().ok().mount().await;
        mock_server.mock_sync().ok(|_| {}).mount().await;
        mock_server
    }

    /// Runs `handle_login` and returns the events it emitted up to its
    /// return; the sync task, if any, has not run yet.
    async fn login_events(
        mock_server: &MatrixMockServer,
        start_sync: bool,
    ) -> (tempfile::TempDir, WorkerState, Vec<Event>) {
        let profile = tempfile::TempDir::new().expect("temporary profile directory");
        let config = CoreConfig {
            profile_dir: profile.path().join("profile"),
            store_passphrase: "test-store-passphrase".to_owned(),
        };
        let (event_tx, event_rx) = mpsc::channel::<Event>();
        let mut state = WorkerState::NoClient;

        assert!(
            !handle_login(
                &config,
                &event_tx,
                &mut state,
                login_request(mock_server, start_sync)
            )
            .await
        );

        (profile, state, drain_events(&event_rx))
    }

    fn event_position(events: &[Event], matches: impl Fn(&Event) -> bool) -> Option<usize> {
        events.iter().position(matches)
    }

    #[test]
    fn login_with_start_sync_emits_logged_in_with_resolved_homeserver_then_sync_started() {
        run_local(async {
            let mock_server = mock_login_server().await;
            let (_profile, mut state, events) = login_events(&mock_server, true).await;

            // The SDK normalises the discovered URL with a trailing slash.
            let resolved = format!("{}/", mock_server.uri());
            let logged_in = event_position(&events, |event| {
                matches!(
                    event,
                    Event::LoggedIn { homeserver_url, .. } if *homeserver_url == resolved
                )
            })
            .unwrap_or_else(|| panic!("LoggedIn at {resolved} in {events:?}"));
            let synced_rooms =
                event_position(&events, |event| matches!(event, Event::SyncedRooms(_)))
                    .unwrap_or_else(|| panic!("SyncedRooms in {events:?}"));
            assert_eq!(
                event_position(&events, |event| *event == Event::SyncStarted),
                Some(events.len() - 1),
                "{events:?}"
            );
            assert!(logged_in < synced_rooms);
            // No `m.push_rules` from the mock server: the server defaults
            // have no highlight words.
            assert_eq!(
                events.get(synced_rooms + 1),
                Some(&Event::HighlightWords(Vec::new())),
                "{events:?}"
            );
            assert!(!sync_task_is_none(&state));
            shutdown_state(&mut state).await;
        });
    }

    /// The snapshot came with the initial sync: the sync task only
    /// announces the connection.
    #[test]
    fn a_session_that_starts_online_gets_its_rooms_once() {
        run_local(async {
            let mock_server = mock_login_server().await;
            let profile = tempfile::TempDir::new().expect("temporary profile directory");
            let config = CoreConfig {
                profile_dir: profile.path().join("profile"),
                store_passphrase: "test-store-passphrase".to_owned(),
            };
            let (event_tx, event_rx) = mpsc::channel::<Event>();
            let mut state = WorkerState::NoClient;
            assert!(
                !handle_login(
                    &config,
                    &event_tx,
                    &mut state,
                    login_request(&mock_server, true)
                )
                .await
            );

            let events = settled_events(&event_rx).await;
            let snapshot_events = |events: &[Event]| {
                events
                    .iter()
                    .filter(|event| matches!(event, Event::HighlightWords(_)))
                    .count()
            };
            assert_eq!(snapshot_events(&events), 1, "{events:?}");
            let sync_started = event_position(&events, |event| *event == Event::SyncStarted);
            let sync_connected = event_position(&events, |event| *event == Event::SyncConnected);
            assert!(
                sync_started.is_some() && sync_started < sync_connected,
                "{events:?}"
            );
            shutdown_state(&mut state).await;
        });
    }

    #[test]
    fn login_without_start_sync_leaves_sync_idle() {
        run_local(async {
            let mock_server = mock_login_server().await;
            let (_profile, mut state, events) = login_events(&mock_server, false).await;

            assert!(
                events
                    .iter()
                    .any(|event| matches!(event, Event::LoggedIn { .. })),
                "{events:?}"
            );
            assert!(!events.contains(&Event::SyncStarted), "{events:?}");
            assert!(sync_task_is_none(&state));
            shutdown_state(&mut state).await;
        });
    }

    /// The mock login's user and Bob, joined, as state or member events.
    fn logged_in_user_room_members<T>() -> Vec<Raw<T>> {
        ["@cheeky_monkey:matrix.org", "@bob:example.org"]
            .into_iter()
            .enumerate()
            .map(|(index, user_id)| {
                serde_json::json!({
                    "type": "m.room.member", "state_key": user_id, "sender": user_id,
                    "event_id": format!("$member-{index}:example.org"),
                    "origin_server_ts": 1u64, "content": { "membership": "join" }
                })
            })
            .map(|member| Raw::from_json_string(member.to_string()).expect("raw member event"))
            .collect()
    }

    fn bob_message(event_id: &str) -> Raw<matrix_sdk::ruma::events::AnySyncTimelineEvent> {
        Raw::from_json_string(
            serde_json::json!({
                "type": "m.room.message",
                "content": { "msgtype": "m.text", "body": "unread" },
                "event_id": event_id,
                "sender": "@bob:example.org",
                "origin_server_ts": 1700000000000u64
            })
            .to_string(),
        )
        .expect("raw timeline event")
    }

    /// The events emitted until now and in the next half second, while the
    /// session's tasks (and the SDK's event cache) run.
    async fn settled_events(event_rx: &mpsc::Receiver<Event>) -> Vec<Event> {
        tokio::time::sleep(Duration::from_millis(500)).await;
        drain_events(event_rx)
    }

    fn read_states_of<'a>(events: &'a [Event], room_id: &str) -> Vec<&'a RoomReadState> {
        events
            .iter()
            .filter_map(|event| match event {
                Event::RoomReadState(read_state) if read_state.room_id == room_id => {
                    Some(read_state)
                }
                _ => None,
            })
            .collect()
    }

    /// mirx started before the network was up: the first sync that gets
    /// through brings the whole room snapshot, before `SyncConnected`, and
    /// later reconnections do not repeat it.
    #[test]
    fn a_session_that_starts_offline_gets_its_rooms_at_the_first_sync_that_connects() {
        run_local(async {
            let mock_server = MatrixMockServer::new().await;
            mock_server.mock_versions().ok().mount().await;
            mock_server.mock_login().ok().mount().await;
            mock_server
                .mock_get_members()
                .expect_any_access_token()
                .ok(logged_in_user_room_members())
                .mount()
                .await;
            mock_server
                .mock_room_state_encryption()
                .expect_any_access_token()
                .plain()
                .mount()
                .await;
            let failing_sync =
                mock_server
                    .mock_sync()
                    .respond_with(wiremock::ResponseTemplate::new(400).set_body_json(
                        serde_json::json!({ "errcode": "M_UNKNOWN", "error": "down" }),
                    ))
                    .mount_as_scoped()
                    .await;
            let profile = tempfile::TempDir::new().expect("temporary profile directory");
            let config = CoreConfig {
                profile_dir: profile.path().join("profile"),
                store_passphrase: "test-store-passphrase".to_owned(),
            };
            let (event_tx, event_rx) = mpsc::channel::<Event>();
            let mut state = WorkerState::NoClient;
            assert!(
                !handle_login(
                    &config,
                    &event_tx,
                    &mut state,
                    login_request(&mock_server, true)
                )
                .await
            );
            let events = settled_events(&event_rx).await;
            assert!(
                events.iter().any(|event| matches!(
                    event,
                    Event::Error(error) if error.operation == Operation::Login
                )),
                "{events:?}"
            );
            assert!(
                events
                    .iter()
                    .any(|event| matches!(event, Event::SyncStopped(SyncStopReason::Lost(_)))),
                "{events:?}"
            );
            assert!(
                !events.iter().any(|event| matches!(
                    event,
                    Event::SyncedRooms(_) | Event::HighlightWords(_) | Event::SyncConnected
                )),
                "{events:?}"
            );

            drop(failing_sync);
            let room_id = room_id();
            mock_server
                .mock_sync()
                .ok(|builder| {
                    builder.add_joined_room(
                        JoinedRoomBuilder::new(room_id.as_ref())
                            .add_state_bulk(logged_in_user_room_members()),
                    );
                })
                .mount()
                .await;
            assert!(!handle_start_sync(&event_tx, &mut state));
            let events = settled_events(&event_rx).await;
            let position = |name: &str, matches: &dyn Fn(&Event) -> bool| {
                event_position(&events, matches).unwrap_or_else(|| panic!("{name} in {events:?}"))
            };
            let sync_started = position("SyncStarted", &|event| *event == Event::SyncStarted);
            let synced_rooms = position("SyncedRooms", &|event| {
                matches!(event, Event::SyncedRooms(rooms)
                    if rooms.iter().any(|room| room.room_id == room_id.as_str()))
            });
            let highlight_words = position("HighlightWords", &|event| {
                matches!(event, Event::HighlightWords(_))
            });
            let ignored_users = position("IgnoredUsers", &|event| {
                matches!(event, Event::IgnoredUsers(_))
            });
            let room_members = position(
                "RoomMembers",
                &|event| matches!(event, Event::RoomMembers(members) if members.room_id == room_id.as_str()),
            );
            let sync_connected = position("SyncConnected", &|event| *event == Event::SyncConnected);
            assert!(
                sync_started < synced_rooms
                    && synced_rooms < highlight_words
                    && highlight_words < ignored_users
                    && ignored_users < room_members
                    && room_members < sync_connected,
                "{events:?}"
            );

            assert!(!handle_stop_sync(&event_tx, &mut state));
            assert!(!handle_start_sync(&event_tx, &mut state));
            let events = settled_events(&event_rx).await;
            assert!(events.contains(&Event::SyncConnected), "{events:?}");
            assert!(
                !events.iter().any(|event| matches!(
                    event,
                    Event::HighlightWords(_) | Event::IgnoredUsers(_)
                )),
                "{events:?}"
            );
            shutdown_state(&mut state).await;
        });
    }

    /// A signed-in session (no sync yet) in a joined room, whose sends
    /// `send` answers.
    async fn session_in_a_room(
        mock_server: &MatrixMockServer,
        config: &CoreConfig,
        event_tx: &EventSender,
    ) -> WorkerState {
        mock_server.mock_versions().ok().mount().await;
        mock_server.mock_login().ok().mount().await;
        mock_server
            .mock_room_state_encryption()
            .expect_any_access_token()
            .plain()
            .mount()
            .await;
        mock_server
            .mock_get_members()
            .expect_any_access_token()
            .ok(logged_in_user_room_members())
            .mount()
            .await;
        mock_server
            .mock_sync()
            .ok(|builder| {
                builder.add_joined_room(
                    JoinedRoomBuilder::new(room_id().as_ref())
                        .add_state_bulk(logged_in_user_room_members()),
                );
            })
            .mount()
            .await;
        let mut state = WorkerState::NoClient;
        assert!(
            !handle_login(
                config,
                event_tx,
                &mut state,
                login_request(mock_server, false)
            )
            .await
        );
        state
    }

    fn send_line(body: &str) -> Command {
        Command::SendMessage(SendMessageRequest {
            room_id: room_id().to_string(),
            presentation: RoomMessagePresentationKind::Text,
            body: body.to_owned(),
            styles: Vec::new(),
            thread: None,
            reply: None,
        })
    }

    /// Waits for the event `matches` picks, collecting every event until
    /// then.
    async fn events_until(
        event_rx: &mpsc::Receiver<Event>,
        matches: impl Fn(&Event) -> bool,
    ) -> Vec<Event> {
        let mut events = Vec::new();
        loop {
            let event = next_event(event_rx).await;
            let done = matches(&event);
            events.push(event);
            if done {
                return events;
            }
        }
    }

    fn message_update(event: &Event) -> Option<(&str, &RoomActivityUpdateKind)> {
        match event {
            Event::RoomActivityUpdate(RoomActivityUpdate {
                target: RoomActivityUpdateTarget::Transaction { transaction_id },
                kind,
                ..
            }) => Some((transaction_id, kind)),
            _ => None,
        }
    }

    fn local_echo_transaction(events: &[Event], body: &str) -> String {
        events
            .iter()
            .find_map(|event| match event {
                Event::RoomActivity(activity) => match &activity.kind {
                    RoomActivityKind::Message(message) if message.body == body => {
                        message.transaction_id.clone()
                    }
                    _ => None,
                },
                _ => None,
            })
            .unwrap_or_else(|| panic!("the local echo of {body:?} in {events:?}"))
    }

    /// A line typed while the server is unreachable waits in the queue, is
    /// no error, and goes by itself after the next sync response.
    #[test]
    fn a_send_the_network_stopped_goes_after_the_next_sync_response() {
        run_local(async {
            let mock_server = MatrixMockServer::new().await;
            let profile = tempfile::TempDir::new().expect("temporary profile directory");
            let config = CoreConfig {
                profile_dir: profile.path().join("profile"),
                store_passphrase: "test-store-passphrase".to_owned(),
            };
            let (event_tx, event_rx) = mpsc::channel::<Event>();
            let mut state = session_in_a_room(&mock_server, &config, &event_tx).await;
            // The session's own events, the E2EE status's included.
            settled_events(&event_rx).await;
            let unavailable = mock_server
                .mock_room_send()
                .respond_with(wiremock::ResponseTemplate::new(503))
                .mount_as_scoped()
                .await;

            assert!(!handle_command(send_line("hello"), &config, &event_tx, &mut state).await);
            let events = events_until(&event_rx, |event| {
                matches!(
                    message_update(event),
                    Some((_, RoomActivityUpdateKind::MessageFailed { .. }))
                )
            })
            .await;
            let transaction_id = local_echo_transaction(&events, "hello");
            assert!(
                matches!(
                    message_update(events.last().expect("the failure")),
                    Some((id, RoomActivityUpdateKind::MessageFailed { recoverable: true, .. }))
                        if id == transaction_id
                ),
                "{events:?}"
            );
            assert!(
                !events.iter().any(|event| matches!(event, Event::Error(_))),
                "{events:?}"
            );

            drop(unavailable);
            mock_server
                .mock_room_send()
                .ok(OwnedEventId::try_from("$sent:example.org").expect("valid event id"))
                .mount()
                .await;
            // Nothing resumes the queue but a sync response.
            let events = settled_events(&event_rx).await;
            assert!(
                !events.iter().any(|event| message_update(event).is_some()),
                "{events:?}"
            );

            assert!(!handle_start_sync(&event_tx, &mut state));
            let events = events_until(&event_rx, |event| message_update(event).is_some()).await;
            assert_eq!(
                message_update(events.last().expect("the send")),
                Some((
                    transaction_id.as_str(),
                    &RoomActivityUpdateKind::MessageSent {
                        event_id: "$sent:example.org".to_owned()
                    }
                )),
                "{events:?}"
            );
            shutdown_state(&mut state).await;
        });
    }

    /// A send the server refused is final, and does not hold up the room's
    /// next line.
    #[test]
    fn a_refused_send_fails_and_the_next_line_goes() {
        run_local(async {
            let mock_server = MatrixMockServer::new().await;
            let profile = tempfile::TempDir::new().expect("temporary profile directory");
            let config = CoreConfig {
                profile_dir: profile.path().join("profile"),
                store_passphrase: "test-store-passphrase".to_owned(),
            };
            let (event_tx, event_rx) = mpsc::channel::<Event>();
            let mut state = session_in_a_room(&mock_server, &config, &event_tx).await;
            // The session's own events, the E2EE status's included.
            settled_events(&event_rx).await;
            let refused = mock_server
                .mock_room_send()
                .respond_with(wiremock::ResponseTemplate::new(403).set_body_json(
                    serde_json::json!({ "errcode": "M_FORBIDDEN", "error": "not here" }),
                ))
                .mount_as_scoped()
                .await;

            assert!(!handle_command(send_line("refused"), &config, &event_tx, &mut state).await);
            let events = events_until(&event_rx, |event| matches!(event, Event::Error(_))).await;
            let transaction_id = local_echo_transaction(&events, "refused");
            assert!(
                events.iter().any(|event| matches!(
                    message_update(event),
                    Some((id, RoomActivityUpdateKind::MessageFailed { recoverable: false, .. }))
                        if id == transaction_id
                )),
                "{events:?}"
            );
            assert!(
                matches!(
                    events.last(),
                    Some(Event::Error(error)) if error.operation == Operation::SendMessage
                ),
                "{events:?}"
            );

            drop(refused);
            mock_server
                .mock_room_send()
                .ok(OwnedEventId::try_from("$next:example.org").expect("valid event id"))
                .mount()
                .await;
            assert!(!handle_command(send_line("next"), &config, &event_tx, &mut state).await);
            let events = events_until(&event_rx, |event| {
                matches!(
                    message_update(event),
                    Some((_, RoomActivityUpdateKind::MessageSent { .. }))
                )
            })
            .await;
            let next = local_echo_transaction(&events, "next");
            assert_eq!(
                message_update(events.last().expect("the send")),
                Some((
                    next.as_str(),
                    &RoomActivityUpdateKind::MessageSent {
                        event_id: "$next:example.org".to_owned()
                    }
                )),
                "{events:?}"
            );
            shutdown_state(&mut state).await;
        });
    }

    /// The SDK's event cache counts unread messages, those of the session's
    /// first sync included, and keeps the counts in its store
    /// (`RoomInfo.read_receipts`): a restored session reports them before
    /// any new event arrives, so the treebar and switchbar colours come back
    /// after a restart.
    #[test]
    fn restore_reports_stored_unread_counts_without_new_events() {
        run_local(async {
            let mock_server = MatrixMockServer::new().await;
            mock_server.mock_versions().ok().mount().await;
            mock_server.mock_login().ok().mount().await;
            mock_server
                .mock_room_state_encryption()
                .expect_any_access_token()
                .plain()
                .mount()
                .await;
            mock_server
                .mock_get_members()
                .expect_any_access_token()
                .ok(logged_in_user_room_members())
                .mount()
                .await;
            let room_id = room_id();
            let first_sync = mock_server
                .mock_sync()
                .ok(|builder| {
                    builder.add_joined_room(
                        JoinedRoomBuilder::new(room_id.as_ref())
                            .add_state_bulk(logged_in_user_room_members())
                            .add_timeline_event(bob_message("$one:example.org"))
                            .add_timeline_event(bob_message("$two:example.org")),
                    );
                })
                .mount_as_scoped()
                .await;
            let profile = tempfile::TempDir::new().expect("temporary profile directory");
            let config = CoreConfig {
                profile_dir: profile.path().join("profile"),
                store_passphrase: "test-store-passphrase".to_owned(),
            };
            let (event_tx, event_rx) = mpsc::channel::<Event>();
            let mut state = WorkerState::NoClient;
            assert!(
                !handle_login(
                    &config,
                    &event_tx,
                    &mut state,
                    login_request(&mock_server, false)
                )
                .await
            );
            let events = settled_events(&event_rx).await;
            let stored = (*read_states_of(&events, room_id.as_str())
                .last()
                .unwrap_or_else(|| panic!("RoomReadState in {events:?}")))
            .clone();
            assert_eq!(
                (
                    stored.num_unread,
                    stored.num_notifications,
                    stored.num_mentions
                ),
                (2, 2, 0),
                "{events:?}"
            );
            let (session, homeserver_url) = events
                .iter()
                .find_map(|event| match event {
                    Event::LoggedIn {
                        session,
                        homeserver_url,
                    } => Some((session.clone(), homeserver_url.clone())),
                    _ => None,
                })
                .expect("LoggedIn");
            shutdown_state(&mut state).await;
            drop(first_sync);

            mock_server
                .mock_sync()
                .ok(|builder| {
                    builder.add_joined_room(JoinedRoomBuilder::new(room_id.as_ref()));
                })
                .mount()
                .await;
            assert!(
                !handle_restore_session(
                    &config,
                    &event_tx,
                    &mut state,
                    RestoreSessionRequest {
                        homeserver_url,
                        session,
                        start_sync: false,
                    },
                )
                .await
            );
            let events = settled_events(&event_rx).await;
            let synced_rooms =
                event_position(&events, |event| matches!(event, Event::SyncedRooms(_)))
                    .unwrap_or_else(|| panic!("SyncedRooms in {events:?}"));
            let restored = read_states_of(&events[synced_rooms..], room_id.as_str());
            assert!(!restored.is_empty(), "{events:?}");
            assert!(
                restored.iter().all(|read_state| **read_state == stored),
                "{events:?}"
            );
            shutdown_state(&mut state).await;
        });
    }
}
