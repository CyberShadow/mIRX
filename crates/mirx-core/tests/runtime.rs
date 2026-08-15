use std::path::PathBuf;
use std::time::Duration;

use mirx_core::{
    AcceptRoomInviteRequest, BackfillRoomHistoryRequest, BanRoomMemberRequest, Command, CoreConfig,
    CreateDirectRoomRequest, E2eeBackupState, E2eeBackupStatus, E2eeCrossSigningStatus,
    E2eeRecoveryOperation, E2eeRecoverySetupSucceeded, E2eeRecoveryState, E2eeRecoverySucceeded,
    E2eeStatus, EditMessageRequest, Event, FetchRoomMediaRequest, JoinRoomRequest,
    KickRoomMemberRequest, LeaveRoomRequest, LoginRequest, MarkRoomReadRequest, Operation,
    ROOM_HISTORY_BACKFILL_DEFAULT_LIMIT, ROOM_HISTORY_BACKFILL_LIMIT_MAX, RecoverE2eeRequest,
    RedactEventRequest, RejectRoomInviteRequest, RoomActivity, RoomActivityKind,
    RoomActivityUpdate, RoomActivityUpdateKind, RoomActivityUpdateTarget, RoomAudioActivity,
    RoomAudioInfo, RoomEncryptedFileHashes, RoomEncryptedFileKey, RoomFileActivity, RoomFileInfo,
    RoomHistoryBackfill, RoomHistoryBackfillEvent, RoomImageActivity, RoomImageInfo, RoomMedia,
    RoomMediaFormat, RoomMediaKind, RoomMediaSource, RoomMemberSummary, RoomMembers,
    RoomMembership, RoomMessageActivity, RoomMessagePresentationKind, RoomMessageSendState,
    RoomReadState, RoomRedactedActivity, RoomStateNoticeActivity, RoomStateNoticeContent,
    RoomSummary, RoomTagSummary, RoomTyping, SendFileRequest, SendMessageRequest,
    SendReactionRequest, SetRoomTopicRequest, SetupE2eeRecoveryRequest, SpaceChildSummary,
    StoredSession, UnsupportedRoomMessageActivity, VerificationDecimals, VerificationEmoji,
    VerificationFlowId, VerificationRequest, VerificationState, VerificationStateEvent, start,
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

fn encrypted_file_key() -> RoomEncryptedFileKey {
    RoomEncryptedFileKey {
        kty: "oct".to_owned(),
        key_ops: vec!["decrypt".to_owned(), "encrypt".to_owned()],
        alg: "A256CTR".to_owned(),
        k: "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA".to_owned(),
        ext: true,
    }
}

fn encrypted_file_hashes() -> RoomEncryptedFileHashes {
    RoomEncryptedFileHashes {
        sha256: "AQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQE".to_owned(),
    }
}

fn encrypted_media_source(url: &str) -> RoomMediaSource {
    RoomMediaSource::Encrypted {
        url: url.to_owned(),
        key: encrypted_file_key(),
        iv: "AgICAgICAgICAgICAgICAg".to_owned(),
        hashes: encrypted_file_hashes(),
        v: "v2".to_owned(),
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

fn assert_invalid_authenticated_command(
    command: Command,
    expected_operation: Operation,
    expected_message: &str,
) {
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
        .expect("send authenticated command");

    match handle
        .events()
        .recv_timeout(Duration::from_secs(2))
        .expect("authenticated command error")
    {
        Event::Error(err) => {
            assert_eq!(err.operation, expected_operation);
            assert_eq!(err.message, expected_message);
        }
        other => panic!("expected authenticated command error, got {other:?}"),
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
fn send_file_before_login_emits_error() {
    assert_invalid_authenticated_command(
        Command::SendFile(SendFileRequest {
            room_id: "!room:example.org".to_owned(),
            path: PathBuf::from("report.txt"),
            filename: None,
            content_type: None,
        }),
        Operation::SendFile,
        "send file requires an authenticated session",
    );
}

#[test]
fn send_file_request_round_trips_and_rejects_unknown_fields() {
    let request = SendFileRequest {
        room_id: "!room:example.org".to_owned(),
        path: PathBuf::from("C:/tmp/report.txt"),
        filename: Some("report.txt".to_owned()),
        content_type: Some("application/octet-stream".to_owned()),
    };
    let json = serde_json::to_string(&request).expect("serialize send file request");
    assert_eq!(
        serde_json::from_str::<SendFileRequest>(&json).expect("deserialize send file request"),
        request
    );
    assert!(serde_json::from_str::<SendFileRequest>(
        r#"{"room_id":"!room:example.org","path":"report.txt","filename":null,"content_type":null,"extra":true}"#
    )
    .is_err());
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
            thread: None,
            reply: None,
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
        .send(Command::CreateDirectRoom(CreateDirectRoomRequest {
            user_id: "@alice:example.org".to_owned(),
        }))
        .expect("send create direct room");
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
    let create_direct_room_error = handle
        .events()
        .recv_timeout(Duration::from_secs(2))
        .expect("create direct room error");
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
    match create_direct_room_error {
        Event::Error(err) => {
            assert_eq!(err.operation, Operation::CreateDirectRoom);
            assert_eq!(
                err.message,
                "create direct room requires an authenticated session"
            );
        }
        other => panic!("expected create direct room error, got {other:?}"),
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
fn join_room_before_login_emits_join_room_error() {
    assert_invalid_authenticated_command(
        Command::JoinRoom(JoinRoomRequest {
            room_or_alias: "#ops:example.org".to_owned(),
        }),
        Operation::JoinRoom,
        "join room requires an authenticated session",
    );
}

#[test]
fn create_direct_room_before_login_emits_create_direct_room_error() {
    assert_invalid_authenticated_command(
        Command::CreateDirectRoom(CreateDirectRoomRequest {
            user_id: "@alice:example.org".to_owned(),
        }),
        Operation::CreateDirectRoom,
        "create direct room requires an authenticated session",
    );
}

#[test]
fn leave_room_before_login_emits_leave_room_error() {
    assert_invalid_authenticated_command(
        Command::LeaveRoom(LeaveRoomRequest {
            room_id: "!ops:example.org".to_owned(),
        }),
        Operation::LeaveRoom,
        "leave room requires an authenticated session",
    );
}

#[test]
fn kick_room_member_before_login_emits_kick_room_member_error() {
    assert_invalid_authenticated_command(
        Command::KickRoomMember(KickRoomMemberRequest {
            room_id: "!ops:example.org".to_owned(),
            user_id: "@bob:example.org".to_owned(),
        }),
        Operation::KickRoomMember,
        "kick room member requires an authenticated session",
    );
}

#[test]
fn ban_room_member_before_login_emits_ban_room_member_error() {
    assert_invalid_authenticated_command(
        Command::BanRoomMember(BanRoomMemberRequest {
            room_id: "!ops:example.org".to_owned(),
            user_id: "@bob:example.org".to_owned(),
        }),
        Operation::BanRoomMember,
        "ban room member requires an authenticated session",
    );
}

#[test]
fn accept_room_invite_before_login_emits_accept_room_invite_error() {
    assert_invalid_authenticated_command(
        Command::AcceptRoomInvite(AcceptRoomInviteRequest {
            room_id: "!ops:example.org".to_owned(),
        }),
        Operation::AcceptRoomInvite,
        "accept room invitation requires an authenticated session",
    );
}

#[test]
fn reject_room_invite_before_login_emits_reject_room_invite_error() {
    assert_invalid_authenticated_command(
        Command::RejectRoomInvite(RejectRoomInviteRequest {
            room_id: "!ops:example.org".to_owned(),
        }),
        Operation::RejectRoomInvite,
        "reject room invitation requires an authenticated session",
    );
}

#[test]
fn send_reaction_before_login_emits_send_reaction_error() {
    assert_invalid_authenticated_command(
        Command::SendReaction(SendReactionRequest {
            room_id: "!ops:example.org".to_owned(),
            target_event_id: "$event:example.org".to_owned(),
            key: "+1".to_owned(),
        }),
        Operation::SendReaction,
        "send reaction requires an authenticated session",
    );
}

#[test]
fn edit_message_before_login_emits_edit_message_error() {
    assert_invalid_authenticated_command(
        Command::EditMessage(EditMessageRequest {
            room_id: "!ops:example.org".to_owned(),
            target_event_id: "$event:example.org".to_owned(),
            presentation: RoomMessagePresentationKind::Text,
            body: "edited body".to_owned(),
        }),
        Operation::EditMessage,
        "edit message requires an authenticated session",
    );
}

#[test]
fn redact_event_before_login_emits_redact_event_error() {
    assert_invalid_authenticated_command(
        Command::RedactEvent(RedactEventRequest {
            room_id: "!ops:example.org".to_owned(),
            event_id: "$reaction:example.org".to_owned(),
        }),
        Operation::RedactEvent,
        "redact event requires an authenticated session",
    );
}

#[test]
fn set_room_topic_before_login_emits_set_room_topic_error() {
    assert_invalid_authenticated_command(
        Command::SetRoomTopic(SetRoomTopicRequest {
            room_id: "!ops:example.org".to_owned(),
            topic: "incident review".to_owned(),
        }),
        Operation::SetRoomTopic,
        "set room topic requires an authenticated session",
    );
}

#[test]
fn fetch_room_media_before_login_emits_fetch_room_media_error() {
    assert_invalid_authenticated_command(
        Command::FetchRoomMedia(FetchRoomMediaRequest {
            room_id: "!ops:example.org".to_owned(),
            activity_id: "event:$image:example.org".to_owned(),
            event_id: "$image:example.org".to_owned(),
            source: RoomMediaSource::Plain {
                url: "mxc://example.org/image".to_owned(),
            },
            format: RoomMediaFormat::Thumbnail {
                width: 160,
                height: 120,
            },
        }),
        Operation::FetchRoomMedia,
        "fetch room media requires an authenticated session",
    );
}

#[test]
fn backfill_room_history_before_login_emits_backfill_room_history_error() {
    assert_invalid_authenticated_command(
        Command::BackfillRoomHistory(BackfillRoomHistoryRequest {
            room_id: "!ops:example.org".to_owned(),
            from: Some("t123-456".to_owned()),
            limit: ROOM_HISTORY_BACKFILL_DEFAULT_LIMIT,
        }),
        Operation::BackfillRoomHistory,
        "backfill room history requires an authenticated session",
    );
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
fn refresh_e2ee_status_before_login_emits_status_error() {
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
        .send(Command::RefreshE2eeStatus)
        .expect("send refresh e2ee status");

    match handle
        .events()
        .recv_timeout(Duration::from_secs(2))
        .expect("e2ee status error")
    {
        Event::Error(err) => {
            assert_eq!(err.operation, Operation::E2eeStatus);
            assert_eq!(err.message, "E2EE status requires an authenticated session");
        }
        other => panic!("expected e2ee status error, got {other:?}"),
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
fn recover_e2ee_before_login_emits_recovery_error() {
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
        .send(Command::RecoverE2ee(RecoverE2eeRequest {
            recovery_secret: "test recovery secret".to_owned(),
        }))
        .expect("send recover e2ee");

    match handle
        .events()
        .recv_timeout(Duration::from_secs(2))
        .expect("e2ee recovery error")
    {
        Event::Error(err) => {
            assert_eq!(err.operation, Operation::RecoverE2ee);
            assert_eq!(
                err.message,
                "E2EE recovery requires an authenticated session"
            );
        }
        other => panic!("expected e2ee recovery error, got {other:?}"),
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
fn setup_e2ee_recovery_before_login_emits_setup_error() {
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
        .send(Command::SetupE2eeRecovery(SetupE2eeRecoveryRequest {
            passphrase: Some("test passphrase".to_owned()),
            wait_for_backups_to_upload: true,
        }))
        .expect("send setup e2ee recovery");

    match handle
        .events()
        .recv_timeout(Duration::from_secs(2))
        .expect("e2ee recovery setup error")
    {
        Event::Error(err) => {
            assert_eq!(err.operation, Operation::SetupE2eeRecovery);
            assert_eq!(
                err.message,
                "E2EE recovery setup requires an authenticated session"
            );
        }
        other => panic!("expected e2ee recovery setup error, got {other:?}"),
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
            device_display_name: "mirx runtime test".to_owned(),
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
fn join_room_request_round_trips_through_serde() {
    let request = JoinRoomRequest {
        room_or_alias: "#ops:example.org".to_owned(),
    };

    let json = serde_json::to_string(&request).expect("serialize join room request");
    assert_eq!(json, r##"{"room_or_alias":"#ops:example.org"}"##);

    let restored: JoinRoomRequest =
        serde_json::from_str(&json).expect("deserialize join room request");
    assert_eq!(restored, request);
}

#[test]
fn create_direct_room_request_round_trips_through_serde() {
    let request = CreateDirectRoomRequest {
        user_id: "@alice:example.org".to_owned(),
    };

    let json = serde_json::to_string(&request).expect("serialize create direct room request");
    assert_eq!(json, r#"{"user_id":"@alice:example.org"}"#);

    let restored: CreateDirectRoomRequest =
        serde_json::from_str(&json).expect("deserialize create direct room request");
    assert_eq!(restored, request);
}

#[test]
fn leave_room_request_round_trips_through_serde() {
    let request = LeaveRoomRequest {
        room_id: "!ops:example.org".to_owned(),
    };

    let json = serde_json::to_string(&request).expect("serialize leave room request");
    assert_eq!(json, r#"{"room_id":"!ops:example.org"}"#);

    let restored: LeaveRoomRequest =
        serde_json::from_str(&json).expect("deserialize leave room request");
    assert_eq!(restored, request);
}

#[test]
fn member_moderation_requests_and_commands_preserve_room_and_user_ids() {
    let kick_request = KickRoomMemberRequest {
        room_id: "!ops:example.org".to_owned(),
        user_id: "@alice:example.org".to_owned(),
    };
    let ban_request = BanRoomMemberRequest {
        room_id: "!ops:example.org".to_owned(),
        user_id: "@bob:example.org".to_owned(),
    };

    let kick_json = r#"{"room_id":"!ops:example.org","user_id":"@alice:example.org"}"#;
    assert_eq!(
        serde_json::to_string(&kick_request).expect("serialize kick request"),
        kick_json
    );
    assert_eq!(
        serde_json::from_str::<KickRoomMemberRequest>(kick_json).expect("deserialize kick request"),
        kick_request.clone()
    );

    let ban_json = r#"{"room_id":"!ops:example.org","user_id":"@bob:example.org"}"#;
    assert_eq!(
        serde_json::to_string(&ban_request).expect("serialize ban request"),
        ban_json
    );
    assert_eq!(
        serde_json::from_str::<BanRoomMemberRequest>(ban_json).expect("deserialize ban request"),
        ban_request
    );

    assert!(matches!(
        Command::KickRoomMember(kick_request),
        Command::KickRoomMember(KickRoomMemberRequest { room_id, user_id })
            if room_id == "!ops:example.org" && user_id == "@alice:example.org"
    ));
    assert!(matches!(
        Command::BanRoomMember(ban_request),
        Command::BanRoomMember(BanRoomMemberRequest { room_id, user_id })
            if room_id == "!ops:example.org" && user_id == "@bob:example.org"
    ));
}

#[test]
fn member_moderation_requests_reject_unknown_fields() {
    for json in [
        r#"{"room_id":"!ops:example.org","user_id":"@alice:example.org","unexpected":true}"#,
        r#"{"room_id":"!ops:example.org","unexpected":true}"#,
    ] {
        assert!(
            serde_json::from_str::<KickRoomMemberRequest>(json).is_err(),
            "kick request must reject invalid fields"
        );
        assert!(
            serde_json::from_str::<BanRoomMemberRequest>(json).is_err(),
            "ban request must reject invalid fields"
        );
    }
}

#[test]
fn member_moderation_operations_round_trip_through_serde() {
    for (operation, json) in [
        (Operation::KickRoomMember, r#""KickRoomMember""#),
        (Operation::BanRoomMember, r#""BanRoomMember""#),
    ] {
        assert_eq!(
            serde_json::to_string(&operation).expect("serialize operation"),
            json
        );
        assert_eq!(
            serde_json::from_str::<Operation>(json).expect("deserialize operation"),
            operation
        );
    }
}

#[test]
fn room_membership_round_trips_through_serde() {
    for (membership, json) in [
        (RoomMembership::Join, r#""join""#),
        (RoomMembership::Invite, r#""invite""#),
    ] {
        assert_eq!(
            serde_json::to_string(&membership).expect("serialize room membership"),
            json
        );
        assert_eq!(
            serde_json::from_str::<RoomMembership>(json).expect("deserialize room membership"),
            membership
        );
    }
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
fn backfill_room_history_request_round_trips_through_serde() {
    for (request, json) in [
        (
            BackfillRoomHistoryRequest {
                room_id: "!room:example.org".to_owned(),
                from: None,
                limit: ROOM_HISTORY_BACKFILL_DEFAULT_LIMIT,
            },
            r#"{"room_id":"!room:example.org","from":null,"limit":50}"#,
        ),
        (
            BackfillRoomHistoryRequest {
                room_id: "!room:example.org".to_owned(),
                from: Some("t4747337_219380_2603".to_owned()),
                limit: ROOM_HISTORY_BACKFILL_LIMIT_MAX,
            },
            r#"{"room_id":"!room:example.org","from":"t4747337_219380_2603","limit":50}"#,
        ),
    ] {
        let serialized =
            serde_json::to_string(&request).expect("serialize backfill room history request");
        assert_eq!(serialized, json);

        let restored: BackfillRoomHistoryRequest =
            serde_json::from_str(json).expect("deserialize backfill room history request");
        assert_eq!(restored, request);
    }
}

#[test]
fn backfill_room_history_request_rejects_unknown_fields() {
    assert!(
        serde_json::from_str::<BackfillRoomHistoryRequest>(
            r#"{"room_id":"!room:example.org","from":null,"limit":50,"unexpected":"value"}"#
        )
        .is_err(),
        "backfill room history request must reject unknown fields"
    );
}

#[test]
fn send_reaction_request_round_trips_through_serde() {
    let request = SendReactionRequest {
        room_id: "!room:example.org".to_owned(),
        target_event_id: "$event:example.org".to_owned(),
        key: "+1".to_owned(),
    };

    let json = serde_json::to_string(&request).expect("serialize send reaction request");
    assert_eq!(
        json,
        r#"{"room_id":"!room:example.org","target_event_id":"$event:example.org","key":"+1"}"#
    );

    let restored: SendReactionRequest =
        serde_json::from_str(&json).expect("deserialize send reaction request");
    assert_eq!(restored, request);
}

#[test]
fn redact_event_request_round_trips_through_serde() {
    let request = RedactEventRequest {
        room_id: "!room:example.org".to_owned(),
        event_id: "$reaction:example.org".to_owned(),
    };

    let json = serde_json::to_string(&request).expect("serialize redact event request");
    assert_eq!(
        json,
        r#"{"room_id":"!room:example.org","event_id":"$reaction:example.org"}"#
    );

    let restored: RedactEventRequest =
        serde_json::from_str(&json).expect("deserialize redact event request");
    assert_eq!(restored, request);
}

#[test]
fn fetch_room_media_request_round_trips_through_serde() {
    for (request, json) in [
        (
            FetchRoomMediaRequest {
                room_id: "!room:example.org".to_owned(),
                activity_id: "event:$image:example.org".to_owned(),
                event_id: "$image:example.org".to_owned(),
                source: RoomMediaSource::Plain {
                    url: "mxc://example.org/image".to_owned(),
                },
                format: RoomMediaFormat::Thumbnail {
                    width: 160,
                    height: 120,
                },
            },
            r#"{"room_id":"!room:example.org","activity_id":"event:$image:example.org","event_id":"$image:example.org","source":{"type":"plain","url":"mxc://example.org/image"},"format":{"type":"thumbnail","width":160,"height":120}}"#,
        ),
        (
            FetchRoomMediaRequest {
                room_id: "!room:example.org".to_owned(),
                activity_id: "event:$image:example.org".to_owned(),
                event_id: "$image:example.org".to_owned(),
                source: encrypted_media_source("mxc://example.org/encrypted-image"),
                format: RoomMediaFormat::File,
            },
            r#"{"room_id":"!room:example.org","activity_id":"event:$image:example.org","event_id":"$image:example.org","source":{"type":"encrypted","url":"mxc://example.org/encrypted-image","key":{"kty":"oct","key_ops":["decrypt","encrypt"],"alg":"A256CTR","k":"AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA","ext":true},"iv":"AgICAgICAgICAgICAgICAg","hashes":{"sha256":"AQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQE"},"v":"v2"},"format":{"type":"file"}}"#,
        ),
    ] {
        let serialized =
            serde_json::to_string(&request).expect("serialize fetch room media request");
        assert_eq!(serialized, json);

        let restored: FetchRoomMediaRequest =
            serde_json::from_str(json).expect("deserialize fetch room media request");
        assert_eq!(restored, request);
    }
}

#[test]
fn room_media_source_and_image_info_round_trip_through_serde() {
    for source in [
        RoomMediaSource::Plain {
            url: "mxc://example.org/plain-image".to_owned(),
        },
        encrypted_media_source("mxc://example.org/encrypted-image"),
    ] {
        let json = serde_json::to_string(&source).expect("serialize room media source");
        let restored: RoomMediaSource =
            serde_json::from_str(&json).expect("deserialize room media source");
        assert_eq!(restored, source);
    }

    let info = RoomImageInfo {
        mimetype: Some("image/png".to_owned()),
        size: Some(4096),
        width: Some(800),
        height: Some(600),
        is_animated: Some(false),
        thumbnail_source: Some(encrypted_media_source("mxc://example.org/encrypted-thumb")),
    };

    let json = serde_json::to_string(&info).expect("serialize room image info");
    assert_eq!(
        json,
        r#"{"mimetype":"image/png","size":4096,"width":800,"height":600,"is_animated":false,"thumbnail_source":{"type":"encrypted","url":"mxc://example.org/encrypted-thumb","key":{"kty":"oct","key_ops":["decrypt","encrypt"],"alg":"A256CTR","k":"AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA","ext":true},"iv":"AgICAgICAgICAgICAgICAg","hashes":{"sha256":"AQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQE"},"v":"v2"}}"#
    );

    let restored: RoomImageInfo = serde_json::from_str(&json).expect("deserialize room image info");
    assert_eq!(restored, info);
}

#[test]
fn recover_e2ee_request_round_trips_through_serde() {
    let request = RecoverE2eeRequest {
        recovery_secret: "EsTl...secret".to_owned(),
    };

    let json = serde_json::to_string(&request).expect("serialize recover e2ee request");
    assert_eq!(json, r#"{"recovery_secret":"EsTl...secret"}"#);

    let restored: RecoverE2eeRequest =
        serde_json::from_str(&json).expect("deserialize recover e2ee request");
    assert_eq!(restored, request);
}

#[test]
fn setup_e2ee_recovery_request_round_trips_through_serde() {
    let request = SetupE2eeRecoveryRequest {
        passphrase: Some("test passphrase".to_owned()),
        wait_for_backups_to_upload: true,
    };

    let json = serde_json::to_string(&request).expect("serialize setup e2ee recovery request");
    assert_eq!(
        json,
        r#"{"passphrase":"test passphrase","wait_for_backups_to_upload":true}"#
    );

    let restored: SetupE2eeRecoveryRequest =
        serde_json::from_str(&json).expect("deserialize setup e2ee recovery request");
    assert_eq!(restored, request);
}

#[test]
fn setup_e2ee_recovery_request_debug_redacts_passphrase() {
    let request = SetupE2eeRecoveryRequest {
        passphrase: Some("test passphrase".to_owned()),
        wait_for_backups_to_upload: true,
    };

    let debug = format!("{request:?}");
    assert!(!debug.contains("test passphrase"));
    assert!(debug.contains("Some(\"<redacted>\")"));
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
            topic: None,
            membership: RoomMembership::Join,
            encrypted: false,
            is_space: true,
            is_direct: false,
            direct_user_ids: Vec::new(),
            tags: Vec::new(),
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
            topic: None,
            membership: RoomMembership::Join,
            encrypted: true,
            is_space: false,
            is_direct: false,
            direct_user_ids: Vec::new(),
            tags: Vec::new(),
            space_children: Vec::new(),
        },
        RoomSummary {
            room_id: "!beta:example.org".to_owned(),
            display_name: "Beta".to_owned(),
            topic: None,
            membership: RoomMembership::Join,
            encrypted: false,
            is_space: false,
            is_direct: false,
            direct_user_ids: Vec::new(),
            tags: Vec::new(),
            space_children: Vec::new(),
        },
        RoomSummary {
            room_id: "!lobby:example.org".to_owned(),
            display_name: "Lobby".to_owned(),
            topic: None,
            membership: RoomMembership::Join,
            encrypted: false,
            is_space: false,
            is_direct: false,
            direct_user_ids: Vec::new(),
            tags: Vec::new(),
            space_children: Vec::new(),
        },
    ]);

    let json = serde_json::to_string(&event).expect("serialize synced rooms");
    assert_eq!(
        json,
        r#"{"SyncedRooms":[{"room_id":"!space:example.org","display_name":"Space","topic":null,"membership":"join","encrypted":false,"is_space":true,"is_direct":false,"direct_user_ids":[],"tags":[],"space_children":[{"room_id":"!alpha:example.org","suggested":true,"order":"aaa"},{"room_id":"!beta:example.org","suggested":false,"order":null}]},{"room_id":"!alpha:example.org","display_name":"Alpha","topic":null,"membership":"join","encrypted":true,"is_space":false,"is_direct":false,"direct_user_ids":[],"tags":[],"space_children":[]},{"room_id":"!beta:example.org","display_name":"Beta","topic":null,"membership":"join","encrypted":false,"is_space":false,"is_direct":false,"direct_user_ids":[],"tags":[],"space_children":[]},{"room_id":"!lobby:example.org","display_name":"Lobby","topic":null,"membership":"join","encrypted":false,"is_space":false,"is_direct":false,"direct_user_ids":[],"tags":[],"space_children":[]}]}"#
    );

    let restored: Event = serde_json::from_str(&json).expect("deserialize synced rooms");
    assert_eq!(restored, event);
}

#[test]
fn room_summary_and_space_child_reject_unknown_fields() {
    let room_error = serde_json::from_str::<Event>(
        r#"{"SyncedRooms":[{"room_id":"!space:example.org","display_name":"Space","topic":null,"membership":"join","encrypted":false,"is_space":true,"is_direct":false,"direct_user_ids":[],"tags":[],"space_children":[],"unexpected":true}]}"#,
    )
    .expect_err("room summary must reject unknown fields");
    assert!(
        room_error
            .to_string()
            .contains("unknown field `unexpected`, expected one of `room_id`, `display_name`, `topic`, `membership`, `encrypted`, `is_space`, `is_direct`, `direct_user_ids`, `tags`, `space_children`"),
        "unexpected room summary error: {room_error}"
    );

    let child_error = serde_json::from_str::<Event>(
        r#"{"SyncedRooms":[{"room_id":"!space:example.org","display_name":"Space","topic":null,"membership":"join","encrypted":false,"is_space":true,"is_direct":false,"direct_user_ids":[],"tags":[],"space_children":[{"room_id":"!alpha:example.org","suggested":true,"order":"aaa","unexpected":true}]}]}"#,
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
fn room_summary_requires_explicit_topic_field() {
    let error = serde_json::from_str::<Event>(
        r#"{"SyncedRooms":[{"room_id":"!ops:example.org","display_name":"Ops","membership":"join","encrypted":false,"is_space":false,"is_direct":false,"direct_user_ids":[],"tags":[],"space_children":[]}]}"#,
    )
    .expect_err("room summary without topic must be rejected");

    assert!(error.to_string().contains("missing field `topic`"));
}

#[test]
fn room_summary_requires_membership() {
    let error = serde_json::from_str::<Event>(
        r#"{"SyncedRooms":[{"room_id":"!space:example.org","display_name":"Space","topic":null,"encrypted":false,"is_space":true,"is_direct":false,"direct_user_ids":[],"tags":[],"space_children":[]}]}"#,
    )
    .expect_err("room summary must require membership");
    assert!(
        error.to_string().contains("missing field `membership`"),
        "unexpected room summary missing-membership error: {error}"
    );
}

#[test]
fn flat_room_summary_payload_round_trips_with_explicit_space_defaults() {
    let event = Event::SyncedRooms(vec![RoomSummary {
        room_id: "!ops:example.org".to_owned(),
        display_name: "Ops".to_owned(),
        topic: None,
        membership: RoomMembership::Join,
        encrypted: true,
        is_space: false,
        is_direct: false,
        direct_user_ids: Vec::new(),
        tags: Vec::new(),
        space_children: Vec::new(),
    }]);

    let json = serde_json::to_string(&event).expect("serialize flat synced rooms");
    assert_eq!(
        json,
        r#"{"SyncedRooms":[{"room_id":"!ops:example.org","display_name":"Ops","topic":null,"membership":"join","encrypted":true,"is_space":false,"is_direct":false,"direct_user_ids":[],"tags":[],"space_children":[]}]}"#
    );

    let restored: Event = serde_json::from_str(&json).expect("deserialize flat synced rooms");
    assert_eq!(restored, event);
}

#[test]
fn synced_rooms_event_round_trips_account_data_metadata_payload() {
    let event = Event::SyncedRooms(vec![RoomSummary {
        room_id: "!dm:example.org".to_owned(),
        display_name: "Alice".to_owned(),
        topic: None,
        membership: RoomMembership::Join,
        encrypted: true,
        is_space: false,
        is_direct: true,
        direct_user_ids: vec!["@alice:example.org".to_owned()],
        tags: vec![
            RoomTagSummary {
                tag: "m.favourite".to_owned(),
                order: Some("0.25".to_owned()),
            },
            RoomTagSummary {
                tag: "u.work".to_owned(),
                order: None,
            },
        ],
        space_children: Vec::new(),
    }]);

    let json = serde_json::to_string(&event).expect("serialize synced rooms");
    assert_eq!(
        json,
        r#"{"SyncedRooms":[{"room_id":"!dm:example.org","display_name":"Alice","topic":null,"membership":"join","encrypted":true,"is_space":false,"is_direct":true,"direct_user_ids":["@alice:example.org"],"tags":[{"tag":"m.favourite","order":"0.25"},{"tag":"u.work","order":null}],"space_children":[]}]}"#
    );

    let restored: Event = serde_json::from_str(&json).expect("deserialize synced rooms");
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
    let message_event = Event::RoomActivity(Box::new(RoomActivity {
        room_id: "!ops:example.org".to_owned(),
        activity_id: "event:$event:example.org".to_owned(),
        timestamp_ms: 1_700_000_000_000,
        relation: None,
        kind: RoomActivityKind::Message(RoomMessageActivity {
            event_id: Some("$event:example.org".to_owned()),
            transaction_id: Some("txn-1".to_owned()),
            sender: "@alice:example.org".to_owned(),
            presentation: RoomMessagePresentationKind::Text,
            body: "hello".to_owned(),
            send_state: RoomMessageSendState::Sent,
        }),
    }));
    let pending_message_event = Event::RoomActivity(Box::new(RoomActivity {
        room_id: "!ops:example.org".to_owned(),
        activity_id: "txn:txn-1".to_owned(),
        timestamp_ms: 1_700_000_000_000,
        relation: None,
        kind: RoomActivityKind::Message(RoomMessageActivity {
            event_id: None,
            transaction_id: Some("txn-1".to_owned()),
            sender: "@alice:example.org".to_owned(),
            presentation: RoomMessagePresentationKind::Text,
            body: "hello".to_owned(),
            send_state: RoomMessageSendState::Pending,
        }),
    }));
    let state_notice_event = Event::RoomActivity(Box::new(RoomActivity {
        room_id: "!ops:example.org".to_owned(),
        activity_id: "state:$topic:example.org".to_owned(),
        timestamp_ms: 1_700_000_000_001,
        relation: None,
        kind: RoomActivityKind::StateNotice(RoomStateNoticeActivity {
            event_id: "$topic:example.org".to_owned(),
            sender: "@alice:example.org".to_owned(),
            state_key: String::new(),
            content: RoomStateNoticeContent::Topic {
                topic: "incident review".to_owned(),
            },
        }),
    }));
    let member_notice_event = Event::RoomActivity(Box::new(RoomActivity {
        room_id: "!ops:example.org".to_owned(),
        activity_id: "state:$member:example.org".to_owned(),
        timestamp_ms: 1_700_000_000_002,
        relation: None,
        kind: RoomActivityKind::StateNotice(RoomStateNoticeActivity {
            event_id: "$member:example.org".to_owned(),
            sender: "@alice:example.org".to_owned(),
            state_key: "@carol:example.org".to_owned(),
            content: RoomStateNoticeContent::Member {
                membership: "join".to_owned(),
                displayname: Some("carol".to_owned()),
            },
        }),
    }));
    let image_event = Event::RoomActivity(Box::new(RoomActivity {
        room_id: "!ops:example.org".to_owned(),
        activity_id: "event:$image1:example.org".to_owned(),
        timestamp_ms: 1_700_000_000_003,
        relation: None,
        kind: RoomActivityKind::Image(Box::new(RoomImageActivity {
            event_id: "$image1:example.org".to_owned(),
            sender: "@alice:example.org".to_owned(),
            body: "diagram.png".to_owned(),
            filename: Some("Incident diagram.png".to_owned()),
            source: RoomMediaSource::Plain {
                url: "mxc://example.org/image".to_owned(),
            },
            info: Some(RoomImageInfo {
                mimetype: Some("image/png".to_owned()),
                size: Some(4096),
                width: Some(800),
                height: Some(600),
                is_animated: Some(false),
                thumbnail_source: Some(RoomMediaSource::Plain {
                    url: "mxc://example.org/thumb".to_owned(),
                }),
            }),
        })),
    }));
    let encrypted_image_event = Event::RoomActivity(Box::new(RoomActivity {
        room_id: "!ops:example.org".to_owned(),
        activity_id: "event:$image2:example.org".to_owned(),
        timestamp_ms: 1_700_000_000_004,
        relation: None,
        kind: RoomActivityKind::Image(Box::new(RoomImageActivity {
            event_id: "$image2:example.org".to_owned(),
            sender: "@alice:example.org".to_owned(),
            body: "secret.png".to_owned(),
            filename: None,
            source: encrypted_media_source("mxc://example.org/encrypted-image"),
            info: Some(RoomImageInfo {
                mimetype: Some("image/png".to_owned()),
                size: Some(2048),
                width: Some(320),
                height: Some(240),
                is_animated: Some(false),
                thumbnail_source: Some(encrypted_media_source("mxc://example.org/encrypted-thumb")),
            }),
        })),
    }));
    let file_event = Event::RoomActivity(Box::new(RoomActivity {
        room_id: "!ops:example.org".to_owned(),
        activity_id: "event:$file1:example.org".to_owned(),
        timestamp_ms: 1_700_000_000_005,
        relation: None,
        kind: RoomActivityKind::File(Box::new(RoomFileActivity {
            event_id: "$file1:example.org".to_owned(),
            sender: "@alice:example.org".to_owned(),
            body: "Quarterly report".to_owned(),
            filename: Some("report.pdf".to_owned()),
            source: RoomMediaSource::Plain {
                url: "mxc://example.org/file".to_owned(),
            },
            info: Some(RoomFileInfo {
                mimetype: Some("application/pdf".to_owned()),
                size: Some(65_536),
            }),
        })),
    }));
    let audio_event = Event::RoomActivity(Box::new(RoomActivity {
        room_id: "!ops:example.org".to_owned(),
        activity_id: "event:$audio1:example.org".to_owned(),
        timestamp_ms: 1_700_000_000_006,
        relation: None,
        kind: RoomActivityKind::Audio(Box::new(RoomAudioActivity {
            event_id: "$audio1:example.org".to_owned(),
            sender: "@alice:example.org".to_owned(),
            body: "Voice note".to_owned(),
            filename: Some("voice-note.ogg".to_owned()),
            source: RoomMediaSource::Plain {
                url: "mxc://example.org/audio".to_owned(),
            },
            info: Some(RoomAudioInfo {
                mimetype: Some("audio/ogg".to_owned()),
                size: Some(8192),
                duration_ms: Some(12_000),
            }),
        })),
    }));
    let unsupported_message_event = Event::RoomActivity(Box::new(RoomActivity {
        room_id: "!ops:example.org".to_owned(),
        activity_id: "event:$custom1:example.org".to_owned(),
        timestamp_ms: 1_700_000_000_007,
        relation: None,
        kind: RoomActivityKind::UnsupportedMessage(UnsupportedRoomMessageActivity {
            event_id: "$custom1:example.org".to_owned(),
            sender: "@alice:example.org".to_owned(),
            msgtype: "com.example.custom".to_owned(),
            body: "Custom body".to_owned(),
        }),
    }));
    let redacted_event = Event::RoomActivity(Box::new(RoomActivity {
        room_id: "!ops:example.org".to_owned(),
        activity_id: "event:$redacted:example.org".to_owned(),
        timestamp_ms: 1_700_000_000_008,
        relation: None,
        kind: RoomActivityKind::Redacted(RoomRedactedActivity {
            event_id: "$redacted:example.org".to_owned(),
        }),
    }));
    let sent_update_event = Event::RoomActivityUpdate(RoomActivityUpdate {
        room_id: "!ops:example.org".to_owned(),
        target: RoomActivityUpdateTarget::Transaction {
            transaction_id: "txn-1".to_owned(),
        },
        kind: RoomActivityUpdateKind::MessageSent {
            event_id: "$event:example.org".to_owned(),
        },
    });
    let failed_update_event = Event::RoomActivityUpdate(RoomActivityUpdate {
        room_id: "!ops:example.org".to_owned(),
        target: RoomActivityUpdateTarget::Transaction {
            transaction_id: "txn-1".to_owned(),
        },
        kind: RoomActivityUpdateKind::MessageFailed {
            message: "send failed".to_owned(),
            recoverable: true,
        },
    });
    let redacted_update_event = Event::RoomActivityUpdate(RoomActivityUpdate {
        room_id: "!ops:example.org".to_owned(),
        target: RoomActivityUpdateTarget::Event {
            event_id: "$original:example.org".to_owned(),
        },
        kind: RoomActivityUpdateKind::ActivityRedacted {
            redaction_event_id: "$redaction:example.org".to_owned(),
        },
    });
    let reaction_added_update_event = Event::RoomActivityUpdate(RoomActivityUpdate {
        room_id: "!ops:example.org".to_owned(),
        target: RoomActivityUpdateTarget::Event {
            event_id: "$original:example.org".to_owned(),
        },
        kind: RoomActivityUpdateKind::ReactionAdded {
            reaction_event_id: "$reaction:example.org".to_owned(),
            sender: "@alice:example.org".to_owned(),
            timestamp_ms: 1_700_000_001_500,
            key: "+1".to_owned(),
        },
    });
    let reaction_removed_update_event = Event::RoomActivityUpdate(RoomActivityUpdate {
        room_id: "!ops:example.org".to_owned(),
        target: RoomActivityUpdateTarget::Event {
            event_id: "$original:example.org".to_owned(),
        },
        kind: RoomActivityUpdateKind::ReactionRemoved {
            reaction_event_id: "$reaction:example.org".to_owned(),
            redaction_event_id: "$redaction:example.org".to_owned(),
            key: "+1".to_owned(),
        },
    });

    for event in [
        message_event,
        pending_message_event,
        state_notice_event,
        member_notice_event,
        image_event,
        encrypted_image_event,
        file_event,
        audio_event,
        unsupported_message_event,
        redacted_event,
        sent_update_event,
        failed_update_event,
        redacted_update_event,
        reaction_added_update_event,
        reaction_removed_update_event,
    ] {
        let json = serde_json::to_string(&event).expect("serialize room activity event");
        let restored: Event = serde_json::from_str(&json).expect("deserialize room activity event");
        assert_eq!(restored, event);
    }
}

#[test]
fn room_activity_events_serialize_with_tagged_wire_shape() {
    let pending_message_event = Event::RoomActivity(Box::new(RoomActivity {
        room_id: "!ops:example.org".to_owned(),
        activity_id: "txn:txn-1".to_owned(),
        timestamp_ms: 1_700_000_000_000,
        relation: None,
        kind: RoomActivityKind::Message(RoomMessageActivity {
            event_id: None,
            transaction_id: Some("txn-1".to_owned()),
            sender: "@alice:example.org".to_owned(),
            presentation: RoomMessagePresentationKind::Text,
            body: "hello".to_owned(),
            send_state: RoomMessageSendState::Pending,
        }),
    }));
    let member_notice_event = Event::RoomActivity(Box::new(RoomActivity {
        room_id: "!ops:example.org".to_owned(),
        activity_id: "state:$member:example.org".to_owned(),
        timestamp_ms: 1_700_000_000_002,
        relation: None,
        kind: RoomActivityKind::StateNotice(RoomStateNoticeActivity {
            event_id: "$member:example.org".to_owned(),
            sender: "@alice:example.org".to_owned(),
            state_key: "@carol:example.org".to_owned(),
            content: RoomStateNoticeContent::Member {
                membership: "join".to_owned(),
                displayname: Some("carol".to_owned()),
            },
        }),
    }));
    let image_event = Event::RoomActivity(Box::new(RoomActivity {
        room_id: "!ops:example.org".to_owned(),
        activity_id: "event:$image1:example.org".to_owned(),
        timestamp_ms: 1_700_000_000_003,
        relation: None,
        kind: RoomActivityKind::Image(Box::new(RoomImageActivity {
            event_id: "$image1:example.org".to_owned(),
            sender: "@alice:example.org".to_owned(),
            body: "diagram.png".to_owned(),
            filename: Some("Incident diagram.png".to_owned()),
            source: RoomMediaSource::Plain {
                url: "mxc://example.org/image".to_owned(),
            },
            info: Some(RoomImageInfo {
                mimetype: Some("image/png".to_owned()),
                size: Some(4096),
                width: Some(800),
                height: Some(600),
                is_animated: Some(false),
                thumbnail_source: None,
            }),
        })),
    }));
    let file_event = Event::RoomActivity(Box::new(RoomActivity {
        room_id: "!ops:example.org".to_owned(),
        activity_id: "event:$file1:example.org".to_owned(),
        timestamp_ms: 1_700_000_000_004,
        relation: None,
        kind: RoomActivityKind::File(Box::new(RoomFileActivity {
            event_id: "$file1:example.org".to_owned(),
            sender: "@alice:example.org".to_owned(),
            body: "Quarterly report".to_owned(),
            filename: Some("report.pdf".to_owned()),
            source: RoomMediaSource::Plain {
                url: "mxc://example.org/file".to_owned(),
            },
            info: Some(RoomFileInfo {
                mimetype: Some("application/pdf".to_owned()),
                size: Some(65_536),
            }),
        })),
    }));
    let audio_event = Event::RoomActivity(Box::new(RoomActivity {
        room_id: "!ops:example.org".to_owned(),
        activity_id: "event:$audio1:example.org".to_owned(),
        timestamp_ms: 1_700_000_000_005,
        relation: None,
        kind: RoomActivityKind::Audio(Box::new(RoomAudioActivity {
            event_id: "$audio1:example.org".to_owned(),
            sender: "@alice:example.org".to_owned(),
            body: "Voice note".to_owned(),
            filename: Some("voice-note.ogg".to_owned()),
            source: RoomMediaSource::Plain {
                url: "mxc://example.org/audio".to_owned(),
            },
            info: Some(RoomAudioInfo {
                mimetype: Some("audio/ogg".to_owned()),
                size: Some(8192),
                duration_ms: Some(12_000),
            }),
        })),
    }));
    let unsupported_message_event = Event::RoomActivity(Box::new(RoomActivity {
        room_id: "!ops:example.org".to_owned(),
        activity_id: "event:$custom1:example.org".to_owned(),
        timestamp_ms: 1_700_000_000_006,
        relation: None,
        kind: RoomActivityKind::UnsupportedMessage(UnsupportedRoomMessageActivity {
            event_id: "$custom1:example.org".to_owned(),
            sender: "@alice:example.org".to_owned(),
            msgtype: "com.example.custom".to_owned(),
            body: "Custom body".to_owned(),
        }),
    }));
    let redacted_event = Event::RoomActivity(Box::new(RoomActivity {
        room_id: "!ops:example.org".to_owned(),
        activity_id: "event:$redacted:example.org".to_owned(),
        timestamp_ms: 1_700_000_000_007,
        relation: None,
        kind: RoomActivityKind::Redacted(RoomRedactedActivity {
            event_id: "$redacted:example.org".to_owned(),
        }),
    }));
    let failed_update_event = Event::RoomActivityUpdate(RoomActivityUpdate {
        room_id: "!ops:example.org".to_owned(),
        target: RoomActivityUpdateTarget::Transaction {
            transaction_id: "txn-1".to_owned(),
        },
        kind: RoomActivityUpdateKind::MessageFailed {
            message: "send failed".to_owned(),
            recoverable: true,
        },
    });
    let replaced_update_event = Event::RoomActivityUpdate(RoomActivityUpdate {
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
    });
    let redacted_update_event = Event::RoomActivityUpdate(RoomActivityUpdate {
        room_id: "!ops:example.org".to_owned(),
        target: RoomActivityUpdateTarget::Event {
            event_id: "$original:example.org".to_owned(),
        },
        kind: RoomActivityUpdateKind::ActivityRedacted {
            redaction_event_id: "$redaction:example.org".to_owned(),
        },
    });
    let reaction_added_update_event = Event::RoomActivityUpdate(RoomActivityUpdate {
        room_id: "!ops:example.org".to_owned(),
        target: RoomActivityUpdateTarget::Event {
            event_id: "$original:example.org".to_owned(),
        },
        kind: RoomActivityUpdateKind::ReactionAdded {
            reaction_event_id: "$reaction:example.org".to_owned(),
            sender: "@alice:example.org".to_owned(),
            timestamp_ms: 1_700_000_001_500,
            key: "+1".to_owned(),
        },
    });
    let reaction_removed_update_event = Event::RoomActivityUpdate(RoomActivityUpdate {
        room_id: "!ops:example.org".to_owned(),
        target: RoomActivityUpdateTarget::Event {
            event_id: "$original:example.org".to_owned(),
        },
        kind: RoomActivityUpdateKind::ReactionRemoved {
            reaction_event_id: "$reaction:example.org".to_owned(),
            redaction_event_id: "$redaction:example.org".to_owned(),
            key: "+1".to_owned(),
        },
    });

    assert_eq!(
        serde_json::to_string(&pending_message_event).expect("serialize pending room activity"),
        r#"{"RoomActivity":{"room_id":"!ops:example.org","activity_id":"txn:txn-1","timestamp_ms":1700000000000,"relation":null,"kind":{"type":"message","event_id":null,"transaction_id":"txn-1","sender":"@alice:example.org","presentation":"text","body":"hello","send_state":{"state":"pending"}}}}"#
    );
    assert_eq!(
        serde_json::to_string(&member_notice_event).expect("serialize member room activity"),
        r#"{"RoomActivity":{"room_id":"!ops:example.org","activity_id":"state:$member:example.org","timestamp_ms":1700000000002,"relation":null,"kind":{"type":"state_notice","event_id":"$member:example.org","sender":"@alice:example.org","state_key":"@carol:example.org","content":{"type":"member","membership":"join","displayname":"carol"}}}}"#
    );
    assert_eq!(
        serde_json::to_string(&image_event).expect("serialize image room activity"),
        r#"{"RoomActivity":{"room_id":"!ops:example.org","activity_id":"event:$image1:example.org","timestamp_ms":1700000000003,"relation":null,"kind":{"type":"image","event_id":"$image1:example.org","sender":"@alice:example.org","body":"diagram.png","filename":"Incident diagram.png","source":{"type":"plain","url":"mxc://example.org/image"},"info":{"mimetype":"image/png","size":4096,"width":800,"height":600,"is_animated":false,"thumbnail_source":null}}}}"#
    );
    assert_eq!(
        serde_json::to_string(&file_event).expect("serialize file room activity"),
        r#"{"RoomActivity":{"room_id":"!ops:example.org","activity_id":"event:$file1:example.org","timestamp_ms":1700000000004,"relation":null,"kind":{"type":"file","event_id":"$file1:example.org","sender":"@alice:example.org","body":"Quarterly report","filename":"report.pdf","source":{"type":"plain","url":"mxc://example.org/file"},"info":{"mimetype":"application/pdf","size":65536}}}}"#
    );
    assert_eq!(
        serde_json::to_string(&audio_event).expect("serialize audio room activity"),
        r#"{"RoomActivity":{"room_id":"!ops:example.org","activity_id":"event:$audio1:example.org","timestamp_ms":1700000000005,"relation":null,"kind":{"type":"audio","event_id":"$audio1:example.org","sender":"@alice:example.org","body":"Voice note","filename":"voice-note.ogg","source":{"type":"plain","url":"mxc://example.org/audio"},"info":{"mimetype":"audio/ogg","size":8192,"duration_ms":12000}}}}"#
    );
    assert_eq!(
        serde_json::to_string(&unsupported_message_event)
            .expect("serialize unsupported room activity"),
        r#"{"RoomActivity":{"room_id":"!ops:example.org","activity_id":"event:$custom1:example.org","timestamp_ms":1700000000006,"relation":null,"kind":{"type":"unsupported_message","event_id":"$custom1:example.org","sender":"@alice:example.org","msgtype":"com.example.custom","body":"Custom body"}}}"#
    );
    assert_eq!(
        serde_json::to_string(&redacted_event).expect("serialize redacted room activity"),
        r#"{"RoomActivity":{"room_id":"!ops:example.org","activity_id":"event:$redacted:example.org","timestamp_ms":1700000000007,"relation":null,"kind":{"type":"redacted","event_id":"$redacted:example.org"}}}"#
    );
    assert_eq!(
        serde_json::to_string(&failed_update_event).expect("serialize failed room activity update"),
        r#"{"RoomActivityUpdate":{"room_id":"!ops:example.org","target":{"type":"transaction","transaction_id":"txn-1"},"kind":{"type":"message_failed","message":"send failed","recoverable":true}}}"#
    );
    assert_eq!(
        serde_json::to_string(&replaced_update_event)
            .expect("serialize replaced room activity update"),
        r#"{"RoomActivityUpdate":{"room_id":"!ops:example.org","target":{"type":"event","event_id":"$original:example.org"},"kind":{"type":"message_replaced","replacement_event_id":"$edit:example.org","sender":"@alice:example.org","timestamp_ms":1700000001000,"presentation":"text","body":"edited body"}}}"#
    );
    assert_eq!(
        serde_json::to_string(&redacted_update_event)
            .expect("serialize redacted room activity update"),
        r#"{"RoomActivityUpdate":{"room_id":"!ops:example.org","target":{"type":"event","event_id":"$original:example.org"},"kind":{"type":"activity_redacted","redaction_event_id":"$redaction:example.org"}}}"#
    );
    assert_eq!(
        serde_json::to_string(&reaction_added_update_event)
            .expect("serialize reaction added room activity update"),
        r#"{"RoomActivityUpdate":{"room_id":"!ops:example.org","target":{"type":"event","event_id":"$original:example.org"},"kind":{"type":"reaction_added","reaction_event_id":"$reaction:example.org","sender":"@alice:example.org","timestamp_ms":1700000001500,"key":"+1"}}}"#
    );
    assert_eq!(
        serde_json::to_string(&reaction_removed_update_event)
            .expect("serialize reaction removed room activity update"),
        r#"{"RoomActivityUpdate":{"room_id":"!ops:example.org","target":{"type":"event","event_id":"$original:example.org"},"kind":{"type":"reaction_removed","reaction_event_id":"$reaction:example.org","redaction_event_id":"$redaction:example.org","key":"+1"}}}"#
    );
}

#[test]
fn redacted_room_activity_round_trips_through_json() {
    let activity = RoomActivityKind::Redacted(RoomRedactedActivity {
        event_id: "$redacted:example.org".to_owned(),
    });
    let json = serde_json::json!({
        "type": "redacted",
        "event_id": "$redacted:example.org",
    });

    assert_eq!(
        serde_json::to_value(&activity).expect("serialize redacted activity"),
        json
    );
    assert_eq!(
        serde_json::from_value::<RoomActivityKind>(json).expect("deserialize redacted activity"),
        activity
    );
}

#[test]
fn activity_redacted_update_round_trips_through_json() {
    let update = RoomActivityUpdateKind::ActivityRedacted {
        redaction_event_id: "$redaction:example.org".to_owned(),
    };
    let json = serde_json::json!({
        "type": "activity_redacted",
        "redaction_event_id": "$redaction:example.org",
    });

    assert_eq!(
        serde_json::to_value(&update).expect("serialize activity redacted update"),
        json
    );
    assert_eq!(
        serde_json::from_value::<RoomActivityUpdateKind>(json)
            .expect("deserialize activity redacted update"),
        update
    );
}

#[test]
fn reaction_added_update_round_trips_through_json() {
    let update = RoomActivityUpdateKind::ReactionAdded {
        reaction_event_id: "$reaction:example.org".to_owned(),
        sender: "@alice:example.org".to_owned(),
        timestamp_ms: 1_700_000_001_500,
        key: "+1".to_owned(),
    };
    let json = serde_json::json!({
        "type": "reaction_added",
        "reaction_event_id": "$reaction:example.org",
        "sender": "@alice:example.org",
        "timestamp_ms": 1700000001500u64,
        "key": "+1",
    });

    assert_eq!(
        serde_json::to_value(&update).expect("serialize reaction added update"),
        json
    );
    assert_eq!(
        serde_json::from_value::<RoomActivityUpdateKind>(json)
            .expect("deserialize reaction added update"),
        update
    );
}

#[test]
fn reaction_removed_update_round_trips_through_json() {
    let update = RoomActivityUpdateKind::ReactionRemoved {
        reaction_event_id: "$reaction:example.org".to_owned(),
        redaction_event_id: "$redaction:example.org".to_owned(),
        key: "+1".to_owned(),
    };
    let json = serde_json::json!({
        "type": "reaction_removed",
        "reaction_event_id": "$reaction:example.org",
        "redaction_event_id": "$redaction:example.org",
        "key": "+1",
    });

    assert_eq!(
        serde_json::to_value(&update).expect("serialize reaction removed update"),
        json
    );
    assert_eq!(
        serde_json::from_value::<RoomActivityUpdateKind>(json)
            .expect("deserialize reaction removed update"),
        update
    );
}

#[test]
fn redacted_room_activity_rejects_missing_or_extra_fields() {
    assert!(
        serde_json::from_str::<RoomRedactedActivity>(r#"{}"#).is_err(),
        "redacted room activity must require event_id"
    );
    assert!(
        serde_json::from_str::<RoomRedactedActivity>(
            r#"{"event_id":"$redacted:example.org","unexpected":"value"}"#
        )
        .is_err(),
        "redacted room activity must reject unknown fields"
    );
}

#[test]
fn activity_redacted_update_rejects_missing_or_extra_fields() {
    assert!(
        serde_json::from_str::<RoomActivityUpdateKind>(r#"{"type":"activity_redacted"}"#).is_err(),
        "activity redacted update must require redaction_event_id"
    );
    assert!(
        serde_json::from_str::<RoomActivityUpdateKind>(
            r#"{"type":"activity_redacted","redaction_event_id":"$redaction:example.org","unexpected":"value"}"#
        )
        .is_err(),
        "activity redacted update must reject unknown fields"
    );
}

#[test]
fn reaction_added_update_rejects_missing_or_extra_fields() {
    assert!(
        serde_json::from_str::<RoomActivityUpdateKind>(
            r#"{"type":"reaction_added","reaction_event_id":"$reaction:example.org","sender":"@alice:example.org","key":"+1"}"#
        )
        .is_err(),
        "reaction added update must require timestamp_ms"
    );
    assert!(
        serde_json::from_str::<RoomActivityUpdateKind>(
            r#"{"type":"reaction_added","reaction_event_id":"$reaction:example.org","sender":"@alice:example.org","timestamp_ms":1700000001500,"key":"+1","unexpected":"value"}"#
        )
        .is_err(),
        "reaction added update must reject unknown fields"
    );
}

#[test]
fn reaction_removed_update_rejects_missing_or_extra_fields() {
    assert!(
        serde_json::from_str::<RoomActivityUpdateKind>(
            r#"{"type":"reaction_removed","reaction_event_id":"$reaction:example.org","redaction_event_id":"$redaction:example.org"}"#
        )
        .is_err(),
        "reaction removed update must require key"
    );
    assert!(
        serde_json::from_str::<RoomActivityUpdateKind>(
            r#"{"type":"reaction_removed","reaction_event_id":"$reaction:example.org","redaction_event_id":"$redaction:example.org","key":"+1","unexpected":"value"}"#
        )
        .is_err(),
        "reaction removed update must reject unknown fields"
    );
}

#[test]
fn room_history_backfill_event_round_trips_through_serde() {
    let event = Event::RoomHistoryBackfill(RoomHistoryBackfill {
        room_id: "!ops:example.org".to_owned(),
        start: "t4747337_219380_2603".to_owned(),
        end: Some("t4747337_219379_2603".to_owned()),
        events: vec![
            RoomHistoryBackfillEvent::Activity(Box::new(RoomActivity {
                room_id: "!ops:example.org".to_owned(),
                activity_id: "event:$message:example.org".to_owned(),
                timestamp_ms: 1_700_000_000_000,
                relation: None,
                kind: RoomActivityKind::Message(RoomMessageActivity {
                    event_id: Some("$message:example.org".to_owned()),
                    transaction_id: None,
                    sender: "@alice:example.org".to_owned(),
                    presentation: RoomMessagePresentationKind::Text,
                    body: "older message".to_owned(),
                    send_state: RoomMessageSendState::Sent,
                }),
            })),
            RoomHistoryBackfillEvent::ActivityUpdate(RoomActivityUpdate {
                room_id: "!ops:example.org".to_owned(),
                target: RoomActivityUpdateTarget::Event {
                    event_id: "$message:example.org".to_owned(),
                },
                kind: RoomActivityUpdateKind::ReactionAdded {
                    reaction_event_id: "$reaction:example.org".to_owned(),
                    sender: "@bob:example.org".to_owned(),
                    timestamp_ms: 1_700_000_000_100,
                    key: "heart".to_owned(),
                },
            }),
        ],
    });

    let json = serde_json::to_string(&event).expect("serialize room history backfill event");
    assert_eq!(
        json,
        r#"{"RoomHistoryBackfill":{"room_id":"!ops:example.org","start":"t4747337_219380_2603","end":"t4747337_219379_2603","events":[{"type":"activity","room_id":"!ops:example.org","activity_id":"event:$message:example.org","timestamp_ms":1700000000000,"relation":null,"kind":{"type":"message","event_id":"$message:example.org","transaction_id":null,"sender":"@alice:example.org","presentation":"text","body":"older message","send_state":{"state":"sent"}}},{"type":"activity_update","room_id":"!ops:example.org","target":{"type":"event","event_id":"$message:example.org"},"kind":{"type":"reaction_added","reaction_event_id":"$reaction:example.org","sender":"@bob:example.org","timestamp_ms":1700000000100,"key":"heart"}}]}}"#
    );

    let restored: Event =
        serde_json::from_str(&json).expect("deserialize room history backfill event");
    assert_eq!(restored, event);
}

#[test]
fn room_history_backfill_rejects_unknown_fields() {
    assert!(
        serde_json::from_str::<RoomHistoryBackfill>(
            r#"{"room_id":"!room:example.org","start":"t1","end":null,"events":[],"unexpected":"value"}"#
        )
        .is_err(),
        "room history backfill must reject unknown fields"
    );
}

#[test]
fn room_history_backfill_rejects_invalid_nested_event_tag() {
    assert!(
        serde_json::from_str::<RoomHistoryBackfill>(
            r#"{"room_id":"!room:example.org","start":"t1","end":null,"events":[{"type":"unknown","room_id":"!room:example.org"}]}"#
        )
        .is_err(),
        "room history backfill must reject events with unknown tags"
    );
}

#[test]
fn room_history_backfill_event_rejects_unknown_fields() {
    assert!(
        serde_json::from_str::<RoomHistoryBackfillEvent>(
            r#"{"type":"activity","room_id":"!room:example.org","activity_id":"event:$event:example.org","timestamp_ms":1700000000000,"relation":null,"kind":{"type":"message","event_id":"$event:example.org","transaction_id":null,"sender":"@alice:example.org","presentation":"text","body":"hello","send_state":{"state":"sent"}},"unexpected":"value"}"#
        )
        .is_err(),
        "room history backfill event must reject unknown fields"
    );
}

#[test]
fn room_history_backfill_event_rejects_invalid_tag() {
    assert!(
        serde_json::from_str::<RoomHistoryBackfillEvent>(
            r#"{"type":"unknown","room_id":"!room:example.org"}"#
        )
        .is_err(),
        "room history backfill event must reject unknown tags"
    );
}

#[test]
fn room_media_events_round_trip_through_serde() {
    let loaded_event = Event::RoomMedia(RoomMedia {
        room_id: "!ops:example.org".to_owned(),
        activity_id: "event:$image:example.org".to_owned(),
        event_id: "$image:example.org".to_owned(),
        kind: RoomMediaKind::Loaded {
            format: RoomMediaFormat::Thumbnail {
                width: 160,
                height: 120,
            },
            bytes: vec![1, 2, 3],
        },
    });
    let failed_event = Event::RoomMedia(RoomMedia {
        room_id: "!ops:example.org".to_owned(),
        activity_id: "event:$image:example.org".to_owned(),
        event_id: "$image:example.org".to_owned(),
        kind: RoomMediaKind::Failed {
            format: RoomMediaFormat::File,
            message: "failed to fetch room media: unavailable".to_owned(),
        },
    });

    assert_eq!(
        serde_json::to_string(&loaded_event).expect("serialize loaded room media"),
        r#"{"RoomMedia":{"room_id":"!ops:example.org","activity_id":"event:$image:example.org","event_id":"$image:example.org","kind":{"type":"loaded","format":{"type":"thumbnail","width":160,"height":120},"bytes":[1,2,3]}}}"#
    );
    assert_eq!(
        serde_json::to_string(&failed_event).expect("serialize failed room media"),
        r#"{"RoomMedia":{"room_id":"!ops:example.org","activity_id":"event:$image:example.org","event_id":"$image:example.org","kind":{"type":"failed","format":{"type":"file"},"message":"failed to fetch room media: unavailable"}}}"#
    );

    for event in [loaded_event, failed_event] {
        let json = serde_json::to_string(&event).expect("serialize room media event");
        let restored: Event = serde_json::from_str(&json).expect("deserialize room media event");
        assert_eq!(restored, event);
    }
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
fn room_members_event_round_trips_multiple_members_and_allows_empty_members() {
    let populated_event = Event::RoomMembers(RoomMembers {
        room_id: "!ops:example.org".to_owned(),
        members: vec![
            RoomMemberSummary {
                user_id: "@alice:example.org".to_owned(),
                display_name: Some("Alice".to_owned()),
                can_kick: true,
                can_ban: false,
            },
            RoomMemberSummary {
                user_id: "@bob:example.org".to_owned(),
                display_name: None,
                can_kick: false,
                can_ban: true,
            },
        ],
    });

    let populated_json = serde_json::to_string(&populated_event).expect("serialize room members");
    assert_eq!(
        populated_json,
        r#"{"RoomMembers":{"room_id":"!ops:example.org","members":[{"user_id":"@alice:example.org","display_name":"Alice","can_kick":true,"can_ban":false},{"user_id":"@bob:example.org","display_name":null,"can_kick":false,"can_ban":true}]}}"#
    );

    let restored: Event =
        serde_json::from_str(&populated_json).expect("deserialize populated room members");
    assert_eq!(restored, populated_event);

    let empty_event = Event::RoomMembers(RoomMembers {
        room_id: "!empty:example.org".to_owned(),
        members: Vec::new(),
    });

    let empty_json = serde_json::to_string(&empty_event).expect("serialize empty room members");
    assert_eq!(
        empty_json,
        r#"{"RoomMembers":{"room_id":"!empty:example.org","members":[]}}"#
    );

    let restored: Event =
        serde_json::from_str(&empty_json).expect("deserialize empty room members");
    assert_eq!(restored, empty_event);
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

    let restored: Event = serde_json::from_str(&json).expect("deserialize room read state event");
    assert_eq!(restored, event);
}

#[test]
fn e2ee_status_event_round_trips_through_serde() {
    let event = Event::E2eeStatus(E2eeStatus {
        recovery: E2eeRecoveryState::Enabled,
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
    });

    let json = serde_json::to_string(&event).expect("serialize e2ee status event");
    assert_eq!(
        json,
        r#"{"E2eeStatus":{"recovery":"enabled","backup":{"state":"enabled","enabled_locally":true,"exists_on_server":true},"cross_signing":{"has_master":true,"has_self_signing":true,"has_user_signing":false,"complete":false}}}"#
    );

    let restored: Event = serde_json::from_str(&json).expect("deserialize e2ee status event");
    assert_eq!(restored, event);
}

#[test]
fn e2ee_recovery_succeeded_event_round_trips_through_serde() {
    let event = Event::E2eeRecoverySucceeded(E2eeRecoverySucceeded {
        operation: E2eeRecoveryOperation::Recover,
    });

    let json = serde_json::to_string(&event).expect("serialize e2ee recovery succeeded");
    assert_eq!(json, r#"{"E2eeRecoverySucceeded":{"operation":"recover"}}"#);

    let restored: Event = serde_json::from_str(&json).expect("deserialize e2ee recovery succeeded");
    assert_eq!(restored, event);
}

#[test]
fn e2ee_recovery_setup_succeeded_event_round_trips_through_serde() {
    let event = Event::E2eeRecoverySetupSucceeded(E2eeRecoverySetupSucceeded {
        recovery_key: "EsTl...recovery-key".to_owned(),
    });

    let json = serde_json::to_string(&event).expect("serialize e2ee recovery setup succeeded");
    assert_eq!(
        json,
        r#"{"E2eeRecoverySetupSucceeded":{"recovery_key":"EsTl...recovery-key"}}"#
    );

    let restored: Event =
        serde_json::from_str(&json).expect("deserialize e2ee recovery setup succeeded");
    assert_eq!(restored, event);
}

#[test]
fn e2ee_recovery_setup_succeeded_debug_redacts_recovery_key() {
    let succeeded = E2eeRecoverySetupSucceeded {
        recovery_key: "EsTl...recovery-key".to_owned(),
    };

    let debug = format!("{succeeded:?}");
    assert!(!debug.contains("EsTl...recovery-key"));
    assert!(debug.contains("<redacted>"));
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
fn e2ee_dtos_reject_unknown_fields() {
    assert!(
        serde_json::from_str::<RecoverE2eeRequest>(
            r#"{"recovery_secret":"EsTl...secret","unexpected":"value"}"#
        )
        .is_err(),
        "recover e2ee request must reject unknown fields"
    );

    assert!(
        serde_json::from_str::<SetupE2eeRecoveryRequest>(
            r#"{"passphrase":"test passphrase","wait_for_backups_to_upload":false,"unexpected":"value"}"#
        )
        .is_err(),
        "setup e2ee recovery request must reject unknown fields"
    );

    assert!(
        serde_json::from_str::<Event>(
            r#"{"E2eeRecoverySetupSucceeded":{"recovery_key":"EsTl...recovery-key","unexpected":"value"}}"#
        )
        .is_err(),
        "e2ee recovery setup succeeded event must reject unknown fields"
    );

    assert!(
        serde_json::from_str::<Event>(
            r#"{"E2eeStatus":{"recovery":"enabled","backup":{"state":"enabled","enabled_locally":true,"exists_on_server":true,"unexpected":"value"},"cross_signing":{"has_master":true,"has_self_signing":true,"has_user_signing":true,"complete":true}}}"#
        )
        .is_err(),
        "e2ee backup status must reject unknown fields"
    );

    assert!(
        serde_json::from_str::<Event>(
            r#"{"E2eeStatus":{"recovery":"enabled","backup":{"state":"enabled","enabled_locally":true,"exists_on_server":true},"cross_signing":{"has_master":true,"has_self_signing":true,"has_user_signing":true,"complete":true,"unexpected":"value"}}}"#
        )
        .is_err(),
        "e2ee cross-signing status must reject unknown fields"
    );
}

#[test]
fn join_room_request_rejects_unknown_fields() {
    assert!(
        serde_json::from_str::<JoinRoomRequest>(
            r##"{"room_or_alias":"#ops:example.org","unexpected":"value"}"##
        )
        .is_err(),
        "join room request must reject unknown fields"
    );
}

#[test]
fn invite_request_dtos_reject_unknown_fields() {
    assert!(
        serde_json::from_str::<AcceptRoomInviteRequest>(
            r#"{"room_id":"!ops:example.org","unexpected":"value"}"#
        )
        .is_err(),
        "accept room invite request must reject unknown fields"
    );

    assert!(
        serde_json::from_str::<RejectRoomInviteRequest>(
            r#"{"room_id":"!ops:example.org","unexpected":"value"}"#
        )
        .is_err(),
        "reject room invite request must reject unknown fields"
    );
}

#[test]
fn fetch_room_media_dtos_reject_unknown_fields() {
    assert!(
        serde_json::from_str::<FetchRoomMediaRequest>(
            r#"{"room_id":"!ops:example.org","activity_id":"event:$image:example.org","event_id":"$image:example.org","source":{"type":"plain","url":"mxc://example.org/image"},"format":{"type":"thumbnail","width":160,"height":120},"unexpected":"value"}"#
        )
        .is_err(),
        "fetch room media request must reject unknown fields"
    );

    assert!(
        serde_json::from_str::<RoomMedia>(
            r#"{"room_id":"!ops:example.org","activity_id":"event:$image:example.org","event_id":"$image:example.org","kind":{"type":"loaded","format":{"type":"file"},"bytes":[1,2,3]},"unexpected":"value"}"#
        )
        .is_err(),
        "room media must reject unknown fields"
    );

    assert!(
        serde_json::from_str::<RoomMediaFormat>(
            r#"{"type":"thumbnail","width":160,"height":120,"unexpected":"value"}"#
        )
        .is_err(),
        "room media format must reject unknown fields"
    );

    assert_eq!(
        serde_json::from_str::<RoomMediaFormat>(r#"{"type":"file"}"#)
            .expect("deserialize file room media format"),
        RoomMediaFormat::File
    );
    assert!(
        serde_json::from_str::<RoomMediaFormat>(r#"{"type":"file","width":1}"#).is_err(),
        "file room media format must reject unknown fields"
    );

    assert!(
        serde_json::from_str::<RoomMediaKind>(
            r#"{"type":"failed","format":{"type":"file"},"message":"unavailable","unexpected":"value"}"#
        )
        .is_err(),
        "room media kind must reject unknown fields"
    );

    assert!(
        serde_json::from_str::<RoomMediaSource>(
            r#"{"type":"encrypted","url":"mxc://example.org/image"}"#
        )
        .is_err(),
        "encrypted room media source must reject the old url-only shape"
    );
}

#[test]
fn create_direct_room_request_rejects_unknown_fields() {
    assert!(
        serde_json::from_str::<CreateDirectRoomRequest>(
            r#"{"user_id":"@alice:example.org","unexpected":"value"}"#
        )
        .is_err(),
        "create direct room request must reject unknown fields"
    );
}

#[test]
fn leave_room_request_rejects_unknown_fields() {
    assert!(
        serde_json::from_str::<LeaveRoomRequest>(
            r#"{"room_id":"!ops:example.org","unexpected":"value"}"#
        )
        .is_err(),
        "leave room request must reject unknown fields"
    );
}

#[test]
fn reaction_request_dtos_reject_unknown_fields() {
    assert!(
        serde_json::from_str::<SendReactionRequest>(
            r#"{"room_id":"!ops:example.org","target_event_id":"$event:example.org","key":"+1","unexpected":"value"}"#
        )
        .is_err(),
        "send reaction request must reject unknown fields"
    );

    assert!(
        serde_json::from_str::<RedactEventRequest>(
            r#"{"room_id":"!ops:example.org","event_id":"$reaction:example.org","unexpected":"value"}"#
        )
        .is_err(),
        "redact event request must reject unknown fields"
    );
}

#[test]
fn edit_message_request_round_trips_through_json() {
    let request = EditMessageRequest {
        room_id: "!ops:example.org".to_owned(),
        target_event_id: "$event:example.org".to_owned(),
        presentation: RoomMessagePresentationKind::Notice,
        body: "updated incident note".to_owned(),
    };

    assert_eq!(
        serde_json::to_value(&request).expect("serialize edit message request"),
        serde_json::json!({
            "room_id": "!ops:example.org",
            "target_event_id": "$event:example.org",
            "presentation": "notice",
            "body": "updated incident note"
        })
    );
    assert_eq!(
        serde_json::from_value::<EditMessageRequest>(serde_json::json!({
            "room_id": "!ops:example.org",
            "target_event_id": "$event:example.org",
            "presentation": "notice",
            "body": "updated incident note"
        }))
        .expect("deserialize edit message request"),
        request
    );
}

#[test]
fn edit_message_request_rejects_unknown_fields() {
    assert!(
        serde_json::from_str::<EditMessageRequest>(
            r#"{"room_id":"!ops:example.org","target_event_id":"$event:example.org","presentation":"text","body":"edited body","unexpected":"value"}"#
        )
        .is_err(),
        "edit message request must reject unknown fields"
    );
}

#[test]
fn set_room_topic_request_round_trips_through_json() {
    let request = SetRoomTopicRequest {
        room_id: "!ops:example.org".to_owned(),
        topic: "incident review".to_owned(),
    };

    assert_eq!(
        serde_json::to_value(&request).expect("serialize set room topic request"),
        serde_json::json!({
            "room_id": "!ops:example.org",
            "topic": "incident review"
        })
    );
    assert_eq!(
        serde_json::from_value::<SetRoomTopicRequest>(serde_json::json!({
            "room_id": "!ops:example.org",
            "topic": "incident review"
        }))
        .expect("deserialize set room topic request"),
        request
    );
}

#[test]
fn set_room_topic_request_rejects_unknown_fields() {
    assert!(
        serde_json::from_str::<SetRoomTopicRequest>(
            r#"{"room_id":"!ops:example.org","topic":"incident review","unexpected":"value"}"#
        )
        .is_err(),
        "set room topic request must reject unknown fields"
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
        serde_json::from_str::<UnsupportedRoomMessageActivity>(
            r#"{"event_id":"$audio1:example.org","sender":"@alice:example.org","msgtype":"m.audio","body":"voice.ogg","unexpected":"value"}"#
        )
        .is_err(),
        "unsupported room message activity must reject unknown fields"
    );

    assert!(
        serde_json::from_str::<RoomImageActivity>(
            r#"{"event_id":"$image1:example.org","sender":"@alice:example.org","body":"diagram.png","filename":"Incident diagram.png","source":{"type":"plain","url":"mxc://example.org/image"},"info":{"mimetype":"image/png","size":4096,"width":800,"height":600,"is_animated":false},"unexpected":"value"}"#
        )
        .is_err(),
        "room image activity must reject unknown fields"
    );

    assert!(
        serde_json::from_str::<RoomFileActivity>(
            r#"{"event_id":"$file1:example.org","sender":"@alice:example.org","body":"Quarterly report","filename":"report.pdf","source":{"type":"plain","url":"mxc://example.org/file"},"info":{"mimetype":"application/pdf","size":65536},"unexpected":"value"}"#
        )
        .is_err(),
        "room file activity must reject unknown fields"
    );

    assert!(
        serde_json::from_str::<RoomAudioActivity>(
            r#"{"event_id":"$audio1:example.org","sender":"@alice:example.org","body":"Voice note","filename":"voice-note.ogg","source":{"type":"plain","url":"mxc://example.org/audio"},"info":{"mimetype":"audio/ogg","size":8192,"duration_ms":12000},"unexpected":"value"}"#
        )
        .is_err(),
        "room audio activity must reject unknown fields"
    );

    assert!(
        serde_json::from_str::<RoomMediaSource>(
            r#"{"type":"plain","url":"mxc://example.org/image","unexpected":"value"}"#
        )
        .is_err(),
        "room media source must reject unknown fields"
    );

    assert!(
        serde_json::from_str::<RoomImageInfo>(
            r#"{"mimetype":"image/png","size":4096,"width":800,"height":600,"is_animated":false,"unexpected":"value"}"#
        )
        .is_err(),
        "room image info must reject unknown fields"
    );

    assert!(
        serde_json::from_str::<RoomFileInfo>(
            r#"{"mimetype":"application/pdf","size":65536,"unexpected":"value"}"#
        )
        .is_err(),
        "room file info must reject unknown fields"
    );

    assert!(
        serde_json::from_str::<RoomAudioInfo>(
            r#"{"mimetype":"audio/ogg","size":8192,"duration_ms":12000,"unexpected":"value"}"#
        )
        .is_err(),
        "room audio info must reject unknown fields"
    );

    assert!(
        serde_json::from_str::<RoomMediaSource>(
            r#"{"type":"encrypted","url":"mxc://example.org/image","key":{"kty":"oct","key_ops":["decrypt","encrypt"],"alg":"A256CTR","k":"AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA","ext":true,"unexpected":"value"},"iv":"AgICAgICAgICAgICAgICAg","hashes":{"sha256":"AQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQE"},"v":"v2"}"#
        )
        .is_err(),
        "encrypted room media source key must reject unknown fields"
    );

    assert!(
        serde_json::from_str::<RoomImageInfo>(
            r#"{"mimetype":"image/png","size":4096,"width":800,"height":600,"is_animated":false,"thumbnail_source":{"type":"encrypted","url":"mxc://example.org/image","key":{"kty":"oct","key_ops":["decrypt","encrypt"],"alg":"A256CTR","k":"AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA","ext":true},"iv":"AgICAgICAgICAgICAgICAg","hashes":{"sha256":"AQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQE","unexpected":"value"},"v":"v2"}}"#
        )
        .is_err(),
        "encrypted room image info hashes must reject unknown fields"
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
        serde_json::from_str::<Event>(
            r#"{"RoomActivity":{"room_id":"!ops:example.org","activity_id":"event:$custom1:example.org","timestamp_ms":1700000000006,"kind":{"type":"unsupported_message","event_id":"$custom1:example.org","sender":"@alice:example.org","msgtype":"com.example.custom","body":"Custom body","unexpected":"value"}}}"#
        )
        .is_err(),
        "unsupported room activity kind must reject unknown fields"
    );

    assert!(
        serde_json::from_str::<RoomActivityUpdate>(
            r#"{"room_id":"!ops:example.org","target":{"type":"transaction","transaction_id":"txn-1"},"kind":{"type":"message_sent","event_id":"$event:example.org"},"unexpected":"value"}"#
        )
        .is_err(),
        "room activity update must reject unknown fields"
    );

    assert!(
        serde_json::from_str::<Event>(
            r#"{"RoomActivityUpdate":{"room_id":"!ops:example.org","target":{"type":"transaction","transaction_id":"txn-1","unexpected":"value"},"kind":{"type":"message_failed","message":"send failed","recoverable":true}}}"#
        )
        .is_err(),
        "room activity update target must reject unknown fields"
    );

    assert!(
        serde_json::from_str::<Event>(
            r#"{"RoomActivityUpdate":{"room_id":"!ops:example.org","target":{"type":"event","event_id":"$original:example.org"},"kind":{"type":"message_replaced","replacement_event_id":"$edit:example.org","sender":"@alice:example.org","timestamp_ms":1700000001000,"presentation":"text","body":"edited body","unexpected":"value"}}}"#
        )
        .is_err(),
        "message replaced room activity update must reject unknown fields"
    );
    assert!(
        serde_json::from_str::<Event>(
            r#"{"RoomActivityUpdate":{"room_id":"!ops:example.org","target":{"type":"event","event_id":"$original:example.org"},"kind":{"type":"reaction_added","reaction_event_id":"$reaction:example.org","sender":"@alice:example.org","timestamp_ms":1700000001500,"key":"+1","unexpected":"value"}}}"#
        )
        .is_err(),
        "reaction added room activity update must reject unknown fields"
    );
    assert!(
        serde_json::from_str::<Event>(
            r#"{"RoomActivityUpdate":{"room_id":"!ops:example.org","target":{"type":"event","event_id":"$original:example.org"},"kind":{"type":"reaction_removed","reaction_event_id":"$reaction:example.org","redaction_event_id":"$redaction:example.org","key":"+1","unexpected":"value"}}}"#
        )
        .is_err(),
        "reaction removed room activity update must reject unknown fields"
    );

    assert!(
        serde_json::from_str::<RoomTyping>(
            r#"{"room_id":"!ops:example.org","user_ids":["@alice:example.org"],"unexpected":"value"}"#
        )
        .is_err(),
        "room typing must reject unknown fields"
    );

    assert!(
        serde_json::from_str::<RoomMemberSummary>(
            r#"{"user_id":"@alice:example.org","display_name":"Alice","can_kick":true,"can_ban":false,"unexpected":"value"}"#
        )
        .is_err(),
        "room member summary must reject unknown fields"
    );

    for json in [
        r#"{"user_id":"@alice:example.org","display_name":"Alice","can_ban":false}"#,
        r#"{"user_id":"@alice:example.org","display_name":"Alice","can_kick":true}"#,
    ] {
        assert!(
            serde_json::from_str::<RoomMemberSummary>(json).is_err(),
            "room member summary must require each moderation capability"
        );
    }

    assert!(
        serde_json::from_str::<RoomMembers>(
            r#"{"room_id":"!ops:example.org","members":[],"unexpected":"value"}"#
        )
        .is_err(),
        "room members must reject unknown fields"
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
