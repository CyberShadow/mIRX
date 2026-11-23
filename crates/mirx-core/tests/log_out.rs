//! Log Out and the room keys: without a key backup a new session could
//! open, the core asks before it loses them.

mod common;

use common::{Harness, log_out_anyway};
use mirx_core::{Command, E2eeRecoveryState, Event, LogoutRequest, Operation, RoomKeysAtRisk};
use serde_json::json;
use wiremock::matchers::{method, path_regex};
use wiremock::{Mock, ResponseTemplate};

const ROOM: &str = "!a:example.org";

fn log_out() -> Command {
    Command::Logout(LogoutRequest {
        discard_room_keys: false,
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
                Event::LogoutWouldLoseRoomKeys(_)
                    | Event::LoggingOut
                    | Event::LoggedOut
                    | Event::SyncStarted
                    | Event::SyncStopped(_)
                    | Event::Error(_)
            )
        })
        .collect();
    assert_eq!(
        answers,
        [&Event::LogoutWouldLoseRoomKeys(RoomKeysAtRisk::RecoveryOff)],
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
    let events = harness.events_until("LoggedOut", |event| *event == Event::LoggedOut);
    let logging_out = events.iter().filter(|event| **event == Event::LoggingOut);
    assert_eq!(logging_out.count(), 1, "{events:#?}");
    assert_eq!(harness.requests("POST", "/logout").len(), 1);

    harness.shut_down();
}

/// A server that answers `/logout` with errors, as a proxy in front of a
/// stopped server does: the logout fails in seconds, and the session goes
/// on, syncing again.
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
                    | Event::LoggedOut
                    | Event::SyncStarted
                    | Event::SyncStopped(_)
                    | Event::Error(_)
            )
        })
        .collect();
    assert!(
        matches!(
            answers[..],
            [Event::LoggingOut, Event::Error(error), Event::SyncStarted]
                if error.operation == Operation::Logout && error.message.contains("restarting")
        ),
        "{events:#?}"
    );
    // The first request and 4 retries.
    assert_eq!(harness.requests("POST", "/logout").len(), 5);

    harness.shut_down();
}
