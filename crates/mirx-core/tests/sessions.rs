//! Signing out (`SignOutDevice`) and renaming (`RenameDevice`) your own
//! sessions against a mock homeserver. The account has this session
//! (`MIRX`) and two others, `PH0NE` and `L4PT0P`.

mod common;

use common::{Harness, UIAA_SESSION, password_flow, uiaa};
use mirx_core::{
    Command, CoreErrorEvent, Event, Operation, RenameDeviceRequest, SignOutDeviceRequest,
};
use serde_json::json;
use url::Url;
use wiremock::matchers::{body_partial_json, method, path_regex};
use wiremock::{Mock, ResponseTemplate};

const DELETE_DEVICES: &str = r"^/_matrix/client/.*/delete_devices$";

impl Harness {
    /// The restored session, whose server knows its three sessions.
    fn with_sessions(self) -> Self {
        for device_id in ["MIRX", "PH0NE", "L4PT0P"] {
            self.mount(
                Mock::given(method("GET"))
                    .and(path_regex(format!(
                        r"^/_matrix/client/.*/devices/{device_id}$"
                    )))
                    .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                        "device_id": device_id,
                    }))),
            );
        }
        self.mount(
            Mock::given(method("GET"))
                .and(path_regex(r"^/_matrix/client/.*/devices/[^/]+$"))
                .respond_with(not_found())
                .with_priority(10),
        );
        self
    }

    fn sign_out(&self, device_id: &str, password: Option<&str>) -> Event {
        self.send(Command::SignOutDevice(SignOutDeviceRequest {
            device_id: device_id.to_owned(),
            password: password.map(str::to_owned),
        }));
        self.wait_for("the sign-out's answer", |event| {
            matches!(
                event,
                Event::DeviceSignedOut(_)
                    | Event::DeviceSignOutApproval { .. }
                    | Event::Error(CoreErrorEvent {
                        operation: Operation::SignOutDevice,
                        ..
                    })
            )
        })
    }

    fn rename(&self, device_id: &str, display_name: &str) -> Event {
        self.send(Command::RenameDevice(RenameDeviceRequest {
            device_id: device_id.to_owned(),
            display_name: display_name.to_owned(),
        }));
        self.wait_for("the rename's answer", |event| {
            matches!(
                event,
                Event::DeviceRenamed { .. }
                    | Event::Error(CoreErrorEvent {
                        operation: Operation::RenameDevice,
                        ..
                    })
            )
        })
    }
}

fn not_found() -> ResponseTemplate {
    ResponseTemplate::new(404).set_body_json(json!({
        "errcode": "M_NOT_FOUND",
        "error": "Unknown device",
    }))
}

fn error(operation: Operation, message: &str) -> Event {
    Event::Error(CoreErrorEvent {
        operation,
        room_id: None,
        message: message.to_owned(),
    })
}

/// `POST /delete_devices` for `PH0NE` that answers the server's auth
/// session with `password`, answered with `response` once.
fn delete_with_password(password: &str, response: ResponseTemplate) -> Mock {
    Mock::given(method("POST"))
        .and(path_regex(DELETE_DEVICES))
        .and(body_partial_json(json!({
            "devices": ["PH0NE"],
            "auth": {
                "type": "m.login.password",
                "session": UIAA_SESSION,
                "password": password,
                "identifier": { "type": "m.id.user", "user": "@alice:example.org" },
            },
        })))
        .respond_with(response)
        // Ahead of the unanswered probe's mock, which matches it too.
        .with_priority(1)
        .expect(1)
}

/// The first `POST /delete_devices`, without auth.
fn delete_probe() -> wiremock::MockBuilder {
    Mock::given(method("POST"))
        .and(path_regex(DELETE_DEVICES))
        .and(body_partial_json(json!({ "devices": ["PH0NE"] })))
}

fn no_delete() -> Mock {
    Mock::given(method("POST"))
        .and(path_regex(DELETE_DEVICES))
        .respond_with(ResponseTemplate::new(500))
        .expect(0)
}

#[test]
fn a_password_signs_another_session_out() {
    let harness = Harness::restored().with_sessions();
    harness.mount(delete_probe().respond_with(uiaa(password_flow())).expect(1));
    harness.mount(delete_with_password(
        "hunter2",
        ResponseTemplate::new(200).set_body_json(json!({})),
    ));

    assert_eq!(
        harness.sign_out("PH0NE", Some("hunter2")),
        Event::DeviceSignedOut("PH0NE".to_owned())
    );
    harness.shut_down();
}

#[test]
fn a_wrong_password_is_the_servers_word() {
    let harness = Harness::restored().with_sessions();
    harness.mount(delete_probe().respond_with(uiaa(password_flow())).expect(1));
    harness.mount(delete_with_password(
        "wrong",
        ResponseTemplate::new(401).set_body_json(json!({
            "errcode": "M_FORBIDDEN",
            "error": "Invalid password",
            "flows": password_flow(),
            "params": {},
            "session": UIAA_SESSION,
        })),
    ));

    assert_eq!(
        harness.sign_out("PH0NE", Some("wrong")),
        error(
            Operation::SignOutDevice,
            "failed to sign out PH0NE: Invalid password"
        )
    );
    harness.shut_down();
}

#[test]
fn a_server_that_asks_no_auth_signs_out_at_once() {
    let harness = Harness::restored().with_sessions();
    harness.mount(
        delete_probe()
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({})))
            .expect(1),
    );

    assert_eq!(
        harness.sign_out("PH0NE", None),
        Event::DeviceSignedOut("PH0NE".to_owned())
    );
    harness.shut_down();
}

#[test]
fn no_password_for_a_password_flow_is_refused() {
    let harness = Harness::restored().with_sessions();
    harness.mount(delete_probe().respond_with(uiaa(password_flow())).expect(1));

    assert_eq!(
        harness.sign_out("PH0NE", None),
        error(
            Operation::SignOutDevice,
            "failed to sign out PH0NE: signing out a session needs your account password"
        )
    );
    harness.shut_down();
}

#[test]
fn an_sso_only_server_is_refused_with_its_flows() {
    let harness = Harness::restored().with_sessions();
    harness.mount(
        delete_probe()
            .respond_with(uiaa(json!([{ "stages": ["m.login.sso"] }])))
            .expect(1),
    );

    assert_eq!(
        harness.sign_out("PH0NE", Some("hunter2")),
        error(
            Operation::SignOutDevice,
            "failed to sign out PH0NE: your server asks to confirm with m.login.sso; mirx can \
             confirm with your password only"
        )
    );
    harness.shut_down();
}

#[test]
fn this_session_is_refused() {
    let harness = Harness::restored().with_sessions();
    harness.mount(no_delete());

    assert_eq!(
        harness.sign_out("MIRX", Some("hunter2")),
        error(
            Operation::SignOutDevice,
            "MIRX is this session; File > Log Out signs it out"
        )
    );
    harness.shut_down();
}

#[test]
fn an_unknown_session_is_refused() {
    let harness = Harness::restored().with_sessions();
    harness.mount(no_delete());

    assert_eq!(
        harness.sign_out("TYP0", Some("hunter2")),
        error(
            Operation::SignOutDevice,
            "you have no session TYP0; /devices lists yours"
        )
    );
    harness.shut_down();
}

#[test]
fn an_oauth_session_signs_out_on_the_account_page() {
    let harness = Harness::restored_oauth().with_sessions();
    harness.mount(no_delete());

    let Event::DeviceSignOutApproval { device_id, url } = harness.sign_out("PH0NE", None) else {
        panic!("expected the account page");
    };
    assert_eq!(device_id, "PH0NE");
    let url = Url::parse(&url).expect("the account page is a URL");
    assert_eq!(
        url.query_pairs().collect::<Vec<_>>(),
        [
            ("action".into(), "org.matrix.session_end".into()),
            ("device_id".into(), "PH0NE".into()),
        ]
    );
    // The account page of the mock's metadata.
    assert!(
        url.as_str()
            .starts_with(&format!("{}/", harness.server.uri())),
        "{url}"
    );
    harness.shut_down();
}

#[test]
fn an_oauth_session_refuses_a_password() {
    let harness = Harness::restored_oauth().with_sessions();
    harness.mount(no_delete());

    assert_eq!(
        harness.sign_out("PH0NE", Some("hunter2")),
        error(
            Operation::SignOutDevice,
            "your server signs sessions out in the browser, not with a password"
        )
    );
    harness.shut_down();
}

fn rename_mock(device_id: &str, display_name: &str) -> wiremock::MockBuilder {
    Mock::given(method("PUT"))
        .and(path_regex(format!(
            r"^/_matrix/client/.*/devices/{device_id}$"
        )))
        .and(body_partial_json(json!({ "display_name": display_name })))
}

#[test]
fn rename_names_a_session() {
    let harness = Harness::restored().with_sessions();
    harness.mount(
        rename_mock("PH0NE", "Work phone")
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({})))
            .expect(1),
    );

    assert_eq!(
        harness.rename("PH0NE", "Work phone"),
        Event::DeviceRenamed {
            device_id: "PH0NE".to_owned(),
            display_name: "Work phone".to_owned(),
        }
    );
    harness.shut_down();
}

#[test]
fn rename_this_session_is_allowed() {
    let harness = Harness::restored().with_sessions();
    harness.mount(
        rename_mock("MIRX", "Desk")
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({})))
            .expect(1),
    );

    assert_eq!(
        harness.rename("MIRX", "Desk"),
        Event::DeviceRenamed {
            device_id: "MIRX".to_owned(),
            display_name: "Desk".to_owned(),
        }
    );
    harness.shut_down();
}

#[test]
fn rename_an_unknown_session_is_refused() {
    let harness = Harness::restored().with_sessions();
    harness.mount(
        rename_mock("TYP0", "Phone")
            .respond_with(not_found())
            .expect(1),
    );

    assert_eq!(
        harness.rename("TYP0", "Phone"),
        error(
            Operation::RenameDevice,
            "you have no session TYP0; /devices lists yours"
        )
    );
    harness.shut_down();
}
