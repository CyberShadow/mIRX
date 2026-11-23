use std::cell::Cell;
use std::error::Error;
use std::fmt::{self, Display, Formatter};
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver};
use std::thread::{self, JoinHandle};

use matrix_sdk::Client;
use matrix_sdk::ruma::{events::presence::PresenceEvent, serde::Raw};
use tokio::runtime::Builder;
use tokio::sync::mpsc::{UnboundedSender, unbounded_channel};
use tokio::task::LocalSet;

use crate::api::{
    Command, CommandId, CoreConfig, CoreErrorEvent, Event, HighlightWord, Operation, RoomMembers,
    RoomMembership, RoomReadState, RoomSummary,
};
use crate::matrix::{self, MatrixCoreError};

mod commands;
mod core;
mod e2ee_status;
mod jobs;
mod lifecycle;
mod session;
mod snapshots;
#[cfg(test)]
mod tests;

use self::core::Core;
pub(crate) use self::e2ee_status::E2eeStatusWatch;
use self::e2ee_status::FirstE2eeStatus;
pub(crate) use self::snapshots::RoomSnapshots;
use self::snapshots::SnapshotStart;

pub(crate) type EventSender = mpsc::Sender<Event>;

/// Sends commands to the core and numbers them; clones share the count.
/// The core answers each command with `Event::CommandHandled(id)` after
/// the command's own events, so `sent()` minus the acks applied is the
/// number of commands the core has not answered yet.
#[derive(Debug, Clone)]
pub struct CommandSender {
    sender: UnboundedSender<(CommandId, Command)>,
    sent: Arc<AtomicU64>,
}

impl CommandSender {
    pub fn new(sender: UnboundedSender<(CommandId, Command)>) -> Self {
        Self {
            sender,
            sent: Arc::new(AtomicU64::new(0)),
        }
    }

    /// Sends `command` and returns the ID its ack will name.
    pub fn send(&self, command: Command) -> Result<CommandId, SendCommandError> {
        // Counted before it is sent, so no reader ever sees the command's
        // ack before its count. A failed send means the core is gone: the
        // command stays unanswered, as it is.
        let id = CommandId(self.sent.fetch_add(1, Ordering::Relaxed) + 1);
        self.sender
            .send((id, command))
            .map(|()| id)
            .map_err(|_| SendCommandError)
    }

    /// How many commands were sent through this sender and its clones.
    pub fn sent(&self) -> u64 {
        self.sent.load(Ordering::Relaxed)
    }

    /// The ID the next `send` hands out, through this sender or a clone.
    pub fn next_id(&self) -> CommandId {
        CommandId(self.sent() + 1)
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
            local.block_on(&runtime, Core::new(config, event_tx).run(command_rx));
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

/// The account-wide lists the first room snapshot carries.
pub(super) struct AccountLists {
    highlight_words: Result<Vec<HighlightWord>, MatrixCoreError>,
    ignored_users: Result<Vec<String>, MatrixCoreError>,
}

impl AccountLists {
    pub(super) async fn read(client: &Client) -> Self {
        Self {
            highlight_words: matrix::highlight_words(client).await,
            ignored_users: matrix::ignored_users(client).await,
        }
    }
}

/// A session's first room snapshot: the room list, the account lists, and
/// every joined room's members and read state. Later changes come as they
/// happen.
pub(super) struct RoomSnapshot {
    start: SnapshotStart,
    rooms: Vec<RoomSummary>,
    room_members: Vec<RoomMembers>,
    /// The store's presence of `room_members`' users.
    presence: Vec<Raw<PresenceEvent>>,
    room_read_states: Vec<RoomReadState>,
    account_lists: AccountLists,
}

impl RoomSnapshot {
    /// From the store, which has the rooms of the last sync processed into
    /// it; it asks the server nothing for a room it has summarized before.
    pub(super) async fn read(
        client: &Client,
        snapshots: &RoomSnapshots,
    ) -> Result<Self, MatrixCoreError> {
        let room_members = matrix::summarize_joined_room_members(client).await?;
        Ok(Self {
            start: snapshots.start(),
            rooms: matrix::summarize_rooms(client).await?,
            presence: matrix::stored_presence(client, &room_members).await?,
            room_members,
            room_read_states: matrix::summarize_joined_room_read_states(client).await,
            account_lists: AccountLists::read(client).await,
        })
    }

    /// Emits the snapshot; its room list only if no later one was emitted.
    pub(super) fn emit(self, snapshots: &RoomSnapshots, event_tx: &EventSender) {
        let Self {
            start,
            rooms,
            room_members,
            presence,
            room_read_states,
            account_lists,
        } = self;
        let joined_room_ids = rooms
            .iter()
            .filter(|room| room.membership == RoomMembership::Join)
            .map(|room| room.room_id.clone())
            .collect::<std::collections::HashSet<_>>();

        if snapshots.supersede(start) {
            emit_event(event_tx, Event::SyncedRooms(rooms));
        }
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
        matrix::emit_presence_changes(event_tx, &presence);
        for read_state in room_read_states {
            if joined_room_ids.contains(&read_state.room_id) {
                emit_event(event_tx, Event::RoomReadState(read_state));
            }
        }
    }
}

/// What a session does once it first reaches the server, if its entry
/// did not: the first room snapshot, if its initial sync or reading it
/// failed; and the session's first E2EE status, which asks the server, if
/// the entry did not sync (it read a store that had synced, or the sync
/// failed). Shared by the session's sync tasks, one at a time.
#[derive(Clone)]
pub(crate) struct FirstContact(Rc<FirstContactDuties>);

struct FirstContactDuties {
    snapshot: Cell<bool>,
    e2ee_status: Cell<bool>,
    e2ee_status_watch: E2eeStatusWatch,
}

impl FirstContact {
    pub(super) fn pending(
        snapshot: bool,
        e2ee_status: bool,
        e2ee_status_watch: E2eeStatusWatch,
    ) -> Self {
        Self(Rc::new(FirstContactDuties {
            snapshot: Cell::new(snapshot),
            e2ee_status: Cell::new(e2ee_status),
            e2ee_status_watch,
        }))
    }

    /// At a sync task's first successful response: emits the snapshot if
    /// it is still to come, then the session's first E2EE status. Failing
    /// to read the snapshot leaves it to the next task. The E2EE status is
    /// asked once: the server answered the sync, so its error is the
    /// server's answer, and `RefreshE2eeStatus` asks again.
    pub(crate) async fn complete(
        &self,
        client: &Client,
        snapshots: &RoomSnapshots,
        event_tx: &EventSender,
    ) {
        if self.0.snapshot.get() {
            match RoomSnapshot::read(client, snapshots).await {
                Ok(snapshot) => {
                    snapshot.emit(snapshots, event_tx);
                    self.0.snapshot.set(false);
                }
                Err(err) => emit_matrix_error(event_tx, Operation::RoomList, err),
            }
        }
        if self.0.e2ee_status.replace(false) {
            let first = FirstE2eeStatus::read(client).await;
            self.0.e2ee_status_watch.report_first(event_tx, first);
        }
    }
}

pub(crate) fn emit_event(event_tx: &EventSender, event: Event) {
    let _ = event_tx.send(event);
}

pub(super) fn emit_error(
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
pub(super) fn emit_invalid_state(event_tx: &EventSender, operation: Operation, message: &str) {
    emit_error(event_tx, operation, None, message.to_owned());
}

/// An account-wide operation's failure.
pub(super) fn emit_matrix_error(
    event_tx: &EventSender,
    operation: Operation,
    err: MatrixCoreError,
) {
    emit_error(event_tx, operation, None, err.to_string());
}

/// The failure of a request about a room.
pub(super) fn emit_room_matrix_error(
    event_tx: &EventSender,
    operation: Operation,
    room_id: &str,
    err: MatrixCoreError,
) {
    emit_error(event_tx, operation, Some(room_id), err.to_string());
}
