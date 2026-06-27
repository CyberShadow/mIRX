use std::ffi::OsString;
use std::time::Duration;

use mmatrix_testkit::{
    LaunchOptions, MdiState, MdiView, MmatrixApp, Model, NavigationNode, ProtocolVersion,
    TestkitError, TimelineLine, UiError, UiState, Verification, ViewIdentity,
};

#[test]
fn launch_options_default_has_expected_timeout() {
    assert_eq!(
        LaunchOptions::default(),
        LaunchOptions {
            startup_timeout: Duration::from_secs(10),
        }
    );
}

#[test]
fn error_display_formats_each_public_variant() {
    assert_eq!(
        TestkitError::UnsupportedPlatform("windows only").to_string(),
        "windows only"
    );
    assert_eq!(
        TestkitError::Timeout("timed out".to_owned()).to_string(),
        "timed out"
    );
    assert_eq!(
        TestkitError::Launch("launch failed".to_owned()).to_string(),
        "launch failed"
    );
    assert_eq!(
        TestkitError::Transport("transport failed".to_owned()).to_string(),
        "transport failed"
    );
    assert_eq!(
        TestkitError::InvalidResponse("bad response".to_owned()).to_string(),
        "bad response"
    );
    assert_eq!(
        TestkitError::Protocol {
            code: "unknown_room".to_owned(),
            message: "missing".to_owned(),
        }
        .to_string(),
        "protocol error unknown_room: missing"
    );
}

#[test]
fn data_models_round_trip_through_json() {
    let model = Model {
        status: "synced".to_owned(),
        lifecycle_state: "ready".to_owned(),
        rooms: vec![mmatrix_testkit::Room {
            room_id: "!room:example".to_owned(),
            display_name: "Room".to_owned(),
            encrypted: true,
        }],
        last_timeline: Some(TimelineLine {
            room_id: "!room:example".to_owned(),
            body: "hello".to_owned(),
        }),
        last_verification: Some(Verification {
            flow_id: "flow".to_owned(),
            user_id: "@alice:example".to_owned(),
        }),
        last_error: Some(UiError {
            operation: "send text".to_owned(),
            message: "failed".to_owned(),
        }),
        selected_room: Some("!room:example".to_owned()),
    };
    let version = ProtocolVersion {
        protocol_version: 1,
        app_version: "0.1.0".to_owned(),
    };
    let ui_state = UiState {
        navigation: NavigationNode {
            text: "mmatrix".to_owned(),
            children: vec![
                NavigationNode {
                    text: "status".to_owned(),
                    children: vec![],
                },
                NavigationNode {
                    text: "rooms: 1".to_owned(),
                    children: vec![NavigationNode {
                        text: "Room".to_owned(),
                        children: vec![],
                    }],
                },
            ],
        },
        mdi: MdiState {
            active: ViewIdentity::Room {
                room_id: "!room:example".to_owned(),
            },
            views: vec![
                MdiView {
                    identity: ViewIdentity::Status,
                    title: "Status".to_owned(),
                    active: false,
                    lines: vec!["status: synced".to_owned()],
                },
                MdiView {
                    identity: ViewIdentity::Room {
                        room_id: "!room:example".to_owned(),
                    },
                    title: "Room".to_owned(),
                    active: true,
                    lines: vec!["hello".to_owned()],
                },
            ],
        },
    };
    let events = serde_json::json!([
        "Started",
        {
            "SyncedRooms": [
                {
                    "room_id": "!room:example",
                    "display_name": "Room",
                    "encrypted": true
                }
            ]
        }
    ]);

    let model_json = serde_json::to_vec(&model).expect("serialize model");
    let version_json = serde_json::to_vec(&version).expect("serialize version");
    let ui_state_json = serde_json::to_vec(&ui_state).expect("serialize ui state");
    let events_json = serde_json::to_vec(&events).expect("serialize events");

    assert_eq!(
        serde_json::from_slice::<Model>(&model_json).expect("deserialize model"),
        model
    );
    assert_eq!(
        serde_json::from_slice::<ProtocolVersion>(&version_json).expect("deserialize version"),
        version
    );
    assert_eq!(
        serde_json::from_slice::<UiState>(&ui_state_json).expect("deserialize ui state"),
        ui_state
    );
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&events_json).expect("deserialize events"),
        events
    );
}

#[test]
fn data_models_reject_unknown_fields() {
    assert!(
        serde_json::from_str::<Model>(
            r#"{
            "status":"synced",
            "lifecycle_state":"ready",
            "rooms":[],
            "last_timeline":null,
            "last_verification":null,
            "last_error":null,
            "selected_room":null,
            "unexpected":true
        }"#
        )
        .is_err()
    );
    assert!(
        serde_json::from_str::<ProtocolVersion>(
            r#"{
            "protocol_version":1,
            "app_version":"0.1.0",
            "unexpected":true
        }"#
        )
        .is_err()
    );
    assert!(
        serde_json::from_str::<UiState>(
            r#"{
            "navigation":{"text":"mmatrix","children":[]},
            "mdi":{"active":{"kind":"status"},"views":[]},
            "unexpected":true
        }"#
        )
        .is_err()
    );
    assert!(
        serde_json::from_str::<NavigationNode>(
            r#"{
            "text":"mmatrix",
            "children":[],
            "unexpected":true
        }"#
        )
        .is_err()
    );
    assert!(
        serde_json::from_str::<MdiState>(
            r#"{
            "active":{"kind":"status"},
            "views":[],
            "unexpected":true
        }"#
        )
        .is_err()
    );
    assert!(
        serde_json::from_str::<MdiView>(
            r#"{
            "identity":{"kind":"status"},
            "title":"Status",
            "active":true,
            "lines":[],
            "unexpected":true
        }"#
        )
        .is_err()
    );
    assert!(
        serde_json::from_str::<ViewIdentity>(
            r#"{
            "kind":"status",
            "unexpected":true
        }"#
        )
        .is_err()
    );
    assert!(
        serde_json::from_str::<ViewIdentity>(
            r#"{
            "kind":"room",
            "room_id":"!room:example",
            "unexpected":true
        }"#
        )
        .is_err()
    );
}

#[cfg(not(windows))]
#[test]
fn non_windows_platform_reports_unsupported_driver() {
    let error = MmatrixApp::find(Duration::from_millis(1)).expect_err("find must fail off Windows");
    assert_eq!(
        error.to_string(),
        "mmatrix-testkit requires the Windows target"
    );

    let launch_error = MmatrixApp::launch("mmatrix-ui.exe", &[OsString::from("--automation")])
        .expect_err("launch must fail off Windows");
    assert_eq!(
        launch_error.to_string(),
        "mmatrix-testkit requires the Windows target"
    );
}

#[cfg(not(windows))]
#[test]
fn non_windows_new_methods_report_unsupported_driver() {
    let app: MmatrixApp = unsafe { std::mem::MaybeUninit::zeroed().assume_init() };

    let error = app
        .dump_ui_state()
        .expect_err("dump_ui_state must fail off Windows");
    assert_eq!(
        error.to_string(),
        "mmatrix-testkit requires the Windows target"
    );

    let error = app
        .select_view(ViewIdentity::Status)
        .expect_err("select_view must fail off Windows");
    assert_eq!(
        error.to_string(),
        "mmatrix-testkit requires the Windows target"
    );

    let error = app
        .select_status_view()
        .expect_err("select_status_view must fail off Windows");
    assert_eq!(
        error.to_string(),
        "mmatrix-testkit requires the Windows target"
    );
}
