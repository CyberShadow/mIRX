//! A core restored into a session of `@alice:example.org` against a mock
//! homeserver, for the tests of what a signed-in session does. The core
//! runs on its own thread, as in the app.
//!
//! The account already has a cross-signing identity, set up by another
//! session (`L0ST`) the user no longer has; this session is `MIRX`. The
//! mock server's crypto preset keeps the keys it is given.

// Every test binary compiles this module and uses a part of it.
#![allow(dead_code)]

use std::time::{Duration, Instant};

use matrix_sdk::ruma::serde::Raw;
use matrix_sdk::ruma::{owned_device_id, owned_user_id};
use matrix_sdk::test_utils::mocks::MatrixMockServer;
use matrix_sdk_test::{JoinedRoomBuilder, SyncResponseBuilder};
use mirx_core::{
    Command, CoreConfig, CoreHandle, Event, RestoreSessionRequest, SessionAuth, StoredSession,
    start,
};
use serde_json::json;
use tempfile::TempDir;
use wiremock::matchers::{method, path_regex};
use wiremock::{Mock, Request, ResponseTemplate};

pub const EVENT_TIMEOUT: Duration = Duration::from_secs(20);

pub struct Harness {
    pub runtime: tokio::runtime::Runtime,
    pub server: MatrixMockServer,
    pub core: CoreHandle,
    _profile: TempDir,
}

impl Harness {
    /// The core, restored into a password session.
    pub fn restored() -> Self {
        Self::restored_with(SessionAuth::Matrix, |_| {})
    }

    /// The same, signed in through OAuth 2.0.
    pub fn restored_oauth() -> Self {
        Self::restored_with(
            SessionAuth::OAuth {
                client_id: "restored-client".to_owned(),
            },
            |_| {},
        )
    }

    /// A password session joined to `rooms`, which are not encrypted and
    /// have only alice in them. Every `/sync` answers with them, and with
    /// `account_data` as the account's global account data.
    pub fn restored_in_rooms(rooms: &[&str], account_data: &[serde_json::Value]) -> Self {
        Self::restored_with(SessionAuth::Matrix, |sync| {
            for room in rooms {
                sync.add_joined_room(
                    JoinedRoomBuilder::new(
                        &matrix_sdk::ruma::OwnedRoomId::try_from(*room).expect("a room ID"),
                    )
                    .add_state_bulk([Raw::from_json_string(alice_member_event().to_string())
                        .expect("a raw member event")]),
                );
            }
            for event in account_data {
                sync.add_custom_global_account_data(event.clone());
            }
        })
    }

    fn restored_with(auth: SessionAuth, sync: impl FnOnce(&mut SyncResponseBuilder)) -> Self {
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
                .ok(sync)
                .mount()
                .await;
            mount_account_data_and_backups(&server).await;
            server
                .mock_room_state_encryption()
                .expect_any_access_token()
                .plain()
                .mount()
                .await;
            Mock::given(method("GET"))
                .and(path_regex(r"^/_matrix/client/v3/rooms/.*/members"))
                .respond_with(
                    ResponseTemplate::new(200)
                        .set_body_json(json!({ "chunk": [alice_member_event()] })),
                )
                .mount(server.server())
                .await;
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
        harness.wait_for("the restore's ack", |event| *event == Event::CommandHandled);
        harness
    }

    pub fn send(&self, command: Command) {
        self.core
            .command_sender()
            .send(command)
            .expect("send command");
    }

    /// Every event up to and including the first that `matches`.
    pub fn events_until(&self, what: &str, matches: impl Fn(&Event) -> bool) -> Vec<Event> {
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

    pub fn wait_for(&self, what: &str, matches: impl Fn(&Event) -> bool) -> Event {
        self.events_until(what, matches)
            .pop()
            .expect("the matching event")
    }

    pub fn mount(&self, mock: Mock) {
        self.runtime.block_on(mock.mount(self.server.server()));
    }

    /// Mounts a mock that answers `method` requests to paths matching
    /// `path` only after an hour: a request the server never answers in the
    /// test's time.
    pub fn hang(&self, method_name: &str, path: &str) {
        self.mount(
            Mock::given(method(method_name))
                .and(path_regex(path))
                .respond_with(ResponseTemplate::new(200).set_delay(Duration::from_secs(3600)))
                .with_priority(1),
        );
    }

    /// The requests the server got so far for `method` and paths that
    /// contain `path`, in arrival order.
    pub fn requests(&self, method_name: &str, path: &str) -> Vec<Request> {
        self.runtime
            .block_on(self.server.server().received_requests())
            .expect("the mock server records requests")
            .into_iter()
            .filter(|request| {
                (method_name.is_empty() || request.method.as_str() == method_name)
                    && request.url.path().contains(path)
            })
            .collect()
    }

    /// Waits until the server got `count` requests for `method` and paths
    /// that contain `path`, and returns them.
    pub fn wait_for_requests(&self, method_name: &str, path: &str, count: usize) -> Vec<Request> {
        let deadline = Instant::now() + EVENT_TIMEOUT;
        loop {
            let requests = self.requests(method_name, path);
            if requests.len() >= count {
                return requests;
            }
            assert!(
                Instant::now() < deadline,
                "only {} of {count} {method_name} {path} requests",
                requests.len()
            );
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    pub fn shut_down(self) {
        self.send(Command::Shutdown);
        self.wait_for("Stopped", |event| *event == Event::Stopped);
        self.join_stopped();
    }

    /// After `Stopped`: the core's thread ends.
    pub fn join_stopped(self) {
        self.core.join().expect("join core");
        // The mocks' expectations are checked as the server drops.
        self.runtime.block_on(async move { drop(self.server) });
    }
}

fn alice_member_event() -> serde_json::Value {
    json!({
        "type": "m.room.member",
        "state_key": "@alice:example.org",
        "sender": "@alice:example.org",
        "event_id": "$alice-join",
        "origin_server_ts": 1,
        "content": { "membership": "join", "displayname": "alice" },
    })
}

/// What the SDK's backup handling (and a reset) ask besides the keys: the
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

/// The server's user-interactive auth answer: these `flows`, in session
/// `oFIJVvtEOCKmRUTYKTYIIPHL`.
pub fn uiaa(flows: serde_json::Value) -> ResponseTemplate {
    ResponseTemplate::new(401).set_body_json(json!({
        "flows": flows,
        "params": {},
        "session": UIAA_SESSION,
    }))
}

pub const UIAA_SESSION: &str = "oFIJVvtEOCKmRUTYKTYIIPHL";

pub fn password_flow() -> serde_json::Value {
    json!([{ "stages": ["m.login.password"] }])
}
