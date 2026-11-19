//! Connecting like mIRC: File > Connect and Disconnect, `/server` and
//! `/quit`, over the core's login, restore and sync commands.
//!
//! Pure code: the connection state follows core events, and Connect picks
//! the step that brings the account online from where it is.

use mirx_core::{Event, Operation};

use crate::automation::ConnectionState;

/// The connection after `event`. Submitting a login or restore moves it to
/// `SigningIn` before any event.
pub(super) fn after_event(connection: ConnectionState, event: &Event) -> ConnectionState {
    match event {
        Event::LoggedIn { .. } | Event::RestoredSession => ConnectionState::SignedIn,
        Event::SyncStarted => ConnectionState::Syncing,
        Event::SyncStopped(_) => ConnectionState::SignedIn,
        Event::LoggedOut => ConnectionState::SignedOut,
        // Signed in already, this is the initial room sync failing after
        // LoggedIn or RestoredSession: the session stands.
        Event::Error(error)
            if matches!(
                error.operation,
                Operation::Login | Operation::RestoreSession
            ) && connection == ConnectionState::SigningIn =>
        {
            ConnectionState::SignedOut
        }
        _ => connection,
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
}

/// `server` is `/server`'s argument. Matrix needs a password, so a named
/// server always goes through the dialog rather than the command line.
pub(super) fn connect_action(
    connection: ConnectionState,
    server: Option<&str>,
    session_saved: bool,
) -> Result<ConnectAction, &'static str> {
    match (connection, server) {
        (ConnectionState::SigningIn, _) => Err("already connecting"),
        (ConnectionState::Syncing, _) => Err("already connected"),
        (ConnectionState::SignedIn, None) => Ok(ConnectAction::StartSync),
        (ConnectionState::SignedIn, Some(_)) => Err("already signed in; log out to change server"),
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

/// File > Disconnect and `/quit` stop sync and keep the session.
pub(super) fn check_disconnect(connection: ConnectionState) -> Result<(), &'static str> {
    match connection {
        ConnectionState::Syncing => Ok(()),
        ConnectionState::SignedOut | ConnectionState::SigningIn | ConnectionState::SignedIn => {
            Err("not connected")
        }
    }
}

/// A login or restore starts from a signed-out account; the core would
/// reject it otherwise.
pub(super) fn check_sign_in(connection: ConnectionState) -> Result<(), &'static str> {
    match connection {
        ConnectionState::SignedOut => Ok(()),
        ConnectionState::SigningIn => Err("already connecting"),
        ConnectionState::SignedIn | ConnectionState::Syncing => {
            Err("already signed in; log out first")
        }
    }
}

#[cfg(test)]
mod tests {
    use mirx_core::{CoreErrorEvent, StoredSession, SyncStopReason};

    use super::*;

    use ConnectionState::{SignedIn, SignedOut, SigningIn, Syncing};

    const ALL: [ConnectionState; 4] = [SignedOut, SigningIn, SignedIn, Syncing];

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
                &Event::SyncStopped(SyncStopReason::Failed("boom".to_owned()))
            ),
            SignedIn
        );
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
            connect_action(SignedIn, Some("matrix.org"), true),
            Err("already signed in; log out to change server")
        );
        for server in [None, Some("matrix.org")] {
            assert_eq!(
                connect_action(SigningIn, server, true),
                Err("already connecting")
            );
            assert_eq!(
                connect_action(Syncing, server, true),
                Err("already connected")
            );
        }
    }

    #[test]
    fn only_a_syncing_account_disconnects_and_only_a_signed_out_one_signs_in() {
        assert_eq!(check_disconnect(Syncing), Ok(()));
        for connection in [SignedOut, SigningIn, SignedIn] {
            assert_eq!(check_disconnect(connection), Err("not connected"));
        }
        assert_eq!(check_sign_in(SignedOut), Ok(()));
        assert_eq!(check_sign_in(SigningIn), Err("already connecting"));
        for connection in [SignedIn, Syncing] {
            assert_eq!(
                check_sign_in(connection),
                Err("already signed in; log out first")
            );
        }
    }
}
