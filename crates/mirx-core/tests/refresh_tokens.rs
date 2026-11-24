//! Refresh tokens as Synapse hands them out: single use, an access token
//! that lives minutes, and a refresh that rotates both. The core runs on
//! its own thread against `Synapse`, a mock homeserver that keeps the
//! tokens' history as Synapse does.

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use matrix_sdk::test_utils::mocks::MatrixMockServer;
use mirx_core::{
    Command, CommandId, CoreConfig, CoreHandle, Event, RestoreSessionRequest, SessionAuth,
    StoredSession, SyncStopReason, start,
};
use serde_json::json;
use tempfile::TempDir;
use wiremock::matchers::{method, path, path_regex};
use wiremock::{Match, Mock, Request, Respond, ResponseTemplate};

const EVENT_TIMEOUT: Duration = Duration::from_secs(20);

/// One refresh's tokens, and what happened to them since.
struct Generation {
    access_token: String,
    refresh_token: String,
    /// The generation its refresh made, if it was refreshed.
    next: Option<usize>,
    /// Whether a request used the access token.
    used: bool,
    /// The access token's lifetime is over.
    expired: bool,
    /// Gone from the server: a predecessor of the last two.
    deleted: bool,
}

/// The tokens' history as Synapse keeps it (synapse/handlers/auth.py
/// `refresh_token`, storage/databases/main/registration.py
/// `lookup_refresh_token` and `replace_refresh_token`).
#[derive(Default)]
struct Ledger {
    generations: Vec<Generation>,
    refreshes: usize,
    /// The access tokens expire as the next `/sync` arrives.
    expire_at_next_sync: bool,
}

impl Ledger {
    fn issue(&mut self) -> usize {
        let index = self.generations.len();
        self.generations.push(Generation {
            access_token: format!("AT{index}"),
            refresh_token: format!("RT{index}"),
            next: None,
            used: false,
            expired: false,
            deleted: false,
        });
        index
    }

    /// Whether a request with `access_token` is let through; one that is
    /// marks the token used.
    fn accepts(&mut self, access_token: &str) -> bool {
        match self.generations.iter_mut().find(|generation| {
            generation.access_token == access_token && !generation.deleted && !generation.expired
        }) {
            Some(generation) => {
                generation.used = true;
                true
            }
            None => false,
        }
    }

    /// Every access token there is has lived its 5 minutes.
    fn expire(&mut self) {
        for generation in &mut self.generations {
            generation.expired = true;
        }
    }

    fn refresh(&mut self, refresh_token: &str) -> ResponseTemplate {
        self.refreshes += 1;
        let Some(index) = self.generations.iter().position(|generation| {
            generation.refresh_token == refresh_token && !generation.deleted
        }) else {
            return error(401, "M_UNKNOWN_TOKEN", "refresh token does not exist");
        };
        if let Some(next) = self.generations[index].next {
            let next = &mut self.generations[next];
            if next.used || next.next.is_some() {
                return error(403, "M_FORBIDDEN", "refresh token isn't valid anymore");
            }
            // The refresh was interrupted: its tokens are replaced.
            next.deleted = true;
        }
        let new = self.issue();
        self.generations[index].next = Some(new);
        for generation in &mut self.generations {
            if generation.next == Some(index) {
                generation.deleted = true;
            }
        }
        let generation = &self.generations[new];
        ResponseTemplate::new(200).set_body_json(json!({
            "access_token": generation.access_token,
            "refresh_token": generation.refresh_token,
            "expires_in_ms": 300_000,
        }))
    }
}

fn error(status: u16, errcode: &str, message: &str) -> ResponseTemplate {
    ResponseTemplate::new(status).set_body_json(json!({ "errcode": errcode, "error": message }))
}

fn bearer(request: &Request) -> Option<&str> {
    request
        .headers
        .get("authorization")?
        .to_str()
        .ok()?
        .strip_prefix("Bearer ")
}

#[derive(Clone)]
struct SharedLedger(Arc<Mutex<Ledger>>);

/// Matches an authenticated request the server refuses: its access token
/// expired or is gone.
struct Refused(SharedLedger);

impl Match for Refused {
    fn matches(&self, request: &Request) -> bool {
        let mut ledger = self.0.0.lock().unwrap();
        if ledger.expire_at_next_sync && request.url.path().ends_with("/sync") {
            ledger.expire_at_next_sync = false;
            ledger.expire();
        }
        bearer(request).is_some_and(|token| !ledger.accepts(token))
    }
}

/// Synapse's answer to a refused access token. A sync's answer takes
/// `sync_delay`.
struct SoftLogout {
    sync_delay: Duration,
}

impl Respond for SoftLogout {
    fn respond(&self, request: &Request) -> ResponseTemplate {
        let delay = if request.url.path().ends_with("/sync") {
            self.sync_delay
        } else {
            Duration::ZERO
        };
        ResponseTemplate::new(401)
            .set_body_json(json!({
                "errcode": "M_UNKNOWN_TOKEN",
                "error": "Access token has expired",
                "soft_logout": true,
            }))
            .set_delay(delay)
    }
}

struct Refresh(SharedLedger);

impl Respond for Refresh {
    fn respond(&self, request: &Request) -> ResponseTemplate {
        let body: serde_json::Value =
            serde_json::from_slice(&request.body).expect("a refresh request's JSON body");
        let refresh_token = body["refresh_token"]
            .as_str()
            .expect("a refresh request names its refresh token");
        self.0.0.lock().unwrap().refresh(refresh_token)
    }
}

/// A homeserver of `@alice:example.org` with Synapse's tokens. Its first
/// generation, `AT0` and `RT0`, is the session mirx saved.
struct Synapse {
    runtime: tokio::runtime::Runtime,
    server: MatrixMockServer,
    ledger: SharedLedger,
}

impl Synapse {
    fn new(sync_delay: Duration) -> Self {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("build tokio runtime");
        let ledger = SharedLedger(Arc::new(Mutex::new(Ledger::default())));
        ledger.0.lock().unwrap().issue();
        let server = runtime.block_on(async {
            let server = MatrixMockServer::new().await;
            Mock::given(Refused(ledger.clone()))
                .respond_with(SoftLogout { sync_delay })
                .with_priority(1)
                .mount(server.server())
                .await;
            Mock::given(method("POST"))
                .and(path("/_matrix/client/v3/refresh"))
                .respond_with(Refresh(ledger.clone()))
                .mount(server.server())
                .await;
            server.mock_versions().ok().mount().await;
            server
                .mock_sync()
                .expect_any_access_token()
                .ok(|_| {})
                .mount()
                .await;
            Mock::given(method("POST"))
                .and(path_regex(r"^/_matrix/client/v3/keys/upload$"))
                .respond_with(
                    ResponseTemplate::new(200).set_body_json(json!({ "one_time_key_counts": {} })),
                )
                .mount(server.server())
                .await;
            Mock::given(method("POST"))
                .and(path_regex(r"^/_matrix/client/v3/keys/query$"))
                .respond_with(ResponseTemplate::new(200).set_body_json(json!({})))
                .mount(server.server())
                .await;
            Mock::given(method("PUT"))
                .and(path_regex(r"^/_matrix/client/v3/profile/.*/displayname$"))
                .respond_with(ResponseTemplate::new(200).set_body_json(json!({})))
                .mount(server.server())
                .await;
            server
        });
        Self {
            runtime,
            server,
            ledger,
        }
    }

    fn ledger(&self) -> std::sync::MutexGuard<'_, Ledger> {
        self.ledger.0.lock().unwrap()
    }

    /// The session of generation `index`, as mirx saves it.
    fn session(&self, index: usize) -> StoredSession {
        let ledger = self.ledger();
        let generation = &ledger.generations[index];
        StoredSession {
            user_id: "@alice:example.org".to_owned(),
            device_id: "MIRX".to_owned(),
            access_token: generation.access_token.clone(),
            refresh_token: Some(generation.refresh_token.clone()),
            auth: SessionAuth::Matrix,
        }
    }
}

impl Drop for Synapse {
    fn drop(&mut self) {
        // The server must drop inside its runtime.
        let server = std::mem::replace(
            &mut self.server,
            self.runtime.block_on(MatrixMockServer::new()),
        );
        self.runtime.block_on(async move { drop(server) });
    }
}

struct Mirx {
    core: CoreHandle,
    _profile: TempDir,
}

impl Mirx {
    /// A core on a new profile, restored into `session` and syncing.
    fn restored(server: &Synapse, session: StoredSession) -> Self {
        let profile = TempDir::new().expect("temporary profile directory");
        let core = start(CoreConfig {
            profile_dir: profile.path().join("profile"),
            store_passphrase: "test-store-passphrase".to_owned(),
        })
        .expect("start core");
        let mirx = Self {
            core,
            _profile: profile,
        };
        mirx.wait_for("Started", |event| *event == Event::Started);
        let _ = mirx.send(Command::RestoreSession(RestoreSessionRequest {
            homeserver_url: server.server.uri(),
            session,
            start_sync: true,
        }));
        mirx
    }

    fn send(&self, command: Command) -> CommandId {
        self.core
            .command_sender()
            .send(command)
            .expect("send command")
    }

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

    fn shut_down(self) {
        self.send(Command::Shutdown);
        self.wait_for("Stopped", |event| *event == Event::Stopped);
        self.core.join().expect("join core");
    }
}

/// The saved session's refresh token was used, and the access token it
/// gave too (by a run whose save was lost): Synapse refuses it with 403
/// `M_FORBIDDEN`, and no retry can help. Before, a refresh counted as
/// refused only when it answered `M_UNKNOWN_TOKEN`, so mirx reconnected
/// forever.
#[test]
fn a_used_refresh_token_ends_the_session() {
    let server = Synapse::new(Duration::ZERO);
    let saved = server.session(0);
    server.ledger().refresh("RT0");
    assert!(server.ledger().accepts("AT1"));
    server.ledger().expire();

    let mirx = Mirx::restored(&server, saved);

    let stopped = mirx.wait_for("SyncStopped", |event| {
        matches!(event, Event::SyncStopped(_))
    });
    assert_eq!(
        stopped,
        Event::SyncStopped(SyncStopReason::SessionEnded(
            "the server returned an error: [403 / M_FORBIDDEN] refresh token isn't valid anymore"
                .to_owned()
        ))
    );
    mirx.shut_down();
}
