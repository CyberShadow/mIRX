//! Signing in through the homeserver's web page (OAuth 2.0 and legacy SSO),
//! and the OAuth sessions it leaves, against a mock homeserver. The core
//! runs on its own thread, as in the app; this thread plays the browser.

use std::sync::mpsc::Receiver;
use std::time::{Duration, Instant};

use matrix_sdk::test_utils::mocks::MatrixMockServer;
use mirx_core::{
    Command, CoreConfig, CoreHandle, Event, Operation, RestoreSessionRequest, SessionAuth,
    StoredSession, start,
};
use tempfile::TempDir;

const EVENT_TIMEOUT: Duration = Duration::from_secs(20);

/// The core and the mock homeserver it talks to, with the runtime that
/// mounts the mocks.
struct Harness {
    runtime: tokio::runtime::Runtime,
    server: MatrixMockServer,
    core: CoreHandle,
    _profile: TempDir,
}

impl Harness {
    fn new(mount: impl AsyncFnOnce(&MatrixMockServer)) -> Self {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("build tokio runtime");
        let server = runtime.block_on(async {
            let server = MatrixMockServer::new().await;
            mount(&server).await;
            server
        });
        let profile = TempDir::new().expect("temporary profile directory");
        let core = start(CoreConfig {
            profile_dir: profile.path().join("profile"),
            store_passphrase: "test-store-passphrase".to_owned(),
        })
        .expect("start core");
        assert_eq!(next_event(core.events()), Event::Started);
        Self {
            runtime,
            server,
            core,
            _profile: profile,
        }
    }

    fn send(&self, command: Command) {
        self.core
            .command_sender()
            .send(command)
            .expect("send command");
    }

    /// Skips events until one `matches`, and returns it.
    fn wait_for(&self, what: &str, matches: impl Fn(&Event) -> bool) -> Event {
        let deadline = Instant::now() + EVENT_TIMEOUT;
        let mut skipped = Vec::new();
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            match self.core.events().recv_timeout(left) {
                Ok(event) if matches(&event) => return event,
                Ok(event) => skipped.push(event),
                Err(err) => panic!("no {what} ({err}) after {skipped:#?}"),
            }
        }
    }

    fn shut_down(self) {
        self.send(Command::Shutdown);
        self.wait_for("Stopped", |event| *event == Event::Stopped);
        self.core.join().expect("join core");
        // The mocks' expectations are checked as the server drops.
        self.runtime.block_on(async move { drop(self.server) });
    }
}

fn next_event(events: &Receiver<Event>) -> Event {
    events.recv_timeout(EVENT_TIMEOUT).expect("core event")
}

fn oauth_session(access_token: &str, refresh_token: &str) -> StoredSession {
    StoredSession {
        user_id: "@alice:example.org".to_owned(),
        device_id: "DEVICE".to_owned(),
        access_token: access_token.to_owned(),
        refresh_token: Some(refresh_token.to_owned()),
        auth: SessionAuth::OAuth {
            client_id: "restored-client".to_owned(),
        },
    }
}

fn restore(harness: &Harness, session: StoredSession) {
    harness.send(Command::RestoreSession(RestoreSessionRequest {
        homeserver_url: harness.server.uri(),
        session,
        start_sync: false,
    }));
}

#[test]
fn a_restored_oauth_session_saves_its_refreshed_tokens() {
    let harness = Harness::new(async |server| {
        server.mock_versions().ok().mount().await;
        server.oauth().mock_server_metadata().ok().mount().await;
        // The first request with the saved token finds it expired.
        server
            .mock_sync()
            .error_unknown_token(false)
            .up_to_n_times(1)
            .mount()
            .await;
        server
            .oauth()
            .mock_token()
            .ok_with_tokens("AT2", "RT2")
            .expect(1)
            .mount()
            .await;
        server.mock_sync().ok(|_| {}).mount().await;
    });

    restore(&harness, oauth_session("AT1", "RT1"));

    assert_eq!(
        harness.wait_for("SessionUpdated", |event| matches!(
            event,
            Event::SessionUpdated { .. }
        )),
        Event::SessionUpdated {
            session: oauth_session("AT2", "RT2"),
        }
    );
    harness.wait_for("RestoredSession", |event| *event == Event::RestoredSession);
    harness.shut_down();
}

/// The revocation itself cannot be mocked: the OAuth client refuses a
/// revocation endpoint that is not https (RFC 7009), while the mock server
/// is http. The refusal shows the logout went to the authorization server
/// rather than to the Matrix `/logout`.
#[test]
fn logging_out_of_an_oauth_session_goes_to_the_authorization_server() {
    let harness = Harness::new(async |server| {
        server.mock_versions().ok().mount().await;
        server.oauth().mock_server_metadata().ok().mount().await;
        server.mock_sync().ok(|_| {}).mount().await;
        server.mock_logout().ok().expect(0).mount().await;
    });

    restore(&harness, oauth_session("AT1", "RT1"));
    harness.wait_for("RestoredSession", |event| *event == Event::RestoredSession);
    harness.send(Command::Logout);
    let Event::Error(error) = harness.wait_for(
        "the logout error",
        |event| matches!(event, Event::Error(error) if error.operation == Operation::Logout),
    ) else {
        unreachable!()
    };
    assert_eq!(
        error.message,
        "failed to log out of Matrix: failed to log out: Scheme for revocation endpoint URL must be HTTPS"
    );
    harness.shut_down();
}
