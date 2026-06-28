use std::error::Error;
use std::fmt::{self, Display, Formatter};
use std::sync::mpsc::{self, Receiver};
use std::thread::{self, JoinHandle};

use matrix_sdk::Client;
use tokio::runtime::Builder;
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel};
use tokio::task::{JoinHandle as TokioJoinHandle, LocalSet};

use crate::api::{
    Command, CoreConfig, CoreErrorEvent, Event, LoginRequest, Operation, RestoreSessionRequest,
};
use crate::matrix::{self, MatrixCoreError};

pub(crate) type EventSender = mpsc::Sender<Event>;

#[derive(Debug, Clone)]
pub struct CommandSender {
    sender: UnboundedSender<Command>,
}

impl CommandSender {
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
        f.write_str("mmatrix-core command channel is closed")
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
            Self::ThreadSpawn(err) => write!(f, "failed to spawn mmatrix-core thread: {err}"),
            Self::RuntimeInitialization(message) => {
                write!(f, "failed to initialize mmatrix-core runtime: {message}")
            }
            Self::WorkerStopped => f.write_str("mmatrix-core worker stopped during startup"),
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
    Ready {
        client: Client,
        sync_task: Option<TokioJoinHandle<()>>,
        session_update_task: TokioJoinHandle<()>,
        send_queue_update_task: TokioJoinHandle<()>,
    },
}

pub fn start(config: CoreConfig) -> Result<CoreHandle, StartError> {
    let (command_tx, command_rx) = unbounded_channel();
    let (event_tx, event_rx) = mpsc::channel();
    let (startup_tx, startup_rx) = mpsc::channel();

    let worker = thread::Builder::new()
        .name("mmatrix-core".to_owned())
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
            command_sender: CommandSender { sender: command_tx },
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

    while let Some(command) = command_rx.recv().await {
        if handle_command(command, &config, &event_tx, &mut state).await {
            break;
        }
    }

    shutdown_state(&mut state);
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
        Command::RestoreSession(request) => {
            handle_restore_session(config, event_tx, state, request).await
        }
        Command::StartSync => handle_start_sync(event_tx, state),
        Command::SendText(request) => handle_send_text(event_tx, state, request).await,
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

    let client = match matrix::build_client(config, &request.homeserver_url).await {
        Ok(client) => client,
        Err(err) => {
            emit_matrix_error(event_tx, Operation::Login, err);
            return false;
        }
    };

    matrix::register_event_handlers(&client, event_tx.clone());

    let session = match matrix::login(&client, &request).await {
        Ok(session) => session,
        Err(err) => {
            emit_matrix_error(event_tx, Operation::Login, err);
            return false;
        }
    };

    client
        .send_queue()
        .respawn_tasks_for_rooms_with_unsent_requests()
        .await;
    let session_update_task = matrix::start_session_update_task(client.clone(), event_tx.clone());
    let send_queue_update_task = matrix::start_send_queue_update_task(
        client.clone(),
        session.user_id.clone(),
        event_tx.clone(),
    );
    let rooms = matrix::initial_room_sync(&client).await;

    *state = WorkerState::Ready {
        client,
        sync_task: None,
        session_update_task,
        send_queue_update_task,
    };
    emit_event(event_tx, Event::LoggedIn { session });

    match rooms {
        Ok(rooms) => emit_event(event_tx, Event::SyncedRooms(rooms)),
        Err(err) => emit_matrix_error(event_tx, Operation::Login, err),
    }

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

    let client = match matrix::build_client(config, &request.homeserver_url).await {
        Ok(client) => client,
        Err(err) => {
            emit_matrix_error(event_tx, Operation::RestoreSession, err);
            return false;
        }
    };

    matrix::register_event_handlers(&client, event_tx.clone());

    let current_user_id = request.session.user_id.clone();

    if let Err(err) = matrix::restore_session(&client, request.session).await {
        emit_matrix_error(event_tx, Operation::RestoreSession, err);
        return false;
    }

    client
        .send_queue()
        .respawn_tasks_for_rooms_with_unsent_requests()
        .await;
    let session_update_task = matrix::start_session_update_task(client.clone(), event_tx.clone());
    let send_queue_update_task =
        matrix::start_send_queue_update_task(client.clone(), current_user_id, event_tx.clone());
    let rooms = matrix::initial_room_sync(&client).await;

    *state = WorkerState::Ready {
        client,
        sync_task: None,
        session_update_task,
        send_queue_update_task,
    };
    emit_event(event_tx, Event::RestoredSession);

    match rooms {
        Ok(rooms) => emit_event(event_tx, Event::SyncedRooms(rooms)),
        Err(err) => emit_matrix_error(event_tx, Operation::RestoreSession, err),
    }

    false
}

fn handle_start_sync(event_tx: &EventSender, state: &mut WorkerState) -> bool {
    let WorkerState::Ready {
        client, sync_task, ..
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

    *sync_task = Some(matrix::start_sync_task(client.clone(), event_tx.clone()));
    emit_event(event_tx, Event::SyncStarted);
    false
}

async fn handle_send_text(
    event_tx: &EventSender,
    state: &mut WorkerState,
    request: crate::api::SendTextRequest,
) -> bool {
    let WorkerState::Ready { client, .. } = state else {
        emit_invalid_state(
            event_tx,
            Operation::SendText,
            "send text requires an authenticated session",
        );
        return false;
    };

    if let Err(err) = matrix::send_text(client, request).await {
        emit_matrix_error(event_tx, Operation::SendText, err);
    }

    false
}

async fn handle_logout(event_tx: &EventSender, state: &mut WorkerState) -> bool {
    let WorkerState::Ready {
        client,
        sync_task,
        session_update_task,
        send_queue_update_task,
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

    if let Err(err) = matrix::logout(client.clone()).await {
        emit_matrix_error(event_tx, Operation::Logout, err);
        return false;
    }

    *state = WorkerState::NoClient;
    emit_event(event_tx, Event::LoggedOut);
    false
}

fn shutdown_state(state: &mut WorkerState) {
    if let WorkerState::Ready {
        sync_task,
        session_update_task,
        send_queue_update_task,
        ..
    } = state
    {
        abort_sync_task(sync_task);
        session_update_task.abort();
        send_queue_update_task.abort();
    }

    *state = WorkerState::NoClient;
}

fn abort_sync_task(sync_task: &mut Option<TokioJoinHandle<()>>) {
    if let Some(task) = sync_task.take() {
        task.abort();
    }
}

pub(crate) fn emit_event(event_tx: &EventSender, event: Event) {
    let _ = event_tx.send(event);
}

fn emit_invalid_state(event_tx: &EventSender, operation: Operation, message: &str) {
    emit_event(
        event_tx,
        Event::Error(CoreErrorEvent {
            operation,
            message: message.to_owned(),
        }),
    );
}

fn emit_matrix_error(event_tx: &EventSender, operation: Operation, err: MatrixCoreError) {
    emit_event(
        event_tx,
        Event::Error(CoreErrorEvent {
            operation,
            message: err.to_string(),
        }),
    );
}

#[cfg(test)]
mod tests {
    use std::sync::mpsc;
    use std::time::Duration;

    use super::{EventSender, WorkerState, handle_start_sync};
    use crate::api::{Event, Operation};

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
                sync_task: Some(existing_task),
                session_update_task: tokio::task::spawn_local(std::future::pending::<()>()),
                send_queue_update_task: tokio::task::spawn_local(std::future::pending::<()>()),
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
                session_update_task,
                send_queue_update_task,
                ..
            } = &mut state
            {
                if let Some(task) = sync_task.take() {
                    task.abort();
                }
                session_update_task.abort();
                send_queue_update_task.abort();
            }
        });
    }
}
