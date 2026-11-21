//! Resetting your E2EE identity (`ResetE2eeIdentity`) against a mock
//! homeserver: with the account password, with approval in the browser,
//! and cancelled. The core runs on its own thread, as in the app.
//!
//! The account already has a cross-signing identity, set up by another
//! session the user no longer has: the case a reset is for. The mock
//! server's crypto preset keeps the keys it is given; the mocks of each
//! test answer the reset's cross-signing upload before it.

use std::time::{Duration, Instant};

use matrix_sdk::ruma::{owned_device_id, owned_user_id};
use matrix_sdk::test_utils::mocks::MatrixMockServer;
use mirx_core::{
    Command, CoreConfig, CoreErrorEvent, CoreHandle, E2eeIdentityResetOutcome, Event, Operation,
    ResetE2eeIdentityRequest, RestoreSessionRequest, SessionAuth, StoredSession, start,
};
use serde_json::json;
use tempfile::TempDir;
use wiremock::matchers::{body_partial_json, method, path_regex};
use wiremock::{Mock, ResponseTemplate};

const EVENT_TIMEOUT: Duration = Duration::from_secs(20);

const UPLOAD: &str = r"^/_matrix/client/.*/keys/device_signing/upload$";

struct Harness {
    runtime: tokio::runtime::Runtime,
    server: MatrixMockServer,
    core: CoreHandle,
    _profile: TempDir,
}

impl Harness {
    /// The core, restored into a password session of `@alice:example.org`
    /// whose identity another of her sessions set up.
    fn restored() -> Self {
        Self::restored_with(SessionAuth::Matrix)
    }

    /// The same, signed in through OAuth 2.0.
    fn restored_oauth() -> Self {
        Self::restored_with(SessionAuth::OAuth {
            client_id: "restored-client".to_owned(),
        })
    }

    fn restored_with(auth: SessionAuth) -> Self {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("build tokio runtime");
        let alice = owned_user_id!("@alice:example.org");
        let (server, access_token) = runtime.block_on(async {
            let server = MatrixMockServer::new().await;
            server.mock_crypto_endpoints_preset().await;
            let other_session = server
                .client_builder_for_crypto_end_to_end(&alice, &owned_device_id!("L0ST"))
                .build()
                .await;
            other_session
                .encryption()
                .bootstrap_cross_signing(None)
                .await
                .expect("set up the identity");
            // The crypto preset knows the session by its token: the next
            // one it hands out.
            drop(server.client_builder_for_crypto_end_to_end(&alice, &owned_device_id!("MIRX")));
            server.mock_versions().ok().mount().await;
            server.oauth().mock_server_metadata().ok().mount().await;
            server
                .mock_sync()
                .expect_any_access_token()
                .ok(|_| {})
                .mount()
                .await;
            mount_account_data_and_backups(&server).await;
            (server, "TOKEN_1".to_owned())
        });
        let profile = TempDir::new().expect("temporary profile directory");
        let core = start(CoreConfig {
            profile_dir: profile.path().join("profile"),
            store_passphrase: "test-store-passphrase".to_owned(),
        })
        .expect("start core");
        let harness = Self {
            runtime,
            server,
            core,
            _profile: profile,
        };
        harness.wait_for("Started", |event| *event == Event::Started);
        harness.send(Command::RestoreSession(RestoreSessionRequest {
            homeserver_url: harness.server.uri(),
            session: StoredSession {
                user_id: alice.to_string(),
                device_id: "MIRX".to_owned(),
                access_token,
                refresh_token: matches!(auth, SessionAuth::OAuth { .. })
                    .then(|| "REFRESH".to_owned()),
                auth,
            },
            start_sync: false,
        }));
        harness.wait_for("RestoredSession", |event| *event == Event::RestoredSession);
        harness.wait_for("the session's E2EE status", |event| {
            matches!(event, Event::E2eeStatus(_))
        });
        harness
    }

    fn send(&self, command: Command) {
        self.core
            .command_sender()
            .send(command)
            .expect("send command");
    }

    /// Every event up to and including the first that `matches`.
    fn events_until(&self, what: &str, matches: impl Fn(&Event) -> bool) -> Vec<Event> {
        let deadline = Instant::now() + EVENT_TIMEOUT;
        let mut events = Vec::new();
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            match self.core.events().recv_timeout(left) {
                Ok(event) => {
                    let done = matches(&event);
                    events.push(event);
                    if done {
                        return events;
                    }
                }
                Err(err) => panic!("no {what} ({err}) after {events:#?}"),
            }
        }
    }

    fn wait_for(&self, what: &str, matches: impl Fn(&Event) -> bool) -> Event {
        self.events_until(what, matches)
            .pop()
            .expect("the matching event")
    }

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

    fn mount(&self, mock: Mock) {
        self.runtime.block_on(mock.mount(self.server.server()));
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

    fn shut_down(self) {
        self.send(Command::Shutdown);
        self.wait_for("Stopped", |event| *event == Event::Stopped);
        self.core.join().expect("join core");
        // The mocks' expectations are checked as the server drops.
        self.runtime.block_on(async move { drop(self.server) });
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

/// What a reset and the SDK's backup handling ask besides the keys: the
/// account data it writes and reads, and a server without a key backup
/// until one is created.
async fn mount_account_data_and_backups(server: &MatrixMockServer) {
    Mock::given(method("PUT"))
        .and(path_regex(r"^/_matrix/client/.*/user/.*/account_data/.*"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({})))
        .mount(server.server())
        .await;
    Mock::given(method("GET"))
        .and(path_regex(r"^/_matrix/client/.*/user/.*/account_data/.*"))
        .respond_with(ResponseTemplate::new(404).set_body_json(json!({
            "errcode": "M_NOT_FOUND",
            "error": "Account data not found",
        })))
        .mount(server.server())
        .await;
    Mock::given(method("GET"))
        .and(path_regex(r"^/_matrix/client/.*/room_keys/version$"))
        .respond_with(ResponseTemplate::new(404).set_body_json(json!({
            "errcode": "M_NOT_FOUND",
            "error": "No current backup version",
        })))
        .mount(server.server())
        .await;
    Mock::given(method("POST"))
        .and(path_regex(r"^/_matrix/client/.*/room_keys/version$"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "version": "1" })))
        .mount(server.server())
        .await;
}

fn uiaa(flows: serde_json::Value) -> ResponseTemplate {
    ResponseTemplate::new(401).set_body_json(json!({
        "flows": flows,
        "params": {},
        "session": "oFIJVvtEOCKmRUTYKTYIIPHL",
    }))
}

fn password_flow() -> serde_json::Value {
    json!([{ "stages": ["m.login.password"] }])
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
