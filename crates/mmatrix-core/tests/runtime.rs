use std::time::Duration;

use mmatrix_core::{
    Command, CoreConfig, Event, LoginRequest, MarkRoomReadRequest, Operation, RoomActivity,
    RoomActivityKind, RoomActivityUpdate, RoomActivityUpdateKind, RoomMessageActivity,
    RoomMessagePresentationKind, RoomMessageSendState, RoomReadState,
    RoomStateNoticeActivity, RoomStateNoticeContent, RoomSummary, RoomTyping,
    SendMessageRequest, SpaceChildSummary, StoredSession, VerificationDecimals,
    VerificationEmoji, VerificationFlowId, VerificationRequest, VerificationState,
    VerificationStateEvent, start,
};
use tempfile::TempDir;

fn test_config() -> (TempDir, CoreConfig) {
    let temp_dir = TempDir::new().expect("temporary profile directory");
    let config = CoreConfig {
        profile_dir: temp_dir.path().join("profile"),
        store_passphrase: "test-store-passphrase".to_owned(),
    };

    (temp_dir, config)
}

fn verification_flow_id() -> VerificationFlowId {
    VerificationFlowId {
        flow_id: "txn-123".to_owned(),
        user_id: "@alice:example.org".to_owned(),
    }
}

fn assert_invalid_verification_command(command: Command) {
    let (_temp_dir, config) = test_config();
    let handle = start(config).expect("start core");

    assert_eq!(
        handle
            .events()
            .recv_timeout(Duration::from_secs(2))
            .expect("started event"),
        Event::Started
    );

    handle
        .command_sender()
        .send(command)
        .expect("send verification command");

    match handle
        .events()
        .recv_timeout(Duration::from_secs(2))
        .expect("verification error")
    {
        Event::Error(err) => {
            assert_eq!(err.operation, Operation::Verification);
            assert_eq!(
                err.message,
                "verification requires an authenticated session"
            );
        }
        other => panic!("expected verification error, got {other:?}"),
    }

    handle
        .command_sender()
        .send(Command::Shutdown)
        .expect("send shutdown");
    assert_eq!(
        handle
            .events()
            .recv_timeout(Duration::from_secs(2))
            .expect("stopped event"),
        Event::Stopped
    );
    handle.join().expect("join worker");
}

#[test]
fn startup_and_shutdown_emit_lifecycle_events() {
    let (_temp_dir, config) = test_config();
    let handle = start(config).expect("start core");

    assert_eq!(
        handle
            .events()
            .recv_timeout(Duration::from_secs(2))
            .expect("started event"),
        Event::Started
    );

    handle
        .command_sender()
        .send(Command::Shutdown)
        .expect("send shutdown");

    assert_eq!(
        handle
            .events()
            .recv_timeout(Duration::from_secs(2))
            .expect("stopped event"),
        Event::Stopped
    );

    handle.join().expect("join worker");
}

#[test]
fn invalid_command_ordering_emits_typed_error_events() {
    let (_temp_dir, config) = test_config();
    let handle = start(config).expect("start core");

    assert_eq!(
        handle
            .events()
            .recv_timeout(Duration::from_secs(2))
            .expect("started event"),
        Event::Started
    );

    handle
        .command_sender()
        .send(Command::StartSync)
        .expect("send start sync");
    handle
        .command_sender()
        .send(Command::SendMessage(SendMessageRequest {
            room_id: "!room:example.org".to_owned(),
            presentation: RoomMessagePresentationKind::Text,
            body: "hello".to_owned(),
        }))
        .expect("send message");
    handle
        .command_sender()
        .send(Command::MarkRoomRead(MarkRoomReadRequest {
            room_id: "!room:example.org".to_owned(),
            event_id: "$event:example.org".to_owned(),
        }))
        .expect("send mark room read");
    handle
        .command_sender()
        .send(Command::Logout)
        .expect("send logout");

    let start_sync_error = handle
        .events()
        .recv_timeout(Duration::from_secs(2))
        .expect("start sync error");
    let send_message_error = handle
        .events()
        .recv_timeout(Duration::from_secs(2))
        .expect("send message error");
    let mark_room_read_error = handle
        .events()
        .recv_timeout(Duration::from_secs(2))
        .expect("mark room read error");
    let logout_error = handle
        .events()
        .recv_timeout(Duration::from_secs(2))
        .expect("logout error");

    match start_sync_error {
        Event::Error(err) => assert_eq!(err.operation, Operation::StartSync),
        other => panic!("expected start sync error, got {other:?}"),
    }
    match send_message_error {
        Event::Error(err) => assert_eq!(err.operation, Operation::SendMessage),
        other => panic!("expected send message error, got {other:?}"),
    }
    match mark_room_read_error {
        Event::Error(err) => {
            assert_eq!(err.operation, Operation::MarkRoomRead);
            assert_eq!(
                err.message,
                "mark room read requires an authenticated session"
            );
        }
        other => panic!("expected mark room read error, got {other:?}"),
    }
    match logout_error {
        Event::Error(err) => assert_eq!(err.operation, Operation::Logout),
        other => panic!("expected logout error, got {other:?}"),
    }

    handle
        .command_sender()
        .send(Command::Shutdown)
        .expect("send shutdown");
    assert_eq!(
        handle
            .events()
            .recv_timeout(Duration::from_secs(2))
            .expect("stopped event"),
        Event::Stopped
    );
    handle.join().expect("join worker");
}

#[test]
fn accept_verification_before_login_emits_verification_error() {
    assert_invalid_verification_command(Command::AcceptVerification(verification_flow_id()));
}

#[test]
fn cancel_verification_before_login_emits_verification_error() {
    assert_invalid_verification_command(Command::CancelVerification(verification_flow_id()));
}

#[test]
fn confirm_verification_sas_before_login_emits_verification_error() {
    assert_invalid_verification_command(Command::ConfirmVerificationSas(verification_flow_id()));
}

#[test]
fn reject_verification_sas_before_login_emits_verification_error() {
    assert_invalid_verification_command(Command::RejectVerificationSas(verification_flow_id()));
}

#[test]
fn login_store_initialization_failure_emits_typed_error_event() {
    let temp_dir = TempDir::new().expect("temporary profile directory");
    let profile_file = temp_dir.path().join("profile-file");
    std::fs::write(&profile_file, "not a directory").expect("write profile file");
    let config = CoreConfig {
        profile_dir: profile_file,
        store_passphrase: "test-store-passphrase".to_owned(),
    };
    let handle = start(config).expect("start core");

    assert_eq!(
        handle
            .events()
            .recv_timeout(Duration::from_secs(2))
            .expect("started event"),
        Event::Started
    );

    handle
        .command_sender()
        .send(Command::Login(LoginRequest {
            homeserver_url: "https://example.invalid".to_owned(),
            username: "user".to_owned(),
            password: "password".to_owned(),
            device_display_name: "mmatrix runtime test".to_owned(),
        }))
        .expect("send login");

    match handle
        .events()
        .recv_timeout(Duration::from_secs(2))
        .expect("login error")
    {
        Event::Error(err) => {
            assert_eq!(err.operation, Operation::Login);
            assert!(
                err.message.contains("failed to create store directory"),
                "unexpected login error: {}",
                err.message
            );
        }
        other => panic!("expected login error, got {other:?}"),
    }

    handle
        .command_sender()
        .send(Command::Shutdown)
        .expect("send shutdown");
    assert_eq!(
        handle
            .events()
            .recv_timeout(Duration::from_secs(2))
            .expect("stopped event"),
        Event::Stopped
    );
    handle.join().expect("join worker");
}

#[test]
fn stored_session_serializes_and_deserializes_with_existing_fields() {
    let session = StoredSession {
        user_id: "@user:example.org".to_owned(),
        device_id: "DEVICE".to_owned(),
        access_token: "access-token".to_owned(),
        refresh_token: Some("refresh-token".to_owned()),
    };

    let json = serde_json::to_string(&session).expect("serialize stored session");
    assert_eq!(
        json,
        r#"{"user_id":"@user:example.org","device_id":"DEVICE","access_token":"access-token","refresh_token":"refresh-token"}"#
    );

    let restored: StoredSession = serde_json::from_str(&json).expect("deserialize stored session");
    assert_eq!(restored, session);
}

#[test]
fn stored_session_serializes_and_deserializes_without_refresh_token() {
    let session = StoredSession {
        user_id: "@user:example.org".to_owned(),
        device_id: "DEVICE".to_owned(),
        access_token: "access-token".to_owned(),
        refresh_token: None,
    };

    let json = serde_json::to_string(&session).expect("serialize stored session");
    assert_eq!(
        json,
        r#"{"user_id":"@user:example.org","device_id":"DEVICE","access_token":"access-token","refresh_token":null}"#
    );

    let restored: StoredSession = serde_json::from_str(&json).expect("deserialize stored session");
    assert_eq!(restored, session);
}

#[test]
fn stored_session_rejects_unknown_fields() {
    assert!(
        serde_json::from_str::<StoredSession>(
            r#"{"user_id":"@user:example.org","device_id":"DEVICE","access_token":"access-token","refresh_token":null,"unexpected":"value"}"#
        )
        .is_err(),
        "stored session must reject unknown fields"
    );
}

#[test]
fn mark_room_read_request_round_trips_through_serde() {
    let request = MarkRoomReadRequest {
        room_id: "!room:example.org".to_owned(),
        event_id: "$event:example.org".to_owned(),
    };

    let json = serde_json::to_string(&request).expect("serialize mark room read request");
    assert_eq!(
        json,
        r#"{"room_id":"!room:example.org","event_id":"$event:example.org"}"#
    );

    let restored: MarkRoomReadRequest =
        serde_json::from_str(&json).expect("deserialize mark room read request");
    assert_eq!(restored, request);
}

#[test]
fn logged_in_event_serializes_and_deserializes_session_payload() {
    let session = StoredSession {
        user_id: "@user:example.org".to_owned(),
        device_id: "DEVICE".to_owned(),
        access_token: "access-token".to_owned(),
        refresh_token: Some("refresh-token".to_owned()),
    };
    let event = Event::LoggedIn {
        session: session.clone(),
    };

    let json = serde_json::to_string(&event).expect("serialize logged-in event");
    assert_eq!(
        json,
        r#"{"LoggedIn":{"session":{"user_id":"@user:example.org","device_id":"DEVICE","access_token":"access-token","refresh_token":"refresh-token"}}}"#
    );

    let restored: Event = serde_json::from_str(&json).expect("deserialize logged-in event");
    assert_eq!(restored, event);
}

#[test]
fn session_updated_event_serializes_and_deserializes_session_payload() {
    let session = StoredSession {
        user_id: "@user:example.org".to_owned(),
        device_id: "DEVICE".to_owned(),
        access_token: "updated-access-token".to_owned(),
        refresh_token: Some("updated-refresh-token".to_owned()),
    };
    let event = Event::SessionUpdated {
        session: session.clone(),
    };

    let json = serde_json::to_string(&event).expect("serialize session-updated event");
    assert_eq!(
        json,
        r#"{"SessionUpdated":{"session":{"user_id":"@user:example.org","device_id":"DEVICE","access_token":"updated-access-token","refresh_token":"updated-refresh-token"}}}"#
    );

    let restored: Event = serde_json::from_str(&json).expect("deserialize session-updated event");
    assert_eq!(restored, event);
}

#[test]
fn synced_rooms_event_round_trips_space_hierarchy_payload() {
    let event = Event::SyncedRooms(vec![
        RoomSummary {
            room_id: "!space:example.org".to_owned(),
            display_name: "Space".to_owned(),
            encrypted: false,
            is_space: true,
            space_children: vec![
                SpaceChildSummary {
                    room_id: "!alpha:example.org".to_owned(),
                    suggested: true,
                    order: Some("aaa".to_owned()),
                },
                SpaceChildSummary {
                    room_id: "!beta:example.org".to_owned(),
                    suggested: false,
                    order: None,
                },
            ],
        },
        RoomSummary {
            room_id: "!alpha:example.org".to_owned(),
            display_name: "Alpha".to_owned(),
            encrypted: true,
            is_space: false,
            space_children: Vec::new(),
        },
        RoomSummary {
            room_id: "!beta:example.org".to_owned(),
            display_name: "Beta".to_owned(),
            encrypted: false,
            is_space: false,
            space_children: Vec::new(),
        },
        RoomSummary {
            room_id: "!lobby:example.org".to_owned(),
            display_name: "Lobby".to_owned(),
            encrypted: false,
            is_space: false,
            space_children: Vec::new(),
        },
    ]);

    let json = serde_json::to_string(&event).expect("serialize synced rooms");
    assert_eq!(
        json,
        r#"{"SyncedRooms":[{"room_id":"!space:example.org","display_name":"Space","encrypted":false,"is_space":true,"space_children":[{"room_id":"!alpha:example.org","suggested":true,"order":"aaa"},{"room_id":"!beta:example.org","suggested":false,"order":null}]},{"room_id":"!alpha:example.org","display_name":"Alpha","encrypted":true,"is_space":false,"space_children":[]},{"room_id":"!beta:example.org","display_name":"Beta","encrypted":false,"is_space":false,"space_children":[]},{"room_id":"!lobby:example.org","display_name":"Lobby","encrypted":false,"is_space":false,"space_children":[]}]}"#
    );

    let restored: Event = serde_json::from_str(&json).expect("deserialize synced rooms");
    assert_eq!(restored, event);
}

#[test]
fn room_summary_and_space_child_reject_unknown_fields() {
    let room_error = serde_json::from_str::<Event>(
        r#"{"SyncedRooms":[{"room_id":"!space:example.org","display_name":"Space","encrypted":false,"is_space":true,"space_children":[],"unexpected":true}]}"#,
    )
    .expect_err("room summary must reject unknown fields");
    assert!(
        room_error
            .to_string()
            .contains("unknown field `unexpected`, expected one of `room_id`, `display_name`, `encrypted`, `is_space`, `space_children`"),
        "unexpected room summary error: {room_error}"
    );

    let child_error = serde_json::from_str::<Event>(
        r#"{"SyncedRooms":[{"room_id":"!space:example.org","display_name":"Space","encrypted":false,"is_space":true,"space_children":[{"room_id":"!alpha:example.org","suggested":true,"order":"aaa","unexpected":true}]}]}"#,
    )
    .expect_err("space child summary must reject unknown fields");
    assert!(
        child_error.to_string().contains(
            "unknown field `unexpected`, expected one of `room_id`, `suggested`, `order`"
        ),
        "unexpected space child error: {child_error}"
    );
}

#[test]
fn flat_room_summary_payload_round_trips_with_explicit_space_defaults() {
    let event = Event::SyncedRooms(vec![RoomSummary {
        room_id: "!ops:example.org".to_owned(),
        display_name: "Ops".to_owned(),
        encrypted: true,
        is_space: false,
        space_children: Vec::new(),
    }]);

    let json = serde_json::to_string(&event).expect("serialize flat synced rooms");
    assert_eq!(
        json,
        r#"{"SyncedRooms":[{"room_id":"!ops:example.org","display_name":"Ops","encrypted":true,"is_space":false,"space_children":[]}]}"#
    );

    let restored: Event = serde_json::from_str(&json).expect("deserialize flat synced rooms");
    assert_eq!(restored, event);
}

#[test]
fn sync_started_event_round_trips_and_logged_in_no_longer_uses_unit_shape() {
    let sync_started = serde_json::to_string(&Event::SyncStarted).expect("serialize sync started");
    assert_eq!(sync_started, r#""SyncStarted""#);

    let restored: Event = serde_json::from_str(&sync_started).expect("deserialize sync started");
    assert_eq!(restored, Event::SyncStarted);

    assert!(
        serde_json::from_str::<Event>(r#""LoggedIn""#).is_err(),
        "logged-in event must include a session payload"
    );
}

#[test]
fn room_message_presentations_round_trip_through_serde() {
    for (presentation, json) in [
        (RoomMessagePresentationKind::Text, r#""text""#),
        (RoomMessagePresentationKind::Notice, r#""notice""#),
        (RoomMessagePresentationKind::Emote, r#""emote""#),
    ] {
        assert_eq!(
            serde_json::to_string(&presentation).expect("serialize presentation"),
            json
        );

        let restored: RoomMessagePresentationKind =
            serde_json::from_str(json).expect("deserialize presentation");
        assert_eq!(restored, presentation);
    }
}

#[test]
fn room_activity_events_round_trip_through_serde() {
    let message_event = Event::RoomActivity(RoomActivity {
        room_id: "!ops:example.org".to_owned(),
        activity_id: "event:$event:example.org".to_owned(),
        timestamp_ms: 1_700_000_000_000,
        kind: RoomActivityKind::Message(RoomMessageActivity {
            event_id: Some("$event:example.org".to_owned()),
            transaction_id: Some("txn-1".to_owned()),
            sender: "@alice:example.org".to_owned(),
            presentation: RoomMessagePresentationKind::Text,
            body: "hello".to_owned(),
            send_state: RoomMessageSendState::Sent,
        }),
    });
    let pending_message_event = Event::RoomActivity(RoomActivity {
        room_id: "!ops:example.org".to_owned(),
        activity_id: "txn:txn-1".to_owned(),
        timestamp_ms: 1_700_000_000_000,
        kind: RoomActivityKind::Message(RoomMessageActivity {
            event_id: None,
            transaction_id: Some("txn-1".to_owned()),
            sender: "@alice:example.org".to_owned(),
            presentation: RoomMessagePresentationKind::Text,
            body: "hello".to_owned(),
            send_state: RoomMessageSendState::Pending,
        }),
    });
    let state_notice_event = Event::RoomActivity(RoomActivity {
        room_id: "!ops:example.org".to_owned(),
        activity_id: "state:$topic:example.org".to_owned(),
        timestamp_ms: 1_700_000_000_001,
        kind: RoomActivityKind::StateNotice(RoomStateNoticeActivity {
            event_id: "$topic:example.org".to_owned(),
            sender: "@alice:example.org".to_owned(),
            state_key: String::new(),
            content: RoomStateNoticeContent::Topic {
                topic: "incident review".to_owned(),
            },
        }),
    });
    let member_notice_event = Event::RoomActivity(RoomActivity {
        room_id: "!ops:example.org".to_owned(),
        activity_id: "state:$member:example.org".to_owned(),
        timestamp_ms: 1_700_000_000_002,
        kind: RoomActivityKind::StateNotice(RoomStateNoticeActivity {
            event_id: "$member:example.org".to_owned(),
            sender: "@alice:example.org".to_owned(),
            state_key: "@carol:example.org".to_owned(),
            content: RoomStateNoticeContent::Member {
                membership: "join".to_owned(),
                displayname: Some("carol".to_owned()),
            },
        }),
    });
    let sent_update_event = Event::RoomActivityUpdate(RoomActivityUpdate {
        room_id: "!ops:example.org".to_owned(),
        transaction_id: "txn-1".to_owned(),
        kind: RoomActivityUpdateKind::MessageSent {
            event_id: "$event:example.org".to_owned(),
        },
    });
    let failed_update_event = Event::RoomActivityUpdate(RoomActivityUpdate {
        room_id: "!ops:example.org".to_owned(),
        transaction_id: "txn-1".to_owned(),
        kind: RoomActivityUpdateKind::MessageFailed {
            message: "send failed".to_owned(),
            recoverable: true,
        },
    });

    for event in [
        message_event,
        pending_message_event,
        state_notice_event,
        member_notice_event,
        sent_update_event,
        failed_update_event,
    ] {
        let json = serde_json::to_string(&event).expect("serialize room activity event");
        let restored: Event = serde_json::from_str(&json).expect("deserialize room activity event");
        assert_eq!(restored, event);
    }
}

#[test]
fn room_activity_events_serialize_with_tagged_wire_shape() {
    let pending_message_event = Event::RoomActivity(RoomActivity {
        room_id: "!ops:example.org".to_owned(),
        activity_id: "txn:txn-1".to_owned(),
        timestamp_ms: 1_700_000_000_000,
        kind: RoomActivityKind::Message(RoomMessageActivity {
            event_id: None,
            transaction_id: Some("txn-1".to_owned()),
            sender: "@alice:example.org".to_owned(),
            presentation: RoomMessagePresentationKind::Text,
            body: "hello".to_owned(),
            send_state: RoomMessageSendState::Pending,
        }),
    });
    let member_notice_event = Event::RoomActivity(RoomActivity {
        room_id: "!ops:example.org".to_owned(),
        activity_id: "state:$member:example.org".to_owned(),
        timestamp_ms: 1_700_000_000_002,
        kind: RoomActivityKind::StateNotice(RoomStateNoticeActivity {
            event_id: "$member:example.org".to_owned(),
            sender: "@alice:example.org".to_owned(),
            state_key: "@carol:example.org".to_owned(),
            content: RoomStateNoticeContent::Member {
                membership: "join".to_owned(),
                displayname: Some("carol".to_owned()),
            },
        }),
    });
    let failed_update_event = Event::RoomActivityUpdate(RoomActivityUpdate {
        room_id: "!ops:example.org".to_owned(),
        transaction_id: "txn-1".to_owned(),
        kind: RoomActivityUpdateKind::MessageFailed {
            message: "send failed".to_owned(),
            recoverable: true,
        },
    });

    assert_eq!(
        serde_json::to_string(&pending_message_event).expect("serialize pending room activity"),
        r#"{"RoomActivity":{"room_id":"!ops:example.org","activity_id":"txn:txn-1","timestamp_ms":1700000000000,"kind":{"type":"message","event_id":null,"transaction_id":"txn-1","sender":"@alice:example.org","presentation":"text","body":"hello","send_state":{"state":"pending"}}}}"#
    );
    assert_eq!(
        serde_json::to_string(&member_notice_event).expect("serialize member room activity"),
        r#"{"RoomActivity":{"room_id":"!ops:example.org","activity_id":"state:$member:example.org","timestamp_ms":1700000000002,"kind":{"type":"state_notice","event_id":"$member:example.org","sender":"@alice:example.org","state_key":"@carol:example.org","content":{"type":"member","membership":"join","displayname":"carol"}}}}"#
    );
    assert_eq!(
        serde_json::to_string(&failed_update_event).expect("serialize failed room activity update"),
        r#"{"RoomActivityUpdate":{"room_id":"!ops:example.org","transaction_id":"txn-1","kind":{"type":"message_failed","message":"send failed","recoverable":true}}}"#
    );
}

#[test]
fn room_typing_event_round_trips_through_serde() {
    let event = Event::RoomTyping(RoomTyping {
        room_id: "!ops:example.org".to_owned(),
        user_ids: vec![
            "@alice:example.org".to_owned(),
            "@bob:example.org".to_owned(),
        ],
    });

    let json = serde_json::to_string(&event).expect("serialize room typing event");
    assert_eq!(
        json,
        r#"{"RoomTyping":{"room_id":"!ops:example.org","user_ids":["@alice:example.org","@bob:example.org"]}}"#
    );

    let restored: Event = serde_json::from_str(&json).expect("deserialize room typing event");
    assert_eq!(restored, event);
}

#[test]
fn room_read_state_event_round_trips_through_serde() {
    let event = Event::RoomReadState(RoomReadState {
        room_id: "!ops:example.org".to_owned(),
        latest_active_event_id: Some("$latest:example.org".to_owned()),
        fully_read_event_id: Some("$fully-read:example.org".to_owned()),
        marked_unread: true,
        num_unread: 7,
        num_notifications: 3,
        num_mentions: 1,
    });

    let json = serde_json::to_string(&event).expect("serialize room read state event");
    assert_eq!(
        json,
        r#"{"RoomReadState":{"room_id":"!ops:example.org","latest_active_event_id":"$latest:example.org","fully_read_event_id":"$fully-read:example.org","marked_unread":true,"num_unread":7,"num_notifications":3,"num_mentions":1}}"#
    );

    let restored: Event =
        serde_json::from_str(&json).expect("deserialize room read state event");
    assert_eq!(restored, event);
}

#[test]
fn verification_request_event_preserves_existing_passive_wire_shape() {
    let event = Event::VerificationRequest(VerificationRequest {
        flow_id: "txn-123".to_owned(),
        user_id: "@alice:example.org".to_owned(),
    });

    let json = serde_json::to_string(&event).expect("serialize verification request");
    assert_eq!(
        json,
        r#"{"VerificationRequest":{"flow_id":"txn-123","user_id":"@alice:example.org"}}"#
    );

    let restored: Event = serde_json::from_str(&json).expect("deserialize verification request");
    assert_eq!(restored, event);
}

#[test]
fn verification_state_event_serializes_with_tagged_state_payload() {
    let event = Event::VerificationState(VerificationStateEvent {
        flow_id: "txn-123".to_owned(),
        user_id: "@alice:example.org".to_owned(),
        state: VerificationState::SasReady {
            emojis: vec![VerificationEmoji {
                symbol: "🐶".to_owned(),
                description: "Dog".to_owned(),
            }],
            decimals: VerificationDecimals {
                first: 1234,
                second: 5678,
                third: 9012,
            },
        },
    });

    let json = serde_json::to_string(&event).expect("serialize verification state");
    assert_eq!(
        json,
        r#"{"VerificationState":{"flow_id":"txn-123","user_id":"@alice:example.org","state":{"state":"sas_ready","emojis":[{"symbol":"🐶","description":"Dog"}],"decimals":{"first":1234,"second":5678,"third":9012}}}}"#
    );

    let restored: Event = serde_json::from_str(&json).expect("deserialize verification state");
    assert_eq!(restored, event);
}

#[test]
fn verification_dtos_reject_unknown_fields() {
    assert!(
        serde_json::from_str::<VerificationFlowId>(
            r#"{"flow_id":"txn-123","user_id":"@alice:example.org","unexpected":"value"}"#
        )
        .is_err(),
        "verification flow id must reject unknown fields"
    );

    assert!(
        serde_json::from_str::<VerificationState>(r#"{"state":"requested","unexpected":"value"}"#)
            .is_err(),
        "verification state must reject unknown fields"
    );

    assert!(
        serde_json::from_str::<VerificationStateEvent>(
            r#"{"flow_id":"txn-123","user_id":"@alice:example.org","state":{"state":"requested","unexpected":"value"}}"#
        )
        .is_err(),
        "verification state event must reject unknown nested state fields"
    );

    assert!(
        serde_json::from_str::<VerificationStateEvent>(
            r#"{"flow_id":"txn-123","user_id":"@alice:example.org","state":{"state":"requested"},"unexpected":"value"}"#
        )
        .is_err(),
        "verification state event must reject top-level unknown fields"
    );

    assert!(
        serde_json::from_str::<VerificationEmoji>(
            r#"{"symbol":"🐶","description":"Dog","unexpected":"value"}"#
        )
        .is_err(),
        "verification emoji must reject unknown fields"
    );

    assert!(
        serde_json::from_str::<VerificationDecimals>(
            r#"{"first":1234,"second":5678,"third":9012,"unexpected":"value"}"#
        )
        .is_err(),
        "verification decimals must reject unknown fields"
    );
}

#[test]
fn room_activity_dtos_reject_unknown_fields() {
    assert!(
        serde_json::from_str::<MarkRoomReadRequest>(
            r#"{"room_id":"!ops:example.org","event_id":"$event:example.org","unexpected":"value"}"#
        )
        .is_err(),
        "mark room read request must reject unknown fields"
    );

    assert!(
        serde_json::from_str::<RoomActivity>(
            r#"{"room_id":"!ops:example.org","activity_id":"event:$event:example.org","timestamp_ms":1700000000000,"kind":{"type":"message","event_id":"$event:example.org","transaction_id":"txn-1","sender":"@alice:example.org","presentation":"text","body":"hello","send_state":{"state":"sent"}},"unexpected":"value"}"#
        )
        .is_err(),
        "room activity must reject unknown fields"
    );

    assert!(
        serde_json::from_str::<RoomMessageActivity>(
            r#"{"event_id":"$event:example.org","transaction_id":"txn-1","sender":"@alice:example.org","presentation":"text","body":"hello","send_state":{"state":"sent"},"unexpected":"value"}"#
        )
        .is_err(),
        "room message activity must reject unknown fields"
    );

    assert!(
        serde_json::from_str::<Event>(
            r#"{"RoomActivity":{"room_id":"!ops:example.org","activity_id":"txn:txn-1","timestamp_ms":1700000000000,"kind":{"type":"message","event_id":null,"transaction_id":"txn-1","sender":"@alice:example.org","presentation":"text","body":"hello","send_state":{"state":"failed","message":"send failed","recoverable":true,"unexpected":"value"}}}}"#
        )
        .is_err(),
        "failed message send state must reject unknown fields"
    );

    assert!(
        serde_json::from_str::<RoomMessageActivity>(
            r#"{"event_id":"$event:example.org","transaction_id":"txn-1","sender":"@alice:example.org","body":"hello","send_state":{"state":"sent"}}"#
        )
        .is_err(),
        "room message activity must require presentation"
    );

    assert!(
        serde_json::from_str::<RoomMessageActivity>(
            r#"{"event_id":"$event:example.org","transaction_id":"txn-1","sender":"@alice:example.org","presentation":"html","body":"hello","send_state":{"state":"sent"}}"#
        )
        .is_err(),
        "room message activity must reject unknown presentation"
    );

    assert!(
        serde_json::from_str::<RoomStateNoticeActivity>(
            r#"{"event_id":"$topic:example.org","sender":"@alice:example.org","state_key":"","content":{"type":"topic","topic":"incident review"},"unexpected":"value"}"#
        )
        .is_err(),
        "room state notice activity must reject unknown fields"
    );

    assert!(
        serde_json::from_str::<Event>(
            r#"{"RoomActivity":{"room_id":"!ops:example.org","activity_id":"state:$topic:example.org","timestamp_ms":1700000000001,"kind":{"type":"state_notice","event_id":"$topic:example.org","sender":"@alice:example.org","state_key":"","content":{"type":"topic","topic":"incident review","unexpected":"value"}}}}"#
        )
        .is_err(),
        "room state notice content must reject unknown fields"
    );

    assert!(
        serde_json::from_str::<RoomActivityUpdate>(
            r#"{"room_id":"!ops:example.org","transaction_id":"txn-1","kind":{"type":"message_sent","event_id":"$event:example.org"},"unexpected":"value"}"#
        )
        .is_err(),
        "room activity update must reject unknown fields"
    );

    assert!(
        serde_json::from_str::<Event>(
            r#"{"RoomActivityUpdate":{"room_id":"!ops:example.org","transaction_id":"txn-1","kind":{"type":"message_failed","message":"send failed","recoverable":true,"unexpected":"value"}}}"#
        )
        .is_err(),
        "failed room activity update must reject unknown fields"
    );

    assert!(
        serde_json::from_str::<RoomTyping>(
            r#"{"room_id":"!ops:example.org","user_ids":["@alice:example.org"],"unexpected":"value"}"#
        )
        .is_err(),
        "room typing must reject unknown fields"
    );

    assert!(
        serde_json::from_str::<RoomReadState>(
            r#"{"room_id":"!ops:example.org","latest_active_event_id":"$latest:example.org","fully_read_event_id":"$fully-read:example.org","marked_unread":true,"num_unread":7,"num_notifications":3,"num_mentions":1,"unexpected":"value"}"#
        )
        .is_err(),
        "room read state must reject unknown fields"
    );
}

#[test]
fn room_state_notice_content_mismatch_is_unrepresentable() {
    assert!(
        serde_json::from_str::<Event>(
            r#"{"RoomActivity":{"room_id":"!ops:example.org","activity_id":"state:$member:example.org","timestamp_ms":1700000000002,"kind":{"type":"state_notice","event_id":"$member:example.org","sender":"@alice:example.org","state_key":"@carol:example.org","content":{"type":"member","topic":"incident review"}}}}"#
        )
        .is_err(),
        "tagged room state notice content must reject mismatched payloads"
    );
}
