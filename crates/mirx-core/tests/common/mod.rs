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
    Command, CommandId, CoreConfig, CoreHandle, E2eeRecoveryState, Event, LogoutRequest,
    RestoreSessionRequest, SessionAuth, StoredSession, start,
};
use serde_json::json;
use tempfile::TempDir;
use wiremock::matchers::{method, path_regex};
use wiremock::{Mock, Request, ResponseTemplate};

pub const EVENT_TIMEOUT: Duration = Duration::from_secs(20);

/// Log Out as File > Log Out's warning lets the user: whatever recovery
/// and the key backup say. The tests of what Log Out does to the session,
/// whose mocks have no recovery, use it.
pub fn log_out_anyway() -> Command {
    Command::Logout(LogoutRequest {
        discard_room_keys: true,
        this_computer_only: false,
    })
}

pub struct Harness {
    pub runtime: tokio::runtime::Runtime,
    pub server: MatrixMockServer,
    pub core: CoreHandle,
    /// The restored session; `None` for a core that never had one.
    session: Option<StoredSession>,
    profile: TempDir,
}

impl Harness {
    /// A core with no session, and a server that knows its versions and
    /// answers every `/sync` with nothing.
    pub fn idle() -> Self {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("build tokio runtime");
        let server = runtime.block_on(async {
            let server = MatrixMockServer::new().await;
            server.mock_versions().ok().mount().await;
            server
                .mock_sync()
                .expect_any_access_token()
                .ok(|_| {})
                .mount()
                .await;
            server
        });
        let profile = TempDir::new().expect("temporary profile directory");
        let core = start_core(&profile);
        let harness = Self {
            runtime,
            server,
            core,
            session: None,
            profile,
        };
        harness.wait_for("Started", |event| *event == Event::Started);
        harness
    }

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

    /// A password session, and the events of its restore; every `/sync`
    /// answers with what `sync` builds.
    pub fn restored_with_sync(sync: impl FnOnce(&mut SyncResponseBuilder)) -> (Self, Vec<Event>) {
        Self::restored_with_events(SessionAuth::Matrix, sync)
    }

    fn restored_with(auth: SessionAuth, sync: impl FnOnce(&mut SyncResponseBuilder)) -> Self {
        Self::restored_with_events(auth, sync).0
    }

    fn restored_with_events(
        auth: SessionAuth,
        sync: impl FnOnce(&mut SyncResponseBuilder),
    ) -> (Self, Vec<Event>) {
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
            (server, "TOKEN_1".to_owned())
        });
        let profile = TempDir::new().expect("temporary profile directory");
        let core = start_core(&profile);
        let harness = Self {
            runtime,
            server,
            core,
            session: Some(StoredSession {
                user_id: alice.to_string(),
                device_id: "MIRX".to_owned(),
                access_token,
                refresh_token: matches!(auth, SessionAuth::OAuth { .. })
                    .then(|| "REFRESH".to_owned()),
                auth,
            }),
            profile,
        };
        let events = harness.restore();
        (harness, events)
    }

    /// Shuts the core down and starts another on the same profile,
    /// restored into the same session, as the app is after a restart; and
    /// the events of the restore, up to its ack. The store has synced, so
    /// the restore reads it and asks the server nothing: its E2EE status
    /// comes once a sync reaches the server.
    pub fn restarted(self) -> (Self, Vec<Event>) {
        let harness = self.restarted_core();
        let events = harness.restore_stored(&harness.server.uri());
        (harness, events)
    }

    /// The same, with the network down: the restore's homeserver URL is a
    /// port nothing listens on, so every request is refused.
    pub fn restarted_offline(self) -> (Self, Vec<Event>) {
        let harness = self.restarted_core();
        let events = harness.restore_stored(&closed_port_url());
        (harness, events)
    }

    /// Shuts the core down and starts another on the same profile.
    fn restarted_core(self) -> Self {
        self.send(Command::Shutdown);
        self.wait_for("Stopped", |event| *event == Event::Stopped);
        let Self {
            runtime,
            server,
            core,
            session,
            profile,
        } = self;
        core.join().expect("join core");
        Self {
            runtime,
            server,
            core: start_core(&profile),
            session,
            profile,
        }
    }

    /// Every event of the restore of a new profile, up to its ack: its
    /// store has never synced, so the restore syncs once and reads the
    /// E2EE status before the ack.
    fn restore(&self) -> Vec<Event> {
        let restore = self.send_restore(&self.server.uri());
        let mut events =
            self.events_until("RestoredSession", |event| *event == Event::RestoredSession);
        events.extend(self.events_until("the session's E2EE status", |event| {
            matches!(event, Event::E2eeStatus(_))
        }));
        events.extend(self.events_until("the restore's ack", |event| {
            *event == Event::CommandHandled(restore)
        }));
        events
    }

    /// Every event of the restore of a profile whose store has synced, up
    /// to its ack.
    fn restore_stored(&self, homeserver_url: &str) -> Vec<Event> {
        let restore = self.send_restore(homeserver_url);
        self.events_until("the restore's ack", |event| {
            *event == Event::CommandHandled(restore)
        })
    }

    fn send_restore(&self, homeserver_url: &str) -> CommandId {
        self.wait_for("Started", |event| *event == Event::Started);
        self.send(Command::RestoreSession(RestoreSessionRequest {
            homeserver_url: homeserver_url.to_owned(),
            session: self.session.clone().expect("a restored harness's session"),
            start_sync: false,
        }))
    }

    pub fn send(&self, command: Command) -> CommandId {
        self.core
            .command_sender()
            .send(command)
            .expect("send command")
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

    /// Every event that comes within `period`: what background tasks say
    /// after a moment.
    pub fn events_for(&self, period: Duration) -> Vec<Event> {
        let deadline = Instant::now() + period;
        let mut events = Vec::new();
        while let Ok(event) = self
            .core
            .events()
            .recv_timeout(deadline.saturating_duration_since(Instant::now()))
        {
            events.push(event);
        }
        events
    }

    pub fn wait_for(&self, what: &str, matches: impl Fn(&Event) -> bool) -> Event {
        self.events_until(what, matches)
            .pop()
            .expect("the matching event")
    }

    /// Asks for the E2EE status until recovery is `state`: it is `Unknown`
    /// until the SDK's E2EE setup of the session has read it.
    pub fn wait_for_recovery(&self, state: E2eeRecoveryState) {
        let deadline = Instant::now() + EVENT_TIMEOUT;
        loop {
            let refresh = self.send(Command::RefreshE2eeStatus);
            let events = self.events_until("the status's ack", |event| {
                *event == Event::CommandHandled(refresh)
            });
            if events
                .iter()
                .any(|event| matches!(event, Event::E2eeStatus(status) if status.recovery == state))
            {
                return;
            }
            assert!(Instant::now() < deadline, "recovery never {state:?}");
            std::thread::sleep(Duration::from_millis(20));
        }
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

    /// Whether the session's store is on disk.
    pub fn has_store(&self) -> bool {
        self.profile.path().join("profile/matrix-store").is_dir()
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

/// A URL of a local port nothing listens on: the network is down.
fn closed_port_url() -> String {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind a local port");
    let port = listener.local_addr().expect("the bound address").port();
    drop(listener);
    format!("http://127.0.0.1:{port}")
}

fn start_core(profile: &TempDir) -> CoreHandle {
    start(CoreConfig {
        profile_dir: profile.path().join("profile"),
        store_passphrase: "test-store-passphrase".to_owned(),
    })
    .expect("start core")
}

pub fn alice_member_event() -> serde_json::Value {
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
