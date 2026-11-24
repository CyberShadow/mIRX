//! The core's actor: it reads commands, routes them, and applies what the
//! jobs and the session's lifecycle bring back.
//!
//! In a session, a command runs as a job on its lane (`jobs`), beside the
//! actor. Signing in, restoring and logging out are transitions: futures
//! the actor polls beside the commands, so they can be cut short:
//! `CancelLogin` a sign-in, `StopSync` a logout, and `Shutdown` either.
//! The other commands that come during a transition wait in `deferred`
//! and run, in order, when it ends.
//!
//! The actor answers every command with exactly one `CommandHandled(id)`,
//! naming it, after the command's own events: an immediate command after its
//! handling, a lane command after its job's followup is applied, a
//! transition's command at its end, and a command that a logout, a cancel
//! or a shutdown drops when it drops it. A logout that ends the session
//! drops the session's jobs and the commands for the session that came
//! during it.
//!
//! The actor awaits only in its `select!` and in teardowns that end
//! without the server (aborting and joining tasks): it never waits for
//! the network, but at shutdown for the read receipts and markers, which
//! get `SHUTDOWN_GRACE`.

use std::collections::VecDeque;
use std::io;
use std::ops::ControlFlow;
use std::time::Duration;

use futures_util::future::LocalBoxFuture;
use matrix_sdk::Client;
use tokio::sync::mpsc::UnboundedReceiver;
use tokio::task::JoinHandle;

use super::commands::{LaneCommand, Route, identity_reset_ended, route};
use super::jobs::{Finished, Jobs, Lane, Owes, Work};
use super::lifecycle::{self, Handlers, SignInEnd};
use super::session::{Followup, IdentityResetEnd, Session, abort_and_join};
use super::{EventSender, emit_error, emit_event, emit_invalid_state};
use crate::api::{
    Command, CommandId, CoreConfig, Event, LoginRequest, LogoutRefusal, LogoutRequest, LogoutScope,
    Operation, RestoreSessionRequest, SyncStopReason,
};
use crate::loopback::LoopbackRedirect;
use crate::matrix::{self, BrowserFlow, ErrorChain};

/// How long shutting down lets the read receipts and markers already sent
/// go on (`Lane::outlasts_shutdown`). The UI has gone by then (the window
/// is hidden at once), so this is how long at most the process outlives
/// it; under Windows' allowance for a program at the session's end.
const SHUTDOWN_GRACE: Duration = Duration::from_secs(3);

pub(super) struct Core {
    config: CoreConfig,
    events: EventSender,
    state: Lifecycle,
    jobs: Jobs<Session, Followup>,
    /// The commands that came during the transition under way, in order.
    /// Empty when none is.
    deferred: VecDeque<(CommandId, Route)>,
}

enum Lifecycle {
    Idle,
    /// A sign-in or restore under way: until the session is read, or until
    /// the browser's page is ready.
    SigningIn(SigningIn),
    /// A browser sign-in waits for the browser to come back to the
    /// loopback listener.
    AwaitingBrowser(Box<PendingBrowserLogin>),
    Ready(Session),
    /// A logout waits for the key backup, then for the server. Its jobs
    /// go on; the session stays whole if either fails, or if `StopSync`
    /// stops it.
    LoggingOut(LoggingOut),
}

struct SigningIn {
    /// Spawns nothing (see `lifecycle`): dropping it cancels the sign-in.
    transition: LocalBoxFuture<'static, SignInEnd>,
    /// A `Login` or `RestoreSession` is owed its ack at the end; the end
    /// of a browser sign-in is not.
    owes: Owes,
}

struct LoggingOut {
    session: Session,
    /// Dropping it mid-wait (`StopSync`, `Shutdown`) is safe: the SDK's
    /// upload task goes on until the client drops. A refusal is the
    /// `LogoutRefused`'s reason.
    transition: LocalBoxFuture<'static, Result<LogoutScope, LogoutRefusal>>,
    /// Sync ran before the logout stopped it.
    resume_sync: bool,
    /// The `Logout`, owed its ack at the end.
    logout: CommandId,
    /// What it asked, for a refusal to name.
    request: LogoutRequest,
}

struct PendingBrowserLogin {
    client: Client,
    handlers: Handlers,
    flow: BrowserFlow,
    /// The listener, answering until the browser's request for the
    /// redirect URI; it closes its port when it ends.
    redirect: JoinHandle<io::Result<LoopbackRedirect>>,
    start_sync: bool,
}

/// What the actor does next.
#[expect(
    clippy::large_enum_variant,
    reason = "one value at a time, on the actor's stack for one turn: boxing the command would allocate per command for nothing"
)]
enum Next {
    Command(Option<(CommandId, Command)>),
    Job(Finished<Followup>),
    SignInEnd(SignInEnd),
    Redirect(io::Result<LoopbackRedirect>),
    LogoutEnd(Result<LogoutScope, LogoutRefusal>),
    IdentityResetEnd(IdentityResetEnd),
}

impl Lifecycle {
    fn in_transition(&self) -> bool {
        matches!(self, Self::SigningIn(_) | Self::LoggingOut(_))
    }

    /// The session, while jobs can run.
    fn session(&mut self) -> &mut Session {
        match self {
            Self::Ready(session) | Self::LoggingOut(LoggingOut { session, .. }) => session,
            Self::Idle | Self::SigningIn(_) | Self::AwaitingBrowser(_) => {
                unreachable!("jobs run only in a session")
            }
        }
    }

    /// What this state waits for besides commands and jobs. Cancel-safe.
    async fn next(&mut self) -> Next {
        match self {
            Self::SigningIn(signing_in) => Next::SignInEnd((&mut signing_in.transition).await),
            Self::LoggingOut(logging_out) => Next::LogoutEnd((&mut logging_out.transition).await),
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
            deferred: VecDeque::new(),
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

    pub(super) async fn run(mut self, mut commands: UnboundedReceiver<(CommandId, Command)>) {
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
            Next::Command(Some((id, command))) => self.dispatch(id, route(command)).await?,
            Next::Job(finished) => self.finish_job(finished),
            Next::SignInEnd(end) => self.sign_in_ended(end),
            Next::Redirect(redirect) => self.finish_browser_login(redirect),
            Next::LogoutEnd(result) => self.logout_ended(result).await,
            Next::IdentityResetEnd(end) => {
                let Lifecycle::Ready(session) = &mut self.state else {
                    unreachable!("only a session waits for an identity reset");
                };
                // Settling it and the status ask the server.
                self.jobs
                    .enqueue(Lane::E2ee, identity_reset_ended(end), session);
            }
        }
        // A transition that ended lets the commands that came during it run.
        while !self.state.in_transition()
            && let Some((id, route)) = self.deferred.pop_front()
        {
            self.dispatch(id, route).await?;
        }
        ControlFlow::Continue(())
    }

    async fn dispatch(&mut self, id: CommandId, route: Route) -> ControlFlow<()> {
        match (route, &self.state) {
            (Route::Shutdown, _) => {
                self.shut_down().await;
                ack(&self.events, id);
                return ControlFlow::Break(());
            }
            (Route::CancelLogin, Lifecycle::SigningIn(_)) => self.cancel_sign_in(id),
            (Route::StopSync, Lifecycle::LoggingOut(_)) => self.cancel_log_out(id),
            (route, Lifecycle::SigningIn(_) | Lifecycle::LoggingOut(_)) => {
                self.deferred.push_back((id, route));
            }
            (Route::Login(request), _) => self.log_in(id, request),
            (Route::RestoreSession(request), _) => self.restore(id, request),
            (Route::CancelLogin, _) => self.cancel_login(id).await,
            (Route::Logout(request), _) => self.log_out(id, request).await,
            (Route::StartSync, _) => self.start_sync(id),
            (Route::StopSync, _) => self.stop_sync(id),
            (Route::Lane(command), _) => self.enqueue(id, command),
        }
        ControlFlow::Continue(())
    }

    fn enqueue(&mut self, id: CommandId, command: LaneCommand) {
        let LaneCommand {
            lane,
            refusal,
            start,
        } = command;
        match &mut self.state {
            Lifecycle::Ready(session) => {
                self.jobs.enqueue(lane, Work::command(id, start), session);
            }
            Lifecycle::Idle | Lifecycle::AwaitingBrowser(_) => {
                refusal.emit(&self.events);
                ack(&self.events, id);
            }
            Lifecycle::SigningIn(_) | Lifecycle::LoggingOut(_) => {
                unreachable!("commands wait out a transition")
            }
        }
    }

    fn finish_job(&mut self, finished: Finished<Followup>) {
        let Finished { lane, owes, output } = finished;
        let session = self.state.session();
        match output {
            Followup::Done => {}
            Followup::IdentityResetStarted(started) => session.begin_identity_reset(*started),
        }
        if let Owes::Ack(id) = owes {
            ack(&self.events, id);
        }
        self.jobs.release(&lane, session);
    }

    fn log_in(&mut self, id: CommandId, request: LoginRequest) {
        if !matches!(self.state, Lifecycle::Idle) {
            emit_invalid_state(
                &self.events,
                Operation::Login,
                "login requires an idle core; logout first",
            );
            ack(&self.events, id);
            return;
        }
        self.state = Lifecycle::SigningIn(SigningIn {
            transition: Box::pin(lifecycle::log_in(
                self.config.clone(),
                self.events.clone(),
                request,
            )),
            owes: Owes::Ack(id),
        });
    }

    fn restore(&mut self, id: CommandId, request: RestoreSessionRequest) {
        if !matches!(self.state, Lifecycle::Idle) {
            emit_invalid_state(
                &self.events,
                Operation::RestoreSession,
                "session restore requires an idle core; logout first",
            );
            ack(&self.events, id);
            return;
        }
        self.state = Lifecycle::SigningIn(SigningIn {
            transition: Box::pin(lifecycle::restore(
                self.config.clone(),
                self.events.clone(),
                request,
            )),
            owes: Owes::Ack(id),
        });
    }

    /// The sign-in or restore under way ended: applies its end.
    fn sign_in_ended(&mut self, end: SignInEnd) {
        let Lifecycle::SigningIn(SigningIn {
            transition: _,
            owes,
        }) = std::mem::replace(&mut self.state, Lifecycle::Idle)
        else {
            unreachable!("only a sign-in ends a sign-in");
        };
        match end {
            SignInEnd::Entered(entered) => {
                self.state = Lifecycle::Ready(Session::start(*entered, &self.events));
            }
            SignInEnd::BrowserReady(ready) => {
                let redirect = tokio::task::spawn_local(ready.listener.next_redirect());
                emit_event(&self.events, Event::SignInUrl { url: ready.url });
                self.state = Lifecycle::AwaitingBrowser(Box::new(PendingBrowserLogin {
                    client: ready.client,
                    handlers: ready.handlers,
                    flow: ready.flow,
                    redirect,
                    start_sync: ready.start_sync,
                }));
            }
            SignInEnd::Failed(operation, message) => {
                emit_error(&self.events, operation, None, message);
            }
        }
        if let Owes::Ack(id) = owes {
            ack(&self.events, id);
        }
    }

    /// The browser came back: the sign-in goes on, as a transition the
    /// `Login` is no longer owed by.
    fn finish_browser_login(&mut self, redirect: io::Result<LoopbackRedirect>) {
        let Lifecycle::AwaitingBrowser(pending) =
            std::mem::replace(&mut self.state, Lifecycle::Idle)
        else {
            unreachable!("only a browser sign-in waits for a redirect");
        };
        let PendingBrowserLogin {
            client,
            handlers,
            flow,
            redirect: _,
            start_sync,
        } = *pending;
        match redirect {
            Ok(redirect) => {
                self.state = Lifecycle::SigningIn(SigningIn {
                    transition: Box::pin(lifecycle::finish_browser_login(
                        client,
                        handlers,
                        flow,
                        redirect,
                        self.events.clone(),
                        start_sync,
                    )),
                    owes: Owes::Nothing,
                });
            }
            Err(err) => emit_error(
                &self.events,
                Operation::Login,
                None,
                format!(
                    "failed to receive the browser's sign-in: {}",
                    ErrorChain(&err)
                ),
            ),
        }
    }

    /// Drops the sign-in or restore under way, and the client it built,
    /// here on the runtime.
    /// `id` is the `CancelLogin`'s.
    fn cancel_sign_in(&mut self, id: CommandId) {
        let Lifecycle::SigningIn(SigningIn { transition, owes }) =
            std::mem::replace(&mut self.state, Lifecycle::Idle)
        else {
            unreachable!("only a sign-in is cancelled");
        };
        drop(transition);
        emit_event(&self.events, Event::LoginCancelled);
        if let Owes::Ack(sign_in) = owes {
            ack(&self.events, sign_in);
        }
        ack(&self.events, id);
    }

    async fn cancel_login(&mut self, id: CommandId) {
        let Lifecycle::AwaitingBrowser(pending) =
            std::mem::replace(&mut self.state, Lifecycle::Idle)
        else {
            emit_invalid_state(&self.events, Operation::Login, "no sign-in to cancel");
            ack(&self.events, id);
            return;
        };
        // Closes the listener's port; the half-signed-in client drops here,
        // on the runtime.
        abort_and_join(pending.redirect).await;
        emit_event(&self.events, Event::LoginCancelled);
        ack(&self.events, id);
    }

    fn start_sync(&mut self, id: CommandId) {
        let Lifecycle::Ready(session) = &mut self.state else {
            emit_invalid_state(
                &self.events,
                Operation::StartSync,
                "start sync requires an authenticated session",
            );
            ack(&self.events, id);
            return;
        };
        if let Err(refusal) = session.start_sync() {
            emit_invalid_state(&self.events, Operation::StartSync, refusal);
        }
        ack(&self.events, id);
    }

    /// Stops continuous sync and keeps the session: mIRC's Disconnect, not
    /// a logout.
    fn stop_sync(&mut self, id: CommandId) {
        let Lifecycle::Ready(session) = &mut self.state else {
            emit_invalid_state(
                &self.events,
                Operation::StopSync,
                "stop sync requires an authenticated session",
            );
            ack(&self.events, id);
            return;
        };
        match session.stop_sync() {
            Ok(()) => emit_event(&self.events, Event::SyncStopped(SyncStopReason::Requested)),
            Err(refusal) => emit_invalid_state(&self.events, Operation::StopSync, refusal),
        }
        ack(&self.events, id);
    }

    async fn log_out(&mut self, id: CommandId, request: LogoutRequest) {
        let LogoutRequest {
            discard_room_keys,
            this_computer_only,
        } = request;
        let mut session = match std::mem::replace(&mut self.state, Lifecycle::Idle) {
            Lifecycle::Ready(session) => session,
            state => {
                self.state = state;
                emit_invalid_state(
                    &self.events,
                    Operation::Logout,
                    "logout requires an authenticated session",
                );
                ack(&self.events, id);
                return;
            }
        };
        // Recovery's state is known here and now: a refusal leaves sync
        // alone.
        if !discard_room_keys && let Some(risk) = matrix::recovery_risk(session.client()) {
            self.state = Lifecycle::Ready(session);
            emit_event(
                &self.events,
                Event::LogoutRefused {
                    request,
                    reason: LogoutRefusal::RoomKeysAtRisk(risk),
                },
            );
            ack(&self.events, id);
            return;
        }
        // Before the backup upload, so that no new room keys come; and
        // before the server ends the session: a sync racing it would report
        // `SyncStopped(SessionEnded)` first.
        let resume_sync = session.stop_sync_quietly().await;
        emit_event(&self.events, Event::LoggingOut);
        let client = session.client().clone();
        let device_id = client
            .device_id()
            .expect("a session's client is signed in")
            .to_string();
        let transition = Box::pin(async move {
            if !discard_room_keys {
                matrix::back_up_room_keys(&client)
                    .await
                    .map_err(LogoutRefusal::RoomKeysAtRisk)?;
            }
            if this_computer_only {
                return Ok(LogoutScope::ThisComputerOnly { device_id });
            }
            match matrix::logout(client).await {
                Ok(()) => Ok(LogoutScope::Everywhere),
                Err(err) => Err(LogoutRefusal::ServerKeepsSession {
                    device_id,
                    reason: err.to_string(),
                }),
            }
        });
        self.state = Lifecycle::LoggingOut(LoggingOut {
            session,
            transition,
            resume_sync,
            logout: id,
            request,
        });
    }

    /// `StopSync` during a logout, mIRC's Disconnect: drops the logout
    /// under way and keeps the session, with sync stopped even if it ran
    /// before. If the server's answer was on its way, the server may have
    /// ended the session already; the next sync then says so.
    /// `id` is the `StopSync`'s.
    fn cancel_log_out(&mut self, id: CommandId) {
        let Lifecycle::LoggingOut(LoggingOut {
            session,
            transition,
            resume_sync: _,
            logout,
            request: _,
        }) = std::mem::replace(&mut self.state, Lifecycle::Idle)
        else {
            unreachable!("only a logout is stopped");
        };
        drop(transition);
        self.state = Lifecycle::Ready(session);
        emit_event(&self.events, Event::LogoutCancelled);
        // The `Logout`'s, then the `StopSync`'s.
        ack(&self.events, logout);
        ack(&self.events, id);
    }

    /// The logout under way ended.
    async fn logout_ended(&mut self, result: Result<LogoutScope, LogoutRefusal>) {
        let Lifecycle::LoggingOut(LoggingOut {
            mut session,
            transition: _,
            resume_sync,
            logout,
            request,
        }) = std::mem::replace(&mut self.state, Lifecycle::Idle)
        else {
            unreachable!("only a logout ends a logout");
        };
        match result {
            // The session's client is gone with it: its store can go too.
            Ok(scope) => {
                let mut dropped = self.jobs.abort_all().await;
                dropped.extend(self.drop_session_commands());
                session.shut_down().await;
                acks(&self.events, dropped);
                let removed = matrix::remove_store(&self.config).await;
                // The session is gone from here either way: the UI forgets
                // it.
                emit_event(&self.events, Event::LoggedOut(scope));
                if let Err(err) = removed {
                    emit_error(
                        &self.events,
                        Operation::Logout,
                        None,
                        format!(
                            "logged out, but could not remove the session's data in {}",
                            ErrorChain(&err)
                        ),
                    );
                }
            }
            // The session stays whole.
            Err(reason) => {
                emit_event(&self.events, Event::LogoutRefused { request, reason });
                if resume_sync {
                    session.start_sync().expect("the logout stopped sync");
                }
                self.state = Lifecycle::Ready(session);
            }
        }
        ack(&self.events, logout);
    }

    /// Drops the commands that came for the session during its logout, as
    /// its jobs are dropped; the others run once the logout ends. Returns
    /// the ones it dropped.
    fn drop_session_commands(&mut self) -> Vec<CommandId> {
        let (dropped, kept): (VecDeque<_>, _) = std::mem::take(&mut self.deferred)
            .into_iter()
            .partition(|(_, route)| route.is_for_the_session());
        self.deferred = kept;
        dropped.into_iter().map(|(id, _)| id).collect()
    }

    /// Stops every job, the transition under way and the session's tasks,
    /// and waits until each is gone, inside the runtime (see
    /// `Session::shut_down`); then acks the commands dropped. The read
    /// receipts and markers go first, for `SHUTDOWN_GRACE` at most; the
    /// other queued work never starts.
    async fn shut_down(&mut self) {
        let mut dropped = self.jobs.drop_queued(|lane| !lane.outlasts_shutdown());
        self.finish_receipts().await;
        dropped.extend(self.jobs.abort_all().await);
        dropped.extend(self.deferred.drain(..).map(|(id, _)| id));
        match std::mem::replace(&mut self.state, Lifecycle::Idle) {
            Lifecycle::Idle => {}
            // The client it built drops here, on the runtime.
            Lifecycle::SigningIn(SigningIn { transition, owes }) => {
                drop(transition);
                dropped.extend(owes.command());
            }
            // The listener's task does not own the client, which drops here,
            // on the runtime.
            Lifecycle::AwaitingBrowser(pending) => abort_and_join(pending.redirect).await,
            Lifecycle::Ready(session) => session.shut_down().await,
            // The `Logout` is dropped with it.
            Lifecycle::LoggingOut(LoggingOut {
                session,
                transition,
                resume_sync: _,
                logout,
                request: _,
            }) => {
                drop(transition);
                session.shut_down().await;
                dropped.push(logout);
            }
        }
        acks(&self.events, dropped);
    }

    /// Lets the receipts' lanes run until they are done, or for
    /// `SHUTDOWN_GRACE`: what is left then is aborted with the rest. The
    /// other jobs that end meanwhile are applied and acked as usual.
    async fn finish_receipts(&mut self) {
        let receipts = async {
            while self.jobs.has_work(Lane::outlasts_shutdown) {
                let finished = self.jobs.next().await.expect("a receipt job runs");
                self.finish_job(finished);
            }
        };
        // Past the grace the server's answer no longer matters: the
        // process ends.
        let _past_the_grace = tokio::time::timeout(SHUTDOWN_GRACE, receipts).await;
    }

    /// Handles `command` and drives the actor until no job or transition
    /// runs, for the tests of what commands do: one command at a time,
    /// that is until its ack. Returns whether the core stopped.
    #[cfg(test)]
    pub(super) async fn run_until_idle(
        &mut self,
        id: CommandId,
        command: Command,
    ) -> ControlFlow<()> {
        self.handle(Next::Command(Some((id, command)))).await?;
        loop {
            let next = if self.state.in_transition() {
                tokio::select! {
                    Some(finished) = self.jobs.next() => Next::Job(finished),
                    next = self.state.next() => next,
                }
            } else {
                match self.jobs.next().await {
                    Some(finished) => Next::Job(finished),
                    None => return ControlFlow::Continue(()),
                }
            };
            self.handle(next).await?;
        }
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

/// The command `id`'s answer: every event it emitted is ahead of this one.
fn ack(events: &EventSender, id: CommandId) {
    emit_event(events, Event::CommandHandled(id));
}

/// The answers of the commands that were dropped.
fn acks(events: &EventSender, ids: Vec<CommandId>) {
    for id in ids {
        ack(events, id);
    }
}

#[cfg(test)]
fn test_config() -> CoreConfig {
    CoreConfig {
        profile_dir: std::env::temp_dir().join("mirx-core-test-unused-profile"),
        store_passphrase: String::new(),
    }
}
