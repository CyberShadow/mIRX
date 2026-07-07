use std::time::Duration;

#[cfg(not(windows))]
use std::ffi::OsString;
use std::path::PathBuf;

use mmatrix_core::{
    E2eeBackupState, E2eeBackupStatus, E2eeCrossSigningStatus, E2eeRecoveryState, E2eeStatus,
    Event as CoreEvent, FetchRoomImageMediaRequest as CoreFetchRoomImageMediaRequest,
    RoomActivityUpdate as CoreRoomActivityUpdate,
    RoomActivityUpdateKind as CoreRoomActivityUpdateKind,
    RoomActivityUpdateTarget as CoreRoomActivityUpdateTarget, RoomImageMedia as CoreRoomImageMedia,
    RoomImageMediaFormat as CoreRoomImageMediaFormat, RoomImageMediaKind as CoreRoomImageMediaKind,
    RoomMediaSource as CoreRoomMediaSource, RoomReadState,
};
use mmatrix_testkit::{
    ComposeKey, FetchRoomImageMediaRequest, FrameSize, LaunchOptions, LoginArgs, MdiState, MdiView,
    MembershipCommandAction, Model, NavigationNode, NavigationPathSegment, ProtocolVersion,
    RecoverE2eeArgs, RestoreSessionArgs, RoomActivity, RoomActivityKind, RoomActivityRelation,
    RoomActivityUpdate, RoomActivityUpdateKind, RoomActivityUpdateTarget, RoomImageActivity,
    RoomImageInfo, RoomImageMedia, RoomImageMediaFormat, RoomImageMediaKind, RoomImageMediaState,
    RoomMediaSource, RoomMemberView, RoomMessageActivity, RoomMessageActivityLine,
    RoomMessagePresentationKind, RoomMessageSendState, RoomRedactedActivity,
    RoomStateNoticeActivity, RoomStateNoticeContent, RoomTag, SpaceChild, StatusBarState,
    StoredSession, SubmittedDirectRoomCommand, SubmittedMembershipCommand,
    SubmittedRoomTopicCommand, SwitchbarItem, SwitchbarState, TestkitError, UiError,
    UiRoomReadState, UiRoomState, UiState, UnsupportedRoomMessageActivity, Verification,
    VerificationActionArgs, VerificationDecimals, VerificationEmoji, VerificationState,
    ViewIdentity,
};

#[cfg(not(windows))]
use mmatrix_testkit::MmatrixApp;

fn stored_session() -> StoredSession {
    StoredSession {
        user_id: "@alice:example".to_owned(),
        device_id: "DEVICE".to_owned(),
        access_token: "secret-token".to_owned(),
        refresh_token: Some("refresh-token".to_owned()),
    }
}

fn sample_e2ee_status() -> E2eeStatus {
    E2eeStatus {
        recovery: E2eeRecoveryState::Incomplete,
        backup: E2eeBackupStatus {
            state: E2eeBackupState::Enabled,
            enabled_locally: true,
            exists_on_server: Some(true),
        },
        cross_signing: Some(E2eeCrossSigningStatus {
            has_master: true,
            has_self_signing: true,
            has_user_signing: false,
            complete: false,
        }),
    }
}

fn sample_room_encrypted_file_key() -> mmatrix_testkit::RoomEncryptedFileKey {
    mmatrix_testkit::RoomEncryptedFileKey {
        kty: "oct".to_owned(),
        key_ops: vec!["encrypt".to_owned(), "decrypt".to_owned()],
        alg: "A256CTR".to_owned(),
        k: "base64url-key".to_owned(),
        ext: true,
    }
}

fn sample_room_encrypted_file_hashes() -> mmatrix_testkit::RoomEncryptedFileHashes {
    mmatrix_testkit::RoomEncryptedFileHashes {
        sha256: "base64-sha256".to_owned(),
    }
}

fn sample_encrypted_room_media_source(url: &str) -> RoomMediaSource {
    RoomMediaSource::Encrypted {
        url: url.to_owned(),
        key: sample_room_encrypted_file_key(),
        iv: "base64-iv".to_owned(),
        hashes: sample_room_encrypted_file_hashes(),
        v: "v2".to_owned(),
    }
}

fn sample_room_image_activity(media_state: Option<RoomImageMediaState>) -> RoomImageActivity {
    RoomImageActivity {
        event_id: "$image1:example.org".to_owned(),
        sender: "@alice:example.org".to_owned(),
        body: "diagram.png".to_owned(),
        filename: Some("diagram.png".to_owned()),
        source: RoomMediaSource::Plain {
            url: "mxc://example.org/diagram".to_owned(),
        },
        info: Some(RoomImageInfo {
            mimetype: Some("image/png".to_owned()),
            size: Some(12_345),
            width: Some(640),
            height: Some(480),
            is_animated: Some(false),
            thumbnail_source: None,
        }),
        media_state,
    }
}

#[test]
fn launch_options_default_has_expected_timeout() {
    assert_eq!(
        LaunchOptions::default(),
        LaunchOptions {
            startup_timeout: Duration::from_secs(10),
            local_app_data: None,
            frame_size: None,
        }
    );
}

#[test]
fn frame_size_parse_arg_accepts_expected_shape() {
    assert_eq!(
        FrameSize::parse_arg("640x768"),
        Ok(FrameSize {
            width: 640,
            height: 768,
        })
    );
    assert_eq!(
        FrameSize {
            width: 640,
            height: 768,
        }
        .to_string(),
        "640x768"
    );
}

#[test]
fn frame_size_parse_arg_rejects_malformed_values() {
    for value in [
        "640",
        "640X768",
        "x768",
        "640x",
        "0x768",
        "640x0",
        "-1x768",
        "640x768x1",
    ] {
        FrameSize::parse_arg(value).expect_err("frame size must reject malformed values");
    }
}

#[test]
fn launch_options_support_explicit_frame_size() {
    let options = LaunchOptions {
        startup_timeout: Duration::from_secs(1),
        local_app_data: Some(PathBuf::from("/var/tmp/mmatrix-testkit")),
        frame_size: Some(FrameSize {
            width: 640,
            height: 768,
        }),
    };

    assert_eq!(
        options.frame_size,
        Some(FrameSize {
            width: 640,
            height: 768,
        })
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
                is_direct: false,
                direct_user_ids: Vec::new(),
                tags: Vec::new(),
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
                is_direct: true,
                direct_user_ids: vec!["@alice:example".to_owned()],
                tags: vec![
                    RoomTag {
                        tag: "m.favourite".to_owned(),
                        order: Some("0.25".to_owned()),
                    },
                    RoomTag {
                        tag: "u.work".to_owned(),
                        order: None,
                    },
                ],
                space_children: vec![],
            },
            mmatrix_testkit::Room {
                room_id: "!beta:example".to_owned(),
                display_name: "Beta".to_owned(),
                encrypted: false,
                is_space: false,
                is_direct: false,
                direct_user_ids: Vec::new(),
                tags: Vec::new(),
                space_children: vec![],
            },
            mmatrix_testkit::Room {
                room_id: "!lobby:example".to_owned(),
                display_name: "Lobby".to_owned(),
                encrypted: false,
                is_space: false,
                is_direct: false,
                direct_user_ids: Vec::new(),
                tags: Vec::new(),
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
        e2ee_status: Some(sample_e2ee_status()),
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
        submitted_membership_commands: Vec::new(),
        submitted_direct_room_commands: Vec::new(),
        submitted_room_topic_commands: Vec::new(),
        submitted_verification_actions: Vec::new(),
        last_error: Some(UiError {
            operation: "send text".to_owned(),
            message: "failed".to_owned(),
        }),
        selected_room: Some("!room:example".to_owned()),
    };
    let version = ProtocolVersion {
        protocol_version: 15,
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
                    members: Vec::new(),
                },
                MdiView {
                    identity: ViewIdentity::Room {
                        room_id: "!room:example".to_owned(),
                    },
                    title: "Room".to_owned(),
                    active: true,
                    lines: vec!["hello".to_owned()],
                    members: vec![RoomMemberView {
                        user_id: "@alice:example".to_owned(),
                        display_name: Some("Alice".to_owned()),
                        label: "Alice".to_owned(),
                    }],
                },
                MdiView {
                    identity: ViewIdentity::Thread {
                        room_id: "!room:example".to_owned(),
                        thread_root_event_id: "$thread-root:example".to_owned(),
                    },
                    title: "Thread".to_owned(),
                    active: false,
                    lines: vec!["reply".to_owned()],
                    members: Vec::new(),
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
                SwitchbarItem {
                    identity: ViewIdentity::Thread {
                        room_id: "!room:example".to_owned(),
                        thread_root_event_id: "$thread-root:example".to_owned(),
                    },
                    title: "Thread".to_owned(),
                    active: false,
                    has_activity: true,
                },
            ],
        },
        rooms: vec![UiRoomState {
            room_id: "!room:example".to_owned(),
            display_name: "Room".to_owned(),
            encrypted: true,
            is_space: false,
            is_direct: true,
            direct_user_ids: vec!["@alice:example".to_owned()],
            tags: vec![RoomTag {
                tag: "m.favourite".to_owned(),
                order: Some("0.25".to_owned()),
            }],
            space_children: vec![],
            has_activity: true,
            read_target_event_id: Some("$event:example".to_owned()),
            read_state: Some(UiRoomReadState {
                latest_active_event_id: Some("$latest:example".to_owned()),
                fully_read_event_id: Some("$fully-read:example".to_owned()),
                marked_unread: true,
                num_unread: 7,
                num_notifications: 3,
                num_mentions: 1,
            }),
        }],
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
                    "is_direct": false,
                    "direct_user_ids": [],
                    "tags": [],
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
                    "is_direct": true,
                    "direct_user_ids": ["@alice:example"],
                    "tags": [
                        {
                            "tag": "m.favourite",
                            "order": "0.25"
                        },
                        {
                            "tag": "u.work",
                            "order": null
                        }
                    ],
                    "space_children": []
                },
                {
                    "room_id": "!beta:example",
                    "display_name": "Beta",
                    "encrypted": false,
                    "is_space": false,
                    "is_direct": false,
                    "direct_user_ids": [],
                    "tags": [],
                    "space_children": []
                },
                {
                    "room_id": "!lobby:example",
                    "display_name": "Lobby",
                    "encrypted": false,
                    "is_space": false,
                    "is_direct": false,
                    "direct_user_ids": [],
                    "tags": [],
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
        serde_json::from_slice::<serde_json::Value>(&ui_state_json)
            .expect("deserialize ui state value"),
        serde_json::json!({
            "navigation": {
                "text": "mmatrix",
                "children": [
                    {
                        "text": "status",
                        "children": [],
                    },
                    {
                        "text": "rooms: 1",
                        "children": [
                            {
                                "text": "Room",
                                "children": [],
                            }
                        ],
                    }
                ],
            },
            "mdi": {
                "active": {
                    "kind": "room",
                    "room_id": "!room:example",
                },
                "views": [
                    {
                        "identity": {
                            "kind": "status",
                        },
                        "title": "Status",
                        "active": false,
                        "lines": ["status: synced"],
                        "members": [],
                    },
                    {
                        "identity": {
                            "kind": "room",
                            "room_id": "!room:example",
                        },
                        "title": "Room",
                        "active": true,
                        "lines": ["hello"],
                        "members": [
                            {
                                "user_id": "@alice:example",
                                "display_name": "Alice",
                                "label": "Alice",
                            }
                        ],
                    },
                    {
                        "identity": {
                            "kind": "thread",
                            "room_id": "!room:example",
                            "thread_root_event_id": "$thread-root:example",
                        },
                        "title": "Thread",
                        "active": false,
                        "lines": ["reply"],
                        "members": [],
                    }
                ],
            },
            "status_bar": {
                "text": "synced 1 room(s)",
            },
            "switchbar": {
                "items": [
                    {
                        "identity": {
                            "kind": "status",
                        },
                        "title": "Status",
                        "active": false,
                        "has_activity": false,
                    },
                    {
                        "identity": {
                            "kind": "room",
                            "room_id": "!room:example",
                        },
                        "title": "Room",
                        "active": true,
                        "has_activity": true,
                    },
                    {
                        "identity": {
                            "kind": "thread",
                            "room_id": "!room:example",
                            "thread_root_event_id": "$thread-root:example",
                        },
                        "title": "Thread",
                        "active": false,
                        "has_activity": true,
                    }
                ],
            },
            "rooms": [
                {
                    "room_id": "!room:example",
                    "display_name": "Room",
                    "encrypted": true,
                    "is_space": false,
                    "is_direct": true,
                    "direct_user_ids": ["@alice:example"],
                    "tags": [
                        {
                            "tag": "m.favourite",
                            "order": "0.25",
                        }
                    ],
                    "space_children": [],
                    "has_activity": true,
                    "read_target_event_id": "$event:example",
                    "read_state": {
                        "latest_active_event_id": "$latest:example",
                        "fully_read_event_id": "$fully-read:example",
                        "marked_unread": true,
                        "num_unread": 7u64,
                        "num_notifications": 3u64,
                        "num_mentions": 1u64,
                    },
                }
            ],
        })
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
fn room_summary_metadata_serializes_to_expected_json_shape() {
    let room = mmatrix_testkit::Room {
        room_id: "!alpha:example".to_owned(),
        display_name: "Alpha".to_owned(),
        encrypted: true,
        is_space: false,
        is_direct: true,
        direct_user_ids: vec!["@alice:example.org".to_owned()],
        tags: vec![
            RoomTag {
                tag: "m.favourite".to_owned(),
                order: Some("0.25".to_owned()),
            },
            RoomTag {
                tag: "u.work".to_owned(),
                order: None,
            },
        ],
        space_children: vec![],
    };

    assert_eq!(
        serde_json::to_value(&room).expect("serialize room metadata"),
        serde_json::json!({
            "room_id": "!alpha:example",
            "display_name": "Alpha",
            "encrypted": true,
            "is_space": false,
            "is_direct": true,
            "direct_user_ids": ["@alice:example.org"],
            "tags": [
                {
                    "tag": "m.favourite",
                    "order": "0.25",
                },
                {
                    "tag": "u.work",
                    "order": null,
                }
            ],
            "space_children": [],
        })
    );
}

#[test]
fn room_activity_sent_message_round_trips_through_json() {
    let activity = RoomActivity {
        room_id: "!room:example".to_owned(),
        activity_id: "activity-1".to_owned(),
        timestamp_ms: 1_700_000_000_000,
        relation: None,
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
            "relation": null,
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
        relation: None,
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
            "relation": null,
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
        relation: None,
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
            "relation": null,
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
        relation: None,
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
        "relation": null,
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
        relation: None,
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
        "relation": null,
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
        relation: None,
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
            "relation": null,
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
        relation: None,
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
            "relation": null,
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
fn room_activity_unsupported_message_round_trips_through_json() {
    let activity = RoomActivity {
        room_id: "!room:example".to_owned(),
        activity_id: "event:$audio1:example.org".to_owned(),
        timestamp_ms: 1_710_000_000_000,
        relation: None,
        kind: RoomActivityKind::UnsupportedMessage(UnsupportedRoomMessageActivity {
            event_id: "$audio1:example.org".to_owned(),
            sender: "@alice:example.org".to_owned(),
            msgtype: "m.audio".to_owned(),
            body: "voice-note.ogg".to_owned(),
        }),
    };

    let json = serde_json::json!({
        "room_id": "!room:example",
        "activity_id": "event:$audio1:example.org",
        "timestamp_ms": 1710000000000u64,
        "relation": null,
        "kind": {
            "type": "unsupported_message",
            "event_id": "$audio1:example.org",
            "sender": "@alice:example.org",
            "msgtype": "m.audio",
            "body": "voice-note.ogg"
        }
    });

    assert_eq!(
        serde_json::to_value(&activity).expect("serialize unsupported message activity"),
        json
    );
    assert_eq!(
        serde_json::from_value::<RoomActivity>(json)
            .expect("deserialize unsupported message activity"),
        activity
    );
}

#[test]
fn room_activity_image_round_trips_through_json() {
    let activity = RoomActivity {
        room_id: "!room:example".to_owned(),
        activity_id: "event:$image1:example.org".to_owned(),
        timestamp_ms: 1_710_000_000_000,
        relation: None,
        kind: RoomActivityKind::Image(Box::new(sample_room_image_activity(None))),
    };

    let json = serde_json::json!({
        "room_id": "!room:example",
        "activity_id": "event:$image1:example.org",
        "timestamp_ms": 1710000000000u64,
        "relation": null,
        "kind": {
            "type": "image",
            "event_id": "$image1:example.org",
            "sender": "@alice:example.org",
            "body": "diagram.png",
            "filename": "diagram.png",
            "source": {
                "type": "plain",
                "url": "mxc://example.org/diagram"
            },
            "info": {
                "mimetype": "image/png",
                "size": 12345u64,
                "width": 640u64,
                "height": 480u64,
                "is_animated": false,
                "thumbnail_source": null
            },
            "media_state": null
        }
    });

    assert_eq!(
        serde_json::to_value(&activity).expect("serialize image activity"),
        json
    );
    assert_eq!(
        serde_json::from_value::<RoomActivity>(json).expect("deserialize image activity"),
        activity
    );
}

#[test]
fn room_activity_image_pending_media_state_round_trips_through_json() {
    let activity = RoomActivity {
        room_id: "!room:example".to_owned(),
        activity_id: "event:$image1:example.org".to_owned(),
        timestamp_ms: 1_710_000_000_000,
        relation: None,
        kind: RoomActivityKind::Image(Box::new(sample_room_image_activity(Some(
            RoomImageMediaState::Pending,
        )))),
    };

    let json = serde_json::json!({
        "room_id": "!room:example",
        "activity_id": "event:$image1:example.org",
        "timestamp_ms": 1710000000000u64,
        "relation": null,
        "kind": {
            "type": "image",
            "event_id": "$image1:example.org",
            "sender": "@alice:example.org",
            "body": "diagram.png",
            "filename": "diagram.png",
            "source": {
                "type": "plain",
                "url": "mxc://example.org/diagram"
            },
            "info": {
                "mimetype": "image/png",
                "size": 12345u64,
                "width": 640u64,
                "height": 480u64,
                "is_animated": false,
                "thumbnail_source": null
            },
            "media_state": {
                "type": "pending"
            }
        }
    });

    assert_eq!(
        serde_json::to_value(&activity).expect("serialize image activity with pending media"),
        json
    );
    assert_eq!(
        serde_json::from_value::<RoomActivity>(json)
            .expect("deserialize image activity with pending media"),
        activity
    );
}

#[test]
fn room_activity_image_loaded_media_state_round_trips_through_json() {
    let activity = RoomActivity {
        room_id: "!room:example".to_owned(),
        activity_id: "event:$image1:example.org".to_owned(),
        timestamp_ms: 1_710_000_000_000,
        relation: None,
        kind: RoomActivityKind::Image(Box::new(sample_room_image_activity(Some(
            RoomImageMediaState::Loaded {
                width: 320,
                height: 240,
            },
        )))),
    };

    let json = serde_json::json!({
        "room_id": "!room:example",
        "activity_id": "event:$image1:example.org",
        "timestamp_ms": 1710000000000u64,
        "relation": null,
        "kind": {
            "type": "image",
            "event_id": "$image1:example.org",
            "sender": "@alice:example.org",
            "body": "diagram.png",
            "filename": "diagram.png",
            "source": {
                "type": "plain",
                "url": "mxc://example.org/diagram"
            },
            "info": {
                "mimetype": "image/png",
                "size": 12345u64,
                "width": 640u64,
                "height": 480u64,
                "is_animated": false,
                "thumbnail_source": null
            },
            "media_state": {
                "type": "loaded",
                "width": 320u64,
                "height": 240u64
            }
        }
    });

    assert_eq!(
        serde_json::to_value(&activity).expect("serialize image activity with loaded media"),
        json
    );
    assert_eq!(
        serde_json::from_value::<RoomActivity>(json)
            .expect("deserialize image activity with loaded media"),
        activity
    );
}

#[test]
fn room_activity_image_failed_media_state_round_trips_through_json() {
    let activity = RoomActivity {
        room_id: "!room:example".to_owned(),
        activity_id: "event:$image1:example.org".to_owned(),
        timestamp_ms: 1_710_000_000_000,
        relation: None,
        kind: RoomActivityKind::Image(Box::new(sample_room_image_activity(Some(
            RoomImageMediaState::Failed {
                message: "download failed".to_owned(),
            },
        )))),
    };

    let json = serde_json::json!({
        "room_id": "!room:example",
        "activity_id": "event:$image1:example.org",
        "timestamp_ms": 1710000000000u64,
        "relation": null,
        "kind": {
            "type": "image",
            "event_id": "$image1:example.org",
            "sender": "@alice:example.org",
            "body": "diagram.png",
            "filename": "diagram.png",
            "source": {
                "type": "plain",
                "url": "mxc://example.org/diagram"
            },
            "info": {
                "mimetype": "image/png",
                "size": 12345u64,
                "width": 640u64,
                "height": 480u64,
                "is_animated": false,
                "thumbnail_source": null
            },
            "media_state": {
                "type": "failed",
                "message": "download failed"
            }
        }
    });

    assert_eq!(
        serde_json::to_value(&activity).expect("serialize image activity with failed media"),
        json
    );
    assert_eq!(
        serde_json::from_value::<RoomActivity>(json)
            .expect("deserialize image activity with failed media"),
        activity
    );
}

#[test]
fn room_activity_redacted_round_trips_through_json() {
    let activity = RoomActivity {
        room_id: "!room:example".to_owned(),
        activity_id: "event:$redacted:example.org".to_owned(),
        timestamp_ms: 1_710_000_060_000,
        relation: None,
        kind: RoomActivityKind::Redacted(RoomRedactedActivity {
            event_id: "$redacted:example.org".to_owned(),
        }),
    };

    let json = serde_json::json!({
        "room_id": "!room:example",
        "activity_id": "event:$redacted:example.org",
        "timestamp_ms": 1710000060000u64,
        "relation": null,
        "kind": {
            "type": "redacted",
            "event_id": "$redacted:example.org"
        }
    });

    assert_eq!(
        serde_json::to_value(&activity).expect("serialize redacted activity"),
        json
    );
    assert_eq!(
        serde_json::from_value::<RoomActivity>(json).expect("deserialize redacted activity"),
        activity
    );
}

#[test]
fn room_activity_relation_reply_round_trips_through_json() {
    let relation = RoomActivityRelation::Reply {
        in_reply_to_event_id: "$reply:example".to_owned(),
    };
    let json = serde_json::json!({
        "type": "reply",
        "in_reply_to_event_id": "$reply:example",
    });

    assert_eq!(
        serde_json::to_value(&relation).expect("serialize reply relation"),
        json
    );
    assert_eq!(
        serde_json::from_value::<RoomActivityRelation>(json).expect("deserialize reply relation"),
        relation
    );
    assert!(
        serde_json::from_value::<RoomActivityRelation>(serde_json::json!({
            "type": "reply",
            "in_reply_to_event_id": "$reply:example",
            "unexpected": true,
        }))
        .is_err()
    );
}

#[test]
fn room_activity_relation_thread_round_trips_through_json() {
    let relation = RoomActivityRelation::Thread {
        thread_root_event_id: "$thread-root:example".to_owned(),
        in_reply_to_event_id: Some("$reply:example".to_owned()),
        is_falling_back: false,
    };
    let json = serde_json::json!({
        "type": "thread",
        "thread_root_event_id": "$thread-root:example",
        "in_reply_to_event_id": "$reply:example",
        "is_falling_back": false,
    });

    assert_eq!(
        serde_json::to_value(&relation).expect("serialize thread relation"),
        json
    );
    assert_eq!(
        serde_json::from_value::<RoomActivityRelation>(json).expect("deserialize thread relation"),
        relation
    );
    assert!(
        serde_json::from_value::<RoomActivityRelation>(serde_json::json!({
            "type": "thread",
            "thread_root_event_id": "$thread-root:example",
            "in_reply_to_event_id": "$reply:example",
            "is_falling_back": false,
            "unexpected": true,
        }))
        .is_err()
    );
}

#[test]
fn room_activity_omitted_relation_defaults_to_none() {
    let json = serde_json::json!({
        "room_id": "!room:example",
        "activity_id": "activity-omitted-relation",
        "timestamp_ms": 1700000000000u64,
        "kind": {
            "type": "message",
            "event_id": "$event:example",
            "transaction_id": null,
            "sender": "@alice:example",
            "presentation": "text",
            "body": "hello",
            "send_state": {
                "state": "sent"
            }
        }
    });

    assert_eq!(
        serde_json::from_value::<RoomActivity>(json)
            .expect("deserialize activity without relation"),
        RoomActivity {
            room_id: "!room:example".to_owned(),
            activity_id: "activity-omitted-relation".to_owned(),
            timestamp_ms: 1_700_000_000_000,
            relation: None,
            kind: RoomActivityKind::Message(RoomMessageActivity {
                event_id: Some("$event:example".to_owned()),
                transaction_id: None,
                sender: "@alice:example".to_owned(),
                presentation: RoomMessagePresentationKind::Text,
                body: "hello".to_owned(),
                send_state: RoomMessageSendState::Sent,
            }),
        }
    );
}

#[test]
fn room_activity_thread_relation_round_trips_through_json() {
    let activity = RoomActivity {
        room_id: "!room:example".to_owned(),
        activity_id: "activity-thread".to_owned(),
        timestamp_ms: 1_700_000_300_000,
        relation: Some(RoomActivityRelation::Thread {
            thread_root_event_id: "$thread-root:example".to_owned(),
            in_reply_to_event_id: Some("$reply:example".to_owned()),
            is_falling_back: true,
        }),
        kind: RoomActivityKind::Message(RoomMessageActivity {
            event_id: Some("$thread-event:example".to_owned()),
            transaction_id: None,
            sender: "@alice:example".to_owned(),
            presentation: RoomMessagePresentationKind::Text,
            body: "thread reply".to_owned(),
            send_state: RoomMessageSendState::Sent,
        }),
    };
    let json = serde_json::json!({
        "room_id": "!room:example",
        "activity_id": "activity-thread",
        "timestamp_ms": 1700000300000u64,
        "relation": {
            "type": "thread",
            "thread_root_event_id": "$thread-root:example",
            "in_reply_to_event_id": "$reply:example",
            "is_falling_back": true,
        },
        "kind": {
            "type": "message",
            "event_id": "$thread-event:example",
            "transaction_id": null,
            "sender": "@alice:example",
            "presentation": "text",
            "body": "thread reply",
            "send_state": {
                "state": "sent"
            }
        }
    });

    assert_eq!(
        serde_json::to_value(&activity).expect("serialize thread activity"),
        json
    );
    assert_eq!(
        serde_json::from_value::<RoomActivity>(json).expect("deserialize thread activity"),
        activity
    );
}

#[test]
fn room_activity_update_dtos_round_trip_across_testkit_and_core() {
    let sent = RoomActivityUpdate {
        room_id: "!ops:example.org".to_owned(),
        target: RoomActivityUpdateTarget::Transaction {
            transaction_id: "txn-1".to_owned(),
        },
        kind: RoomActivityUpdateKind::MessageSent {
            event_id: "$event:example.org".to_owned(),
        },
    };
    let failed = RoomActivityUpdate {
        room_id: "!ops:example.org".to_owned(),
        target: RoomActivityUpdateTarget::Transaction {
            transaction_id: "txn-1".to_owned(),
        },
        kind: RoomActivityUpdateKind::MessageFailed {
            message: "send failed".to_owned(),
            recoverable: true,
        },
    };
    let replaced = RoomActivityUpdate {
        room_id: "!ops:example.org".to_owned(),
        target: RoomActivityUpdateTarget::Event {
            event_id: "$original:example.org".to_owned(),
        },
        kind: RoomActivityUpdateKind::MessageReplaced {
            replacement_event_id: "$edit:example.org".to_owned(),
            sender: "@alice:example.org".to_owned(),
            timestamp_ms: 1_700_000_001_000,
            presentation: RoomMessagePresentationKind::Text,
            body: "edited body".to_owned(),
        },
    };
    let redacted = RoomActivityUpdate {
        room_id: "!ops:example.org".to_owned(),
        target: RoomActivityUpdateTarget::Event {
            event_id: "$original:example.org".to_owned(),
        },
        kind: RoomActivityUpdateKind::ActivityRedacted {
            redaction_event_id: "$redaction:example.org".to_owned(),
        },
    };

    let sent_json = serde_json::json!({
        "room_id": "!ops:example.org",
        "target": {
            "type": "transaction",
            "transaction_id": "txn-1"
        },
        "kind": {
            "type": "message_sent",
            "event_id": "$event:example.org"
        }
    });
    let failed_json = serde_json::json!({
        "room_id": "!ops:example.org",
        "target": {
            "type": "transaction",
            "transaction_id": "txn-1"
        },
        "kind": {
            "type": "message_failed",
            "message": "send failed",
            "recoverable": true
        }
    });
    let replaced_json = serde_json::json!({
        "room_id": "!ops:example.org",
        "target": {
            "type": "event",
            "event_id": "$original:example.org"
        },
        "kind": {
            "type": "message_replaced",
            "replacement_event_id": "$edit:example.org",
            "sender": "@alice:example.org",
            "timestamp_ms": 1700000001000u64,
            "presentation": "text",
            "body": "edited body"
        }
    });
    let redacted_json = serde_json::json!({
        "room_id": "!ops:example.org",
        "target": {
            "type": "event",
            "event_id": "$original:example.org"
        },
        "kind": {
            "type": "activity_redacted",
            "redaction_event_id": "$redaction:example.org"
        }
    });

    assert_eq!(
        serde_json::to_value(&sent).expect("serialize sent update"),
        sent_json
    );
    assert_eq!(
        serde_json::to_value(&failed).expect("serialize failed update"),
        failed_json
    );
    assert_eq!(
        serde_json::to_value(&replaced).expect("serialize replaced update"),
        replaced_json
    );
    assert_eq!(
        serde_json::to_value(&redacted).expect("serialize redacted update"),
        redacted_json
    );

    assert_eq!(
        serde_json::from_value::<CoreRoomActivityUpdate>(sent_json.clone())
            .expect("deserialize sent update as core"),
        CoreRoomActivityUpdate {
            room_id: "!ops:example.org".to_owned(),
            target: CoreRoomActivityUpdateTarget::Transaction {
                transaction_id: "txn-1".to_owned(),
            },
            kind: CoreRoomActivityUpdateKind::MessageSent {
                event_id: "$event:example.org".to_owned(),
            },
        }
    );
    assert_eq!(
        serde_json::from_value::<CoreRoomActivityUpdate>(failed_json.clone())
            .expect("deserialize failed update as core"),
        CoreRoomActivityUpdate {
            room_id: "!ops:example.org".to_owned(),
            target: CoreRoomActivityUpdateTarget::Transaction {
                transaction_id: "txn-1".to_owned(),
            },
            kind: CoreRoomActivityUpdateKind::MessageFailed {
                message: "send failed".to_owned(),
                recoverable: true,
            },
        }
    );
    assert_eq!(
        serde_json::from_value::<CoreRoomActivityUpdate>(replaced_json.clone())
            .expect("deserialize replaced update as core"),
        CoreRoomActivityUpdate {
            room_id: "!ops:example.org".to_owned(),
            target: CoreRoomActivityUpdateTarget::Event {
                event_id: "$original:example.org".to_owned(),
            },
            kind: CoreRoomActivityUpdateKind::MessageReplaced {
                replacement_event_id: "$edit:example.org".to_owned(),
                sender: "@alice:example.org".to_owned(),
                timestamp_ms: 1_700_000_001_000,
                presentation: mmatrix_core::RoomMessagePresentationKind::Text,
                body: "edited body".to_owned(),
            },
        }
    );

    assert_eq!(
        serde_json::from_value::<RoomActivityUpdate>(sent_json).expect("deserialize sent update"),
        sent
    );
    assert_eq!(
        serde_json::from_value::<RoomActivityUpdate>(failed_json)
            .expect("deserialize failed update"),
        failed
    );
    assert_eq!(
        serde_json::from_value::<RoomActivityUpdate>(replaced_json)
            .expect("deserialize replaced update"),
        replaced
    );
    assert_eq!(
        serde_json::from_value::<RoomActivityUpdate>(redacted_json)
            .expect("deserialize redacted update"),
        redacted
    );
}

#[test]
fn room_activity_redaction_contract_shapes_reject_invalid_payloads() {
    assert!(
        serde_json::from_str::<RoomActivityUpdateKind>(r#"{"type":"activity_redacted"}"#).is_err(),
        "activity redacted kind must require redaction_event_id"
    );
    assert!(
        serde_json::from_str::<RoomActivityUpdateKind>(
            r#"{"type":"activity_redacted","redaction_event_id":"$redaction:example.org","unexpected":"value"}"#
        )
        .is_err(),
        "activity redacted kind must reject unknown fields"
    );
    assert!(
        serde_json::from_str::<RoomActivityKind>(r#"{"type":"redacted"}"#).is_err(),
        "redacted activity must require event_id"
    );
    assert!(
        serde_json::from_str::<RoomActivityKind>(
            r#"{"type":"redacted","event_id":"$redacted:example.org","unexpected":"value"}"#
        )
        .is_err(),
        "redacted activity must reject unknown fields"
    );
}

#[test]
fn room_activity_update_dtos_reject_unknown_fields_and_invalid_tags() {
    assert!(
        serde_json::from_str::<RoomActivityUpdate>(
            r#"{"room_id":"!ops:example.org","target":{"type":"transaction","transaction_id":"txn-1"},"kind":{"type":"message_sent","event_id":"$event:example.org"},"unexpected":"value"}"#
        )
        .is_err(),
        "room activity update must reject unknown fields"
    );
    assert!(
        serde_json::from_str::<RoomActivityUpdateTarget>(
            r#"{"type":"event","event_id":"$original:example.org","unexpected":"value"}"#
        )
        .is_err(),
        "event target must reject unknown fields"
    );
    assert!(
        serde_json::from_str::<RoomActivityUpdateTarget>(
            r#"{"type":"transaction","transaction_id":"txn-1","unexpected":"value"}"#
        )
        .is_err(),
        "transaction target must reject unknown fields"
    );
    assert!(
        serde_json::from_str::<RoomActivityUpdateKind>(
            r#"{"type":"message_replaced","replacement_event_id":"$edit:example.org","sender":"@alice:example.org","timestamp_ms":1700000001000,"presentation":"text","body":"edited body","unexpected":"value"}"#
        )
        .is_err(),
        "message replaced kind must reject unknown fields"
    );
    assert!(
        serde_json::from_str::<RoomActivityUpdateTarget>(
            r#"{"type":"event","transaction_id":"txn-1"}"#
        )
        .is_err(),
        "event target must reject transaction payloads"
    );
    assert!(
        serde_json::from_str::<RoomActivityUpdateKind>(
            r#"{"type":"message_replaced","replacement_event_id":"$edit:example.org","sender":"@alice:example.org","timestamp_ms":1700000001000,"presentation":"text"}"#
        )
        .is_err(),
        "message replaced kind must require body"
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
        }"#,
    )
    .unwrap_err();

    assert!(error.to_string().contains("unknown field `unexpected`"));
}

#[test]
fn fetch_room_image_media_request_round_trips_through_json() {
    let request = FetchRoomImageMediaRequest {
        room_id: "!room:example".to_owned(),
        activity_id: "event:$image:example".to_owned(),
        event_id: "$image:example".to_owned(),
        source: RoomMediaSource::Plain {
            url: "mxc://example.org/image".to_owned(),
        },
        format: RoomImageMediaFormat::Thumbnail {
            width: 160,
            height: 120,
        },
    };
    let json = serde_json::json!({
        "room_id": "!room:example",
        "activity_id": "event:$image:example",
        "event_id": "$image:example",
        "source": {
            "type": "plain",
            "url": "mxc://example.org/image"
        },
        "format": {
            "type": "thumbnail",
            "width": 160,
            "height": 120
        }
    });

    assert_eq!(
        serde_json::to_value(&request).expect("serialize image media request"),
        json
    );
    assert_eq!(
        serde_json::from_value::<FetchRoomImageMediaRequest>(json.clone())
            .expect("deserialize image media request"),
        request
    );
    assert_eq!(
        serde_json::from_value::<CoreFetchRoomImageMediaRequest>(json)
            .expect("deserialize core image media request"),
        CoreFetchRoomImageMediaRequest {
            room_id: "!room:example".to_owned(),
            activity_id: "event:$image:example".to_owned(),
            event_id: "$image:example".to_owned(),
            source: CoreRoomMediaSource::Plain {
                url: "mxc://example.org/image".to_owned(),
            },
            format: CoreRoomImageMediaFormat::Thumbnail {
                width: 160,
                height: 120,
            },
        }
    );
}

#[test]
fn encrypted_room_media_source_round_trips_through_json() {
    let source = sample_encrypted_room_media_source("mxc://example.org/encrypted-image");
    let json = serde_json::json!({
        "type": "encrypted",
        "url": "mxc://example.org/encrypted-image",
        "key": {
            "kty": "oct",
            "key_ops": ["encrypt", "decrypt"],
            "alg": "A256CTR",
            "k": "base64url-key",
            "ext": true
        },
        "iv": "base64-iv",
        "hashes": {
            "sha256": "base64-sha256"
        },
        "v": "v2"
    });

    assert_eq!(
        serde_json::to_value(&source).expect("serialize encrypted room media source"),
        json
    );
    assert_eq!(
        serde_json::from_value::<RoomMediaSource>(json)
            .expect("deserialize encrypted room media source"),
        source
    );
}

#[test]
fn encrypted_fetch_room_image_media_request_round_trips_through_json() {
    let request = FetchRoomImageMediaRequest {
        room_id: "!room:example".to_owned(),
        activity_id: "event:$image:example".to_owned(),
        event_id: "$image:example".to_owned(),
        source: sample_encrypted_room_media_source("mxc://example.org/encrypted-image"),
        format: RoomImageMediaFormat::Thumbnail {
            width: 160,
            height: 120,
        },
    };
    let json = serde_json::json!({
        "room_id": "!room:example",
        "activity_id": "event:$image:example",
        "event_id": "$image:example",
        "source": {
            "type": "encrypted",
            "url": "mxc://example.org/encrypted-image",
            "key": {
                "kty": "oct",
                "key_ops": ["encrypt", "decrypt"],
                "alg": "A256CTR",
                "k": "base64url-key",
                "ext": true
            },
            "iv": "base64-iv",
            "hashes": {
                "sha256": "base64-sha256"
            },
            "v": "v2"
        },
        "format": {
            "type": "thumbnail",
            "width": 160,
            "height": 120
        }
    });

    assert_eq!(
        serde_json::to_value(&request).expect("serialize encrypted image media request"),
        json
    );
    assert_eq!(
        serde_json::from_value::<FetchRoomImageMediaRequest>(json.clone())
            .expect("deserialize encrypted image media request"),
        request
    );
    assert_eq!(
        serde_json::from_value::<CoreFetchRoomImageMediaRequest>(json)
            .expect("deserialize core encrypted image media request"),
        CoreFetchRoomImageMediaRequest {
            room_id: "!room:example".to_owned(),
            activity_id: "event:$image:example".to_owned(),
            event_id: "$image:example".to_owned(),
            source: CoreRoomMediaSource::Encrypted {
                url: "mxc://example.org/encrypted-image".to_owned(),
                key: mmatrix_core::RoomEncryptedFileKey {
                    kty: "oct".to_owned(),
                    key_ops: vec!["encrypt".to_owned(), "decrypt".to_owned()],
                    alg: "A256CTR".to_owned(),
                    k: "base64url-key".to_owned(),
                    ext: true,
                },
                iv: "base64-iv".to_owned(),
                hashes: mmatrix_core::RoomEncryptedFileHashes {
                    sha256: "base64-sha256".to_owned(),
                },
                v: "v2".to_owned(),
            },
            format: CoreRoomImageMediaFormat::Thumbnail {
                width: 160,
                height: 120,
            },
        }
    );
}

#[test]
fn room_image_media_round_trips_through_json() {
    let loaded = RoomImageMedia {
        room_id: "!room:example".to_owned(),
        activity_id: "event:$image:example".to_owned(),
        event_id: "$image:example".to_owned(),
        kind: RoomImageMediaKind::Loaded {
            format: RoomImageMediaFormat::Thumbnail {
                width: 160,
                height: 120,
            },
            bytes: vec![1, 2, 3],
        },
    };
    let loaded_json = serde_json::json!({
        "room_id": "!room:example",
        "activity_id": "event:$image:example",
        "event_id": "$image:example",
        "kind": {
            "type": "loaded",
            "format": {
                "type": "thumbnail",
                "width": 160,
                "height": 120
            },
            "bytes": [1, 2, 3]
        }
    });
    let failed = RoomImageMedia {
        room_id: "!room:example".to_owned(),
        activity_id: "event:$image:example".to_owned(),
        event_id: "$image:example".to_owned(),
        kind: RoomImageMediaKind::Failed {
            format: RoomImageMediaFormat::File,
            message: "failed to fetch room image media: unavailable".to_owned(),
        },
    };
    let failed_json = serde_json::json!({
        "room_id": "!room:example",
        "activity_id": "event:$image:example",
        "event_id": "$image:example",
        "kind": {
            "type": "failed",
            "format": {
                "type": "file"
            },
            "message": "failed to fetch room image media: unavailable"
        }
    });

    assert_eq!(
        serde_json::to_value(&loaded).expect("serialize loaded room image media"),
        loaded_json
    );
    assert_eq!(
        serde_json::from_value::<RoomImageMedia>(loaded_json.clone())
            .expect("deserialize loaded room image media"),
        loaded
    );
    assert_eq!(
        serde_json::from_value::<CoreRoomImageMedia>(loaded_json)
            .expect("deserialize core loaded room image media"),
        CoreRoomImageMedia {
            room_id: "!room:example".to_owned(),
            activity_id: "event:$image:example".to_owned(),
            event_id: "$image:example".to_owned(),
            kind: CoreRoomImageMediaKind::Loaded {
                format: CoreRoomImageMediaFormat::Thumbnail {
                    width: 160,
                    height: 120,
                },
                bytes: vec![1, 2, 3],
            },
        }
    );

    assert_eq!(
        serde_json::to_value(&failed).expect("serialize failed room image media"),
        failed_json
    );
    assert_eq!(
        serde_json::from_value::<RoomImageMedia>(failed_json.clone())
            .expect("deserialize failed room image media"),
        failed
    );
    assert_eq!(
        serde_json::from_value::<CoreRoomImageMedia>(failed_json)
            .expect("deserialize core failed room image media"),
        CoreRoomImageMedia {
            room_id: "!room:example".to_owned(),
            activity_id: "event:$image:example".to_owned(),
            event_id: "$image:example".to_owned(),
            kind: CoreRoomImageMediaKind::Failed {
                format: CoreRoomImageMediaFormat::File,
                message: "failed to fetch room image media: unavailable".to_owned(),
            },
        }
    );
}

#[test]
fn room_image_media_dtos_reject_unknown_fields() {
    assert!(
        serde_json::from_str::<FetchRoomImageMediaRequest>(
            r#"{
            "room_id":"!room:example",
            "activity_id":"event:$image:example",
            "event_id":"$image:example",
            "source":{"type":"plain","url":"mxc://example.org/image"},
            "format":{"type":"thumbnail","width":160,"height":120},
            "unexpected":true
        }"#
        )
        .is_err()
    );

    assert!(
        serde_json::from_str::<RoomImageMedia>(
            r#"{
            "room_id":"!room:example",
            "activity_id":"event:$image:example",
            "event_id":"$image:example",
            "kind":{"type":"loaded","format":{"type":"file"},"bytes":[1,2,3]},
            "unexpected":true
        }"#
        )
        .is_err()
    );

    assert!(
        serde_json::from_str::<RoomImageMediaFormat>(
            r#"{"type":"thumbnail","width":160,"height":120,"unexpected":true}"#
        )
        .is_err()
    );

    assert!(
        serde_json::from_str::<RoomImageMediaKind>(
            r#"{"type":"failed","format":{"type":"file"},"message":"unavailable","unexpected":true}"#
        )
        .is_err()
    );
}

#[test]
fn encrypted_room_media_source_rejects_malformed_nested_metadata() {
    assert!(
        serde_json::from_value::<RoomMediaSource>(serde_json::json!({
            "type": "encrypted",
            "url": "mxc://example.org/encrypted-image",
            "iv": "base64-iv",
            "hashes": {"sha256": "base64-sha256"},
            "v": "v2"
        }))
        .is_err()
    );

    assert!(
        serde_json::from_value::<RoomMediaSource>(serde_json::json!({
            "type": "encrypted",
            "url": "mxc://example.org/encrypted-image",
            "key": {
                "kty": "oct",
                "key_ops": ["encrypt", "decrypt"],
                "alg": "A256CTR",
                "k": "base64url-key",
                "ext": true
            },
            "iv": "base64-iv",
            "hashes": {},
            "v": "v2"
        }))
        .is_err()
    );

    assert!(
        serde_json::from_value::<RoomMediaSource>(serde_json::json!({
            "type": "encrypted",
            "url": "mxc://example.org/encrypted-image",
            "key": {
                "kty": "oct",
                "key_ops": ["encrypt", "decrypt"],
                "alg": "A256CTR",
                "k": "base64url-key",
                "ext": true,
                "unexpected": true
            },
            "iv": "base64-iv",
            "hashes": {"sha256": "base64-sha256"},
            "v": "v2"
        }))
        .is_err()
    );

    assert!(
        serde_json::from_value::<RoomMediaSource>(serde_json::json!({
            "type": "encrypted",
            "url": "mxc://example.org/encrypted-image",
            "key": {
                "kty": "oct",
                "key_ops": ["encrypt", "decrypt"],
                "alg": "A256CTR",
                "k": "base64url-key",
                "ext": true
            },
            "iv": "base64-iv",
            "hashes": {
                "sha256": "base64-sha256",
                "unexpected": true
            },
            "v": "v2"
        }))
        .is_err()
    );
}

#[test]
fn room_activity_contains_matches_identity_and_send_state_fields() {
    let activity = RoomActivity {
        room_id: "!room:example".to_owned(),
        activity_id: "activity-1".to_owned(),
        timestamp_ms: 1_700_000_000_000,
        relation: None,
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
fn room_activity_contains_matches_unsupported_message_fields() {
    let activity = RoomActivity {
        room_id: "!room:example".to_owned(),
        activity_id: "event:$audio1:example.org".to_owned(),
        timestamp_ms: 1_710_000_000_000,
        relation: None,
        kind: RoomActivityKind::UnsupportedMessage(UnsupportedRoomMessageActivity {
            event_id: "$audio1:example.org".to_owned(),
            sender: "@alice:example.org".to_owned(),
            msgtype: "m.audio".to_owned(),
            body: "voice-note.ogg".to_owned(),
        }),
    };

    assert!(activity.contains("\"type\":\"unsupported_message\""));
    assert!(activity.contains("\"msgtype\":\"m.audio\""));
    assert!(activity.contains("voice-note.ogg"));
}

#[test]
fn room_activity_contains_matches_image_fields() {
    let activity = RoomActivity {
        room_id: "!room:example".to_owned(),
        activity_id: "event:$image1:example.org".to_owned(),
        timestamp_ms: 1_710_000_000_000,
        relation: None,
        kind: RoomActivityKind::Image(Box::new(sample_room_image_activity(None))),
    };

    assert!(activity.contains("\"type\":\"image\""));
    assert!(activity.contains("diagram.png"));
    assert!(activity.contains("mxc://example.org/diagram"));
    assert!(activity.contains("image/png"));
    assert!(activity.contains("640"));
    assert!(activity.contains("480"));
    assert!(activity.contains("12345"));
}

#[test]
fn room_image_activity_rejects_unknown_fields() {
    let error = serde_json::from_value::<RoomImageActivity>(serde_json::json!({
        "event_id": "$image1:example.org",
        "sender": "@alice:example.org",
        "body": "diagram.png",
        "filename": "diagram.png",
        "source": {
            "type": "plain",
            "url": "mxc://example.org/diagram"
        },
        "info": {
            "mimetype": "image/png",
            "size": 12345u64,
            "width": 640u64,
            "height": 480u64,
            "is_animated": false
        },
        "media_state": {
            "type": "pending"
        },
        "unexpected": true
    }))
    .unwrap_err();

    assert!(error.to_string().contains("unknown field `unexpected`"));
}

#[test]
fn room_image_media_state_rejects_unknown_fields() {
    let error = serde_json::from_value::<RoomImageMediaState>(serde_json::json!({
        "type": "loaded",
        "width": 320u64,
        "height": 240u64,
        "unexpected": true
    }))
    .unwrap_err();

    assert!(error.to_string().contains("unknown field `unexpected`"));
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
        e2ee_status: None,
        last_verification: None,
        submitted_membership_commands: Vec::new(),
        submitted_direct_room_commands: Vec::new(),
        submitted_room_topic_commands: Vec::new(),
        submitted_verification_actions: Vec::new(),
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
            "e2ee_status": null,
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
fn model_submitted_membership_commands_round_trip_through_json() {
    let model = Model {
        status: "synced".to_owned(),
        lifecycle_state: "ready".to_owned(),
        rooms: vec![],
        last_activity: None,
        e2ee_status: None,
        last_verification: None,
        submitted_membership_commands: vec![
            SubmittedMembershipCommand {
                action: MembershipCommandAction::Join,
                target: "#ops:example.org".to_owned(),
            },
            SubmittedMembershipCommand {
                action: MembershipCommandAction::Leave,
                target: "!ops:example.org".to_owned(),
            },
        ],
        submitted_direct_room_commands: Vec::new(),
        submitted_room_topic_commands: Vec::new(),
        submitted_verification_actions: Vec::new(),
        last_error: None,
        selected_room: None,
    };

    assert_eq!(
        serde_json::to_value(&model).expect("serialize model with membership commands"),
        serde_json::json!({
            "status": "synced",
            "lifecycle_state": "ready",
            "rooms": [],
            "last_activity": null,
            "e2ee_status": null,
            "last_verification": null,
            "submitted_membership_commands": [
                {
                    "action": "join",
                    "target": "#ops:example.org"
                },
                {
                    "action": "leave",
                    "target": "!ops:example.org"
                }
            ],
            "last_error": null,
            "selected_room": null
        })
    );
    assert_eq!(
        serde_json::from_value::<Model>(serde_json::json!({
            "status": "synced",
            "lifecycle_state": "ready",
            "rooms": [],
            "last_activity": null,
            "e2ee_status": null,
            "last_verification": null,
            "submitted_membership_commands": [
                {
                    "action": "join",
                    "target": "#ops:example.org"
                },
                {
                    "action": "leave",
                    "target": "!ops:example.org"
                }
            ],
            "last_error": null,
            "selected_room": null
        }))
        .expect("deserialize model with membership commands"),
        model
    );
}

#[test]
fn model_submitted_direct_room_commands_round_trip_through_json() {
    let model = Model {
        status: "synced".to_owned(),
        lifecycle_state: "ready".to_owned(),
        rooms: vec![],
        last_activity: None,
        e2ee_status: None,
        last_verification: None,
        submitted_membership_commands: vec![SubmittedMembershipCommand {
            action: MembershipCommandAction::Join,
            target: "#ops:example.org".to_owned(),
        }],
        submitted_direct_room_commands: vec![SubmittedDirectRoomCommand {
            user_id: "@alice:example.org".to_owned(),
        }],
        submitted_room_topic_commands: Vec::new(),
        submitted_verification_actions: Vec::new(),
        last_error: None,
        selected_room: None,
    };

    assert_eq!(
        serde_json::to_value(&model).expect("serialize model with direct room commands"),
        serde_json::json!({
            "status": "synced",
            "lifecycle_state": "ready",
            "rooms": [],
            "last_activity": null,
            "e2ee_status": null,
            "last_verification": null,
            "submitted_membership_commands": [
                {
                    "action": "join",
                    "target": "#ops:example.org"
                }
            ],
            "submitted_direct_room_commands": [
                {
                    "user_id": "@alice:example.org"
                }
            ],
            "last_error": null,
            "selected_room": null
        })
    );
    assert_eq!(
        serde_json::from_value::<Model>(serde_json::json!({
            "status": "synced",
            "lifecycle_state": "ready",
            "rooms": [],
            "last_activity": null,
            "e2ee_status": null,
            "last_verification": null,
            "submitted_membership_commands": [
                {
                    "action": "join",
                    "target": "#ops:example.org"
                }
            ],
            "submitted_direct_room_commands": [
                {
                    "user_id": "@alice:example.org"
                }
            ],
            "last_error": null,
            "selected_room": null
        }))
        .expect("deserialize model with direct room commands"),
        model
    );
}

#[test]
fn model_submitted_room_topic_commands_round_trip_through_json() {
    let model = Model {
        status: "synced".to_owned(),
        lifecycle_state: "ready".to_owned(),
        rooms: vec![],
        last_activity: None,
        e2ee_status: None,
        last_verification: None,
        submitted_membership_commands: vec![SubmittedMembershipCommand {
            action: MembershipCommandAction::Join,
            target: "#ops:example.org".to_owned(),
        }],
        submitted_direct_room_commands: vec![SubmittedDirectRoomCommand {
            user_id: "@alice:example.org".to_owned(),
        }],
        submitted_room_topic_commands: vec![SubmittedRoomTopicCommand {
            room_id: "!ops:example.org".to_owned(),
            topic: "incident review".to_owned(),
        }],
        submitted_verification_actions: Vec::new(),
        last_error: None,
        selected_room: None,
    };

    assert_eq!(
        serde_json::to_value(&model).expect("serialize model with room topic commands"),
        serde_json::json!({
            "status": "synced",
            "lifecycle_state": "ready",
            "rooms": [],
            "last_activity": null,
            "e2ee_status": null,
            "last_verification": null,
            "submitted_membership_commands": [
                {
                    "action": "join",
                    "target": "#ops:example.org"
                }
            ],
            "submitted_direct_room_commands": [
                {
                    "user_id": "@alice:example.org"
                }
            ],
            "submitted_room_topic_commands": [
                {
                    "room_id": "!ops:example.org",
                    "topic": "incident review"
                }
            ],
            "last_error": null,
            "selected_room": null
        })
    );
    assert_eq!(
        serde_json::from_value::<Model>(serde_json::json!({
            "status": "synced",
            "lifecycle_state": "ready",
            "rooms": [],
            "last_activity": null,
            "e2ee_status": null,
            "last_verification": null,
            "submitted_membership_commands": [
                {
                    "action": "join",
                    "target": "#ops:example.org"
                }
            ],
            "submitted_direct_room_commands": [
                {
                    "user_id": "@alice:example.org"
                }
            ],
            "submitted_room_topic_commands": [
                {
                    "room_id": "!ops:example.org",
                    "topic": "incident review"
                }
            ],
            "last_error": null,
            "selected_room": null
        }))
        .expect("deserialize model with room topic commands"),
        model
    );
}

#[test]
fn model_submitted_membership_commands_reject_unknown_nested_fields() {
    let error = serde_json::from_value::<Model>(serde_json::json!({
        "status": "synced",
        "lifecycle_state": "ready",
        "rooms": [],
        "last_activity": null,
        "e2ee_status": null,
        "last_verification": null,
        "submitted_membership_commands": [
            {
                "action": "join",
                "target": "#ops:example.org",
                "unexpected": true
            }
        ],
        "last_error": null,
        "selected_room": null
    }))
    .unwrap_err();

    assert!(error.to_string().contains("unknown field `unexpected`"));
}

#[test]
fn model_submitted_direct_room_commands_reject_unknown_nested_fields() {
    let error = serde_json::from_value::<Model>(serde_json::json!({
        "status": "synced",
        "lifecycle_state": "ready",
        "rooms": [],
        "last_activity": null,
        "e2ee_status": null,
        "last_verification": null,
        "submitted_membership_commands": [],
        "submitted_direct_room_commands": [
            {
                "user_id": "@alice:example.org",
                "unexpected": true
            }
        ],
        "last_error": null,
        "selected_room": null
    }))
    .unwrap_err();

    assert!(error.to_string().contains("unknown field `unexpected`"));
}

#[test]
fn model_submitted_room_topic_commands_reject_unknown_nested_fields() {
    let error = serde_json::from_value::<Model>(serde_json::json!({
        "status": "synced",
        "lifecycle_state": "ready",
        "rooms": [],
        "last_activity": null,
        "e2ee_status": null,
        "last_verification": null,
        "submitted_membership_commands": [],
        "submitted_direct_room_commands": [],
        "submitted_room_topic_commands": [
            {
                "room_id": "!ops:example.org",
                "topic": "incident review",
                "unexpected": true
            }
        ],
        "last_error": null,
        "selected_room": null
    }))
    .unwrap_err();

    assert!(error.to_string().contains("unknown field `unexpected`"));
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
            "e2ee_status":null,
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
            "protocol_version":15,
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
            "is_direct":false,
            "direct_user_ids":[],
            "tags":[],
            "space_children":[],
            "unexpected":true
        }"#
        )
        .is_err()
    );
    assert!(
        serde_json::from_str::<RoomTag>(
            r#"{
            "tag":"m.favourite",
            "order":"0.25",
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
        serde_json::from_str::<RoomImageActivity>(
            r#"{
            "event_id":"$image1:example.org",
            "sender":"@alice:example.org",
            "body":"diagram.png",
            "filename":"diagram.png",
            "source":{"type":"plain","url":"mxc://example.org/diagram"},
            "info":{"mimetype":"image/png","size":12345,"width":640,"height":480,"is_animated":false},
            "unexpected":true
        }"#
        )
        .is_err()
    );
    assert!(
        serde_json::from_str::<RoomMediaSource>(
            r#"{
            "type":"plain",
            "url":"mxc://example.org/diagram",
            "unexpected":true
        }"#
        )
        .is_err()
    );
    assert!(
        serde_json::from_str::<RoomImageInfo>(
            r#"{
            "mimetype":"image/png",
            "size":12345,
            "width":640,
            "height":480,
            "is_animated":false,
            "unexpected":true
        }"#
        )
        .is_err()
    );
    assert!(
        serde_json::from_str::<UnsupportedRoomMessageActivity>(
            r#"{
            "event_id":"$audio1:example.org",
            "sender":"@alice:example.org",
            "msgtype":"m.audio",
            "body":"voice-note.ogg",
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
            "type":"image",
            "event_id":"$image1:example.org",
            "sender":"@alice:example.org",
            "body":"diagram.png",
            "filename":"diagram.png",
            "source":{"type":"plain","url":"mxc://example.org/diagram"},
            "info":{"mimetype":"image/png","size":12345,"width":640,"height":480,"is_animated":false},
            "unexpected":true
        }"#
        )
        .is_err()
    );
    assert!(
        serde_json::from_str::<RoomActivityKind>(
            r#"{
            "type":"unsupported_message",
            "event_id":"$audio1:example.org",
            "sender":"@alice:example.org",
            "msgtype":"m.audio",
            "body":"voice-note.ogg",
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
            "rooms":[],
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
            "switchbar":{"items":[]}
        }"#
        )
        .is_err()
    );
    assert!(
        serde_json::from_str::<UiRoomState>(
            r#"{
            "room_id":"!room:example",
            "display_name":"Room",
            "encrypted":true,
            "is_space":false,
            "is_direct":false,
            "direct_user_ids":[],
            "tags":[],
            "space_children":[],
            "has_activity":false,
            "read_target_event_id":null,
            "read_state":null,
            "unexpected":true
        }"#
        )
        .is_err()
    );
    assert!(
        serde_json::from_str::<UiRoomReadState>(
            r#"{
            "latest_active_event_id":null,
            "fully_read_event_id":null,
            "marked_unread":false,
            "num_unread":0,
            "num_notifications":0,
            "num_mentions":0,
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
            "members":[],
            "unexpected":true
        }"#
        )
        .is_err()
    );
    assert!(
        serde_json::from_str::<RoomMemberView>(
            r#"{
            "user_id":"@alice:example",
            "display_name":"Alice",
            "label":"Alice",
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
fn view_identity_thread_round_trips_through_json() {
    let identity = ViewIdentity::Thread {
        room_id: "!room:example".to_owned(),
        thread_root_event_id: "$thread-root:example".to_owned(),
    };
    let json = serde_json::json!({
        "kind": "thread",
        "room_id": "!room:example",
        "thread_root_event_id": "$thread-root:example",
    });

    assert_eq!(
        serde_json::to_value(&identity).expect("serialize thread identity"),
        json
    );
    assert_eq!(
        serde_json::from_value::<ViewIdentity>(json).expect("deserialize thread identity"),
        identity
    );
}

#[test]
fn view_identity_thread_rejects_missing_or_unknown_fields() {
    for json in [
        serde_json::json!({
            "kind": "thread",
            "thread_root_event_id": "$thread-root:example",
        }),
        serde_json::json!({
            "kind": "thread",
            "room_id": "!room:example",
        }),
        serde_json::json!({
            "kind": "thread",
            "room_id": "!room:example",
            "thread_root_event_id": "$thread-root:example",
            "unexpected": true,
        }),
    ] {
        assert!(serde_json::from_value::<ViewIdentity>(json).is_err());
    }
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
        "e2ee_status":null,
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
            "e2ee_status":null,
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
fn model_e2ee_status_round_trips_typed_state_json() {
    let model = serde_json::from_value::<Model>(serde_json::json!({
        "status":"e2ee status updated",
        "lifecycle_state":"ready",
        "rooms":[],
        "last_activity":null,
        "e2ee_status":{
            "recovery":"incomplete",
            "backup":{
                "state":"enabled",
                "enabled_locally":true,
                "exists_on_server":true
            },
            "cross_signing":{
                "has_master":true,
                "has_self_signing":true,
                "has_user_signing":false,
                "complete":false
            }
        },
        "last_verification":null,
        "last_error":null,
        "selected_room":null
    }))
    .expect("deserialize typed e2ee model");

    assert_eq!(model.e2ee_status, Some(sample_e2ee_status()));

    assert_eq!(
        serde_json::to_value(&model).expect("serialize typed e2ee model"),
        serde_json::json!({
            "status":"e2ee status updated",
            "lifecycle_state":"ready",
            "rooms":[],
            "last_activity":null,
            "e2ee_status":{
                "recovery":"incomplete",
                "backup":{
                    "state":"enabled",
                    "enabled_locally":true,
                    "exists_on_server":true
                },
                "cross_signing":{
                    "has_master":true,
                    "has_self_signing":true,
                    "has_user_signing":false,
                    "complete":false
                }
            },
            "last_verification":null,
            "last_error":null,
            "selected_room":null
        })
    );
}

#[test]
fn model_e2ee_status_rejects_unknown_nested_fields() {
    for json in [
        serde_json::json!({
            "status":"e2ee status updated",
            "lifecycle_state":"ready",
            "rooms":[],
            "last_activity":null,
            "e2ee_status":{
                "recovery":"incomplete",
                "backup":{"state":"enabled","enabled_locally":true,"exists_on_server":true},
                "cross_signing":{"has_master":true,"has_self_signing":true,"has_user_signing":false,"complete":false},
                "unexpected":true
            },
            "last_verification":null,
            "last_error":null,
            "selected_room":null
        }),
        serde_json::json!({
            "status":"e2ee status updated",
            "lifecycle_state":"ready",
            "rooms":[],
            "last_activity":null,
            "e2ee_status":{
                "recovery":"incomplete",
                "backup":{"state":"enabled","enabled_locally":true,"exists_on_server":true,"unexpected":true},
                "cross_signing":{"has_master":true,"has_self_signing":true,"has_user_signing":false,"complete":false}
            },
            "last_verification":null,
            "last_error":null,
            "selected_room":null
        }),
        serde_json::json!({
            "status":"e2ee status updated",
            "lifecycle_state":"ready",
            "rooms":[],
            "last_activity":null,
            "e2ee_status":{
                "recovery":"incomplete",
                "backup":{"state":"enabled","enabled_locally":true,"exists_on_server":true},
                "cross_signing":{"has_master":true,"has_self_signing":true,"has_user_signing":false,"complete":false,"unexpected":true}
            },
            "last_verification":null,
            "last_error":null,
            "selected_room":null
        }),
    ] {
        assert!(serde_json::from_value::<Model>(json).is_err());
    }
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
        rooms: vec![],
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

#[test]
fn recover_e2ee_args_round_trip_and_redact_debug() {
    let args = RecoverE2eeArgs {
        recovery_secret: "sample recovery secret".to_owned(),
    };
    assert_eq!(
        serde_json::to_value(&args).expect("serialize recover args"),
        serde_json::json!({
            "recovery_secret":"sample recovery secret"
        })
    );
    assert_eq!(
        serde_json::from_value::<RecoverE2eeArgs>(serde_json::json!({
            "recovery_secret":"sample recovery secret"
        }))
        .expect("deserialize recover args"),
        args
    );

    let debug = format!("{args:?}");
    assert!(!debug.contains("sample recovery secret"));
    assert!(debug.contains("<redacted>"));
}

#[test]
fn recover_e2ee_args_reject_unknown_fields() {
    assert!(
        serde_json::from_str::<RecoverE2eeArgs>(
            r#"{
            "recovery_secret":"sample recovery secret",
            "unexpected":true
        }"#
        )
        .is_err()
    );
}

#[test]
fn verification_action_args_round_trip_with_omitted_and_explicit_flow() {
    let omitted = VerificationActionArgs::default();
    assert_eq!(
        serde_json::to_value(&omitted).expect("serialize omitted verification args"),
        serde_json::json!({})
    );
    assert_eq!(
        serde_json::from_value::<VerificationActionArgs>(serde_json::json!({}))
            .expect("deserialize omitted verification args"),
        omitted
    );

    let explicit = VerificationActionArgs {
        flow_id: Some("flow-verify-123".to_owned()),
        user_id: Some("@alice:example".to_owned()),
    };
    assert_eq!(
        serde_json::to_value(&explicit).expect("serialize explicit verification args"),
        serde_json::json!({
            "flow_id":"flow-verify-123",
            "user_id":"@alice:example"
        })
    );
    assert_eq!(
        serde_json::from_value::<VerificationActionArgs>(serde_json::json!({
            "flow_id":"flow-verify-123",
            "user_id":"@alice:example"
        }))
        .expect("deserialize explicit verification args"),
        explicit
    );
}

#[test]
fn verification_action_args_reject_unknown_fields() {
    assert!(
        serde_json::from_str::<VerificationActionArgs>(
            r#"{
            "flow_id":"flow-verify-123",
            "user_id":"@alice:example",
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
            local_app_data: Some(PathBuf::from("/var/tmp/mmatrix-testkit")),
            frame_size: None,
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
    unsupported(app.recover_e2ee("sample recovery secret"));
    unsupported(app.accept_verification());
    unsupported(app.accept_verification_flow("flow-1", "@alice:example"));
    unsupported(app.cancel_verification());
    unsupported(app.cancel_verification_flow("flow-1", "@alice:example"));
    unsupported(app.confirm_verification_sas());
    unsupported(app.confirm_verification_sas_flow("flow-1", "@alice:example"));
    unsupported(app.reject_verification_sas());
    unsupported(app.reject_verification_sas_flow("flow-1", "@alice:example"));
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
