//! A server that answers with errors, as a proxy in front of a stopped
//! homeserver does (502, 503): the client retries a request for seconds,
//! then the sync stops as lost and a command reports its error. The SDK's
//! default was to retry for 15 minutes.

mod common;

use common::Harness;
use mirx_core::{Command, Event, Operation, SyncStopReason};
use serde_json::json;
use wiremock::matchers::{method, path_regex};
use wiremock::{Mock, ResponseTemplate};

const ROOM: &str = "!a:example.org";

/// A proxy's answer while the server behind it is down.
fn proxy_error(status: u16) -> ResponseTemplate {
    ResponseTemplate::new(status).set_body_string(format!("<html>{status}</html>"))
}

#[test]
fn a_sync_the_server_answers_with_errors_is_a_lost_connection() {
    let harness = Harness::restored_in_rooms(&[ROOM], &[]);
    harness.send(Command::StartSync);
    harness.wait_for("SyncConnected", |event| *event == Event::SyncConnected);

    // Above the harness's sync mock, from now on.
    harness.mount(
        Mock::given(method("GET"))
            .and(path_regex(r"^/_matrix/client/v3/sync$"))
            .respond_with(proxy_error(502))
            .with_priority(1),
    );
    let stopped = harness.wait_for("SyncStopped", |event| {
        matches!(event, Event::SyncStopped(_))
    });

    let Event::SyncStopped(SyncStopReason::Lost(reason)) = stopped else {
        panic!("{stopped:?}");
    };
    assert!(reason.contains("502"), "{reason}");

    harness.shut_down();
}

#[test]
fn a_command_the_server_answers_with_errors_fails_in_seconds() {
    let harness = Harness::restored_in_rooms(&[ROOM], &[]);
    harness.mount(
        Mock::given(method("GET"))
            .and(path_regex(r"^/_matrix/client/v3/profile/@bob:example.org$"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(json!({ "displayname": "Bob" })),
            ),
    );
    harness.mount(
        Mock::given(method("GET"))
            .and(path_regex(
                r"^/_matrix/client/v3/presence/@bob:example.org/status$",
            ))
            .respond_with(proxy_error(503)),
    );

    harness.send(Command::FetchUserProfile("@bob:example.org".to_owned()));
    let events = harness.events_until("the profile's ack", |event| {
        matches!(event, Event::CommandHandled(_))
    });

    assert!(
        events.iter().any(|event| matches!(
            event,
            Event::Error(error) if error.operation == Operation::FetchUserProfile
                && error.message.contains("503")
        )),
        "{events:#?}"
    );
    // Retried, but not for long: the first request and 4 retries.
    assert_eq!(harness.requests("GET", "/presence/").len(), 5);

    harness.shut_down();
}
