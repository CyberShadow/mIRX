//! Resetting your E2EE identity (`ResetE2eeIdentity`) against a mock
//! homeserver: with the account password, with approval in the browser,
//! and cancelled. The core runs on its own thread, as in the app.
//!
//! The account already has a cross-signing identity, set up by another
//! session the user no longer has: the case a reset is for. The mock
//! server's crypto preset keeps the keys it is given; the mocks of each
//! test answer the reset's cross-signing upload before it.

mod common;

use std::time::{Duration, Instant};

use common::{Harness, password_flow, uiaa};
use matrix_sdk::test_utils::mocks::MatrixMockServer;
use mirx_core::{
    Command, CoreErrorEvent, E2eeIdentityResetOutcome, Event, Operation, ResetE2eeIdentityRequest,
};
use serde_json::json;
use wiremock::matchers::{body_partial_json, method, path_regex};
use wiremock::{Mock, ResponseTemplate};

const UPLOAD: &str = r"^/_matrix/client/.*/keys/device_signing/upload$";

impl Harness {
    fn reset(&self, password: Option<&str>) {
        self.send(Command::ResetE2eeIdentity(ResetE2eeIdentityRequest {
            password: password.map(str::to_owned),
        }));
    }

    /// The events of a reset up to the E2EE status after its answer.
    fn reset_answer(&self) -> Vec<Event> {
        let mut events = self.events_until("the reset's answer", is_reset_answer);
        events.extend(
            self.events_until("the E2EE status after the reset", |event| {
                matches!(event, Event::E2eeStatus(_))
            }),
        );
        events
    }

    /// How many cross-signing uploads the server got.
    fn uploads(&self) -> usize {
        self.runtime.block_on(async {
            self.server
                .server()
                .received_requests()
                .await
                .expect("the mock server records requests")
                .into_iter()
                .filter(|request| request.url.path().ends_with("/keys/device_signing/upload"))
                .count()
        })
    }

    /// How many account data events mirx wrote: a reset's first step
    /// turns recovery off with one.
    fn account_data_writes(&self) -> usize {
        self.runtime.block_on(async {
            self.server
                .server()
                .received_requests()
                .await
                .expect("the mock server records requests")
                .into_iter()
                .filter(|request| {
                    request.method == wiremock::http::Method::PUT
                        && request.url.path().contains("/account_data/")
                })
                .count()
        })
    }
}

fn is_reset_answer(event: &Event) -> bool {
    matches!(
        event,
        Event::E2eeIdentityReset(_)
            | Event::Error(CoreErrorEvent {
                operation: Operation::ResetE2eeIdentity,
                ..
            })
    )
}

/// Answers the reset's cross-signing upload ahead of the crypto preset.
fn upload() -> wiremock::MockBuilder {
    Mock::given(method("POST")).and(path_regex(UPLOAD))
}

fn oauth_stage(server: &MatrixMockServer, stage: &str) -> ResponseTemplate {
    ResponseTemplate::new(401).set_body_json(json!({
        "session": "dummy",
        "flows": [{ "stages": [stage] }],
        "params": {
            stage: {
                "url": format!("{}/account/?action=org.matrix.cross_signing_reset", server.uri()),
            }
        },
        "msg": "To reset your end-to-end encryption cross-signing identity, you first need to approve it and then try again.",
    }))
}

fn reset_outcomes(events: &[Event]) -> Vec<E2eeIdentityResetOutcome> {
    events
        .iter()
        .filter_map(|event| match event {
            Event::E2eeIdentityReset(outcome) => Some(*outcome),
            _ => None,
        })
        .collect()
}

fn reset_errors(events: &[Event]) -> Vec<&str> {
    events
        .iter()
        .filter_map(|event| match event {
            Event::Error(error) if error.operation == Operation::ResetE2eeIdentity => {
                Some(error.message.as_str())
            }
            _ => None,
        })
        .collect()
}

#[test]
fn a_password_confirms_the_reset() {
    let harness = Harness::restored();
    harness.mount(
        upload()
            .respond_with(uiaa(password_flow()))
            .up_to_n_times(1)
            .expect(1)
            .with_priority(1),
    );
    harness.mount(
        upload()
            .and(body_partial_json(json!({
                "auth": {
                    "type": "m.login.password",
                    "identifier": { "type": "m.id.user", "user": "@alice:example.org" },
                    "password": "hunter2",
                    "session": "oFIJVvtEOCKmRUTYKTYIIPHL",
                }
            })))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({})))
            .expect(1)
            .with_priority(1),
    );

    harness.reset(Some("hunter2"));
    let events = harness.reset_answer();
    assert_eq!(
        reset_outcomes(&events),
        vec![E2eeIdentityResetOutcome::Done],
        "{events:#?}"
    );
    assert_eq!(reset_errors(&events), Vec::<&str>::new());
    assert!(
        !events
            .iter()
            .any(|event| matches!(event, Event::E2eeIdentityResetApproval { .. })),
        "a password needs no browser: {events:#?}"
    );
    harness.shut_down();
}

#[test]
fn a_wrong_password_fails_the_reset() {
    let harness = Harness::restored();
    harness.mount(
        upload()
            .respond_with(uiaa(password_flow()))
            .up_to_n_times(1)
            .with_priority(1),
    );
    harness.mount(
        upload()
            .respond_with(ResponseTemplate::new(401).set_body_json(json!({
                "errcode": "M_FORBIDDEN",
                "error": "Invalid password",
                "flows": password_flow(),
                "params": {},
                "session": "oFIJVvtEOCKmRUTYKTYIIPHL",
            })))
            .expect(1)
            .with_priority(1),
    );

    harness.reset(Some("wrong"));
    let events = harness.reset_answer();
    assert_eq!(reset_outcomes(&events), Vec::new(), "{events:#?}");
    let errors = reset_errors(&events);
    assert_eq!(
        errors,
        vec![
            "failed to reset your E2EE identity: Invalid password; your key backup and recovery \
             were turned off before the server was asked"
        ],
        "{events:#?}"
    );
    assert_keeps_old_identity(&events);
    harness.shut_down();
}

/// The reset would delete the backup before the server asked for the
/// password: without one, a password session is refused untouched.
#[test]
fn a_password_session_without_a_password_is_refused_before_anything_changes() {
    let harness = Harness::restored();
    let uploads = harness.uploads();
    let account_data_writes = harness.account_data_writes();
    harness.reset(None);
    assert_eq!(
        harness.wait_for("the refusal", is_reset_answer),
        Event::Error(CoreErrorEvent {
            operation: Operation::ResetE2eeIdentity,
            room_id: None,
            message: "resetting your E2EE identity needs your account password".to_owned(),
        })
    );
    assert_eq!(harness.uploads(), uploads);
    assert_eq!(harness.account_data_writes(), account_data_writes);
    harness.shut_down();
}

#[test]
fn a_server_asking_for_more_than_the_password_is_refused() {
    let harness = Harness::restored();
    harness.mount(
        upload()
            .respond_with(uiaa(json!([
                { "stages": ["m.login.password", "m.login.email.identity"] },
                { "stages": ["m.login.sso"] },
            ])))
            .expect(1)
            .with_priority(1),
    );

    let uploads = harness.uploads();
    harness.reset(Some("hunter2"));
    let events = harness.reset_answer();
    assert_eq!(
        reset_errors(&events),
        vec![
            "failed to reset your E2EE identity: your server asks to confirm with \
             m.login.password + m.login.email.identity, or m.login.sso; mirx can confirm \
             with your password only; your key backup and recovery were turned off before the \
             server was asked"
        ]
    );
    assert_eq!(harness.uploads(), uploads + 1);
    harness.shut_down();
}

/// The reset waits for the browser beside the worker: other commands are
/// answered meanwhile, and the next upload after the approval completes it.
fn approval_in_the_browser_completes_the_reset(stage: &str) {
    let harness = Harness::restored_oauth();
    // Three refused uploads, then the crypto preset takes the keys.
    harness.mount(
        upload()
            .respond_with(oauth_stage(&harness.server, stage))
            .up_to_n_times(4)
            .expect(4)
            .with_priority(1),
    );

    harness.reset(None);
    let Event::E2eeIdentityResetApproval { url } = harness.wait_for("the approval page", |event| {
        matches!(event, Event::E2eeIdentityResetApproval { .. })
    }) else {
        unreachable!()
    };
    assert_eq!(
        url,
        format!(
            "{}/account/?action=org.matrix.cross_signing_reset",
            harness.server.uri()
        )
    );
    harness.send(Command::RefreshE2eeStatus);
    let events = harness.events_until("the refreshed status, or the reset's end", |event| {
        matches!(event, Event::E2eeStatus(_) | Event::E2eeIdentityReset(_))
    });
    assert!(
        matches!(events.last(), Some(Event::E2eeStatus(_))),
        "the worker answers while the reset waits: {events:#?}"
    );
    harness.reset(None);
    assert_eq!(
        harness.wait_for("the refusal of a second reset", is_reset_answer),
        Event::Error(CoreErrorEvent {
            operation: Operation::ResetE2eeIdentity,
            room_id: None,
            message: "an identity reset is already under way; /e2ee reset cancel stops it"
                .to_owned(),
        })
    );

    let events = harness.reset_answer();
    assert_eq!(
        reset_outcomes(&events),
        vec![E2eeIdentityResetOutcome::Done],
        "{events:#?}"
    );
    harness.shut_down();
}

#[test]
fn approval_in_the_browser_completes_the_reset_with_the_stable_stage() {
    approval_in_the_browser_completes_the_reset("m.oauth");
}

#[test]
fn approval_in_the_browser_completes_the_reset_with_the_unstable_stage() {
    approval_in_the_browser_completes_the_reset("org.matrix.cross_signing_reset");
}

#[test]
fn cancelling_a_reset_that_waits_for_the_browser_keeps_the_old_identity() {
    let harness = Harness::restored_oauth();
    harness.mount(
        upload()
            .respond_with(oauth_stage(&harness.server, "m.oauth"))
            .with_priority(1),
    );
    harness.send(Command::CancelE2eeIdentityReset);
    assert_eq!(
        harness.wait_for("the refusal of a stray cancel", is_reset_answer),
        Event::Error(CoreErrorEvent {
            operation: Operation::ResetE2eeIdentity,
            room_id: None,
            message: "no identity reset is under way".to_owned(),
        })
    );
    harness.reset(Some("hunter2"));
    assert_eq!(
        harness.wait_for("the refusal of a password", is_reset_answer),
        Event::Error(CoreErrorEvent {
            operation: Operation::ResetE2eeIdentity,
            room_id: None,
            message: "your server confirms an identity reset in the browser, not with a password"
                .to_owned(),
        })
    );

    harness.reset(None);
    harness.wait_for("the approval page", |event| {
        matches!(event, Event::E2eeIdentityResetApproval { .. })
    });
    let cancelled_at = Instant::now();
    harness.send(Command::CancelE2eeIdentityReset);
    let events = harness.reset_answer();
    assert!(
        cancelled_at.elapsed() < Duration::from_secs(5),
        "the cancel ends the wait: {:?}",
        cancelled_at.elapsed()
    );
    assert_eq!(
        reset_outcomes(&events),
        vec![E2eeIdentityResetOutcome::Cancelled],
        "{events:#?}"
    );
    assert_keeps_old_identity(&events);
    harness.shut_down();
}

/// The server still has the old identity, whose keys this session does not
/// hold: the keys the reset created are gone again.
fn assert_keeps_old_identity(events: &[Event]) {
    let Some(Event::E2eeStatus(status)) = events.last() else {
        panic!("the reset ends with the status: {events:#?}");
    };
    assert!(
        !status
            .cross_signing
            .as_ref()
            .expect("the session has an OlmMachine")
            .has_master,
        "{status:?}"
    );
}
