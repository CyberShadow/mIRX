//! Connecting like mIRC: File > Connect and Disconnect, `/server` and
//! `/quit`, over the core's login, restore and sync commands.
//!
//! Pure code: the connection state follows core events, and Connect picks
//! the step that brings the account online from where it is.

use std::time::Duration;

use mirx_core::{Event, Operation};

use super::settings::ConnectSetting;
use crate::automation::ConnectionState;

/// The connection after `event`. Submitting a login or restore moves it to
/// `SigningIn` before any event.
pub(super) fn after_event(connection: ConnectionState, event: &Event) -> ConnectionState {
    match event {
        Event::SignInUrl { .. } => ConnectionState::WaitingForBrowser,
        Event::LoginCancelled => ConnectionState::SignedOut,
        Event::LoggedIn { .. } | Event::RestoredSession => ConnectionState::SignedIn,
        Event::SyncStarted => ConnectionState::Syncing,
        Event::SyncStopped(_) => ConnectionState::SignedIn,
        Event::LoggingOut => ConnectionState::LoggingOut,
        Event::LogoutCancelled => ConnectionState::SignedIn,
        Event::LoggedOut => ConnectionState::SignedOut,
        // The Log Out under way failed: the session stands, and the core
        // starts sync again if it ran (`SyncStarted`). From another state
        // this is the refusal of a Log Out that never started, which
        // changes nothing.
        Event::LogoutWouldLoseRoomKeys(_) if connection == ConnectionState::LoggingOut => {
            ConnectionState::SignedIn
        }
        Event::Error(error)
            if error.operation == Operation::Logout
                && connection == ConnectionState::LoggingOut =>
        {
            ConnectionState::SignedIn
        }
        // After LoggedIn or RestoredSession (SignedIn already) this is the
        // session entry's error: the initial room sync of a store that had
        // never synced failed, or the first room snapshot could not be
        // read. The session stands.
        Event::Error(error)
            if matches!(
                error.operation,
                Operation::Login | Operation::RestoreSession
            ) && matches!(
                connection,
                ConnectionState::SigningIn | ConnectionState::WaitingForBrowser
            ) =>
        {
            ConnectionState::SignedOut
        }
        _ => connection,
    }
}

/// What to do when sync stops by itself (`SyncStopReason::Lost`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum AfterSyncLost {
    /// mIRC's Reconnect on disconnection: the connection was up.
    ReconnectNow,
    /// mIRC's connect retry: the attempt never connected.
    RetryAfter(Duration),
    /// Every retry failed.
    GiveUp { attempts: u32 },
    /// Reconnect or retry is off: stay disconnected, as mIRC does.
    Stay,
}

/// `was_connected`: the sync that stopped had connected. `attempts`: the
/// retries made since the connection was lost or the user connected.
/// Retrying an attempt that connected and was then lost would be a
/// reconnection; a reconnection that fails is an attempt, so a server that
/// is down is retried with the delay, never in a tight loop.
pub(super) fn after_sync_lost(
    settings: &ConnectSetting,
    was_connected: bool,
    attempts: u32,
) -> AfterSyncLost {
    match (was_connected, settings.retry) {
        (true, _) if settings.reconnect => AfterSyncLost::ReconnectNow,
        (true, _) | (false, false) => AfterSyncLost::Stay,
        (false, true) if attempts < settings.retry_times.get() => {
            AfterSyncLost::RetryAfter(settings.retry_delay.duration())
        }
        (false, true) => AfterSyncLost::GiveUp { attempts },
    }
}

/// The connection's retry bookkeeping, as the core's sync events and the
/// retries move it.
#[derive(Debug, Default)]
pub(super) struct Reconnect {
    /// A `SyncConnected` came since the last `SyncStarted`.
    connected: bool,
    /// The retries made since the connection was lost, or since the user's
    /// own Connect.
    attempts: u32,
    /// The connection was lost and has not come back: its return is news.
    lost: bool,
}

impl Reconnect {
    pub(super) fn sync_started(&mut self) {
        self.connected = false;
    }

    /// Returns whether the connection came back after it was lost.
    pub(super) fn sync_connected(&mut self) -> bool {
        self.connected = true;
        self.attempts = 0;
        std::mem::take(&mut self.lost)
    }

    pub(super) fn sync_lost(&mut self, settings: &ConnectSetting) -> AfterSyncLost {
        self.lost = true;
        after_sync_lost(settings, self.connected, self.attempts)
    }

    /// A retry starts; returns its number, from 1.
    pub(super) fn retry(&mut self) -> u32 {
        self.attempts += 1;
        self.attempts
    }

    /// The user connected, disconnected or logged out: retries start over.
    pub(super) fn reset(&mut self) {
        *self = Self::default();
    }

    pub(super) fn attempts(&self) -> u32 {
        self.attempts
    }

    /// The connection was lost and is being brought back: the reconnect's
    /// `Reconnecting to` or the retry's `Connect retry #n` said so.
    pub(super) fn reconnecting(&self) -> bool {
        self.lost
    }
}

/// What File > Connect or `/server` does.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum ConnectAction {
    /// Restore the saved session and start syncing.
    RestoreSavedSession,
    /// Open Select Server with this server filled in, then log in and sync.
    SelectServer { server: String },
    /// Signed in but offline: start syncing.
    StartSync,
    /// A retry is due: stop waiting and start syncing now.
    RetryNow,
}

/// `server` is `/server`'s argument. Signing in takes a password or the
/// browser, so a named server always goes through the dialog rather than
/// the command line.
pub(super) fn connect_action(
    connection: ConnectionState,
    server: Option<&str>,
    session_saved: bool,
) -> Result<ConnectAction, &'static str> {
    match (connection, server) {
        (ConnectionState::SigningIn | ConnectionState::WaitingForBrowser, _) => {
            Err("already connecting")
        }
        (ConnectionState::Syncing, _) => Err("already connected"),
        (ConnectionState::LoggingOut, _) => Err("logging out"),
        (ConnectionState::SignedIn, None) => Ok(ConnectAction::StartSync),
        (ConnectionState::WaitingToReconnect, None) => Ok(ConnectAction::RetryNow),
        (ConnectionState::SignedIn | ConnectionState::WaitingToReconnect, Some(_)) => {
            Err("already signed in; log out to change server")
        }
        (ConnectionState::SignedOut, Some(server)) => Ok(ConnectAction::SelectServer {
            server: server.to_owned(),
        }),
        (ConnectionState::SignedOut, None) if session_saved => {
            Ok(ConnectAction::RestoreSavedSession)
        }
        (ConnectionState::SignedOut, None) => Ok(ConnectAction::SelectServer {
            server: String::new(),
        }),
    }
}

/// What File > Disconnect or `/quit` does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum DisconnectAction {
    /// Stop syncing and keep the session.
    StopSync,
    /// Stop the sign-in or restore in progress, or the wait for the
    /// browser, as mIRC's Disconnect cancels a connect in progress.
    CancelSignIn,
    /// Stop retrying, as mIRC's `/quit` does.
    CancelReconnect,
    /// Stop the Log Out under way, keeping the session, disconnected.
    StopLogOut,
}

pub(super) fn disconnect_action(
    connection: ConnectionState,
) -> Result<DisconnectAction, &'static str> {
    match connection {
        ConnectionState::Syncing => Ok(DisconnectAction::StopSync),
        ConnectionState::SigningIn | ConnectionState::WaitingForBrowser => {
            Ok(DisconnectAction::CancelSignIn)
        }
        ConnectionState::WaitingToReconnect => Ok(DisconnectAction::CancelReconnect),
        ConnectionState::LoggingOut => Ok(DisconnectAction::StopLogOut),
        ConnectionState::SignedOut | ConnectionState::SignedIn => Err("not connected"),
    }
}

/// A login or restore starts from a signed-out account; the core would
/// reject it otherwise.
pub(super) fn check_sign_in(connection: ConnectionState) -> Result<(), &'static str> {
    match connection {
        ConnectionState::SignedOut => Ok(()),
        ConnectionState::SigningIn | ConnectionState::WaitingForBrowser => {
            Err("already connecting")
        }
        ConnectionState::LoggingOut => Err("logging out"),
        ConnectionState::SignedIn
        | ConnectionState::WaitingToReconnect
        | ConnectionState::Syncing => Err("already signed in; log out first"),
    }
}

#[cfg(test)]
mod tests {
    use mirx_core::{CoreErrorEvent, SessionAuth, StoredSession, SyncStopReason};

    use super::super::settings::{RetryDelay, RetryTimes};
    use super::*;

    use ConnectionState::{
        LoggingOut, SignedIn, SignedOut, SigningIn, Syncing, WaitingForBrowser, WaitingToReconnect,
    };

    const ALL: [ConnectionState; 7] = [
        SignedOut,
        SigningIn,
        WaitingForBrowser,
        SignedIn,
        WaitingToReconnect,
        Syncing,
        LoggingOut,
    ];

    fn error(operation: Operation) -> Event {
        Event::Error(CoreErrorEvent {
            operation,
            room_id: None,
            message: "failed".to_owned(),
        })
    }

    fn logged_in() -> Event {
        Event::LoggedIn {
            session: StoredSession {
                user_id: "@alice:example".to_owned(),
                device_id: "DEVICE".to_owned(),
                access_token: "token".to_owned(),
                refresh_token: None,
                auth: SessionAuth::Matrix,
            },
            homeserver_url: "https://matrix.example/".to_owned(),
        }
    }

    #[test]
    fn signing_in_ends_signed_in_or_signed_out() {
        assert_eq!(after_event(SigningIn, &logged_in()), SignedIn);
        assert_eq!(after_event(SigningIn, &Event::RestoredSession), SignedIn);
        assert_eq!(after_event(SigningIn, &error(Operation::Login)), SignedOut);
        assert_eq!(
            after_event(SigningIn, &error(Operation::RestoreSession)),
            SignedOut
        );
    }

    #[test]
    fn a_browser_sign_in_waits_then_ends_signed_in_signed_out_or_cancelled() {
        let sign_in_url = Event::SignInUrl {
            url: "https://account.example/oauth2/authorize".to_owned(),
        };
        assert_eq!(after_event(SigningIn, &sign_in_url), WaitingForBrowser);
        assert_eq!(after_event(WaitingForBrowser, &logged_in()), SignedIn);
        assert_eq!(
            after_event(WaitingForBrowser, &error(Operation::Login)),
            SignedOut
        );
        assert_eq!(
            after_event(WaitingForBrowser, &Event::LoginCancelled),
            SignedOut
        );
        // The core refuses other commands meanwhile.
        assert_eq!(
            after_event(WaitingForBrowser, &error(Operation::StartSync)),
            WaitingForBrowser
        );
    }

    #[test]
    fn a_late_sign_in_error_keeps_the_session() {
        assert_eq!(after_event(SignedIn, &error(Operation::Login)), SignedIn);
        assert_eq!(
            after_event(SignedIn, &error(Operation::RestoreSession)),
            SignedIn
        );
        assert_eq!(after_event(Syncing, &error(Operation::Login)), Syncing);
    }

    #[test]
    fn sync_start_and_stop_move_between_signed_in_and_syncing() {
        assert_eq!(after_event(SignedIn, &Event::SyncStarted), Syncing);
        assert_eq!(
            after_event(Syncing, &Event::SyncStopped(SyncStopReason::Requested)),
            SignedIn
        );
        assert_eq!(
            after_event(
                Syncing,
                &Event::SyncStopped(SyncStopReason::Lost("boom".to_owned()))
            ),
            SignedIn
        );
        assert_eq!(
            after_event(
                Syncing,
                &Event::SyncStopped(SyncStopReason::SessionEnded("gone".to_owned()))
            ),
            SignedIn
        );
        // Connecting is news for the log, not a change of state.
        assert_eq!(after_event(Syncing, &Event::SyncConnected), Syncing);
    }

    #[test]
    fn a_log_out_under_way_ends_signed_out_or_signed_in() {
        for connection in [SignedIn, Syncing] {
            assert_eq!(after_event(connection, &Event::LoggingOut), LoggingOut);
        }
        assert_eq!(after_event(LoggingOut, &Event::LoggedOut), SignedOut);
        let refusal = Event::LogoutWouldLoseRoomKeys(mirx_core::RoomKeysAtRisk::RecoveryOff);
        assert_eq!(after_event(LoggingOut, &refusal), SignedIn);
        assert_eq!(after_event(LoggingOut, &error(Operation::Logout)), SignedIn);
        // Disconnect stopped it.
        assert_eq!(after_event(LoggingOut, &Event::LogoutCancelled), SignedIn);
        // The core starts the sync the Log Out stopped again.
        assert_eq!(after_event(SignedIn, &Event::SyncStarted), Syncing);
        // A refusal before any Log Out started keeps the connection.
        for connection in [SignedIn, WaitingToReconnect, Syncing] {
            assert_eq!(after_event(connection, &refusal), connection);
            assert_eq!(
                after_event(connection, &error(Operation::Logout)),
                connection
            );
        }
    }

    #[test]
    fn logout_signs_out_from_anywhere() {
        for connection in ALL {
            assert_eq!(after_event(connection, &Event::LoggedOut), SignedOut);
        }
    }

    #[test]
    fn sync_command_rejections_change_nothing() {
        for connection in ALL {
            for operation in [Operation::StartSync, Operation::StopSync] {
                assert_eq!(after_event(connection, &error(operation)), connection);
            }
        }
    }

    #[test]
    fn connect_picks_the_next_step() {
        assert_eq!(
            connect_action(SignedOut, None, true),
            Ok(ConnectAction::RestoreSavedSession)
        );
        assert_eq!(
            connect_action(SignedOut, None, false),
            Ok(ConnectAction::SelectServer {
                server: String::new()
            })
        );
        assert_eq!(
            connect_action(SignedOut, Some("matrix.org"), true),
            Ok(ConnectAction::SelectServer {
                server: "matrix.org".to_owned()
            })
        );
        assert_eq!(
            connect_action(SignedIn, None, true),
            Ok(ConnectAction::StartSync)
        );
        assert_eq!(
            connect_action(WaitingToReconnect, None, true),
            Ok(ConnectAction::RetryNow)
        );
        for signed_in in [SignedIn, WaitingToReconnect] {
            assert_eq!(
                connect_action(signed_in, Some("matrix.org"), true),
                Err("already signed in; log out to change server")
            );
        }
        for server in [None, Some("matrix.org")] {
            for connecting in [SigningIn, WaitingForBrowser] {
                assert_eq!(
                    connect_action(connecting, server, true),
                    Err("already connecting")
                );
            }
            assert_eq!(
                connect_action(Syncing, server, true),
                Err("already connected")
            );
            assert_eq!(connect_action(LoggingOut, server, true), Err("logging out"));
        }
    }

    #[test]
    fn disconnect_stops_sync_or_cancels_the_sign_in_and_only_a_signed_out_account_signs_in() {
        assert_eq!(disconnect_action(Syncing), Ok(DisconnectAction::StopSync));
        for connecting in [SigningIn, WaitingForBrowser] {
            assert_eq!(
                disconnect_action(connecting),
                Ok(DisconnectAction::CancelSignIn)
            );
        }
        assert_eq!(
            disconnect_action(WaitingToReconnect),
            Ok(DisconnectAction::CancelReconnect)
        );
        for connection in [SignedOut, SignedIn] {
            assert_eq!(disconnect_action(connection), Err("not connected"));
        }
        assert_eq!(
            disconnect_action(LoggingOut),
            Ok(DisconnectAction::StopLogOut)
        );
        assert_eq!(check_sign_in(SignedOut), Ok(()));
        assert_eq!(check_sign_in(LoggingOut), Err("logging out"));
        for connecting in [SigningIn, WaitingForBrowser] {
            assert_eq!(check_sign_in(connecting), Err("already connecting"));
        }
        for connection in [SignedIn, WaitingToReconnect, Syncing] {
            assert_eq!(
                check_sign_in(connection),
                Err("already signed in; log out first")
            );
        }
    }

    fn connect_setting(reconnect: bool, retry: bool, retry_times: u16) -> ConnectSetting {
        ConnectSetting {
            reconnect,
            retry,
            retry_times: RetryTimes::new(retry_times).unwrap(),
            retry_delay: RetryDelay::new(10).unwrap(),
            ..ConnectSetting::default()
        }
    }

    #[test]
    fn a_lost_connection_reconnects_and_a_failed_attempt_retries_until_the_count() {
        use AfterSyncLost::{GiveUp, ReconnectNow, RetryAfter, Stay};
        let ten = Duration::from_secs(10);
        for (reconnect, retry, was_connected, attempts, expected) in [
            (true, true, true, 0, ReconnectNow),
            // A connected sync reconnects however many retries came first.
            (true, false, true, 5, ReconnectNow),
            (false, true, true, 0, Stay),
            (false, false, true, 0, Stay),
            (true, true, false, 0, RetryAfter(ten)),
            (false, true, false, 0, RetryAfter(ten)),
            (true, true, false, 2, RetryAfter(ten)),
            (true, true, false, 3, GiveUp { attempts: 3 }),
            (true, false, false, 0, Stay),
            (false, false, false, 0, Stay),
        ] {
            assert_eq!(
                after_sync_lost(
                    &connect_setting(reconnect, retry, 3),
                    was_connected,
                    attempts
                ),
                expected,
                "reconnect {reconnect}, retry {retry}, connected {was_connected}, attempts {attempts}"
            );
        }
    }

    #[test]
    fn reconnect_counts_retries_until_the_connection_comes_back() {
        let settings = connect_setting(true, true, 2);
        let mut reconnect = Reconnect::default();
        reconnect.sync_started();
        assert_eq!(
            reconnect.sync_lost(&settings),
            AfterSyncLost::RetryAfter(Duration::from_secs(10))
        );
        assert_eq!(reconnect.retry(), 1);
        reconnect.sync_started();
        assert_eq!(
            reconnect.sync_lost(&settings),
            AfterSyncLost::RetryAfter(Duration::from_secs(10))
        );
        assert_eq!(reconnect.retry(), 2);
        reconnect.sync_started();
        assert_eq!(
            reconnect.sync_lost(&settings),
            AfterSyncLost::GiveUp { attempts: 2 }
        );
        assert_eq!(reconnect.attempts(), 2);

        // The user's Connect starts over; the connection's return is no
        // reconnection then.
        reconnect.reset();
        reconnect.sync_started();
        assert!(!reconnect.sync_connected());

        // Lost once connected: reconnect at once, and say it came back.
        assert_eq!(reconnect.sync_lost(&settings), AfterSyncLost::ReconnectNow);
        reconnect.sync_started();
        // The immediate reconnection failed too: now it is a retry.
        assert_eq!(
            reconnect.sync_lost(&settings),
            AfterSyncLost::RetryAfter(Duration::from_secs(10))
        );
        assert_eq!(reconnect.retry(), 1);
        reconnect.sync_started();
        assert!(reconnect.sync_connected());
        assert_eq!(reconnect.attempts(), 0);
        assert!(!reconnect.sync_connected());
    }
}
