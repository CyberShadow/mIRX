use std::ffi::OsString;
use std::path::PathBuf;
use std::time::Duration;

use mmatrix_testkit::{
    ComposeKey, LaunchOptions, LoginArgs, MdiState, MdiView, MmatrixApp, Model, NavigationNode,
    NavigationPathSegment, ProtocolVersion, RestoreSessionArgs, RoomMemberState, RoomStateContent,
    RoomStateEventKind, RoomStateLine, RoomTopicState, SpaceChild, StatusBarState, StoredSession,
    SwitchbarItem, SwitchbarState, TestkitError, TimelineEntry, TimelineLine, TimelineSendError,
    TimelineSendState, UiError, UiState, Verification, ViewIdentity,
};

fn stored_session() -> StoredSession {
    StoredSession {
        user_id: "@alice:example".to_owned(),
        device_id: "DEVICE".to_owned(),
        access_token: "secret-token".to_owned(),
        refresh_token: Some("refresh-token".to_owned()),
    }
}

#[test]
fn launch_options_default_has_expected_timeout() {
    assert_eq!(
        LaunchOptions::default(),
        LaunchOptions {
            startup_timeout: Duration::from_secs(10),
            local_app_data: None,
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
        rooms: vec![
            mmatrix_testkit::Room {
                room_id: "!space:example".to_owned(),
                display_name: "Space".to_owned(),
                encrypted: false,
                is_space: true,
                space_children: vec![
                    SpaceChild {
                        room_id: "!alpha:example".to_owned(),
                        suggested: true,
                        order: Some("aaa".to_owned()),
                    },
                    SpaceChild {
                        room_id: "!beta:example".to_owned(),
                        suggested: false,
                        order: None,
                    },
                ],
            },
            mmatrix_testkit::Room {
                room_id: "!alpha:example".to_owned(),
                display_name: "Alpha".to_owned(),
                encrypted: true,
                is_space: false,
                space_children: vec![],
            },
            mmatrix_testkit::Room {
                room_id: "!beta:example".to_owned(),
                display_name: "Beta".to_owned(),
                encrypted: false,
                is_space: false,
                space_children: vec![],
            },
            mmatrix_testkit::Room {
                room_id: "!lobby:example".to_owned(),
                display_name: "Lobby".to_owned(),
                encrypted: false,
                is_space: false,
                space_children: vec![],
            },
        ],
        last_timeline: Some(TimelineLine {
            room_id: "!room:example".to_owned(),
            event_id: Some("$event:example".to_owned()),
            transaction_id: Some("txn-1".to_owned()),
            sender: "@alice:example".to_owned(),
            timestamp_ms: 1_700_000_000_000,
            body: "hello".to_owned(),
            send_state: TimelineSendState::Sent,
            send_error: None,
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
        protocol_version: 9,
        app_version: "0.1.0".to_owned(),
    };
    let session = stored_session();
    let login = LoginArgs {
        homeserver_url: "https://matrix.example".to_owned(),
        username: "@alice:example".to_owned(),
        password: "secret".to_owned(),
        device_display_name: "mmatrix-ui".to_owned(),
    };
    let restore = RestoreSessionArgs {
        homeserver_url: "https://matrix.example".to_owned(),
        session: session.clone(),
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
        status_bar: StatusBarState {
            text: "synced 1 room(s)".to_owned(),
        },
        switchbar: SwitchbarState {
            items: vec![
                SwitchbarItem {
                    identity: ViewIdentity::Status,
                    title: "Status".to_owned(),
                    active: false,
                    has_activity: false,
                },
                SwitchbarItem {
                    identity: ViewIdentity::Room {
                        room_id: "!room:example".to_owned(),
                    },
                    title: "Room".to_owned(),
                    active: true,
                    has_activity: true,
                },
            ],
        },
    };
    let navigation_path = vec![
        NavigationPathSegment::first("mmatrix"),
        NavigationPathSegment {
            text: "Same".to_owned(),
            same_text_index: 1,
        },
    ];
    let events = serde_json::json!([
        "Started",
        {
            "SyncedRooms": [
                {
                    "room_id": "!space:example",
                    "display_name": "Space",
                    "encrypted": false,
                    "is_space": true,
                    "space_children": [
                        {
                            "room_id": "!alpha:example",
                            "suggested": true,
                            "order": "aaa"
                        },
                        {
                            "room_id": "!beta:example",
                            "suggested": false,
                            "order": null
                        }
                    ]
                },
                {
                    "room_id": "!alpha:example",
                    "display_name": "Alpha",
                    "encrypted": true,
                    "is_space": false,
                    "space_children": []
                },
                {
                    "room_id": "!beta:example",
                    "display_name": "Beta",
                    "encrypted": false,
                    "is_space": false,
                    "space_children": []
                },
                {
                    "room_id": "!lobby:example",
                    "display_name": "Lobby",
                    "encrypted": false,
                    "is_space": false,
                    "space_children": []
                }
            ]
        }
    ]);

    let model_json = serde_json::to_vec(&model).expect("serialize model");
    let version_json = serde_json::to_vec(&version).expect("serialize version");
    let session_json = serde_json::to_vec(&session).expect("serialize session");
    let login_json = serde_json::to_vec(&login).expect("serialize login args");
    let restore_json = serde_json::to_vec(&restore).expect("serialize restore args");
    let ui_state_json = serde_json::to_vec(&ui_state).expect("serialize ui state");
    let navigation_path_json =
        serde_json::to_vec(&navigation_path).expect("serialize navigation path");
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
        serde_json::from_slice::<StoredSession>(&session_json).expect("deserialize session"),
        session
    );
    assert_eq!(
        serde_json::from_slice::<LoginArgs>(&login_json).expect("deserialize login args"),
        login
    );
    assert_eq!(
        serde_json::from_slice::<RestoreSessionArgs>(&restore_json)
            .expect("deserialize restore args"),
        restore
    );
    assert_eq!(
        serde_json::from_slice::<UiState>(&ui_state_json).expect("deserialize ui state"),
        ui_state
    );
    assert_eq!(
        serde_json::from_slice::<Vec<NavigationPathSegment>>(&navigation_path_json)
            .expect("deserialize navigation path"),
        navigation_path
    );
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&events_json).expect("deserialize events"),
        events
    );
}

#[test]
fn timeline_entries_round_trip_through_json() {
    let text = TimelineEntry::Text(TimelineLine {
        room_id: "!room:example".to_owned(),
        event_id: Some("$event:example".to_owned()),
        transaction_id: Some("txn-1".to_owned()),
        sender: "@alice:example".to_owned(),
        timestamp_ms: 1_700_000_000_000,
        body: "hello".to_owned(),
        send_state: TimelineSendState::Failed,
        send_error: Some(TimelineSendError {
            message: "send failed".to_owned(),
            recoverable: true,
        }),
    });
    let topic = TimelineEntry::RoomState(RoomStateLine {
        room_id: "!room:example".to_owned(),
        sender: "@alice:example".to_owned(),
        timestamp_ms: 1_700_000_060_000,
        event_kind: RoomStateEventKind::Topic,
        state_key: String::new(),
        content: RoomStateContent::Topic(RoomTopicState {
            topic: "incident review".to_owned(),
        }),
    });
    let member = TimelineEntry::RoomState(RoomStateLine {
        room_id: "!room:example".to_owned(),
        sender: "@carol:example".to_owned(),
        timestamp_ms: 1_700_000_120_000,
        event_kind: RoomStateEventKind::Member,
        state_key: "@carol:example".to_owned(),
        content: RoomStateContent::Member(RoomMemberState {
            membership: "join".to_owned(),
            displayname: Some("carol".to_owned()),
        }),
    });

    assert_eq!(
        serde_json::to_value(&text).expect("serialize text timeline entry"),
        serde_json::json!({
            "type": "text",
            "room_id": "!room:example",
            "event_id": "$event:example",
            "transaction_id": "txn-1",
            "sender": "@alice:example",
            "timestamp_ms": 1700000000000u64,
            "body": "hello",
            "send_state": "failed",
            "send_error": {
                "message": "send failed",
                "recoverable": true
            }
        })
    );
    assert_eq!(
        serde_json::from_value::<TimelineEntry>(serde_json::to_value(&text).unwrap())
            .expect("deserialize text timeline entry"),
        text
    );
    assert_eq!(
        serde_json::to_value(&topic).expect("serialize topic room-state entry"),
        serde_json::json!({
            "type": "room_state",
            "room_id": "!room:example",
            "sender": "@alice:example",
            "timestamp_ms": 1700000060000u64,
            "event_kind": "topic",
            "state_key": "",
            "content": {
                "type": "topic",
                "topic": "incident review"
            }
        })
    );
    assert_eq!(
        serde_json::from_value::<TimelineEntry>(serde_json::to_value(&topic).unwrap())
            .expect("deserialize topic room-state entry"),
        topic
    );
    assert_eq!(
        serde_json::to_value(&member).expect("serialize member room-state entry"),
        serde_json::json!({
            "type": "room_state",
            "room_id": "!room:example",
            "sender": "@carol:example",
            "timestamp_ms": 1700000120000u64,
            "event_kind": "member",
            "state_key": "@carol:example",
            "content": {
                "type": "member",
                "membership": "join",
                "displayname": "carol"
            }
        })
    );
    assert_eq!(
        serde_json::from_value::<TimelineEntry>(serde_json::to_value(&member).unwrap())
            .expect("deserialize member room-state entry"),
        member
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
            "protocol_version":9,
            "app_version":"0.1.0",
            "unexpected":true
        }"#
        )
        .is_err()
    );
    assert!(
        serde_json::from_str::<StoredSession>(
            r#"{
            "user_id":"@alice:example",
            "device_id":"DEVICE",
            "access_token":"secret-token",
            "refresh_token":null,
            "unexpected":true
        }"#
        )
        .is_err()
    );
    assert!(
        serde_json::from_str::<mmatrix_testkit::Room>(
            r#"{
            "room_id":"!room:example",
            "display_name":"Room",
            "encrypted":true,
            "is_space":false,
            "space_children":[],
            "unexpected":true
        }"#
        )
        .is_err()
    );
    assert!(
        serde_json::from_str::<SpaceChild>(
            r#"{
            "room_id":"!room:example",
            "suggested":true,
            "order":"aaa",
            "unexpected":true
        }"#
        )
        .is_err()
    );
    assert!(
        serde_json::from_str::<TimelineLine>(
            r#"{
            "room_id":"!room:example",
            "event_id":"$event:example",
            "transaction_id":"txn-1",
            "sender":"@alice:example",
            "timestamp_ms":1700000000000,
            "body":"hello",
            "send_state":"sent",
            "send_error":null,
            "unexpected":true
        }"#
        )
        .is_err()
    );
    assert!(
        serde_json::from_str::<TimelineEntry>(
            r#"{
            "type":"text",
            "room_id":"!room:example",
            "event_id":"$event:example",
            "transaction_id":"txn-1",
            "sender":"@alice:example",
            "timestamp_ms":1700000000000,
            "body":"hello",
            "send_state":"sent",
            "send_error":null,
            "unexpected":true
        }"#
        )
        .is_err()
    );
    assert!(
        serde_json::from_str::<RoomStateLine>(
            r#"{
            "room_id":"!room:example",
            "sender":"@alice:example",
            "timestamp_ms":1700000060000,
            "event_kind":"topic",
            "state_key":"",
            "content":{"type":"topic","topic":"incident review"},
            "unexpected":true
        }"#
        )
        .is_err()
    );
    assert!(
        serde_json::from_str::<RoomStateContent>(
            r#"{
            "type":"topic",
            "topic":"incident review",
            "unexpected":true
        }"#
        )
        .is_err()
    );
    assert!(
        serde_json::from_str::<RoomTopicState>(
            r#"{
            "topic":"incident review",
            "unexpected":true
        }"#
        )
        .is_err()
    );
    assert!(
        serde_json::from_str::<RoomStateLine>(
            r#"{
            "room_id":"!room:example",
            "sender":"@alice:example",
            "timestamp_ms":1700000060000,
            "event_kind":"topic",
            "state_key":"",
            "content":{
                "type":"topic",
                "topic":"incident review",
                "unexpected":true
            }
        }"#
        )
        .is_err()
    );
    assert!(
        serde_json::from_str::<TimelineEntry>(
            r#"{
            "type":"room_state",
            "room_id":"!ops:example",
            "sender":"@alice:example",
            "timestamp_ms":1700000000000,
            "event_kind":"topic",
            "state_key":"",
            "content":{
                "type":"member",
                "membership":"join",
                "displayname":"alice"
            }
        }"#
        )
        .is_err()
    );
    assert!(
        serde_json::from_str::<RoomStateLine>(
            r#"{
            "room_id":"!ops:example",
            "sender":"@alice:example",
            "timestamp_ms":1700000000000,
            "event_kind":"topic",
            "state_key":"",
            "content":{
                "type":"member",
                "membership":"join",
                "displayname":"alice"
            }
        }"#
        )
        .is_err()
    );
    assert!(
        serde_json::from_str::<RoomMemberState>(
            r#"{
            "membership":"join",
            "displayname":"carol",
            "unexpected":true
        }"#
        )
        .is_err()
    );
    assert!(
        serde_json::from_str::<LoginArgs>(
            r#"{
            "homeserver_url":"https://matrix.example",
            "username":"@alice:example",
            "password":"secret",
            "device_display_name":"mmatrix-ui",
            "unexpected":true
        }"#
        )
        .is_err()
    );
    assert!(
        serde_json::from_str::<RestoreSessionArgs>(
            r#"{
            "homeserver_url":"https://matrix.example",
            "session":{
                "user_id":"@alice:example",
                "device_id":"DEVICE",
                "access_token":"secret-token",
                "refresh_token":null
            },
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
            "status_bar":{"text":"ready"},
            "switchbar":{"items":[]},
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
        serde_json::from_str::<StatusBarState>(
            r#"{
            "text":"ready",
            "unexpected":true
        }"#
        )
        .is_err()
    );
    assert!(
        serde_json::from_str::<SwitchbarState>(
            r#"{
            "items":[],
            "unexpected":true
        }"#
        )
        .is_err()
    );
    assert!(
        serde_json::from_str::<SwitchbarItem>(
            r#"{
            "identity":{"kind":"status"},
            "title":"Status",
            "active":true,
            "has_activity":false,
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
    assert!(
        serde_json::from_str::<ViewIdentity>(
            r#"{
            "kind":"space",
            "room_id":"!space:example"
        }"#
        )
        .is_err()
    );
}

#[test]
fn switchbar_helpers_find_items_and_active_item() {
    let ui_state = UiState {
        navigation: NavigationNode {
            text: "mmatrix".to_owned(),
            children: vec![],
        },
        mdi: MdiState {
            active: ViewIdentity::Room {
                room_id: "!room:example".to_owned(),
            },
            views: vec![],
        },
        status_bar: StatusBarState {
            text: "ready".to_owned(),
        },
        switchbar: SwitchbarState {
            items: vec![
                SwitchbarItem {
                    identity: ViewIdentity::Status,
                    title: "Status".to_owned(),
                    active: false,
                    has_activity: false,
                },
                SwitchbarItem {
                    identity: ViewIdentity::Room {
                        room_id: "!room:example".to_owned(),
                    },
                    title: "Room".to_owned(),
                    active: true,
                    has_activity: true,
                },
            ],
        },
    };

    let identity = ViewIdentity::Room {
        room_id: "!room:example".to_owned(),
    };
    assert_eq!(
        ui_state
            .switchbar_item(&identity)
            .map(|item| item.title.as_str()),
        Some("Room")
    );
    assert_eq!(
        ui_state.switchbar.active_item().map(|item| &item.identity),
        Some(&identity)
    );
}

#[test]
fn compose_key_round_trips_and_rejects_invalid_shapes() {
    for (key, json) in [
        (ComposeKey::Return, r#"{"kind":"return"}"#),
        (ComposeKey::Up, r#"{"kind":"up"}"#),
        (ComposeKey::Down, r#"{"kind":"down"}"#),
    ] {
        assert_eq!(
            serde_json::to_string(&key).expect("compose key must serialize"),
            json
        );
        assert_eq!(
            serde_json::from_str::<ComposeKey>(json).expect("compose key must deserialize"),
            key
        );
    }

    for json in [
        r#"{"kind":"return","unexpected":true}"#,
        r#"{"kind":"left"}"#,
        r#"{"unexpected":true}"#,
    ] {
        assert!(
            serde_json::from_str::<ComposeKey>(json).is_err(),
            "invalid compose key payload unexpectedly deserialized: {json}"
        );
    }
}

#[test]
fn login_args_defaults_device_display_name_when_omitted() {
    assert_eq!(
        serde_json::from_str::<LoginArgs>(
            r#"{
            "homeserver_url":"https://matrix.example",
            "username":"@alice:example",
            "password":"secret"
        }"#
        )
        .expect("login args must deserialize"),
        LoginArgs {
            homeserver_url: "https://matrix.example".to_owned(),
            username: "@alice:example".to_owned(),
            password: "secret".to_owned(),
            device_display_name: "mmatrix-ui".to_owned(),
        }
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

    let error = MmatrixApp::find_by_pid(7, Duration::from_millis(1))
        .expect_err("find_by_pid must fail off Windows");
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

    let launch_with_options_error = MmatrixApp::launch_with_options(
        "mmatrix-ui.exe",
        &[OsString::from("--automation")],
        LaunchOptions {
            startup_timeout: Duration::from_secs(1),
            local_app_data: Some(PathBuf::from("/tmp/mmatrix-testkit")),
        },
    )
    .expect_err("launch_with_options must fail off Windows");
    assert_eq!(
        launch_with_options_error.to_string(),
        "mmatrix-testkit requires the Windows target"
    );

    let app = MmatrixApp::find(Duration::from_millis(1)).expect_err("find must fail off Windows");
    assert_eq!(
        app.to_string(),
        "mmatrix-testkit requires the Windows target"
    );
}

#[cfg(not(windows))]
#[test]
fn non_windows_platform_reports_unsupported_for_new_public_methods() {
    let app: MmatrixApp = unsafe { std::mem::MaybeUninit::zeroed().assume_init() };

    let unsupported = |result: Result<(), TestkitError>| {
        let error = result.expect_err("method must fail off Windows");
        assert_eq!(
            error.to_string(),
            "mmatrix-testkit requires the Windows target"
        );
    };

    unsupported(app.login("https://matrix.example", "@alice:example", "secret", None));
    unsupported(app.restore_session("https://matrix.example", stored_session()));
    unsupported(app.restore_saved_session());
    unsupported(app.start_sync());
    unsupported(app.logout());
    unsupported(app.wait_for_lifecycle_state("ready", Duration::from_millis(1)));
    unsupported(app.wait_for_min_rooms(1, Duration::from_millis(1)));
    assert_eq!(app.pid(), 0);
    unsupported(app.set_compose_text("draft"));
    unsupported(app.send_compose_key(ComposeKey::Return));
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
        .compose_text()
        .expect_err("compose_text must fail off Windows");
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

    let error = app
        .select_navigation_path(&[NavigationPathSegment::first("mmatrix")])
        .expect_err("select_navigation_path must fail off Windows");
    assert_eq!(
        error.to_string(),
        "mmatrix-testkit requires the Windows target"
    );

    let error = app
        .select_navigation_labels(&["mmatrix", "status"])
        .expect_err("select_navigation_labels must fail off Windows");
    assert_eq!(
        error.to_string(),
        "mmatrix-testkit requires the Windows target"
    );

    let error = app
        .selected_navigation_path()
        .expect_err("selected_navigation_path must fail off Windows");
    assert_eq!(
        error.to_string(),
        "mmatrix-testkit requires the Windows target"
    );

    let error = app
        .send_text_target(None, "hello")
        .expect_err("send_text_target must fail off Windows");
    assert_eq!(
        error.to_string(),
        "mmatrix-testkit requires the Windows target"
    );
}
