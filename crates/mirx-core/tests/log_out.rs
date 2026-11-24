//! Log Out and what it asks first: without a key backup a new session
//! could open, the core asks before it loses the room keys; when the server
//! does not end the session, before it removes it from this computer only.

mod common;

use common::{Harness, log_out_anyway};
use mirx_core::{
    Command, E2eeRecoveryState, Event, LogoutRefusal, LogoutRequest, LogoutScope, RoomKeysAtRisk,
};
use serde_json::json;
use wiremock::matchers::{method, path_regex};
use wiremock::{Mock, ResponseTemplate};

const ROOM: &str = "!a:example.org";

fn log_out() -> Command {
    Command::Logout(LogoutRequest {
        discard_room_keys: false,
        this_computer_only: false,
    })
}

fn mount_logout(harness: &Harness) {
    harness.mount(
        Mock::given(method("POST"))
            .and(path_regex(r"^/_matrix/client/v3/logout$"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({}))),
    );
}

/// Recovery is off: the core says the keys are at risk, and the session
/// goes on as it was, syncing, without a word about it.
#[test]
fn logging_out_without_recovery_asks_first() {
    let harness = Harness::restored_in_rooms(&[ROOM], &[]);
    mount_logout(&harness);
    harness.wait_for_recovery(E2eeRecoveryState::Disabled);
    harness.send(Command::StartSync);
    harness.wait_for("SyncConnected", |event| *event == Event::SyncConnected);

    harness.send(log_out());
    let events = harness.events_until("the logout's ack", |event| {
        matches!(event, Event::CommandHandled(_))
    });

    let answers: Vec<_> = events
        .iter()
        .filter(|event| {
            matches!(
                event,
                Event::LogoutRefused { .. }
                    | Event::LoggingOut
                    | Event::LoggedOut(_)
                    | Event::SyncStarted
                    | Event::SyncStopped(_)
                    | Event::Error(_)
            )
        })
        .collect();
    assert_eq!(
        answers,
        [&Event::LogoutRefused {
            request: LogoutRequest {
                discard_room_keys: false,
                this_computer_only: false,
            },
            reason: LogoutRefusal::RoomKeysAtRisk(RoomKeysAtRisk::RecoveryOff),
        }],
        "{events:#?}"
    );
    let syncs = harness.requests("GET", "/sync").len();
    // The sync goes on.
    harness.wait_for_requests("GET", "/sync", syncs + 2);
    assert!(harness.requests("POST", "/logout").is_empty());

    harness.shut_down();
}

/// The user's answer to the warning: Log Out anyway logs out.
#[test]
fn logging_out_anyway_logs_out_without_recovery() {
    let harness = Harness::restored_in_rooms(&[ROOM], &[]);
    mount_logout(&harness);
    harness.wait_for_recovery(E2eeRecoveryState::Disabled);

    harness.send(log_out_anyway());
    let events = harness.events_until("LoggedOut", |event| {
        *event == Event::LoggedOut(LogoutScope::Everywhere)
    });
    let logging_out = events.iter().filter(|event| **event == Event::LoggingOut);
    assert_eq!(logging_out.count(), 1, "{events:#?}");
    assert_eq!(harness.requests("POST", "/logout").len(), 1);

    harness.shut_down();
}

/// A server that answers `/logout` with errors, as a proxy in front of a
/// stopped server does: the logout is refused in seconds, with the
/// server's error, and the session goes on, syncing again.
#[test]
fn a_logout_the_server_answers_with_errors_fails_in_seconds() {
    let harness = Harness::restored_in_rooms(&[ROOM], &[]);
    harness.mount(
        Mock::given(method("POST"))
            .and(path_regex(r"^/_matrix/client/v3/logout$"))
            .respond_with(ResponseTemplate::new(503).set_body_json(json!({
                "errcode": "M_UNKNOWN",
                "error": "the server is restarting",
            }))),
    );
    harness.send(Command::StartSync);
    harness.wait_for("SyncConnected", |event| *event == Event::SyncConnected);

    harness.send(log_out_anyway());
    let events = harness.events_until("the logout's ack", |event| {
        matches!(event, Event::CommandHandled(_))
    });

    let answers: Vec<_> = events
        .iter()
        .filter(|event| {
            matches!(
                event,
                Event::LoggingOut
                    | Event::LogoutRefused { .. }
                    | Event::LoggedOut(_)
                    | Event::SyncStarted
                    | Event::SyncStopped(_)
                    | Event::Error(_)
            )
        })
        .collect();
    assert!(
        matches!(
            answers[..],
            [
                Event::LoggingOut,
                Event::LogoutRefused {
                    request: LogoutRequest {
                        discard_room_keys: true,
                        this_computer_only: false,
                    },
                    reason: LogoutRefusal::ServerKeepsSession { device_id, reason },
                },
                Event::SyncStarted,
            ] if device_id == "MIRX" && reason.contains("restarting")
        ),
        "{events:#?}"
    );
    assert!(harness.has_store());
    // The first request and 4 retries.
    assert_eq!(harness.requests("POST", "/logout").len(), 5);

    harness.shut_down();
}

/// The answers of the `Logout` sent last, up to its ack: what it said about
/// the logout and the session.
fn logout_answers(harness: &Harness) -> Vec<Event> {
    harness
        .events_until("the logout's ack", |event| {
            matches!(event, Event::CommandHandled(_))
        })
        .into_iter()
        .filter(|event| {
            matches!(
                event,
                Event::LoggingOut
                    | Event::LogoutRefused { .. }
                    | Event::LoggedOut(_)
                    | Event::SyncStarted
                    | Event::SyncStopped(_)
                    | Event::Error(_)
            )
        })
        .collect()
}

/// The network is down: the server cannot end the session, so the core
/// says so, naming the session, and keeps it as it was.
#[test]
fn an_unreachable_server_keeps_the_session_and_says_why() {
    let (harness, _) = Harness::restored_in_rooms(&[ROOM], &[]).restarted_offline();

    harness.send(log_out_anyway());
    let answers = logout_answers(&harness);

    assert!(
        matches!(
            &answers[..],
            [
                Event::LoggingOut,
                Event::LogoutRefused {
                    request: LogoutRequest {
                        discard_room_keys: true,
                        this_computer_only: false,
                    },
                    reason: LogoutRefusal::ServerKeepsSession { device_id, reason },
                },
            ] if device_id == "MIRX" && reason.contains("/logout")
        ),
        "{answers:#?}"
    );
    assert!(harness.has_store());

    harness.shut_down();
}

/// The user's answer to that: Log Out from this computer only removes the
/// session's data without asking the server, which goes on listing it.
#[test]
fn this_computer_only_removes_the_session_without_the_server() {
    let harness = Harness::restored_in_rooms(&[ROOM], &[]);
    mount_logout(&harness);
    assert!(harness.has_store());

    harness.send(Command::Logout(LogoutRequest {
        discard_room_keys: true,
        this_computer_only: true,
    }));
    let answers = logout_answers(&harness);

    assert_eq!(
        answers,
        [
            Event::LoggingOut,
            Event::LoggedOut(LogoutScope::ThisComputerOnly {
                device_id: "MIRX".to_owned(),
            }),
        ]
    );
    assert!(!harness.has_store());
    assert!(harness.requests("POST", "/logout").is_empty());

    harness.shut_down();
}

/// Offline as well: nothing is asked of the server.
#[test]
fn this_computer_only_works_offline() {
    let (harness, _) = Harness::restored_in_rooms(&[ROOM], &[]).restarted_offline();

    harness.send(Command::Logout(LogoutRequest {
        discard_room_keys: true,
        this_computer_only: true,
    }));
    let answers = logout_answers(&harness);

    assert_eq!(
        answers,
        [
            Event::LoggingOut,
            Event::LoggedOut(LogoutScope::ThisComputerOnly {
                device_id: "MIRX".to_owned(),
            }),
        ]
    );
    assert!(!harness.has_store());

    harness.shut_down();
}

/// The room keys come first, for this computer only too: the refusal
/// names the whole request, so the answer keeps both permissions.
#[test]
fn this_computer_only_still_asks_about_the_room_keys() {
    let harness = Harness::restored_in_rooms(&[ROOM], &[]);
    harness.wait_for_recovery(E2eeRecoveryState::Disabled);

    harness.send(Command::Logout(LogoutRequest {
        discard_room_keys: false,
        this_computer_only: true,
    }));
    let answers = logout_answers(&harness);

    assert_eq!(
        answers,
        [Event::LogoutRefused {
            request: LogoutRequest {
                discard_room_keys: false,
                this_computer_only: true,
            },
            reason: LogoutRefusal::RoomKeysAtRisk(RoomKeysAtRisk::RecoveryOff),
        }]
    );
    assert!(harness.has_store());

    harness.shut_down();
}
