//! Signing in and restoring: the steps that bring a session up, as
//! futures that spawn nothing and announce nothing. Dropping one, at any
//! await, leaves nothing behind but the client it built; what it found it
//! returns, and the actor applies it.

use futures_util::future::LocalBoxFuture;
use matrix_sdk::Client;

use super::session::EnteredSession;
use super::{EventSender, RoomSnapshot, RoomSnapshots};
use crate::api::{CoreConfig, Event, LoginMethod, LoginRequest, Operation, RestoreSessionRequest};
use crate::loopback::{self, LoopbackListener, LoopbackRedirect};
use crate::matrix::{self, BackupServerStatusMode, BrowserFlow, HomeserverSpec, PausedSendRooms};

/// How a sign-in or restore ended.
pub(super) enum SignInEnd {
    /// Signed in: the session, read and ready to start.
    Entered(Box<EnteredSession>),
    /// The first half of a browser sign-in: the page is ready, and the
    /// listener waits for the browser to come back.
    BrowserReady(Box<BrowserReady>),
    /// Failed before there was a session: this operation's error.
    Failed(Operation, String),
}

pub(super) struct BrowserReady {
    pub(super) client: Client,
    /// The snapshots of the client's event handlers.
    pub(super) snapshots: RoomSnapshots,
    pub(super) flow: BrowserFlow,
    pub(super) listener: LoopbackListener,
    /// The page to open in the browser.
    pub(super) url: String,
    pub(super) start_sync: bool,
}

/// A `Login`: a password sign-in up to the session, or a browser sign-in
/// up to the page the browser opens.
pub(super) async fn log_in(
    config: CoreConfig,
    events: EventSender,
    request: LoginRequest,
) -> SignInEnd {
    let client =
        match matrix::build_client(&config, HomeserverSpec::Discover(&request.server)).await {
            Ok(client) => client,
            Err(err) => return SignInEnd::Failed(Operation::Login, err.to_string()),
        };
    // Before the session exists: signing in starts E2EE.
    let snapshots = RoomSnapshots::default();
    matrix::register_event_handlers(&client, events.clone(), &snapshots);

    match request.method {
        LoginMethod::Password { username, password } => {
            match matrix::login(&client, &username, &password, &request.device_display_name).await {
                Ok(session) => {
                    let signed_in = Event::LoggedIn {
                        session,
                        homeserver_url: client.homeserver().to_string(),
                    };
                    let entered = read_session(
                        client,
                        signed_in,
                        Operation::Login,
                        events,
                        snapshots,
                        request.start_sync,
                    )
                    .await;
                    SignInEnd::Entered(Box::new(entered))
                }
                Err(err) => SignInEnd::Failed(Operation::Login, err.to_string()),
            }
        }
        LoginMethod::Browser { user_id_hint } => {
            start_browser_login(
                client,
                snapshots,
                &request.server,
                user_id_hint.as_deref(),
                &request.device_display_name,
                request.start_sync,
            )
            .await
        }
    }
}

/// Opens the loopback listener and prepares the homeserver's sign-in
/// page.
async fn start_browser_login(
    client: Client,
    snapshots: RoomSnapshots,
    server: &str,
    user_id_hint: Option<&str>,
    device_display_name: &str,
    start_sync: bool,
) -> SignInEnd {
    let listener = match LoopbackListener::bind().await {
        Ok(listener) => listener,
        Err(err) => {
            return SignInEnd::Failed(
                Operation::Login,
                format!("failed to listen for the browser on 127.0.0.1: {err}"),
            );
        }
    };
    match matrix::start_browser_login(
        &client,
        server,
        listener.redirect_uri(),
        user_id_hint,
        device_display_name,
    )
    .await
    {
        Ok(sign_in) => SignInEnd::BrowserReady(Box::new(BrowserReady {
            client,
            snapshots,
            flow: sign_in.flow,
            listener,
            url: sign_in.url,
            start_sync,
        })),
        Err(err) => SignInEnd::Failed(Operation::Login, err.to_string()),
    }
}

/// The browser came back: completes the sign-in, tells the browser how it
/// went, and reads the session.
pub(super) async fn finish_browser_login(
    client: Client,
    snapshots: RoomSnapshots,
    flow: BrowserFlow,
    redirect: LoopbackRedirect,
    events: EventSender,
    start_sync: bool,
) -> SignInEnd {
    let LoopbackRedirect { mut stream, query } = redirect;
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
            let entered = read_session(
                client,
                signed_in,
                Operation::Login,
                events,
                snapshots,
                start_sync,
            )
            .await;
            SignInEnd::Entered(Box::new(entered))
        }
        Err(err) => SignInEnd::Failed(Operation::Login, err.to_string()),
    }
}

/// A `RestoreSession`: the saved session, up to the session.
pub(super) async fn restore(
    config: CoreConfig,
    events: EventSender,
    request: RestoreSessionRequest,
) -> SignInEnd {
    let client =
        match matrix::build_client(&config, HomeserverSpec::Url(&request.homeserver_url)).await {
            Ok(client) => client,
            Err(err) => return SignInEnd::Failed(Operation::RestoreSession, err.to_string()),
        };
    let snapshots = RoomSnapshots::default();
    matrix::register_event_handlers(&client, events.clone(), &snapshots);
    if let Err(err) = matrix::restore_session(&client, request.session).await {
        return SignInEnd::Failed(Operation::RestoreSession, err.to_string());
    }
    let entered = read_session(
        client,
        Event::RestoredSession,
        Operation::RestoreSession,
        events,
        snapshots,
        request.start_sync,
    )
    .await;
    SignInEnd::Entered(Box::new(entered))
}

/// Reads a signed-in client's session: the initial sync, the first room
/// snapshot and the E2EE status. Each update task is made (and so
/// subscribes) at the point where it must start listening; the session
/// spawns them. Initial sync failures are errors of `operation`; the
/// session stands either way, and the first sync task response emits the
/// snapshot instead.
async fn read_session(
    client: Client,
    signed_in: Event,
    operation: Operation,
    events: EventSender,
    snapshots: RoomSnapshots,
    start_sync: bool,
) -> EnteredSession {
    let current_user_id = client
        .user_id()
        .expect("a signed-in client knows its user")
        .to_string();
    let paused_sends = PausedSendRooms::default();
    let mut tasks: Vec<LocalBoxFuture<'static, ()>> = vec![
        Box::pin(matrix::session_update_task(client.clone(), events.clone())),
        Box::pin(matrix::send_queue_update_task(
            client.clone(),
            current_user_id,
            events.clone(),
            paused_sends.clone(),
        )),
    ];
    // Before the first sync: the event cache counts unread messages, here
    // those of the initial sync, and does so in its own task, so the
    // read-state task must already be listening when it is done.
    client
        .event_cache()
        .subscribe()
        .expect("the session's client is alive");
    // Its redecryptor starts with the subscription. The task subscribes to
    // its reports when first polled (the stream borrows the client the
    // task owns): poll it once now, so what the redecryptor decrypts
    // during the first sync (keys from the backup, say) is reported too.
    let mut decryption = Box::pin(matrix::room_decryption_task(client.clone(), events.clone()));
    assert!(
        futures_util::poll!(decryption.as_mut()).is_pending(),
        "the decryption task runs for the session"
    );
    tasks.push(decryption);
    tasks.push(Box::pin(matrix::room_read_state_update_task(
        client.clone(),
        events.clone(),
        client.room_info_notable_update_receiver(),
    )));
    let snapshot = match matrix::initial_room_sync(&client).await {
        Ok(()) => RoomSnapshot::read(&client, &snapshots).await,
        Err(err) => Err(err),
    };
    tasks.push(Box::pin(matrix::room_list_update_task(
        client.clone(),
        events.clone(),
        snapshots.clone(),
    )));
    // Subscribed before the first summary, so no change falls between.
    tasks.push(Box::pin(matrix::e2ee_session_state_task(
        client.clone(),
        events.clone(),
    )));
    let e2ee_status = matrix::summarize_e2ee_status(&client, BackupServerStatusMode::Fetch).await;
    tasks.push(Box::pin(matrix::e2ee_recovery_state_task(
        client.clone(),
        events.clone(),
    )));
    tasks.push(Box::pin(matrix::e2ee_backup_state_task(
        client.clone(),
        events.clone(),
    )));
    tasks.push(Box::pin(matrix::room_identity_task(
        client.clone(),
        events.clone(),
    )));
    EnteredSession {
        client,
        signed_in,
        operation,
        snapshot,
        e2ee_status,
        snapshots,
        paused_sends,
        tasks,
        start_sync,
    }
}
