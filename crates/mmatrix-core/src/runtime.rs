use std::collections::HashMap;
use std::error::Error;
use std::fmt::{self, Display, Formatter};
use std::sync::mpsc::{self, Receiver};
use std::thread::{self, JoinHandle};

use matrix_sdk::Client;
use tokio::runtime::Builder;
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel};
use tokio::task::{JoinHandle as TokioJoinHandle, LocalSet};

use crate::api::{
    AcceptRoomInviteRequest, BackfillRoomHistoryRequest, Command, CoreConfig, CoreErrorEvent,
    CreateDirectRoomRequest, E2eeRecoveryOperation, E2eeRecoverySucceeded, EditMessageRequest,
    Event, JoinRoomRequest, LeaveRoomRequest, LoginRequest, MarkRoomReadRequest, Operation,
    RecoverE2eeRequest, RedactEventRequest, RejectRoomInviteRequest, RestoreSessionRequest,
    RoomMembers, RoomMembership, RoomReadState, RoomSummary, SendReactionRequest,
    SetRoomTopicRequest, VerificationFlowId,
};
use crate::matrix::{self, BackupServerStatusMode, MatrixCoreError};

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
        room_list_update_task: TokioJoinHandle<()>,
        room_read_state_update_task: TokioJoinHandle<()>,
        e2ee_recovery_state_task: TokioJoinHandle<()>,
        e2ee_backup_state_task: TokioJoinHandle<()>,
        verification_tasks: HashMap<(String, String), TokioJoinHandle<()>>,
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
        Command::JoinRoom(request) => handle_join_room(event_tx, state, request).await,
        Command::AcceptRoomInvite(request) => {
            handle_accept_room_invite(event_tx, state, request).await
        }
        Command::CreateDirectRoom(request) => {
            handle_create_direct_room(event_tx, state, request).await
        }
        Command::LeaveRoom(request) => handle_leave_room(event_tx, state, request).await,
        Command::RejectRoomInvite(request) => {
            handle_reject_room_invite(event_tx, state, request).await
        }
        Command::SetRoomTopic(request) => handle_set_room_topic(event_tx, state, request).await,
        Command::SendMessage(request) => handle_send_message(event_tx, state, request).await,
        Command::EditMessage(request) => handle_edit_message(event_tx, state, request).await,
        Command::SendReaction(request) => handle_send_reaction(event_tx, state, request).await,
        Command::RedactEvent(request) => handle_redact_event(event_tx, state, request).await,
        Command::MarkRoomRead(request) => handle_mark_room_read(event_tx, state, request).await,
        Command::BackfillRoomHistory(request) => {
            handle_backfill_room_history(event_tx, state, request).await
        }
        Command::FetchRoomMedia(request) => handle_fetch_room_media(event_tx, state, request).await,
        Command::RefreshE2eeStatus => handle_refresh_e2ee_status(event_tx, state).await,
        Command::RecoverE2ee(request) => handle_recover_e2ee(event_tx, state, &request).await,
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
    let room_members = if rooms.is_ok() {
        Some(matrix::summarize_joined_room_members(&client).await)
    } else {
        None
    };
    let room_read_states = rooms
        .as_ref()
        .ok()
        .map(|_| matrix::summarize_joined_room_read_states(&client));
    emit_event(event_tx, Event::LoggedIn { session });
    match rooms {
        Ok(rooms) => emit_initial_synced_rooms_members_and_read_states(
            event_tx,
            rooms,
            room_members.expect("room members exist after successful sync"),
            room_read_states.expect("read states exist after successful sync"),
        ),
        Err(err) => emit_matrix_error(event_tx, Operation::Login, err),
    }
    let room_read_state_update_task =
        start_room_read_state_update_task_or_placeholder(&client, event_tx);
    let room_list_update_task = start_room_list_update_task_or_placeholder(&client, event_tx);
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
        sync_task: None,
        session_update_task,
        send_queue_update_task,
        room_list_update_task,
        room_read_state_update_task,
        e2ee_recovery_state_task,
        e2ee_backup_state_task,
        verification_tasks: HashMap::new(),
    };

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
    let room_members = if rooms.is_ok() {
        Some(matrix::summarize_joined_room_members(&client).await)
    } else {
        None
    };
    let room_read_states = rooms
        .as_ref()
        .ok()
        .map(|_| matrix::summarize_joined_room_read_states(&client));
    emit_event(event_tx, Event::RestoredSession);
    match rooms {
        Ok(rooms) => emit_initial_synced_rooms_members_and_read_states(
            event_tx,
            rooms,
            room_members.expect("room members exist after successful sync"),
            room_read_states.expect("read states exist after successful sync"),
        ),
        Err(err) => emit_matrix_error(event_tx, Operation::RestoreSession, err),
    }
    let room_read_state_update_task =
        start_room_read_state_update_task_or_placeholder(&client, event_tx);
    let room_list_update_task = start_room_list_update_task_or_placeholder(&client, event_tx);
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
        sync_task: None,
        session_update_task,
        send_queue_update_task,
        room_list_update_task,
        room_read_state_update_task,
        e2ee_recovery_state_task,
        e2ee_backup_state_task,
        verification_tasks: HashMap::new(),
    };

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

    if let Err(err) = matrix::join_room(client, &request.room_or_alias).await {
        emit_matrix_error(event_tx, Operation::JoinRoom, err);
        return false;
    }

    if let Err(err) = matrix::emit_room_snapshot(client, event_tx).await {
        emit_matrix_error(event_tx, Operation::JoinRoom, err);
    }

    false
}

async fn handle_accept_room_invite(
    event_tx: &EventSender,
    state: &mut WorkerState,
    request: AcceptRoomInviteRequest,
) -> bool {
    let WorkerState::Ready { client, .. } = state else {
        emit_invalid_state(
            event_tx,
            Operation::AcceptRoomInvite,
            "accept room invitation requires an authenticated session",
        );
        return false;
    };

    if let Err(err) = matrix::accept_room_invite(client, &request.room_id).await {
        emit_matrix_error(event_tx, Operation::AcceptRoomInvite, err);
        return false;
    }

    if let Err(err) = matrix::emit_room_snapshot(client, event_tx).await {
        emit_matrix_error(event_tx, Operation::AcceptRoomInvite, err);
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

    if let Err(err) = matrix::create_direct_room(client, &request.user_id).await {
        emit_matrix_error(event_tx, Operation::CreateDirectRoom, err);
        return false;
    }

    if let Err(err) = matrix::emit_room_snapshot(client, event_tx).await {
        emit_matrix_error(event_tx, Operation::CreateDirectRoom, err);
    }

    false
}

async fn handle_leave_room(
    event_tx: &EventSender,
    state: &mut WorkerState,
    request: LeaveRoomRequest,
) -> bool {
    let WorkerState::Ready { client, .. } = state else {
        emit_invalid_state(
            event_tx,
            Operation::LeaveRoom,
            "leave room requires an authenticated session",
        );
        return false;
    };

    if let Err(err) = matrix::leave_joined_room(client, &request.room_id).await {
        emit_matrix_error(event_tx, Operation::LeaveRoom, err);
        return false;
    }

    if let Err(err) = matrix::emit_room_snapshot(client, event_tx).await {
        emit_matrix_error(event_tx, Operation::LeaveRoom, err);
    }

    false
}

async fn handle_reject_room_invite(
    event_tx: &EventSender,
    state: &mut WorkerState,
    request: RejectRoomInviteRequest,
) -> bool {
    let WorkerState::Ready { client, .. } = state else {
        emit_invalid_state(
            event_tx,
            Operation::RejectRoomInvite,
            "reject room invitation requires an authenticated session",
        );
        return false;
    };

    if let Err(err) = matrix::reject_room_invite(client, &request.room_id).await {
        emit_matrix_error(event_tx, Operation::RejectRoomInvite, err);
        return false;
    }

    if let Err(err) = matrix::emit_room_snapshot(client, event_tx).await {
        emit_matrix_error(event_tx, Operation::RejectRoomInvite, err);
    }

    false
}

async fn handle_send_message(
    event_tx: &EventSender,
    state: &mut WorkerState,
    request: crate::api::SendMessageRequest,
) -> bool {
    let WorkerState::Ready { client, .. } = state else {
        emit_invalid_state(
            event_tx,
            Operation::SendMessage,
            "send message requires an authenticated session",
        );
        return false;
    };

    if let Err(err) = matrix::send_message(client, request).await {
        emit_matrix_error(event_tx, Operation::SendMessage, err);
    }

    false
}

async fn handle_send_reaction(
    event_tx: &EventSender,
    state: &mut WorkerState,
    request: SendReactionRequest,
) -> bool {
    let WorkerState::Ready { client, .. } = state else {
        emit_invalid_state(
            event_tx,
            Operation::SendReaction,
            "send reaction requires an authenticated session",
        );
        return false;
    };

    if let Err(err) = matrix::send_reaction(client, request).await {
        emit_matrix_error(event_tx, Operation::SendReaction, err);
    }

    false
}

async fn handle_edit_message(
    event_tx: &EventSender,
    state: &mut WorkerState,
    request: EditMessageRequest,
) -> bool {
    let WorkerState::Ready { client, .. } = state else {
        emit_invalid_state(
            event_tx,
            Operation::EditMessage,
            "edit message requires an authenticated session",
        );
        return false;
    };

    if let Err(err) = matrix::edit_message(client, request).await {
        emit_matrix_error(event_tx, Operation::EditMessage, err);
    }

    false
}

async fn handle_redact_event(
    event_tx: &EventSender,
    state: &mut WorkerState,
    request: RedactEventRequest,
) -> bool {
    let WorkerState::Ready { client, .. } = state else {
        emit_invalid_state(
            event_tx,
            Operation::RedactEvent,
            "redact event requires an authenticated session",
        );
        return false;
    };

    if let Err(err) = matrix::redact_event(client, request).await {
        emit_matrix_error(event_tx, Operation::RedactEvent, err);
    }

    false
}

async fn handle_set_room_topic(
    event_tx: &EventSender,
    state: &mut WorkerState,
    request: SetRoomTopicRequest,
) -> bool {
    let WorkerState::Ready { client, .. } = state else {
        emit_invalid_state(
            event_tx,
            Operation::SetRoomTopic,
            "set room topic requires an authenticated session",
        );
        return false;
    };

    if let Err(err) = matrix::set_room_topic(client, request).await {
        emit_matrix_error(event_tx, Operation::SetRoomTopic, err);
    }

    false
}

async fn handle_mark_room_read(
    event_tx: &EventSender,
    state: &mut WorkerState,
    request: MarkRoomReadRequest,
) -> bool {
    let WorkerState::Ready { client, .. } = state else {
        emit_invalid_state(
            event_tx,
            Operation::MarkRoomRead,
            "mark room read requires an authenticated session",
        );
        return false;
    };

    if let Err(err) = matrix::mark_room_read(client, &request).await {
        emit_matrix_error(event_tx, Operation::MarkRoomRead, err);
    }

    false
}

async fn handle_backfill_room_history(
    event_tx: &EventSender,
    state: &mut WorkerState,
    request: BackfillRoomHistoryRequest,
) -> bool {
    let WorkerState::Ready { client, .. } = state else {
        emit_invalid_state(
            event_tx,
            Operation::BackfillRoomHistory,
            "backfill room history requires an authenticated session",
        );
        return false;
    };

    match matrix::backfill_room_history(client, request).await {
        Ok(backfill) => emit_event(event_tx, Event::RoomHistoryBackfill(backfill)),
        Err(err) => emit_matrix_error(event_tx, Operation::BackfillRoomHistory, err),
    }

    false
}

async fn handle_fetch_room_media(
    event_tx: &EventSender,
    state: &mut WorkerState,
    request: crate::api::FetchRoomMediaRequest,
) -> bool {
    let WorkerState::Ready { client, .. } = state else {
        emit_invalid_state(
            event_tx,
            Operation::FetchRoomMedia,
            "fetch room media requires an authenticated session",
        );
        return false;
    };

    let media = matrix::fetch_room_media(client, request).await;
    emit_event(event_tx, Event::RoomMedia(media));

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

fn shutdown_state(state: &mut WorkerState) {
    if let WorkerState::Ready {
        sync_task,
        session_update_task,
        send_queue_update_task,
        room_list_update_task,
        room_read_state_update_task,
        e2ee_recovery_state_task,
        e2ee_backup_state_task,
        verification_tasks,
        ..
    } = state
    {
        abort_sync_task(sync_task);
        session_update_task.abort();
        send_queue_update_task.abort();
        room_list_update_task.abort();
        room_read_state_update_task.abort();
        e2ee_recovery_state_task.abort();
        e2ee_backup_state_task.abort();
        abort_verification_tasks(verification_tasks);
    }

    *state = WorkerState::NoClient;
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

    let task = if let Ok(request) = matrix::lookup_verification_request(client, flow).await {
        tokio::task::spawn_local(matrix::watch_verification_request(
            request,
            flow.clone(),
            event_tx.clone(),
        ))
    } else {
        let Ok(sas) = matrix::lookup_sas_verification(client, flow).await else {
            return;
        };
        tokio::task::spawn_local(matrix::watch_sas_verification(
            sas,
            flow.clone(),
            event_tx.clone(),
        ))
    };

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

fn emit_initial_synced_rooms_members_and_read_states(
    event_tx: &EventSender,
    rooms: Vec<RoomSummary>,
    room_members: Vec<RoomMembers>,
    room_read_states: Vec<RoomReadState>,
) {
    let joined_room_ids = rooms
        .iter()
        .filter(|room| room.membership == RoomMembership::Join)
        .map(|room| room.room_id.clone())
        .collect::<std::collections::HashSet<_>>();

    emit_event(event_tx, Event::SyncedRooms(rooms));
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

fn start_room_read_state_update_task_or_placeholder(
    client: &Client,
    event_tx: &EventSender,
) -> TokioJoinHandle<()> {
    match matrix::start_room_read_state_update_task(client.clone(), event_tx.clone()) {
        Ok(task) => task,
        Err(err) => {
            emit_event(
                event_tx,
                Event::Error(CoreErrorEvent {
                    operation: Operation::RoomReadState,
                    message: format!(
                        "failed to subscribe Matrix event cache for room read-state updates: {err}"
                    ),
                }),
            );
            tokio::task::spawn_local(async {})
        }
    }
}

fn start_room_list_update_task_or_placeholder(
    client: &Client,
    event_tx: &EventSender,
) -> TokioJoinHandle<()> {
    match matrix::start_room_list_update_task(client.clone(), event_tx.clone()) {
        Ok(task) => task,
        Err(err) => {
            emit_event(
                event_tx,
                Event::Error(CoreErrorEvent {
                    operation: Operation::RoomList,
                    message: format!(
                        "failed to subscribe Matrix event cache for room-list updates: {err}"
                    ),
                }),
            );
            tokio::task::spawn_local(async {})
        }
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
    use std::collections::HashMap;
    use std::sync::mpsc::{self, TryRecvError};
    use std::time::Duration;

    use tokio::sync::mpsc::unbounded_channel;

    use super::{
        EventSender, WorkerState, emit_initial_synced_rooms_members_and_read_states,
        handle_start_sync, run_worker, shutdown_state,
    };
    use crate::api::CoreConfig;
    use crate::api::{
        Event, Operation, RoomMemberSummary, RoomMembers, RoomMembership, RoomReadState,
        RoomSummary,
    };

    fn room_summary(room_id: &str, display_name: &str, membership: RoomMembership) -> RoomSummary {
        RoomSummary {
            room_id: room_id.to_owned(),
            display_name: display_name.to_owned(),
            membership,
            encrypted: true,
            is_space: false,
            is_direct: false,
            direct_user_ids: Vec::new(),
            tags: Vec::new(),
            space_children: Vec::new(),
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

        emit_initial_synced_rooms_members_and_read_states(
            &event_tx as &EventSender,
            rooms.clone(),
            room_members.clone(),
            read_states.clone(),
        );

        assert_eq!(
            event_rx
                .recv_timeout(Duration::from_secs(2))
                .expect("synced rooms event"),
            Event::SyncedRooms(rooms)
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
            "initial snapshot must only emit synced rooms, room members, and room read states"
        );
    }

    #[test]
    fn shutdown_state_aborts_verification_watchers() {
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
            let verification_task = tokio::task::spawn_local(std::future::pending::<()>());
            let mut state = WorkerState::Ready {
                client,
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

            shutdown_state(&mut state);
            tokio::task::yield_now().await;

            assert!(matches!(state, WorkerState::NoClient));
        });
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
                profile_dir: std::env::temp_dir().join("mmatrix-core-runtime-test"),
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
}
