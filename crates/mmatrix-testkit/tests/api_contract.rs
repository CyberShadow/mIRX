use std::ffi::OsString;
use std::path::PathBuf;
use std::time::Duration;

use mmatrix_core::{Event as CoreEvent, RoomReadState};
use mmatrix_testkit::{
    ComposeKey, LaunchOptions, LoginArgs, MdiState, MdiView, MmatrixApp, Model, NavigationNode,
    NavigationPathSegment, ProtocolVersion, RestoreSessionArgs, RoomActivity, RoomActivityKind,
    RoomMessageActivity, RoomMessageActivityLine, RoomMessagePresentationKind,
    RoomMessageSendState, RoomStateNoticeActivity, RoomStateNoticeContent, SpaceChild,
    StatusBarState, StoredSession, SwitchbarItem, SwitchbarState, TestkitError, UiError, UiState,
    Verification, VerificationDecimals, VerificationEmoji, VerificationState, ViewIdentity,
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
        last_activity: Some(RoomMessageActivityLine {
            room_id: "!room:example".to_owned(),
            activity_id: "activity-1".to_owned(),
            event_id: Some("$event:example".to_owned()),
            transaction_id: Some("txn-1".to_owned()),
            sender: "@alice:example".to_owned(),
            timestamp_ms: 1_700_000_000_000,
            presentation: RoomMessagePresentationKind::Text,
            body: "hello".to_owned(),
            send_state: RoomMessageSendState::Sent,
        }),
        last_verification: Some(Verification {
            flow_id: "flow".to_owned(),
            user_id: "@alice:example".to_owned(),
            state: VerificationState::SasReady {
                emojis: vec![
                    VerificationEmoji {
                        symbol: "🐶".to_owned(),
                        description: "dog".to_owned(),
                    },
                    VerificationEmoji {
                        symbol: "🔥".to_owned(),
                        description: "fire".to_owned(),
                    },
                ],
                decimals: VerificationDecimals {
                    first: 1234,
                    second: 5678,
                    third: 9012,
                },
            },
        }),
        last_error: Some(UiError {
            operation: "send text".to_owned(),
            message: "failed".to_owned(),
        }),
        selected_room: Some("!room:example".to_owned()),
    };
    let version = ProtocolVersion {
        protocol_version: 11,
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
fn room_activity_sent_message_round_trips_through_json() {
    let activity = RoomActivity {
        room_id: "!room:example".to_owned(),
        activity_id: "activity-1".to_owned(),
        timestamp_ms: 1_700_000_000_000,
        kind: RoomActivityKind::Message(RoomMessageActivity {
            event_id: Some("$event:example".to_owned()),
            transaction_id: Some("txn-1".to_owned()),
            sender: "@alice:example".to_owned(),
            presentation: RoomMessagePresentationKind::Text,
            body: "hello".to_owned(),
            send_state: RoomMessageSendState::Sent,
        }),
    };

    assert_eq!(
        serde_json::to_value(&activity).expect("serialize sent message activity"),
        serde_json::json!({
            "room_id": "!room:example",
            "activity_id": "activity-1",
            "timestamp_ms": 1700000000000u64,
            "kind": {
                "type": "message",
                "event_id": "$event:example",
                "transaction_id": "txn-1",
                "sender": "@alice:example",
                "presentation": "text",
                "body": "hello",
                "send_state": {
                    "state": "sent"
                }
            }
        })
    );
    assert_eq!(
        serde_json::from_value::<RoomActivity>(serde_json::to_value(&activity).unwrap())
            .expect("deserialize sent message activity"),
        activity
    );
}

#[test]
fn room_activity_pending_message_round_trips_through_json() {
    let activity = RoomActivity {
        room_id: "!room:example".to_owned(),
        activity_id: "activity-2".to_owned(),
        timestamp_ms: 1_700_000_060_000,
        kind: RoomActivityKind::Message(RoomMessageActivity {
            event_id: None,
            transaction_id: Some("txn-2".to_owned()),
            sender: "@alice:example".to_owned(),
            presentation: RoomMessagePresentationKind::Text,
            body: "queued".to_owned(),
            send_state: RoomMessageSendState::Pending,
        }),
    };

    assert_eq!(
        serde_json::to_value(&activity).expect("serialize pending message activity"),
        serde_json::json!({
            "room_id": "!room:example",
            "activity_id": "activity-2",
            "timestamp_ms": 1700000060000u64,
            "kind": {
                "type": "message",
                "event_id": null,
                "transaction_id": "txn-2",
                "sender": "@alice:example",
                "presentation": "text",
                "body": "queued",
                "send_state": {
                    "state": "pending"
                }
            }
        })
    );
    assert_eq!(
        serde_json::from_value::<RoomActivity>(serde_json::to_value(&activity).unwrap())
            .expect("deserialize pending message activity"),
        activity
    );
}

#[test]
fn room_activity_failed_message_round_trips_through_json() {
    let activity = RoomActivity {
        room_id: "!room:example".to_owned(),
        activity_id: "activity-3".to_owned(),
        timestamp_ms: 1_700_000_120_000,
        kind: RoomActivityKind::Message(RoomMessageActivity {
            event_id: None,
            transaction_id: Some("txn-3".to_owned()),
            sender: "@alice:example".to_owned(),
            presentation: RoomMessagePresentationKind::Text,
            body: "retry me".to_owned(),
            send_state: RoomMessageSendState::Failed {
                message: "send failed".to_owned(),
                recoverable: true,
            },
        }),
    };

    assert_eq!(
        serde_json::to_value(&activity).expect("serialize failed message activity"),
        serde_json::json!({
            "room_id": "!room:example",
            "activity_id": "activity-3",
            "timestamp_ms": 1700000120000u64,
            "kind": {
                "type": "message",
                "event_id": null,
                "transaction_id": "txn-3",
                "sender": "@alice:example",
                "presentation": "text",
                "body": "retry me",
                "send_state": {
                    "state": "failed",
                    "message": "send failed",
                    "recoverable": true
                }
            }
        })
    );
    assert_eq!(
        serde_json::from_value::<RoomActivity>(serde_json::to_value(&activity).unwrap())
            .expect("deserialize failed message activity"),
        activity
    );
}

#[test]
fn room_activity_notice_message_round_trips_through_json() {
    let activity = RoomActivity {
        room_id: "!room:example".to_owned(),
        activity_id: "activity-notice".to_owned(),
        timestamp_ms: 1_700_000_150_000,
        kind: RoomActivityKind::Message(RoomMessageActivity {
            event_id: Some("$notice:example".to_owned()),
            transaction_id: None,
            sender: "@ops:example".to_owned(),
            presentation: RoomMessagePresentationKind::Notice,
            body: "maintenance window".to_owned(),
            send_state: RoomMessageSendState::Sent,
        }),
    };

    let json = serde_json::json!({
        "room_id": "!room:example",
        "activity_id": "activity-notice",
        "timestamp_ms": 1700000150000u64,
        "kind": {
            "type": "message",
            "event_id": "$notice:example",
            "transaction_id": null,
            "sender": "@ops:example",
            "presentation": "notice",
            "body": "maintenance window",
            "send_state": {
                "state": "sent"
            }
        }
    });

    assert_eq!(
        serde_json::to_value(&activity).expect("serialize notice message activity"),
        json
    );
    assert_eq!(
        serde_json::from_value::<RoomActivity>(json).expect("deserialize notice message activity"),
        activity
    );
}

#[test]
fn room_activity_emote_message_round_trips_through_json() {
    let activity = RoomActivity {
        room_id: "!room:example".to_owned(),
        activity_id: "activity-emote".to_owned(),
        timestamp_ms: 1_700_000_165_000,
        kind: RoomActivityKind::Message(RoomMessageActivity {
            event_id: Some("$emote:example".to_owned()),
            transaction_id: Some("txn-emote".to_owned()),
            sender: "@alice:example".to_owned(),
            presentation: RoomMessagePresentationKind::Emote,
            body: "waves".to_owned(),
            send_state: RoomMessageSendState::Sent,
        }),
    };

    let json = serde_json::json!({
        "room_id": "!room:example",
        "activity_id": "activity-emote",
        "timestamp_ms": 1700000165000u64,
        "kind": {
            "type": "message",
            "event_id": "$emote:example",
            "transaction_id": "txn-emote",
            "sender": "@alice:example",
            "presentation": "emote",
            "body": "waves",
            "send_state": {
                "state": "sent"
            }
        }
    });

    assert_eq!(
        serde_json::to_value(&activity).expect("serialize emote message activity"),
        json
    );
    assert_eq!(
        serde_json::from_value::<RoomActivity>(json).expect("deserialize emote message activity"),
        activity
    );
}

#[test]
fn room_activity_topic_state_notice_round_trips_through_json() {
    let activity = RoomActivity {
        room_id: "!room:example".to_owned(),
        activity_id: "activity-4".to_owned(),
        timestamp_ms: 1_700_000_180_000,
        kind: RoomActivityKind::StateNotice(RoomStateNoticeActivity {
            event_id: "$topic:example".to_owned(),
            sender: "@alice:example".to_owned(),
            state_key: String::new(),
            content: RoomStateNoticeContent::Topic {
                topic: "incident review".to_owned(),
            },
        }),
    };

    assert_eq!(
        serde_json::to_value(&activity).expect("serialize topic state-notice activity"),
        serde_json::json!({
            "room_id": "!room:example",
            "activity_id": "activity-4",
            "timestamp_ms": 1700000180000u64,
            "kind": {
                "type": "state_notice",
                "event_id": "$topic:example",
                "sender": "@alice:example",
                "state_key": "",
                "content": {
                    "type": "topic",
                    "topic": "incident review"
                }
            }
        })
    );
    assert_eq!(
        serde_json::from_value::<RoomActivity>(serde_json::to_value(&activity).unwrap())
            .expect("deserialize topic state-notice activity"),
        activity
    );
}

#[test]
fn room_activity_member_state_notice_round_trips_through_json() {
    let activity = RoomActivity {
        room_id: "!room:example".to_owned(),
        activity_id: "activity-5".to_owned(),
        timestamp_ms: 1_700_000_240_000,
        kind: RoomActivityKind::StateNotice(RoomStateNoticeActivity {
            event_id: "$member:example".to_owned(),
            sender: "@carol:example".to_owned(),
            state_key: "@carol:example".to_owned(),
            content: RoomStateNoticeContent::Member {
                membership: "join".to_owned(),
                displayname: Some("carol".to_owned()),
            },
        }),
    };

    assert_eq!(
        serde_json::to_value(&activity).expect("serialize member state-notice activity"),
        serde_json::json!({
            "room_id": "!room:example",
            "activity_id": "activity-5",
            "timestamp_ms": 1700000240000u64,
            "kind": {
                "type": "state_notice",
                "event_id": "$member:example",
                "sender": "@carol:example",
                "state_key": "@carol:example",
                "content": {
                    "type": "member",
                    "membership": "join",
                    "displayname": "carol"
                }
            }
        })
    );
    assert_eq!(
        serde_json::from_value::<RoomActivity>(serde_json::to_value(&activity).unwrap())
            .expect("deserialize member state-notice activity"),
        activity
    );
}

#[test]
fn room_read_state_event_serializes_to_expected_json_shape() {
    let event = CoreEvent::RoomReadState(RoomReadState {
        room_id: "!ops:example".to_owned(),
        latest_active_event_id: Some("$latest:example".to_owned()),
        fully_read_event_id: Some("$fully-read:example".to_owned()),
        marked_unread: true,
        num_unread: 7,
        num_notifications: 3,
        num_mentions: 1,
    });

    assert_eq!(
        serde_json::to_value(&event).expect("serialize room read state event"),
        serde_json::json!({
            "RoomReadState": {
                "room_id": "!ops:example",
                "latest_active_event_id": "$latest:example",
                "fully_read_event_id": "$fully-read:example",
                "marked_unread": true,
                "num_unread": 7,
                "num_notifications": 3,
                "num_mentions": 1
            }
        })
    );
}

#[test]
fn room_read_state_event_deserializes_from_expected_json_shape() {
    let json = serde_json::json!({
        "RoomReadState": {
            "room_id": "!ops:example",
            "latest_active_event_id": "$latest:example",
            "fully_read_event_id": "$fully-read:example",
            "marked_unread": true,
            "num_unread": 7,
            "num_notifications": 3,
            "num_mentions": 1
        }
    });

    assert_eq!(
        serde_json::from_value::<CoreEvent>(json).expect("deserialize room read state event"),
        CoreEvent::RoomReadState(RoomReadState {
            room_id: "!ops:example".to_owned(),
            latest_active_event_id: Some("$latest:example".to_owned()),
            fully_read_event_id: Some("$fully-read:example".to_owned()),
            marked_unread: true,
            num_unread: 7,
            num_notifications: 3,
            num_mentions: 1,
        })
    );
}

#[test]
fn room_read_state_event_rejects_unknown_fields_inside_payload() {
    let error = serde_json::from_str::<CoreEvent>(
        r#"{
            "RoomReadState":{
                "room_id":"!ops:example",
                "latest_active_event_id":"$latest:example",
                "fully_read_event_id":"$fully-read:example",
                "marked_unread":true,
                "num_unread":7,
                "num_notifications":3,
                "num_mentions":1,
                "unexpected":true
            }
        }"#
    )
    .unwrap_err();

    assert!(error.to_string().contains("unknown field `unexpected`"));
}

#[test]
fn room_activity_contains_matches_identity_and_send_state_fields() {
    let activity = RoomActivity {
        room_id: "!room:example".to_owned(),
        activity_id: "activity-1".to_owned(),
        timestamp_ms: 1_700_000_000_000,
        kind: RoomActivityKind::Message(RoomMessageActivity {
            event_id: Some("$event:example".to_owned()),
            transaction_id: Some("txn-1".to_owned()),
            sender: "@alice:example".to_owned(),
            presentation: RoomMessagePresentationKind::Notice,
            body: "hello".to_owned(),
            send_state: RoomMessageSendState::Failed {
                message: "send failed".to_owned(),
                recoverable: true,
            },
        }),
    };

    assert!(activity.contains("@alice:example"));
    assert!(activity.contains("\"activity_id\":\"activity-1\""));
    assert!(activity.contains("\"event_id\":\"$event:example\""));
    assert!(activity.contains("\"transaction_id\":\"txn-1\""));
    assert!(activity.contains("\"type\":\"message\""));
    assert!(!activity.contains("\"type\":\"state_notice\""));
    assert!(activity.contains("\"presentation\":\"notice\""));
    assert!(!activity.contains("\"presentation\":\"text\""));
    assert!(activity.contains(
        "\"send_state\":{\"state\":\"failed\",\"message\":\"send failed\",\"recoverable\":true}"
    ));
}

#[test]
fn model_last_activity_round_trips_through_json() {
    let model = Model {
        status: "synced".to_owned(),
        lifecycle_state: "ready".to_owned(),
        rooms: vec![],
        last_activity: Some(RoomMessageActivityLine {
            room_id: "!room:example".to_owned(),
            activity_id: "activity-6".to_owned(),
            timestamp_ms: 1_700_000_300_000,
            event_id: Some("$event:last:example".to_owned()),
            transaction_id: None,
            sender: "@alice:example".to_owned(),
            presentation: RoomMessagePresentationKind::Emote,
            body: "latest".to_owned(),
            send_state: RoomMessageSendState::Sent,
        }),
        last_verification: None,
        last_error: None,
        selected_room: Some("!room:example".to_owned()),
    };

    assert_eq!(
        serde_json::to_value(&model).expect("serialize model with last_activity"),
        serde_json::json!({
            "status": "synced",
            "lifecycle_state": "ready",
            "rooms": [],
            "last_activity": {
                "room_id": "!room:example",
                "activity_id": "activity-6",
                "timestamp_ms": 1700000300000u64,
                "event_id": "$event:last:example",
                "transaction_id": null,
                "sender": "@alice:example",
                "presentation": "emote",
                "body": "latest",
                "send_state": {
                    "state": "sent"
                }
            },
            "last_verification": null,
            "last_error": null,
            "selected_room": "!room:example"
        })
    );
    assert_eq!(
        serde_json::from_value::<Model>(serde_json::to_value(&model).unwrap())
            .expect("deserialize model with last_activity"),
        model
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
            "last_activity":null,
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
            "protocol_version":11,
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
        serde_json::from_str::<RoomMessageActivityLine>(
            r#"{
            "room_id":"!room:example",
            "activity_id":"activity-1",
            "timestamp_ms":1700000000000,
            "event_id":"$event:example",
            "transaction_id":"txn-1",
            "sender":"@alice:example",
            "presentation":"text",
            "body":"hello",
            "send_state":{"state":"sent"},
            "unexpected":true
        }"#
        )
        .is_err()
    );
    assert!(
        serde_json::from_str::<RoomActivity>(
            r#"{
            "room_id":"!room:example",
            "activity_id":"activity-1",
            "timestamp_ms":1700000000000,
            "kind":{
                "type":"message",
                "event_id":"$event:example",
                "transaction_id":"txn-1",
                "sender":"@alice:example",
                "presentation":"text",
                "body":"hello",
                "send_state":{"state":"sent"}
            },
            "unexpected":true
        }"#
        )
        .is_err()
    );
    assert!(
        serde_json::from_str::<RoomActivityKind>(
            r#"{
            "type":"message",
            "event_id":"$event:example",
            "transaction_id":"txn-1",
            "sender":"@alice:example",
            "presentation":"text",
            "body":"hello",
            "send_state":{"state":"sent"},
            "unexpected":true
        }"#
        )
        .is_err()
    );
    assert!(
        serde_json::from_str::<RoomMessageActivity>(
            r#"{
            "event_id":"$event:example",
            "transaction_id":"txn-1",
            "sender":"@alice:example",
            "presentation":"text",
            "body":"hello",
            "send_state":{"state":"sent"},
            "unexpected":true
        }"#
        )
        .is_err()
    );
    assert!(
        serde_json::from_str::<RoomMessageSendState>(
            r#"{
            "state":"failed",
            "message":"send failed",
            "recoverable":true,
            "unexpected":true
        }"#
        )
        .is_err()
    );
    assert!(
        serde_json::from_str::<RoomActivityKind>(
            r#"{
            "type":"state_notice",
            "event_id":"$topic:example",
            "sender":"@alice:example",
            "state_key":"",
            "content":{"type":"topic","topic":"incident review"},
            "unexpected":true
        }"#
        )
        .is_err()
    );
    assert!(
        serde_json::from_str::<RoomStateNoticeActivity>(
            r#"{
            "event_id":"$topic:example",
            "sender":"@alice:example",
            "state_key":"",
            "content":{"type":"topic","topic":"incident review"},
            "unexpected":true
        }"#
        )
        .is_err()
    );
    assert!(
        serde_json::from_str::<RoomStateNoticeContent>(
            r#"{
            "type":"topic",
            "topic":"incident review",
            "unexpected":true
        }"#
        )
        .is_err()
    );
    assert!(
        serde_json::from_str::<RoomStateNoticeContent>(
            r#"{
            "type":"member",
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
fn room_message_presentation_contracts_are_strict() {
    assert!(
        serde_json::from_str::<RoomMessageActivity>(
            r#"{
            "event_id":"$event:example",
            "transaction_id":"txn-1",
            "sender":"@alice:example",
            "body":"hello",
            "send_state":{"state":"sent"}
        }"#
        )
        .is_err()
    );
    assert!(
        serde_json::from_str::<RoomMessageActivity>(
            r#"{
            "event_id":"$event:example",
            "transaction_id":"txn-1",
            "sender":"@alice:example",
            "presentation":"status",
            "body":"hello",
            "send_state":{"state":"sent"}
        }"#
        )
        .is_err()
    );
    assert!(
        serde_json::from_str::<RoomMessageActivityLine>(
            r#"{
            "room_id":"!room:example",
            "activity_id":"activity-1",
            "timestamp_ms":1700000000000,
            "event_id":"$event:example",
            "transaction_id":"txn-1",
            "sender":"@alice:example",
            "body":"hello",
            "send_state":{"state":"sent"}
        }"#
        )
        .is_err()
    );
    assert!(
        serde_json::from_str::<RoomMessageActivityLine>(
            r#"{
            "room_id":"!room:example",
            "activity_id":"activity-1",
            "timestamp_ms":1700000000000,
            "event_id":"$event:example",
            "transaction_id":"txn-1",
            "sender":"@alice:example",
            "presentation":"status",
            "body":"hello",
            "send_state":{"state":"sent"}
        }"#
        )
        .is_err()
    );
}

#[test]
fn model_last_verification_round_trips_typed_state_json() {
    let model = serde_json::from_value::<Model>(serde_json::json!({
        "status":"verification sas ready with @alice:example",
        "lifecycle_state":"ready",
        "rooms":[],
        "last_activity":null,
        "last_verification":{
            "flow_id":"flow-verify-123",
            "user_id":"@alice:example",
            "state":{
                "state":"cancelled",
                "reason":"timed out",
                "code":"m.timeout",
                "cancelled_by_us":false
            }
        },
        "last_error":null,
        "selected_room":null
    }))
    .expect("deserialize typed verification model");

    assert_eq!(
        model.last_verification,
        Some(Verification {
            flow_id: "flow-verify-123".to_owned(),
            user_id: "@alice:example".to_owned(),
            state: VerificationState::Cancelled {
                reason: "timed out".to_owned(),
                code: "m.timeout".to_owned(),
                cancelled_by_us: false,
            },
        })
    );

    assert_eq!(
        serde_json::to_value(&model).expect("serialize typed verification model"),
        serde_json::json!({
            "status":"verification sas ready with @alice:example",
            "lifecycle_state":"ready",
            "rooms":[],
            "last_activity":null,
            "last_verification":{
                "flow_id":"flow-verify-123",
                "user_id":"@alice:example",
                "state":{
                    "state":"cancelled",
                    "reason":"timed out",
                    "code":"m.timeout",
                    "cancelled_by_us":false
                }
            },
            "last_error":null,
            "selected_room":null
        })
    );
}

#[test]
fn room_state_notice_content_rejects_impossible_tagged_combinations() {
    assert!(
        serde_json::from_str::<RoomStateNoticeContent>(
            r#"{
            "type":"topic",
            "membership":"join",
            "displayname":"alice"
        }"#
        )
        .is_err()
    );
    assert!(
        serde_json::from_str::<RoomStateNoticeActivity>(
            r#"{
            "event_id":"$member:example",
            "sender":"@alice:example",
            "state_key":"@alice:example",
            "content":{
                "type":"member",
                "membership":"join",
                "topic":"incident review"
            }
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
