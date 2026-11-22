//! Signing in through the homeserver's web page (OAuth 2.0 and legacy SSO),
//! and the OAuth sessions it leaves, against a mock homeserver. The core
//! runs on its own thread, as in the app; this thread plays the browser.

use std::io::{Read, Write};
use std::net::TcpStream;
use std::sync::mpsc::Receiver;
use std::time::{Duration, Instant};

use matrix_sdk::test_utils::mocks::MatrixMockServer;
use mirx_core::{
    Command, CoreConfig, CoreErrorEvent, CoreHandle, Event, LoginMethod, LoginRequest, Operation,
    RestoreSessionRequest, SessionAuth, StoredSession, SyncStopReason, start,
};
use tempfile::TempDir;
use url::Url;
use wiremock::matchers::{body_partial_json, method, path};
use wiremock::{Mock, ResponseTemplate};

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

impl Harness {
    fn browser_login(&self, user_id_hint: Option<&str>) {
        self.send(Command::Login(LoginRequest {
            server: self.server.uri(),
            method: LoginMethod::Browser {
                user_id_hint: user_id_hint.map(str::to_owned),
            },
            device_display_name: "mirx test".to_owned(),
            start_sync: false,
        }));
    }

    /// The page the core asks the browser to open.
    fn sign_in_url(&self) -> Url {
        let Event::SignInUrl { url } = self.wait_for("SignInUrl", |event| {
            matches!(event, Event::SignInUrl { .. })
        }) else {
            unreachable!()
        };
        // The login command is done once it gave the page; the redirect
        // that ends the sign-in is no command.
        assert_eq!(next_event(self.core.events()), Event::CommandHandled);
        Url::parse(&url).expect("the sign-in address is a URL")
    }

    fn login_error(&self) -> String {
        let Event::Error(CoreErrorEvent { message, .. }) = self.wait_for(
            "a Login error",
            |event| matches!(event, Event::Error(error) if error.operation == Operation::Login),
        ) else {
            unreachable!()
        };
        message
    }

    /// The bodies of the requests the homeserver got at `path`.
    fn request_bodies(&self, path: &str) -> Vec<serde_json::Value> {
        self.runtime.block_on(async {
            self.server
                .server()
                .received_requests()
                .await
                .expect("the mock server records requests")
                .into_iter()
                .filter(|request| request.url.path() == path)
                .map(|request| request.body_json().expect("a JSON body"))
                .collect()
        })
    }
}

/// The value of the `name` parameter of `url`'s query.
fn query_param(url: &Url, name: &str) -> Option<String> {
    url.query_pairs()
        .find_map(|(key, value)| (key == name).then(|| value.into_owned()))
}

/// What the homeserver's page does once the user signed in: sends the
/// browser to `redirect` with `query`. Returns the whole response.
fn browse(redirect: &Url, query: &str) -> String {
    let host = redirect.host_str().expect("the redirect names its host");
    let port = redirect.port().expect("the redirect names its port");
    let mut stream = TcpStream::connect((host, port)).expect("connect to the redirect");
    write!(
        stream,
        "GET {}?{query} HTTP/1.1\r\nHost: {host}:{port}\r\n\r\n",
        redirect.path()
    )
    .expect("send the request");
    let mut response = String::new();
    stream
        .read_to_string(&mut response)
        .expect("read the response");
    response
}

/// The OAuth 2.0 redirect URI in the authorization address.
fn oauth_redirect(sign_in: &Url) -> Url {
    Url::parse(&query_param(sign_in, "redirect_uri").expect("redirect_uri")).expect("a URL")
}

/// Signs in on the homeserver's page as the user who agrees: the redirect
/// carries a code and the authorization's state.
fn authorize(sign_in: &Url) -> String {
    let state = query_param(sign_in, "state").expect("state");
    browse(&oauth_redirect(sign_in), &format!("code=42&state={state}"))
}

async fn mount_oauth_server(server: &MatrixMockServer) {
    server.mock_versions().ok().mount().await;
    server.oauth().mock_server_metadata().ok().mount().await;
    server.oauth().mock_registration().ok().mount().await;
    server.oauth().mock_token().ok().mount().await;
    server.mock_who_am_i().ok().mount().await;
    server.mock_sync().ok(|_| {}).mount().await;
}

/// A homeserver without OAuth 2.0, answering its metadata at both paths
/// as Synapse does.
async fn mount_no_oauth_server(server: &MatrixMockServer, login_flows: serde_json::Value) {
    server.mock_versions().ok().mount().await;
    for metadata_path in [
        "/_matrix/client/unstable/org.matrix.msc2965/auth_metadata",
        "/_matrix/client/v1/auth_metadata",
    ] {
        Mock::given(method("GET"))
            .and(path(metadata_path))
            .respond_with(ResponseTemplate::new(404).set_body_json(serde_json::json!({
                "errcode": "M_UNRECOGNIZED",
                "error": "Unrecognized request",
            })))
            .mount(server.server())
            .await;
    }
    Mock::given(method("GET"))
        .and(path("/_matrix/client/v3/login"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(serde_json::json!({ "flows": login_flows })),
        )
        .mount(server.server())
        .await;
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

    // The refresh happened during the restore's sync; the session's update
    // task reports it once the session has started, so it supersedes the
    // tokens the UI saved before.
    harness.wait_for("RestoredSession", |event| *event == Event::RestoredSession);
    assert_eq!(
        harness.wait_for("SessionUpdated", |event| matches!(
            event,
            Event::SessionUpdated { .. }
        )),
        Event::SessionUpdated {
            session: oauth_session("AT2", "RT2"),
        }
    );
    harness.shut_down();
}

/// Signed out elsewhere, an OAuth 2.0 session's sync gets a 401, and the
/// authorization server refuses its refresh token: the session ended.
#[test]
fn an_oauth_session_whose_refresh_is_refused_ends() {
    let harness = Harness::new(async |server| {
        server.mock_versions().ok().mount().await;
        server.oauth().mock_server_metadata().ok().mount().await;
        // The restore's sync; the next ones find the token gone.
        server.mock_sync().ok(|_| {}).up_to_n_times(1).mount().await;
        server.mock_sync().error_unknown_token(false).mount().await;
        server
            .oauth()
            .mock_token()
            .invalid_grant()
            .expect(1..)
            .mount()
            .await;
    });

    restore(&harness, oauth_session("AT1", "RT1"));
    harness.wait_for("RestoredSession", |event| *event == Event::RestoredSession);
    harness.send(Command::StartSync);
    let stopped = harness.wait_for("SyncStopped", |event| {
        matches!(event, Event::SyncStopped(_))
    });
    assert!(
        matches!(stopped, Event::SyncStopped(SyncStopReason::SessionEnded(_))),
        "{stopped:?}"
    );
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
    harness.send(Command::Logout(mirx_core::LogoutRequest {
        discard_room_keys: true,
    }));
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

#[test]
fn oauth_sign_in_registers_mirx_and_signs_in_through_the_redirect() {
    let harness = Harness::new(mount_oauth_server);

    harness.browser_login(None);
    let sign_in = harness.sign_in_url();
    assert_eq!(sign_in.path(), "/oauth2/authorize");
    assert_eq!(query_param(&sign_in, "login_hint"), None);
    let redirect = oauth_redirect(&sign_in);
    assert_eq!(redirect.scheme(), "http");
    assert_eq!(redirect.host_str(), Some("127.0.0.1"));
    assert!(redirect.port().is_some(), "{redirect}");
    assert_eq!(
        harness.request_bodies("/oauth2/registration"),
        [serde_json::json!({
            "application_type": "native",
            "client_name": "mirx",
            "client_uri": "https://github.com/CyberShadow/mIRX",
            "grant_types": ["authorization_code", "refresh_token"],
            "redirect_uris": [redirect.as_str()],
            "response_types": ["code"],
            "token_endpoint_auth_method": "none",
        })]
    );

    // A favicon request on the way does not end the wait.
    let favicon = browse(&redirect.join("/favicon.ico").expect("a URL"), "");
    assert!(
        favicon.starts_with("HTTP/1.1 404 Not Found\r\n"),
        "{favicon}"
    );
    let page = authorize(&sign_in);
    assert!(page.starts_with("HTTP/1.1 200 OK\r\n"), "{page}");
    assert!(
        page.contains("Signed in to mirx as @joe:example.org. You can close this page."),
        "{page}"
    );

    let Event::LoggedIn {
        session,
        homeserver_url,
    } = harness.wait_for("LoggedIn", |event| matches!(event, Event::LoggedIn { .. }))
    else {
        unreachable!()
    };
    assert_eq!(homeserver_url, format!("{}/", harness.server.uri()));
    assert_eq!(
        session,
        StoredSession {
            user_id: "@joe:example.org".to_owned(),
            device_id: session.device_id.clone(),
            access_token: "1234".to_owned(),
            refresh_token: Some("ZYXWV".to_owned()),
            auth: SessionAuth::OAuth {
                client_id: "test_client_id".to_owned(),
            },
        }
    );
    harness.wait_for("SyncedRooms", |event| {
        matches!(event, Event::SyncedRooms(_))
    });
    // The listener is gone with the sign-in.
    assert!(TcpStream::connect(("127.0.0.1", redirect.port().unwrap())).is_err());
    harness.shut_down();
}

#[test]
fn oauth_sign_in_passes_the_user_id_hint() {
    let harness = Harness::new(mount_oauth_server);

    harness.browser_login(Some("@alice:example.org"));
    assert_eq!(
        query_param(&harness.sign_in_url(), "login_hint").as_deref(),
        Some("mxid:@alice:example.org")
    );
    harness.shut_down();
}

#[test]
fn a_denied_oauth_sign_in_fails_and_leaves_the_core_idle() {
    let harness = Harness::new(mount_oauth_server);

    harness.browser_login(None);
    let sign_in = harness.sign_in_url();
    let state = query_param(&sign_in, "state").expect("state");
    let page = browse(
        &oauth_redirect(&sign_in),
        &format!("error=access_denied&state={state}"),
    );
    let error = harness.login_error();
    assert_eq!(
        error,
        "failed to sign in: authorization code grant failed: authorization cancelled by the user"
    );
    assert!(page.starts_with("HTTP/1.1 200 OK\r\n"), "{page}");
    assert!(
        page.contains("mirx could not sign in: failed to sign in: "),
        "{page}"
    );
    assert!(page.contains("Return to mirx and try again."), "{page}");

    // Idle again: another sign-in starts.
    harness.browser_login(None);
    harness.sign_in_url();
    harness.shut_down();
}

#[test]
fn an_oauth_redirect_with_another_state_fails() {
    let harness = Harness::new(mount_oauth_server);

    harness.browser_login(None);
    let sign_in = harness.sign_in_url();
    browse(&oauth_redirect(&sign_in), "code=42&state=forged");
    let error = harness.login_error();
    assert_eq!(
        error,
        "failed to sign in: authorization code grant failed: authorization state value is unexpected"
    );
    harness.shut_down();
}

#[test]
fn without_oauth_the_browser_signs_in_through_legacy_sso() {
    let harness = Harness::new(async |server| {
        mount_no_oauth_server(server, serde_json::json!([{ "type": "m.login.sso" }])).await;
        Mock::given(method("POST"))
            .and(path("/_matrix/client/v3/login"))
            .and(body_partial_json(serde_json::json!({
                "type": "m.login.token",
                "token": "abc",
                "initial_device_display_name": "mirx test",
                "refresh_token": true,
            })))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "access_token": "sso-token",
                "device_id": "SSODEVICE",
                "user_id": "@alice:example.org",
            })))
            .expect(1)
            .mount(server.server())
            .await;
        server.mock_sync().ok(|_| {}).mount().await;
    });

    harness.browser_login(Some("@alice:example.org"));
    let sign_in = harness.sign_in_url();
    assert_eq!(sign_in.path(), "/_matrix/client/v3/login/sso/redirect");
    let redirect =
        Url::parse(&query_param(&sign_in, "redirectUrl").expect("redirectUrl")).expect("a URL");
    assert_eq!(redirect.host_str(), Some("127.0.0.1"));
    let page = browse(&redirect, "loginToken=abc");
    assert!(
        page.contains("Signed in to mirx as @alice:example.org."),
        "{page}"
    );

    let Event::LoggedIn { session, .. } =
        harness.wait_for("LoggedIn", |event| matches!(event, Event::LoggedIn { .. }))
    else {
        unreachable!()
    };
    assert_eq!(
        session,
        StoredSession {
            user_id: "@alice:example.org".to_owned(),
            device_id: "SSODEVICE".to_owned(),
            access_token: "sso-token".to_owned(),
            refresh_token: None,
            auth: SessionAuth::Matrix,
        }
    );
    harness.shut_down();
}

#[test]
fn an_sso_redirect_without_a_login_token_fails() {
    let harness = Harness::new(async |server| {
        mount_no_oauth_server(server, serde_json::json!([{ "type": "m.login.sso" }])).await;
    });

    harness.browser_login(None);
    let sign_in = harness.sign_in_url();
    let redirect =
        Url::parse(&query_param(&sign_in, "redirectUrl").expect("redirectUrl")).expect("a URL");
    let page = browse(&redirect, "");
    assert_eq!(
        harness.login_error(),
        "the sign-in page returned no login token"
    );
    assert!(
        page.contains("mirx could not sign in: the sign-in page returned no login token."),
        "{page}"
    );
    harness.shut_down();
}

#[test]
fn a_password_only_server_offers_no_browser_sign_in() {
    let harness = Harness::new(async |server| {
        mount_no_oauth_server(server, serde_json::json!([{ "type": "m.login.password" }])).await;
    });

    harness.browser_login(None);
    let Event::Error(error) = next_event(harness.core.events()) else {
        panic!("expected the login error first");
    };
    assert_eq!(error.operation, Operation::Login);
    assert_eq!(
        error.message,
        format!(
            "{} offers no browser sign-in; enter a password",
            harness.server.uri()
        )
    );
    harness.shut_down();
}

#[test]
fn cancelling_a_browser_sign_in_closes_the_listener() {
    let harness = Harness::new(mount_oauth_server);

    harness.browser_login(None);
    let sign_in = harness.sign_in_url();
    let port = oauth_redirect(&sign_in).port().expect("a port");
    // A command other than the cancel finds no session.
    harness.send(Command::StartSync);
    let Event::Error(error) = next_event(harness.core.events()) else {
        panic!("expected the start sync error");
    };
    assert_eq!(error.operation, Operation::StartSync);
    assert_eq!(next_event(harness.core.events()), Event::CommandHandled);

    harness.send(Command::CancelLogin);
    assert_eq!(next_event(harness.core.events()), Event::LoginCancelled);
    assert_eq!(next_event(harness.core.events()), Event::CommandHandled);
    assert!(TcpStream::connect(("127.0.0.1", port)).is_err());

    harness.browser_login(None);
    harness.sign_in_url();
    harness.shut_down();
}

#[test]
fn cancel_login_without_a_browser_sign_in_is_refused() {
    let harness = Harness::new(async |_| {});

    harness.send(Command::CancelLogin);
    assert_eq!(
        next_event(harness.core.events()),
        Event::Error(CoreErrorEvent {
            operation: Operation::Login,
            room_id: None,
            message: "no sign-in to cancel".to_owned(),
        })
    );
    harness.shut_down();
}

#[test]
fn shutdown_stops_waiting_for_the_browser() {
    let harness = Harness::new(mount_oauth_server);

    harness.browser_login(None);
    let port = oauth_redirect(&harness.sign_in_url())
        .port()
        .expect("a port");
    harness.send(Command::Shutdown);
    assert_eq!(next_event(harness.core.events()), Event::CommandHandled);
    assert_eq!(next_event(harness.core.events()), Event::Stopped);
    assert!(TcpStream::connect(("127.0.0.1", port)).is_err());
    harness.core.join().expect("join core");
}

#[test]
fn a_sign_in_page_that_is_not_a_web_address_is_refused() {
    let harness = Harness::new(async |server| {
        server.mock_versions().ok().mount().await;
        let issuer = format!("{}/", server.uri());
        Mock::given(method("GET"))
            .and(path(
                "/_matrix/client/unstable/org.matrix.msc2965/auth_metadata",
            ))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "issuer": issuer,
                "authorization_endpoint": "file:///C:/Windows/System32/calc.exe",
                "token_endpoint": format!("{issuer}oauth2/token"),
                "registration_endpoint": format!("{issuer}oauth2/registration"),
                "revocation_endpoint": format!("{issuer}oauth2/revoke"),
                "response_types_supported": ["code"],
                "response_modes_supported": ["query", "fragment"],
                "grant_types_supported": ["authorization_code", "refresh_token"],
                "code_challenge_methods_supported": ["S256"],
            })))
            .mount(server.server())
            .await;
        server.oauth().mock_registration().ok().mount().await;
    });

    harness.browser_login(None);
    let Event::Error(error) = next_event(harness.core.events()) else {
        panic!("expected the login error, and no SignInUrl");
    };
    assert_eq!(error.operation, Operation::Login);
    assert!(
        error
            .message
            .starts_with("the homeserver's sign-in page is not a web address: file:///"),
        "{}",
        error.message
    );
    harness.shut_down();
}
