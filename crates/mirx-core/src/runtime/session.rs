//! A signed-in session: its client, its background tasks (sync, the
//! update tasks, an identity reset waiting for approval), and what jobs
//! take from it.

use std::rc::Rc;

use futures_util::future::LocalBoxFuture;
use matrix_sdk::Client;
use matrix_sdk::encryption::recovery::{IdentityResetHandle, RecoveryError};
use matrix_sdk::ruma::api::client::uiaa::AuthData;
use tokio::task::{JoinHandle, JoinSet};

use super::{
    E2eeStatusWatch, EventSender, FirstContact, FirstE2eeStatus, RoomSnapshot, RoomSnapshots,
    emit_event, emit_matrix_error,
};
use crate::api::{Event, Operation};
use crate::matrix::{self, AwayFlag, MatrixCoreError, PausedSendRooms, VerificationWatch};

/// What a job takes from the session: it owns clones, never the session.
#[derive(Clone)]
pub(super) struct JobCtx {
    pub(super) client: Client,
    pub(super) events: EventSender,
    pub(super) snapshots: RoomSnapshots,
    /// Told a command's answer that asked the server.
    pub(super) e2ee_status: E2eeStatusWatch,
    /// Hands the flows this session asks for to the verification task.
    pub(super) verifications: VerificationWatch,
}

/// What a job leaves for the actor to apply to the session, before the
/// job's ack.
pub(super) enum Followup {
    Done,
    /// The server asked for authentication: run the reset beside the actor
    /// until it ends.
    IdentityResetStarted(Box<StartedReset>),
}

/// A reset waiting for the server, and how to answer its authentication.
pub(super) struct StartedReset {
    pub(super) handle: IdentityResetHandle,
    pub(super) auth: Option<AuthData>,
}

/// A session just signed in or restored, read from the store or the
/// server, with nothing announced and nothing spawned yet.
pub(super) struct EnteredSession {
    pub(super) client: Client,
    /// `LoggedIn{..}` or `RestoredSession`.
    pub(super) signed_in: Event,
    /// `Login` or `RestoreSession`: the operation of the snapshot's error.
    pub(super) operation: Operation,
    /// An error if the initial sync failed (offline, or the server is
    /// down) or the snapshot could not be read: the first sync that gets
    /// through emits the snapshot.
    pub(super) snapshot: Result<RoomSnapshot, MatrixCoreError>,
    /// What the server said, if the entry asked it (it synced); `None`:
    /// the first sync that gets through asks.
    pub(super) e2ee_status: Option<FirstE2eeStatus>,
    /// Watched by the update tasks, which tell its changes once the
    /// session's first status is out.
    pub(super) e2ee_status_watch: E2eeStatusWatch,
    pub(super) snapshots: RoomSnapshots,
    pub(super) verifications: VerificationWatch,
    pub(super) paused_sends: PausedSendRooms,
    /// The update tasks, which subscribed to what they follow when they
    /// were made, so they miss nothing between then and their spawn.
    pub(super) tasks: Vec<LocalBoxFuture<'static, ()>>,
    pub(super) start_sync: bool,
}

/// An identity reset waiting for the server to accept its cross-signing
/// upload: approval in the browser, or the password checked.
pub(super) struct PendingIdentityReset {
    pub(super) handle: Rc<IdentityResetHandle>,
    /// `handle.reset(..)`: it asks every half second for up to two
    /// minutes, so it runs beside the actor.
    task: JoinHandle<Result<(), RecoveryError>>,
    /// `CancelE2eeIdentityReset` was asked. The SDK then ends the task with
    /// `Ok` without uploading, unless an upload just got through: only the
    /// server knows which (see `matrix::settle_identity_reset`).
    pub(super) cancel_requested: bool,
}

/// How the attempt to upload a reset's new identity ended.
pub(super) enum IdentityResetEnd {
    /// The server took the new identity.
    Uploaded,
    /// Stopped by `CancelE2eeIdentityReset`: no upload, unless the last
    /// one got through.
    Cancelled,
    /// Before the SDK asked the server for the upload.
    Failed(MatrixCoreError),
    /// The server's authentication was not passed: the reset had already
    /// turned the key backup and recovery off.
    Refused(MatrixCoreError),
}

pub(super) struct Session {
    ctx: JobCtx,
    /// Given to every sync task, which completes it at its first response.
    first_contact: FirstContact,
    /// Shared by the send queue's update task and every sync task.
    paused_sends: PausedSendRooms,
    /// For the session's life: it outlasts its sync tasks.
    away: AwayFlag,
    sync: Option<JoinHandle<()>>,
    background: JoinSet<()>,
    /// The one identity reset under way, between the server's request for
    /// authentication and its end.
    identity_reset: Option<PendingIdentityReset>,
}

impl Session {
    /// Announces the session: `signed_in`, the room snapshot or its error,
    /// and the E2EE status or its error if the entry asked the server.
    /// Then starts its update tasks, and sync if asked (`SyncStarted`).
    pub(super) fn start(entered: EnteredSession, events: &EventSender) -> Self {
        let EnteredSession {
            client,
            signed_in,
            operation,
            snapshot,
            e2ee_status,
            e2ee_status_watch,
            snapshots,
            verifications,
            paused_sends,
            tasks,
            start_sync,
        } = entered;
        emit_event(events, signed_in);
        let snapshot_pending = match snapshot {
            Ok(snapshot) => {
                snapshot.emit(&snapshots, events);
                false
            }
            Err(err) => {
                emit_matrix_error(events, operation, err);
                true
            }
        };
        let e2ee_status_pending = match e2ee_status {
            Some(first) => {
                e2ee_status_watch.report_first(events, first);
                false
            }
            None => true,
        };
        let first_contact = FirstContact::pending(
            snapshot_pending,
            e2ee_status_pending,
            e2ee_status_watch.clone(),
        );
        let mut background = JoinSet::new();
        for task in tasks {
            background.spawn_local(task);
        }
        let mut session = Self {
            ctx: JobCtx {
                client,
                events: events.clone(),
                snapshots,
                e2ee_status: e2ee_status_watch,
                verifications,
            },
            first_contact,
            paused_sends,
            away: AwayFlag::default(),
            sync: None,
            background,
            identity_reset: None,
        };
        if start_sync {
            session
                .start_sync()
                .expect("a new session does not sync yet");
        }
        session
    }

    /// A session of `client` with no update task but the verification
    /// task, for the tests of what commands do.
    #[cfg(test)]
    pub(super) fn for_client(client: Client, events: &EventSender) -> Self {
        let (verifications, new_flows) = matrix::verification_watch();
        let mut background = JoinSet::new();
        background.spawn_local(matrix::verification_task(new_flows, events.clone()));
        Self {
            ctx: JobCtx {
                client,
                events: events.clone(),
                snapshots: RoomSnapshots::default(),
                e2ee_status: E2eeStatusWatch::default(),
                verifications,
            },
            first_contact: FirstContact::pending(false, false, E2eeStatusWatch::default()),
            paused_sends: PausedSendRooms::default(),
            away: AwayFlag::default(),
            sync: None,
            background,
            identity_reset: None,
        }
    }

    /// Runs `task` as one of the session's background tasks.
    #[cfg(test)]
    pub(super) fn spawn_background(&mut self, task: impl Future<Output = ()> + 'static) {
        self.background.spawn_local(task);
    }

    pub(super) fn ctx(&self) -> JobCtx {
        self.ctx.clone()
    }

    /// Whether the user is away, which `SetAway` changes and every sync
    /// request says.
    pub(super) fn away(&self) -> AwayFlag {
        self.away.clone()
    }

    pub(super) fn client(&self) -> &Client {
        &self.ctx.client
    }

    pub(super) fn sync_running(&self) -> bool {
        self.sync.as_ref().is_some_and(|task| !task.is_finished())
    }

    /// Starts continuous sync, and emits `SyncStarted`.
    pub(super) fn start_sync(&mut self) -> Result<(), &'static str> {
        if self.sync_running() {
            return Err("continuous sync is already running");
        }
        self.sync = Some(tokio::task::spawn_local(matrix::sync_task(
            self.ctx.client.clone(),
            self.ctx.events.clone(),
            self.ctx.snapshots.clone(),
            self.first_contact.clone(),
            self.paused_sends.clone(),
            self.away.clone(),
        )));
        emit_event(&self.ctx.events, Event::SyncStarted);
        Ok(())
    }

    /// Stops continuous sync and keeps the session: mIRC's Disconnect, not
    /// a logout. The caller emits `SyncStopped`.
    pub(super) fn stop_sync(&mut self) -> Result<(), &'static str> {
        if !self.sync_running() {
            return Err("sync is not running");
        }
        self.sync.take().expect("sync runs").abort();
        Ok(())
    }

    /// Stops sync without a word and waits until it is gone; whether it
    /// was running.
    pub(super) async fn stop_sync_quietly(&mut self) -> bool {
        let running = self.sync_running();
        if let Some(task) = self.sync.take() {
            abort_and_join(task).await;
        }
        running
    }

    pub(super) fn identity_reset(&mut self) -> Option<&mut PendingIdentityReset> {
        self.identity_reset.as_mut()
    }

    /// Runs `handle.reset(auth)` beside the actor; `identity_reset_end`
    /// takes its result.
    pub(super) fn begin_identity_reset(&mut self, started: StartedReset) {
        let StartedReset { handle, auth } = started;
        assert!(
            self.identity_reset.is_none(),
            "the E2EE lane runs one reset at a time"
        );
        let handle = Rc::new(handle);
        let task = tokio::task::spawn_local({
            let handle = Rc::clone(&handle);
            async move { handle.reset(auth).await }
        });
        self.identity_reset = Some(PendingIdentityReset {
            handle,
            task,
            cancel_requested: false,
        });
    }

    /// Whether an identity reset waits for the server.
    pub(super) fn resetting(&self) -> bool {
        self.identity_reset.is_some()
    }

    /// Waits for the pending identity reset to end, then takes it.
    /// Cancel-safe: the reset stays pending until it ends.
    pub(super) async fn identity_reset_end(&mut self) -> IdentityResetEnd {
        let pending = self
            .identity_reset
            .as_mut()
            .expect("the actor waits only for a pending reset");
        let result = (&mut pending.task)
            .await
            .unwrap_or_else(|err| std::panic::resume_unwind(err.into_panic()));
        let pending = self.identity_reset.take().expect("the reset just ended");
        match result {
            Ok(()) if pending.cancel_requested => IdentityResetEnd::Cancelled,
            Ok(()) => IdentityResetEnd::Uploaded,
            Err(err) => IdentityResetEnd::Refused(matrix::identity_reset_failure(&err)),
        }
    }

    /// Stops every task of the session and waits until each is gone. The
    /// tasks own `Client` clones, and the SDK store closes its connections
    /// on the runtime when the last one drops. A task still queued when
    /// the core returns would instead be dropped with the `LocalSet`,
    /// outside the runtime, where that panics.
    pub(super) async fn shut_down(mut self) {
        if let Some(pending) = self.identity_reset.take() {
            abort_and_join(pending.task).await;
        }
        if let Some(task) = self.sync.take() {
            abort_and_join(task).await;
        }
        self.background.abort_all();
        while let Some(joined) = self.background.join_next().await {
            if let Err(err) = joined
                && err.is_panic()
            {
                std::panic::resume_unwind(err.into_panic());
            }
        }
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        // See `shut_down`.
        assert!(
            std::thread::panicking()
                || (self.identity_reset.is_none()
                    && self.sync.is_none()
                    && self.background.is_empty()),
            "a session dropped with its tasks running"
        );
    }
}

/// Aborts `task` and waits until it is gone; a panic in it goes on here.
pub(super) async fn abort_and_join<T>(task: JoinHandle<T>) {
    task.abort();
    if let Err(err) = task.await
        && err.is_panic()
    {
        std::panic::resume_unwind(err.into_panic());
    }
}
