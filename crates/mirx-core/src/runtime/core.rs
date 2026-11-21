//! The core's actor: it reads commands, routes them, and applies what the
//! jobs and the session's lifecycle bring back.
//!
//! In a session, a command runs as a job on its lane (`jobs`), beside the
//! actor. The actor answers every command with exactly one
//! `CommandHandled`, after the command's own events: an immediate command
//! after its handling, a lane command after its job's followup is applied,
//! and a command that a logout or shutdown drops when it drops it.

use std::io;
use std::ops::ControlFlow;

use matrix_sdk::Client;
use tokio::sync::mpsc::UnboundedReceiver;
use tokio::task::JoinHandle;

use super::commands::{LaneCommand, Route, identity_reset_ended, route};
use super::jobs::{Finished, Jobs, Lane, Owes, Work};
use super::lifecycle::{self, SignInEnd};
use super::session::{Followup, IdentityResetEnd, Session, abort_and_join};
use super::{
    EventSender, RoomSnapshots, emit_error, emit_event, emit_invalid_state, emit_matrix_error,
};
use crate::api::{
    Command, CoreConfig, Event, LoginRequest, Operation, RestoreSessionRequest, SyncStopReason,
};
use crate::loopback::LoopbackRedirect;
use crate::matrix::{self, BrowserFlow};

pub(super) struct Core {
    config: CoreConfig,
    events: EventSender,
    state: Lifecycle,
    jobs: Jobs<Session, Followup>,
}

enum Lifecycle {
    Idle,
    /// A browser sign-in waits for the browser to come back to the
    /// loopback listener.
    AwaitingBrowser(Box<PendingBrowserLogin>),
    Ready(Session),
}

struct PendingBrowserLogin {
    client: Client,
    snapshots: RoomSnapshots,
    flow: BrowserFlow,
    /// The listener, answering until the browser's request for the
    /// redirect URI; it closes its port when it ends.
    redirect: JoinHandle<io::Result<LoopbackRedirect>>,
    start_sync: bool,
}

/// What the actor does next.
enum Next {
    Command(Option<Command>),
    Job(Finished<Followup>),
    Redirect(io::Result<LoopbackRedirect>),
    IdentityResetEnd(IdentityResetEnd),
}

impl Lifecycle {
    /// What this state waits for besides commands and jobs. Cancel-safe.
    async fn next(&mut self) -> Next {
        match self {
            Self::AwaitingBrowser(pending) => Next::Redirect(
                (&mut pending.redirect)
                    .await
                    .unwrap_or_else(|err| std::panic::resume_unwind(err.into_panic())),
            ),
            Self::Ready(session) if session.resetting() => {
                Next::IdentityResetEnd(session.identity_reset_end().await)
            }
            Self::Idle | Self::Ready(_) => std::future::pending().await,
        }
    }
}

impl Core {
    pub(super) fn new(config: CoreConfig, events: EventSender) -> Self {
        Self {
            config,
            events,
            state: Lifecycle::Idle,
            jobs: Jobs::new(),
        }
    }

    /// A core in a session of `client`, without its update tasks, for the
    /// tests of what commands do.
    #[cfg(test)]
    pub(super) fn for_client(client: Client, events: EventSender) -> Self {
        let session = Session::for_client(client, &events);
        let mut core = Self::new(test_config(), events);
        core.state = Lifecycle::Ready(session);
        core
    }

    pub(super) async fn run(mut self, mut commands: UnboundedReceiver<Command>) {
        emit_event(&self.events, Event::Started);
        loop {
            // Every branch is cancel-safe.
            let next = tokio::select! {
                command = commands.recv() => Next::Command(command),
                Some(finished) = self.jobs.next() => Next::Job(finished),
                next = self.state.next() => next,
            };
            if self.handle(next).await.is_break() {
                break;
            }
        }
        emit_event(&self.events, Event::Stopped);
    }

    async fn handle(&mut self, next: Next) -> ControlFlow<()> {
        match next {
            // The UI is gone: nobody waits for the acks.
            Next::Command(None) => {
                self.shut_down().await;
                return ControlFlow::Break(());
            }
            Next::Command(Some(command)) => return self.dispatch(command).await,
            Next::Job(finished) => self.finish_job(finished),
            Next::Redirect(redirect) => self.finish_browser_login(redirect).await,
            Next::IdentityResetEnd(end) => {
                let Lifecycle::Ready(session) = &mut self.state else {
                    unreachable!("only a session waits for an identity reset");
                };
                // Settling it and the status ask the server.
                self.jobs
                    .enqueue(Lane::E2ee, identity_reset_ended(end), session);
            }
        }
        ControlFlow::Continue(())
    }

    async fn dispatch(&mut self, command: Command) -> ControlFlow<()> {
        match route(command) {
            Route::Login(request) => self.log_in(request).await,
            Route::RestoreSession(request) => self.restore(request).await,
            Route::CancelLogin => self.cancel_login().await,
            Route::Logout => self.log_out().await,
            Route::StartSync => self.start_sync(),
            Route::StopSync => self.stop_sync(),
            Route::Shutdown => {
                self.shut_down().await;
                ack(&self.events);
                return ControlFlow::Break(());
            }
            Route::Lane(command) => return self.enqueue(command),
        }
        ack(&self.events);
        ControlFlow::Continue(())
    }

    fn enqueue(&mut self, command: LaneCommand) -> ControlFlow<()> {
        let LaneCommand {
            lane,
            refusal,
            start,
        } = command;
        match &mut self.state {
            Lifecycle::Ready(session) => self.jobs.enqueue(lane, Work::command(start), session),
            Lifecycle::Idle | Lifecycle::AwaitingBrowser(_) => {
                refusal.emit(&self.events);
                ack(&self.events);
            }
        }
        ControlFlow::Continue(())
    }

    fn finish_job(&mut self, finished: Finished<Followup>) {
        let Finished { lane, owes, output } = finished;
        let Lifecycle::Ready(session) = &mut self.state else {
            unreachable!("jobs run only in a session");
        };
        match output {
            Followup::Done => {}
            Followup::Watch(flow, watcher) => session.watch(&flow, watcher),
            Followup::IdentityResetStarted(started) => session.begin_identity_reset(*started),
        }
        if owes == Owes::Ack {
            ack(&self.events);
        }
        self.jobs.release(&lane, session);
    }

    async fn log_in(&mut self, request: LoginRequest) {
        if !matches!(self.state, Lifecycle::Idle) {
            emit_invalid_state(
                &self.events,
                Operation::Login,
                "login requires an idle core; logout first",
            );
            return;
        }
        let end = lifecycle::log_in(self.config.clone(), self.events.clone(), request).await;
        self.signed_in(end);
    }

    async fn restore(&mut self, request: RestoreSessionRequest) {
        if !matches!(self.state, Lifecycle::Idle) {
            emit_invalid_state(
                &self.events,
                Operation::RestoreSession,
                "session restore requires an idle core; logout first",
            );
            return;
        }
        let end = lifecycle::restore(self.config.clone(), self.events.clone(), request).await;
        self.signed_in(end);
    }

    /// Applies the end of a sign-in or restore to an idle core.
    fn signed_in(&mut self, end: SignInEnd) {
        assert!(
            matches!(self.state, Lifecycle::Idle),
            "a sign-in starts idle"
        );
        match end {
            SignInEnd::Entered(entered) => {
                self.state = Lifecycle::Ready(Session::start(*entered, &self.events));
            }
            SignInEnd::BrowserReady(ready) => {
                let redirect = tokio::task::spawn_local(ready.listener.next_redirect());
                emit_event(&self.events, Event::SignInUrl { url: ready.url });
                self.state = Lifecycle::AwaitingBrowser(Box::new(PendingBrowserLogin {
                    client: ready.client,
                    snapshots: ready.snapshots,
                    flow: ready.flow,
                    redirect,
                    start_sync: ready.start_sync,
                }));
            }
            SignInEnd::Failed(operation, message) => {
                emit_error(&self.events, operation, None, message);
            }
        }
    }

    /// The browser came back.
    async fn finish_browser_login(&mut self, redirect: io::Result<LoopbackRedirect>) {
        let Lifecycle::AwaitingBrowser(pending) =
            std::mem::replace(&mut self.state, Lifecycle::Idle)
        else {
            unreachable!("only a browser sign-in waits for a redirect");
        };
        let PendingBrowserLogin {
            client,
            snapshots,
            flow,
            redirect: _,
            start_sync,
        } = *pending;
        match redirect {
            Ok(redirect) => {
                let end = lifecycle::finish_browser_login(
                    client,
                    snapshots,
                    flow,
                    redirect,
                    self.events.clone(),
                    start_sync,
                )
                .await;
                self.signed_in(end);
            }
            Err(err) => emit_error(
                &self.events,
                Operation::Login,
                None,
                format!("failed to receive the browser's sign-in: {err}"),
            ),
        }
    }

    async fn cancel_login(&mut self) {
        let Lifecycle::AwaitingBrowser(pending) =
            std::mem::replace(&mut self.state, Lifecycle::Idle)
        else {
            emit_invalid_state(&self.events, Operation::Login, "no sign-in to cancel");
            return;
        };
        // Closes the listener's port; the half-signed-in client drops here,
        // on the runtime.
        abort_and_join(pending.redirect).await;
        emit_event(&self.events, Event::LoginCancelled);
    }

    fn start_sync(&mut self) {
        let Lifecycle::Ready(session) = &mut self.state else {
            emit_invalid_state(
                &self.events,
                Operation::StartSync,
                "start sync requires an authenticated session",
            );
            return;
        };
        if let Err(refusal) = session.start_sync() {
            emit_invalid_state(&self.events, Operation::StartSync, refusal);
        }
    }

    /// Stops continuous sync and keeps the session: mIRC's Disconnect, not
    /// a logout.
    fn stop_sync(&mut self) {
        let Lifecycle::Ready(session) = &mut self.state else {
            emit_invalid_state(
                &self.events,
                Operation::StopSync,
                "stop sync requires an authenticated session",
            );
            return;
        };
        match session.stop_sync() {
            Ok(()) => emit_event(&self.events, Event::SyncStopped(SyncStopReason::Requested)),
            Err(refusal) => emit_invalid_state(&self.events, Operation::StopSync, refusal),
        }
    }

    async fn log_out(&mut self) {
        let Lifecycle::Ready(session) = &mut self.state else {
            emit_invalid_state(
                &self.events,
                Operation::Logout,
                "logout requires an authenticated session",
            );
            return;
        };
        // Before the server ends the session: a sync racing it would report
        // `SyncStopped(SessionEnded)` first.
        let resume_sync = session.stop_sync_quietly().await;
        match matrix::logout(session.client().clone()).await {
            Ok(()) => {
                let dropped = self.jobs.abort_all().await;
                let Lifecycle::Ready(session) = std::mem::replace(&mut self.state, Lifecycle::Idle)
                else {
                    unreachable!("the logout ran in a session");
                };
                session.shut_down().await;
                acks(&self.events, dropped);
                emit_event(&self.events, Event::LoggedOut);
            }
            // The session stays whole.
            Err(err) => {
                emit_matrix_error(&self.events, Operation::Logout, err);
                if resume_sync {
                    session.start_sync().expect("the logout stopped sync");
                }
            }
        }
    }

    /// Stops every job and the session's tasks, and waits until each is
    /// gone, inside the runtime (see `Session::shut_down`); then acks the
    /// commands dropped.
    async fn shut_down(&mut self) {
        let dropped = self.jobs.abort_all().await;
        match std::mem::replace(&mut self.state, Lifecycle::Idle) {
            Lifecycle::Idle => {}
            // The listener's task does not own the client, which drops here,
            // on the runtime.
            Lifecycle::AwaitingBrowser(pending) => abort_and_join(pending.redirect).await,
            Lifecycle::Ready(session) => session.shut_down().await,
        }
        acks(&self.events, dropped);
    }

    /// Dispatches `command` and drives the actor until no job runs, for
    /// the tests of what commands do: one command at a time, that is until
    /// its ack. Returns whether the core stopped.
    #[cfg(test)]
    pub(super) async fn run_until_idle(&mut self, command: Command) -> ControlFlow<()> {
        self.dispatch(command).await?;
        while let Some(finished) = self.jobs.next().await {
            self.finish_job(finished);
        }
        ControlFlow::Continue(())
    }

    /// Ends the tests' core as `Shutdown` does, without its ack.
    #[cfg(test)]
    pub(super) async fn stop(mut self) {
        self.shut_down().await;
    }

    #[cfg(test)]
    pub(super) fn session(&mut self) -> &mut Session {
        let Lifecycle::Ready(session) = &mut self.state else {
            panic!("the core is not in a session");
        };
        session
    }
}

/// A command's answer: every event it emitted is ahead of this one.
fn ack(events: &EventSender) {
    emit_event(events, Event::CommandHandled);
}

/// The answers of `count` commands that were dropped.
fn acks(events: &EventSender, count: usize) {
    for _ in 0..count {
        ack(events);
    }
}

#[cfg(test)]
fn test_config() -> CoreConfig {
    CoreConfig {
        profile_dir: std::env::temp_dir().join("mirx-core-test-unused-profile"),
        store_passphrase: String::new(),
    }
}
