use std::time::Duration;

#[cfg(not(windows))]
use std::ffi::OsString;
use std::path::PathBuf;

use mirx_core::{
    BackfillRoomHistoryRequest as CoreBackfillRoomHistoryRequest, E2eeBackupState,
    E2eeBackupStatus, E2eeCrossSigningStatus, E2eeRecoveryState, E2eeSessionState, E2eeStatus,
    Event as CoreEvent, FetchRoomMediaRequest as CoreFetchRoomMediaRequest,
    HistoryFrom as CoreHistoryFrom, PresenceKind, RoomActivity as CoreRoomActivity,
    RoomActivityKind as CoreRoomActivityKind, RoomActivityUpdate as CoreRoomActivityUpdate,
    RoomActivityUpdateKind as CoreRoomActivityUpdateKind,
    RoomActivityUpdateTarget as CoreRoomActivityUpdateTarget,
    RoomHistoryBackfill as CoreRoomHistoryBackfill,
    RoomHistoryBackfillEvent as CoreRoomHistoryBackfillEvent, RoomMedia as CoreRoomMedia,
    RoomMediaFormat as CoreRoomMediaFormat, RoomMediaKind as CoreRoomMediaKind,
    RoomMediaSource as CoreRoomMediaSource, RoomMemberRank, RoomMembership, RoomNotificationMode,
    RoomReadState,
};
use mirx_testkit::{
    AutomationDialog, BackfillRoomHistoryRequest, ComposeKey, ContextMenuCommand,
    E2eeRecoverySetupResult, FetchRoomImageMediaRequest, FindActiveViewArgs, FindActiveViewResult,
    FrameSize, HistoryFrom, LaunchOptions, LoginArgs, MdiArrangement, MdiState, MdiView,
    MdiWindowPlacement, MdiWindowState, MemberMenuState, MemberModerationAction, MemberPowerRole,
    MembershipCommandAction, Model, NavigationNode, NavigationPathSegment, ProtocolVersion,
    RecoverE2eeArgs, RestoreSessionArgs, RoomActivity, RoomActivityKind, RoomActivityRelation,
    RoomActivityUpdate, RoomActivityUpdateKind, RoomActivityUpdateTarget, RoomAudioActivity,
    RoomAudioInfo, RoomFileActivity, RoomFileInfo, RoomHistoryBackfill, RoomHistoryBackfillEvent,
    RoomImageActivity, RoomImageInfo, RoomImageMedia, RoomImageMediaFormat, RoomImageMediaKind,
    RoomImageMediaState, RoomJoinRule, RoomMediaSource, RoomMemberChange, RoomMemberView,
    RoomMessageActivity, RoomMessageActivityLine, RoomMessagePresentationKind,
    RoomMessageSendState, RoomPollActivity, RoomPollContent, RoomPowerLevelUserChange,
    RoomPowerLevelValueChange, RoomReactionSummary, RoomRedactedActivity, RoomStateNoticeActivity,
    RoomStateNoticeContent, RoomTag, RoomUnableToDecryptActivity, RoomUnableToDecryptCause,
    RoomVerificationRequestActivity, RoomVideoActivity, RoomVideoInfo, SendFileArgs, SessionAuth,
    SetupE2eeRecoveryArgs, SpaceChild, StatusBarState, StoredSession, SubmittedDirectRoomCommand,
    SubmittedHistoryCommand, SubmittedMemberModerationCommand, SubmittedMembershipCommand,
    SubmittedMessageCommand, SubmittedPowerLevelChange, SubmittedProfileCommand,
    SubmittedReactionCommand, SubmittedRedactionCommand, SubmittedReplyCommand,
    SubmittedReplyThreadTarget, SubmittedRoomSettingsCommand, SubmittedRoomTopicCommand,
    SubmittedSetupRecoveryCommand, SubmittedUserCommand, SwitchbarItem, SwitchbarState,
    TestkitError, UiActiveTimeline, UiActivityColors, UiColorItem, UiColors, UiComposeTarget,
    UiComposeTargetKind, UiConnectOptions, UiError, UiFindHistorySearchState, UiFindState,
    UiFrameLayout, UiLoggingOptions, UiMarkerOptions, UiMenu, UiMenuItem, UiNickColor,
    UiOptionsDialog, UiPoint, UiRect, UiRefreshCounters, UiRoomHistoryBackfillState, UiRoomList,
    UiRoomReadState, UiRoomState, UiState, UiTextFont, UiTimelineLine, UiTimelineLineKind,
    UiTimelineLink, UiTimestamp, UiTipsOptions, UiToolbarButton, UiTray, UiTrayMenuItem,
    UiTrayOptions, UnsupportedRoomMessageActivity, UserCommandAction, Verification,
    VerificationActionArgs, VerificationDecimals, VerificationEmoji, VerificationState,
    ViewActivityLevel, ViewIdentity,
};

#[test]
fn context_menu_command_serializes_to_strict_shape() {
    assert_eq!(
        serde_json::to_string(&ContextMenuCommand::PartRoom).unwrap(),
        "\"part_room\""
    );
    assert_eq!(
        serde_json::to_string(&ContextMenuCommand::QueryMember).unwrap(),
        "\"query_member\""
    );
    assert_eq!(
        serde_json::to_string(&ContextMenuCommand::MentionMember).unwrap(),
        "\"mention_member\""
    );
    assert_eq!(
        serde_json::from_str::<ContextMenuCommand>("\"mention_member\"").unwrap(),
        ContextMenuCommand::MentionMember
    );
    assert_eq!(
        serde_json::to_string(&ContextMenuCommand::KickMember).unwrap(),
        "\"kick_member\""
    );
    assert_eq!(
        serde_json::to_string(&ContextMenuCommand::BanMember).unwrap(),
        "\"ban_member\""
    );
    for (command, wire) in [
        (ContextMenuCommand::WhoisMember, "\"whois_member\""),
        (ContextMenuCommand::DevicesMember, "\"devices_member\""),
        (ContextMenuCommand::VerifyMember, "\"verify_member\""),
        (
            ContextMenuCommand::AcceptIdentityMember,
            "\"accept_identity_member\"",
        ),
        (ContextMenuCommand::IgnoreMember, "\"ignore_member\""),
        (ContextMenuCommand::UnignoreMember, "\"unignore_member\""),
        (ContextMenuCommand::OpMember, "\"op_member\""),
        (ContextMenuCommand::DeopMember, "\"deop_member\""),
        (ContextMenuCommand::VoiceMember, "\"voice_member\""),
        (ContextMenuCommand::DevoiceMember, "\"devoice_member\""),
    ] {
        assert_eq!(serde_json::to_string(&command).unwrap(), wire);
        assert_eq!(
            serde_json::from_str::<ContextMenuCommand>(wire).unwrap(),
            command
        );
    }
    assert!(serde_json::from_str::<ContextMenuCommand>("\"invalid\"").is_err());
}

#[test]
fn submitted_member_moderation_command_has_a_strict_json_contract() {
    let command = SubmittedMemberModerationCommand {
        action: MemberModerationAction::Kick,
        room_id: "!ops:example.org".to_owned(),
        user_id: "@bob:example.org".to_owned(),
        reason: None,
    };
    assert_eq!(
        serde_json::to_value(&command).unwrap(),
        serde_json::json!({
            "action": "kick",
            "room_id": "!ops:example.org",
            "user_id": "@bob:example.org",
            "reason": null
        })
    );
    for (action, wire) in [
        (MemberModerationAction::Ban, "ban"),
        (MemberModerationAction::Unban, "unban"),
        (MemberModerationAction::Invite, "invite"),
    ] {
        assert_eq!(
            serde_json::from_value::<SubmittedMemberModerationCommand>(serde_json::json!({
                "action": wire,
                "room_id": "!ops:example.org",
                "user_id": "@carol:example.org",
                "reason": "spamming"
            }))
            .unwrap(),
            SubmittedMemberModerationCommand {
                action,
                room_id: "!ops:example.org".to_owned(),
                user_id: "@carol:example.org".to_owned(),
                reason: Some("spamming".to_owned()),
            }
        );
    }
    assert!(serde_json::from_value::<SubmittedMemberModerationCommand>(serde_json::json!({
        "action": "ban", "room_id": "!ops:example.org", "user_id": "@bob:example.org", "extra": true
    })).is_err());
    assert!(
        serde_json::from_value::<SubmittedMemberModerationCommand>(serde_json::json!({
            "room_id": "!ops:example.org", "user_id": "@bob:example.org"
        }))
        .is_err()
    );
    assert!(
        serde_json::from_value::<SubmittedMemberModerationCommand>(serde_json::json!({
            "action": "kick", "user_id": "@bob:example.org"
        }))
        .is_err()
    );
    assert!(
        serde_json::from_value::<SubmittedMemberModerationCommand>(serde_json::json!({
            "action": "kick", "room_id": "!ops:example.org"
        }))
        .is_err()
    );
    assert!(
        serde_json::from_value::<SubmittedMemberModerationCommand>(serde_json::json!({
            "action": "invalid", "room_id": "!ops:example.org", "user_id": "@bob:example.org"
        }))
        .is_err()
    );
}

#[cfg(not(windows))]
use mirx_testkit::MirxApp;

fn stored_session() -> StoredSession {
    StoredSession {
        user_id: "@alice:example".to_owned(),
        device_id: "DEVICE".to_owned(),
        access_token: "secret-token".to_owned(),
        refresh_token: Some("refresh-token".to_owned()),
        auth: SessionAuth::Matrix,
    }
}

#[test]
fn send_file_args_serialize_to_the_strict_automation_shape() {
    let args = SendFileArgs {
        room_id: Some("!room:example"),
        path: "C:\\tmp\\report.txt",
        filename: Some("report.txt"),
        content_type: Some("application/octet-stream"),
    };
    assert_eq!(
        serde_json::to_value(args).expect("serialize send file args"),
        serde_json::json!({
            "room_id":"!room:example",
            "path":"C:\\tmp\\report.txt",
            "filename":"report.txt",
            "content_type":"application/octet-stream"
        })
    );
    assert!(serde_json::from_str::<SendFileArgs<'_>>(
        r#"{"room_id":null,"path":"report.txt","filename":null,"content_type":null,"extra":true}"#
    )
    .is_err());
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
        session: E2eeSessionState::Verified,
    }
}

fn sample_room_encrypted_file_key() -> mirx_testkit::RoomEncryptedFileKey {
    mirx_testkit::RoomEncryptedFileKey {
        kty: "oct".to_owned(),
        key_ops: vec!["encrypt".to_owned(), "decrypt".to_owned()],
        alg: "A256CTR".to_owned(),
        k: "base64url-key".to_owned(),
        ext: true,
    }
}

fn sample_room_encrypted_file_hashes() -> mirx_testkit::RoomEncryptedFileHashes {
    mirx_testkit::RoomEncryptedFileHashes {
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

fn sample_room_file_activity() -> RoomFileActivity {
    RoomFileActivity {
        event_id: "$file1:example.org".to_owned(),
        sender: "@alice:example.org".to_owned(),
        body: "report.pdf".to_owned(),
        filename: Some("incident-report.pdf".to_owned()),
        source: RoomMediaSource::Plain {
            url: "mxc://example.org/report".to_owned(),
        },
        info: Some(RoomFileInfo {
            mimetype: Some("application/pdf".to_owned()),
            size: Some(54_321),
        }),
    }
}

fn sample_room_audio_activity() -> RoomAudioActivity {
    RoomAudioActivity {
        event_id: "$audio1:example.org".to_owned(),
        sender: "@alice:example.org".to_owned(),
        body: "voice-note.ogg".to_owned(),
        filename: Some("voice-note.ogg".to_owned()),
        source: sample_encrypted_room_media_source("mxc://example.org/voice-note"),
        info: Some(RoomAudioInfo {
            mimetype: Some("audio/ogg".to_owned()),
            size: Some(9_876),
            duration_ms: Some(12_345),
        }),
    }
}

fn sample_room_video_activity(media_state: Option<RoomImageMediaState>) -> RoomVideoActivity {
    RoomVideoActivity {
        event_id: "$video1:example.org".to_owned(),
        sender: "@alice:example.org".to_owned(),
        body: "clip.mp4".to_owned(),
        filename: Some("clip.mp4".to_owned()),
        source: sample_encrypted_room_media_source("mxc://example.org/clip"),
        info: Some(RoomVideoInfo {
            mimetype: Some("video/mp4".to_owned()),
            size: Some(54_321),
            duration_ms: Some(12_345),
            width: Some(1920),
            height: Some(1080),
            thumbnail_source: Some(RoomMediaSource::Plain {
                url: "mxc://example.org/clip-thumbnail".to_owned(),
            }),
        }),
        media_state,
    }
}

fn sample_room_history_activity() -> RoomActivity {
    RoomActivity {
        room_id: "!ops:example.org".to_owned(),
        activity_id: "event:$message:example.org".to_owned(),
        timestamp_ms: 1_700_000_000_000,
        highlight: false,
        relation: None,
        reactions: Vec::new(),
        kind: RoomActivityKind::Redacted(RoomRedactedActivity {
            event_id: "$message:example.org".to_owned(),
        }),
    }
}

fn sample_core_room_history_activity() -> CoreRoomActivity {
    CoreRoomActivity {
        room_id: "!ops:example.org".to_owned(),
        activity_id: "event:$message:example.org".to_owned(),
        timestamp_ms: 1_700_000_000_000,
        highlight: false,
        relation: None,
        kind: CoreRoomActivityKind::Redacted(mirx_core::RoomRedactedActivity {
            event_id: "$message:example.org".to_owned(),
        }),
    }
}

fn sample_room_history_activity_update() -> RoomActivityUpdate {
    RoomActivityUpdate {
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
    }
}

fn sample_core_room_history_activity_update() -> CoreRoomActivityUpdate {
    CoreRoomActivityUpdate {
        room_id: "!ops:example.org".to_owned(),
        target: CoreRoomActivityUpdateTarget::Event {
            event_id: "$message:example.org".to_owned(),
        },
        kind: CoreRoomActivityUpdateKind::ReactionAdded {
            reaction_event_id: "$reaction:example.org".to_owned(),
            sender: "@bob:example.org".to_owned(),
            timestamp_ms: 1_700_000_000_100,
            key: "heart".to_owned(),
        },
    }
}

#[test]
fn launch_options_default_has_expected_timeout() {
    assert_eq!(
        LaunchOptions::default(),
        LaunchOptions {
            startup_timeout: Duration::from_secs(20),
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
        local_app_data: Some(PathBuf::from("/var/tmp/mirx-testkit")),
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
fn submitted_notification_commands_serialize_as_tagged_objects() {
    let commands = vec![
        mirx_testkit::SubmittedNotificationCommand::SetRoomNotificationMode {
            room_id: "!ops:example".to_owned(),
            mode: RoomNotificationMode::MentionsAndKeywordsOnly,
        },
        mirx_testkit::SubmittedNotificationCommand::AddHighlightWord {
            word: "deploy".to_owned(),
        },
        mirx_testkit::SubmittedNotificationCommand::RemoveHighlightWord {
            word: "build".to_owned(),
        },
    ];
    let json = serde_json::to_value(&commands).expect("serialize notification commands");
    assert_eq!(
        json,
        serde_json::json!([
            {"type": "set_room_notification_mode", "room_id": "!ops:example", "mode": "mentions_and_keywords_only"},
            {"type": "add_highlight_word", "word": "deploy"},
            {"type": "remove_highlight_word", "word": "build"}
        ])
    );
    assert_eq!(
        serde_json::from_value::<Vec<mirx_testkit::SubmittedNotificationCommand>>(json)
            .expect("deserialize notification commands"),
        commands
    );
}

#[test]
fn menu_state_models_round_trip_and_reject_unknown_fields() {
    let room = mirx_testkit::RoomMenuState {
        accept_invite_enabled: true,
        reject_invite_enabled: false,
        part_room_enabled: false,
        channel_central_enabled: true,
        notification_mode: Some(RoomNotificationMode::Mute),
        notification_items_enabled: true,
    };
    assert_eq!(
        serde_json::to_value(&room).expect("serialize room menu state")["notification_mode"],
        serde_json::json!("mute")
    );
    assert_eq!(
        serde_json::from_value::<mirx_testkit::RoomMenuState>(
            serde_json::to_value(&room).expect("serialize room menu state"),
        )
        .expect("deserialize room menu state"),
        room
    );
    let message = mirx_testkit::MessageMenuState {
        reply_enabled: true,
        edit_enabled: true,
        react_enabled: true,
        redact_enabled: true,
        save_attachment_enabled: false,
        go_to_original_enabled: true,
        selected_event_id: Some("$event:example".to_owned()),
    };
    assert_eq!(
        serde_json::from_value::<mirx_testkit::MessageMenuState>(
            serde_json::to_value(&message).expect("serialize message menu state"),
        )
        .expect("deserialize message menu state"),
        message
    );
    let member = MemberMenuState {
        whois_enabled: true,
        devices_enabled: true,
        verify_enabled: true,
        accept_identity_enabled: false,
        query_enabled: true,
        mention_enabled: true,
        ignore_enabled: true,
        unignore_enabled: false,
        op_enabled: true,
        deop_enabled: false,
        voice_enabled: true,
        devoice_enabled: false,
        kick_enabled: true,
        ban_enabled: false,
        selected_user_id: Some("@bob:example".to_owned()),
        selected_label: Some("Bob".to_owned()),
    };
    assert_eq!(
        serde_json::from_value::<MemberMenuState>(serde_json::to_value(&member).unwrap()).unwrap(),
        member
    );

    assert!(
        serde_json::from_value::<mirx_testkit::RoomMenuState>(serde_json::json!({
            "accept_invite_enabled": true,
            "reject_invite_enabled": false,
            "part_room_enabled": false,
            "channel_central_enabled": false,
            "notification_mode": null,
            "notification_items_enabled": false,
            "unexpected": true
        }))
        .is_err()
    );
    assert!(
        serde_json::from_value::<mirx_testkit::MessageMenuState>(serde_json::json!({
            "reply_enabled": true,
            "redact_enabled": true,
            "selected_event_id": "$event:example",
            "unexpected": true
        }))
        .is_err()
    );
    assert!(serde_json::from_value::<MemberMenuState>(serde_json::json!({"whois_enabled": true, "devices_enabled": true, "verify_enabled": true, "accept_identity_enabled": true, "query_enabled": true, "mention_enabled": true, "ignore_enabled": true, "unignore_enabled": false, "op_enabled": true, "deop_enabled": false, "voice_enabled": true, "devoice_enabled": false, "kick_enabled": true, "ban_enabled": false, "selected_user_id": null, "selected_label": null, "unexpected": true})).is_err());
    assert!(serde_json::from_value::<MemberMenuState>(serde_json::json!({"whois_enabled": true, "devices_enabled": true, "verify_enabled": true, "accept_identity_enabled": true, "query_enabled": true, "mention_enabled": true, "ignore_enabled": true, "unignore_enabled": false, "op_enabled": true, "deop_enabled": false, "voice_enabled": true, "devoice_enabled": false, "ban_enabled": false, "selected_user_id": null, "selected_label": null})).is_err());
}

#[test]
fn data_models_round_trip_through_json() {
    let model = Model {
        status: "synced".to_owned(),
        lifecycle_state: "ready".to_owned(),
        connection: "syncing".to_owned(),
        reconnect_attempts: 0,
        rooms: vec![
            mirx_testkit::Room {
                room_id: "!space:example".to_owned(),
                display_name: "Space".to_owned(),
                topic: None,
                membership: RoomMembership::Join,
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
            mirx_testkit::Room {
                room_id: "!alpha:example".to_owned(),
                display_name: "Alpha".to_owned(),
                topic: None,
                membership: RoomMembership::Join,
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
            mirx_testkit::Room {
                room_id: "!beta:example".to_owned(),
                display_name: "Beta".to_owned(),
                topic: None,
                membership: RoomMembership::Join,
                encrypted: false,
                is_space: false,
                is_direct: false,
                direct_user_ids: Vec::new(),
                tags: Vec::new(),
                space_children: vec![],
            },
            mirx_testkit::Room {
                room_id: "!lobby:example".to_owned(),
                display_name: "Lobby".to_owned(),
                topic: None,
                membership: RoomMembership::Join,
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
        last_e2ee_recovery_setup_result: None,
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
        submitted_member_moderation_commands: Vec::new(),
        submitted_room_settings_commands: Vec::new(),
        submitted_profile_commands: Vec::new(),
        submitted_away_commands: Vec::new(),
        submitted_typing_commands: Vec::new(),
        submitted_user_commands: Vec::new(),
        submitted_direct_room_commands: Vec::new(),
        submitted_room_topic_commands: Vec::new(),
        submitted_reaction_commands: Vec::new(),
        submitted_edit_commands: Vec::new(),
        submitted_reply_commands: Vec::new(),
        submitted_message_commands: Vec::new(),
        submitted_file_commands: Vec::new(),
        submitted_notification_commands: Vec::new(),
        submitted_redaction_commands: Vec::new(),
        submitted_history_commands: Vec::new(),
        submitted_verification_actions: Vec::new(),
        submitted_verification_requests: Vec::new(),
        identity_warnings: Vec::new(),
        submitted_identity_reset_commands: Vec::new(),
        submitted_room_key_transfers: Vec::new(),
        submitted_session_commands: Vec::new(),
        submitted_recovery_commands: Vec::new(),
        submitted_setup_recovery_commands: Vec::new(),
        submitted_room_list_requests: Vec::new(),
        opened_links: Vec::new(),
        shown_tips: Vec::new(),
        last_error: Some(UiError {
            operation: "send text".to_owned(),
            message: "failed".to_owned(),
        }),
        selected_room: Some("!room:example".to_owned()),
    };
    let version = ProtocolVersion {
        protocol_version: 18,
        app_version: "0.1.0".to_owned(),
    };
    let session = stored_session();
    let login = LoginArgs {
        homeserver_url: "https://matrix.example".to_owned(),
        username: "@alice:example".to_owned(),
        password: "secret".to_owned(),
        device_display_name: "mirx-ui".to_owned(),
    };
    let restore = RestoreSessionArgs {
        homeserver_url: "https://matrix.example".to_owned(),
        session: session.clone(),
    };
    let ui_state = UiState {
        navigation: NavigationNode {
            text: "Status".to_owned(),
            view: Some(ViewIdentity::Status),
            space_id: None,
            expanded: Some(true),
            activity: None,
            children: vec![NavigationNode {
                text: "Work".to_owned(),
                view: None,
                space_id: Some("!work:example".to_owned()),
                expanded: Some(false),
                activity: Some(ViewActivityLevel::Message),
                children: vec![NavigationNode {
                    text: "Room".to_owned(),
                    view: Some(ViewIdentity::Room {
                        room_id: "!room:example".to_owned(),
                    }),
                    space_id: None,
                    expanded: None,
                    activity: Some(ViewActivityLevel::Message),
                    children: vec![],
                }],
            }],
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
                    placement: MdiWindowPlacement {
                        state: MdiWindowState::Minimized,
                        x: 0,
                        y: 0,
                        width: 320,
                        height: 200,
                    },
                    lines: vec!["status: synced".to_owned()],
                    members: Vec::new(),
                    log: None,
                },
                MdiView {
                    identity: ViewIdentity::Room {
                        room_id: "!room:example".to_owned(),
                    },
                    title: "Room".to_owned(),
                    active: true,
                    placement: MdiWindowPlacement {
                        state: MdiWindowState::Maximized,
                        x: 0,
                        y: 0,
                        width: 320,
                        height: 200,
                    },
                    lines: vec!["hello".to_owned()],
                    members: vec![RoomMemberView {
                        user_id: "@alice:example".to_owned(),
                        nick: "Alice".to_owned(),
                        label: "Alice".to_owned(),
                        rank: RoomMemberRank::Voiced,
                        can_kick: true,
                        can_ban: false,
                        presence: Some(PresenceKind::Unavailable),
                        color: "#7f7f7f".to_owned(),
                    }],
                    log: None,
                },
                MdiView {
                    identity: ViewIdentity::Thread {
                        room_id: "!room:example".to_owned(),
                        thread_root_event_id: "$thread-root:example".to_owned(),
                    },
                    title: "Thread".to_owned(),
                    active: false,
                    placement: MdiWindowPlacement {
                        state: MdiWindowState::Normal,
                        x: 0,
                        y: 0,
                        width: 320,
                        height: 200,
                    },
                    lines: vec!["reply".to_owned()],
                    members: Vec::new(),
                    log: None,
                },
            ],
        },
        status_bar: StatusBarState {
            text: "synced 1 room(s)".to_owned(),
        },
        switchbar: SwitchbarState {
            rows: 1,
            items: vec![
                SwitchbarItem {
                    identity: ViewIdentity::Status,
                    title: "Status".to_owned(),
                    active: false,
                    activity: None,
                    rect: UiRect {
                        x: 0,
                        y: 0,
                        width: 120,
                        height: 36,
                    },
                },
                SwitchbarItem {
                    identity: ViewIdentity::Room {
                        room_id: "!room:example".to_owned(),
                    },
                    title: "Room".to_owned(),
                    active: true,
                    activity: Some(ViewActivityLevel::Message),
                    rect: UiRect {
                        x: 120,
                        y: 0,
                        width: 120,
                        height: 36,
                    },
                },
                SwitchbarItem {
                    identity: ViewIdentity::Thread {
                        room_id: "!room:example".to_owned(),
                        thread_root_event_id: "$thread-root:example".to_owned(),
                    },
                    title: "Thread".to_owned(),
                    active: false,
                    activity: Some(ViewActivityLevel::Message),
                    rect: UiRect {
                        x: 240,
                        y: 0,
                        width: 120,
                        height: 36,
                    },
                },
            ],
        },
        toolbar: vec![UiToolbarButton {
            command_id: 40030,
            label: "Connect".to_owned(),
            hidden: false,
            enabled: true,
            rect: UiRect {
                x: 0,
                y: 2,
                width: 23,
                height: 22,
            },
        }],
        frame_layout: UiFrameLayout {
            toolbar: Some(UiRect {
                x: 0,
                y: 0,
                width: 1008,
                height: 26,
            }),
            switchbar: Some(UiRect {
                x: 0,
                y: 0,
                width: 1008,
                height: 28,
            }),
            treebar: Some(UiRect {
                x: 0,
                y: 28,
                width: 240,
                height: 600,
            }),
            mdi_client: UiRect {
                x: 240,
                y: 28,
                width: 768,
                height: 600,
            },
        },
        dpi: 96,
        active_find: Some(UiFindState {
            active: ViewIdentity::Room {
                room_id: "!room:example".to_owned(),
            },
            query: "hello".to_owned(),
            matched: true,
            match_count: 1,
            current_match_index: Some(0),
            selected_text: Some("hello".to_owned()),
            history_search: UiFindHistorySearchState::LocalOnly,
        }),
        rooms: vec![UiRoomState {
            room_id: "!room:example".to_owned(),
            display_name: "Room".to_owned(),
            membership: RoomMembership::Join,
            encrypted: true,
            is_space: false,
            is_direct: true,
            direct_user_ids: vec!["@alice:example".to_owned()],
            tags: vec![RoomTag {
                tag: "m.favourite".to_owned(),
                order: Some("0.25".to_owned()),
            }],
            space_children: vec![],
            activity: Some(ViewActivityLevel::Message),
            read_target_event_id: Some("$event:example".to_owned()),
            read_state: Some(UiRoomReadState {
                latest_active_event_id: Some("$latest:example".to_owned()),
                fully_read_event_id: Some("$fully-read:example".to_owned()),
                marked_unread: true,
                num_unread: 7,
                num_notifications: 3,
                num_mentions: 1,
            }),
            history_backfill: UiRoomHistoryBackfillState::Idle {
                next: HistoryFrom::Latest,
            },
            timeline_row_builds: 4,
            buffer_lines: 2,
        }],
        text_font: UiTextFont {
            face: "Fixedsys".to_owned(),
            points: 9,
            bold: false,
            line_height: 15,
            edit_height: 25,
            average_char_width: 8,
            fallback_faces: vec!["Tahoma".to_owned(), "Noto Emoji".to_owned()],
        },
        timestamp: UiTimestamp {
            show: true,
            format: "[HH:nn]".to_owned(),
        },
        colors: UiColors {
            items: vec![UiColorItem {
                item: "background".to_owned(),
                index: 0,
                color: "#ffffff".to_owned(),
            }],
            switchbar: UiActivityColors {
                event: 5,
                message: 4,
                highlight: 12,
            },
        },
        active_system_menu_items: vec!["&Restore".to_owned(), "&Font...".to_owned()],
        active_system_menu_checked: vec![],
        active_system_menu_disabled: vec![],
        menu_bar: vec![UiMenu {
            label: "&File".to_owned(),
            items: vec![UiMenuItem {
                label: "&Disconnect".to_owned(),
                enabled: false,
                checked: false,
            }],
        }],
        compose_target: Some(UiComposeTarget {
            kind: UiComposeTargetKind::Edit,
            event_id: "$mine:example".to_owned(),
        }),
        room_list: Some(UiRoomList {
            columns: vec!["Room".to_owned()],
            rows: vec![vec!["#rust:example".to_owned()]],
            selected: None,
            column_click_points: vec![UiPoint { x: 4, y: 5 }],
            visible_row_click_points: vec![UiPoint { x: 6, y: 7 }],
            top_row: 0,
        }),
        colors_dialog: None,
        address_book: None,
        channel_central: None,
        color_index: None,
        options_dialog: Some(UiOptionsDialog {
            tree: vec!["Display".to_owned()],
            selected: "Display".to_owned(),
            connect: UiConnectOptions {
                startup: false,
                reconnect: true,
                retry: false,
                retry_times: "3".to_owned(),
                retry_delay: "30".to_owned(),
                retry_fields_enabled: false,
            },
            visible_controls: vec![6301],
            highlight_items: vec!["build (notify only)".to_owned()],
            highlight_controls_enabled: true,
            highlight_delete_enabled: true,
            activity_colors: UiActivityColors {
                event: 5,
                message: 9,
                highlight: 12,
            },
            marker: UiMarkerOptions {
                show: false,
                color: 14,
            },
            logging: UiLoggingOptions {
                channels: false,
                queries: true,
                strip_codes: true,
                timestamp: false,
            },
            tray: UiTrayOptions {
                always: false,
                startup: true,
                minimize: false,
                single_click: true,
            },
            send_typing: false,
            tips: UiTipsOptions {
                highlights: false,
                queries: true,
                channels: false,
            },
        }),
        active_timeline: Some(UiActiveTimeline {
            top_line: 0,
            visible_lines: vec![UiTimelineLine {
                text: "[22:13] <Alice> hello".to_owned(),
                kind: UiTimelineLineKind::Message,
                color: "#000000".to_owned(),
                styles: vec![mirx_core::TextStyleSpan {
                    start: 16,
                    end: 21,
                    style: mirx_core::TextStyle {
                        bold: true,
                        foreground: Some(mirx_core::TextColor {
                            red: 0xff,
                            green: 0,
                            blue: 0,
                        }),
                        ..mirx_core::TextStyle::default()
                    },
                }],
                nick_color: Some(UiNickColor {
                    start: 9,
                    end: 14,
                    color: "#00007f".to_owned(),
                }),
                event_id: Some("$hello".to_owned()),
                click_point: UiPoint { x: 8, y: 9 },
                text_left: 16,
                links: vec![UiTimelineLink {
                    start: 0,
                    end: 5,
                    url: "https://example.org".to_owned(),
                    user_id: None,
                }],
            }],
        }),
        tray: UiTray {
            icon: true,
            hidden: true,
            menu: vec![UiTrayMenuItem {
                label: "Room".to_owned(),
                checked: true,
            }],
        },
        ignored_users: Some(vec!["@spam:example".to_owned()]),
        aliases: vec!["/j /join #$$1".to_owned()],
        refresh_counters: UiRefreshCounters {
            timeline_row_builds: 5,
            rows_wrapped: 6,
            treebar_rebuilds: 7,
            switchbar_rebuilds: 8,
        },
    };
    let navigation_path = vec![
        NavigationPathSegment::first("mirx"),
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
                "text": "Status",
                "view": {"kind": "status"},
                "space_id": null,
                "expanded": true,
                "activity": null,
                "children": [
                    {
                        "text": "Work",
                        "view": null,
                        "space_id": "!work:example",
                        "expanded": false,
                        "activity": "message",
                        "children": [
                            {
                                "text": "Room",
                                "view": {"kind": "room", "room_id": "!room:example"},
                                "space_id": null,
                                "expanded": null,
                                "activity": "message",
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
                        "placement": {"state": "minimized", "x": 0, "y": 0, "width": 320, "height": 200},
                        "lines": ["status: synced"],
                        "members": [],
                        "log": null,
                    },
                    {
                        "identity": {
                            "kind": "room",
                            "room_id": "!room:example",
                        },
                        "title": "Room",
                        "active": true,
                        "placement": {"state": "maximized", "x": 0, "y": 0, "width": 320, "height": 200},
                        "lines": ["hello"],
                        "members": [
                            {
                                "user_id": "@alice:example",
                                "nick": "Alice",
                                "label": "Alice",
                                "rank": "voiced",
                                "can_kick": true,
                                "can_ban": false,
                                "presence": "unavailable",
                                "color": "#7f7f7f",
                            }
                        ],
                        "log": null,
                    },
                    {
                        "identity": {
                            "kind": "thread",
                            "room_id": "!room:example",
                            "thread_root_event_id": "$thread-root:example",
                        },
                        "title": "Thread",
                        "active": false,
                        "placement": {"state": "normal", "x": 0, "y": 0, "width": 320, "height": 200},
                        "lines": ["reply"],
                        "members": [],
                        "log": null,
                    }
                ],
            },
            "status_bar": {
                "text": "synced 1 room(s)",
            },
            "switchbar": {
                "rows": 1,
                "items": [
                    {
                        "identity": {
                            "kind": "status",
                        },
                        "title": "Status",
                        "active": false,
                        "activity": null,
                        "rect": {"x": 0, "y": 0, "width": 120, "height": 36},
                    },
                    {
                        "identity": {
                            "kind": "room",
                            "room_id": "!room:example",
                        },
                        "title": "Room",
                        "active": true,
                        "activity": "message",
                        "rect": {"x": 120, "y": 0, "width": 120, "height": 36},
                    },
                    {
                        "identity": {
                            "kind": "thread",
                            "room_id": "!room:example",
                            "thread_root_event_id": "$thread-root:example",
                        },
                        "title": "Thread",
                        "active": false,
                        "activity": "message",
                        "rect": {"x": 240, "y": 0, "width": 120, "height": 36},
                    }
                ],
            },
            "toolbar": [
                {
                    "command_id": 40030,
                    "label": "Connect",
                    "hidden": false,
                    "enabled": true,
                    "rect": {"x": 0, "y": 2, "width": 23, "height": 22},
                }
            ],
            "frame_layout": {
                "toolbar": {"x": 0, "y": 0, "width": 1008, "height": 26},
                "switchbar": {"x": 0, "y": 0, "width": 1008, "height": 28},
                "treebar": {"x": 0, "y": 28, "width": 240, "height": 600},
                "mdi_client": {"x": 240, "y": 28, "width": 768, "height": 600},
            },
            "dpi": 96,
            "active_find": {
                "active": {
                    "kind": "room",
                    "room_id": "!room:example",
                },
                "query": "hello",
                "matched": true,
                "match_count": 1,
                "current_match_index": 0,
                "selected_text": "hello",
                "history_search": {"state": "local_only"},
            },
            "room_list": {
                "columns": ["Room"],
                "rows": [["#rust:example"]],
                "selected": null,
                "column_click_points": [{"x": 4, "y": 5}],
                "visible_row_click_points": [{"x": 6, "y": 7}],
                "top_row": 0,
            },
            "options_dialog": {
                "tree": ["Display"],
                "selected": "Display",
                "connect": {
                    "startup": false,
                    "reconnect": true,
                    "retry": false,
                    "retry_times": "3",
                    "retry_delay": "30",
                    "retry_fields_enabled": false
                },
                "visible_controls": [6301],
                "highlight_items": ["build (notify only)"],
                "highlight_controls_enabled": true,
                "highlight_delete_enabled": true,
                "activity_colors": {"event": 5, "message": 9, "highlight": 12},
                "send_typing": false,
                "marker": {"show": false, "color": 14},
                "logging": {"channels": false, "queries": true, "strip_codes": true, "timestamp": false},
                "tray": {"always": false, "startup": true, "minimize": false, "single_click": true},
                "tips": {"highlights": false, "queries": true, "channels": false},
            },
            "colors_dialog": null,
            "address_book": null,
            "channel_central": null,
            "color_index": null,
            "active_timeline": {
                "top_line": 0,
                "visible_lines": [{
                    "text": "[22:13] <Alice> hello",
                    "kind": "message",
                    "color": "#000000",
                    "styles": [{
                        "start": 16,
                        "end": 21,
                        "style": {
                            "bold": true,
                            "italic": false,
                            "underline": false,
                            "strikethrough": false,
                            "foreground": "#ff0000",
                            "background": null,
                        },
                    }],
                    "nick_color": {"start": 9, "end": 14, "color": "#00007f"},
                    "event_id": "$hello",
                    "click_point": {"x": 8, "y": 9},
                    "text_left": 16,
                    "links": [{"start": 0, "end": 5, "url": "https://example.org", "user_id": null}],
                }],
            },
            "tray": {"icon": true, "hidden": true, "menu": [{"label": "Room", "checked": true}]},
            "ignored_users": ["@spam:example"],
            "aliases": ["/j /join #$$1"],
            "refresh_counters": {
                "timeline_row_builds": 5u64,
                "rows_wrapped": 6u64,
                "treebar_rebuilds": 7u64,
                "switchbar_rebuilds": 8u64,
            },
            "rooms": [
                {
                    "room_id": "!room:example",
                    "display_name": "Room",
                    "membership": "join",
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
                    "activity": "message",
                    "read_target_event_id": "$event:example",
                    "read_state": {
                        "latest_active_event_id": "$latest:example",
                        "fully_read_event_id": "$fully-read:example",
                        "marked_unread": true,
                        "num_unread": 7u64,
                        "num_notifications": 3u64,
                        "num_mentions": 1u64,
                    },
                    "history_backfill": {
                        "state": "idle",
                        "next": { "type": "latest" },
                    },
                    "timeline_row_builds": 4u64,
                    "buffer_lines": 2u64,
                }
            ],
            "text_font": {
                "face": "Fixedsys",
                "points": 9,
                "bold": false,
                "line_height": 15,
                "edit_height": 25,
                "average_char_width": 8,
                "fallback_faces": ["Tahoma", "Noto Emoji"],
            },
            "timestamp": {"show": true, "format": "[HH:nn]"},
            "colors": {
                "items": [{"item": "background", "index": 0, "color": "#ffffff"}],
                "switchbar": {"event": 5, "message": 4, "highlight": 12},
            },
            "active_system_menu_items": ["&Restore", "&Font..."],
            "active_system_menu_checked": [],
            "active_system_menu_disabled": [],
            "menu_bar": [{"label": "&File", "items": [{"label": "&Disconnect", "enabled": false, "checked": false}]}],
            "compose_target": {"kind": "edit", "event_id": "$mine:example"},
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
    let room = mirx_testkit::Room {
        room_id: "!alpha:example".to_owned(),
        display_name: "Alpha".to_owned(),
        topic: None,
        membership: RoomMembership::Join,
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
            "topic": null,
            "membership": "join",
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
fn find_active_view_dtos_round_trip_through_json() {
    let args = FindActiveViewArgs {
        query: "needle".to_owned(),
    };
    let result = FindActiveViewResult {
        active: ViewIdentity::Thread {
            room_id: "!room:example".to_owned(),
            thread_root_event_id: "$thread-root:example".to_owned(),
        },
        query: "needle".to_owned(),
        matched: true,
        match_count: 2,
        current_match_index: Some(1),
        selected_text: Some("needle reply".to_owned()),
        history_search: UiFindHistorySearchState::Loading,
    };
    let ui_find_state = UiFindState {
        active: ViewIdentity::Status,
        query: "synced".to_owned(),
        matched: false,
        match_count: 0,
        current_match_index: None,
        selected_text: None,
        history_search: UiFindHistorySearchState::Failed {
            message: "homeserver unavailable".to_owned(),
        },
    };

    assert_eq!(
        serde_json::from_value::<FindActiveViewArgs>(serde_json::to_value(&args).unwrap()).unwrap(),
        args
    );
    assert_eq!(
        serde_json::from_value::<FindActiveViewResult>(serde_json::to_value(&result).unwrap())
            .unwrap(),
        result
    );
    assert_eq!(
        serde_json::from_value::<UiFindState>(serde_json::to_value(&ui_find_state).unwrap())
            .unwrap(),
        ui_find_state
    );
}

#[test]
fn ui_room_history_backfill_state_round_trips_through_json() {
    let cases = vec![
        (
            UiRoomHistoryBackfillState::Idle {
                next: HistoryFrom::Latest,
            },
            serde_json::json!({
                "state": "idle",
                "next": { "type": "latest" },
            }),
        ),
        (
            UiRoomHistoryBackfillState::Idle {
                next: HistoryFrom::BeforeEvent {
                    event_id: "$kept".to_owned(),
                },
            },
            serde_json::json!({
                "state": "idle",
                "next": { "type": "before_event", "event_id": "$kept" },
            }),
        ),
        (
            UiRoomHistoryBackfillState::Loading {
                from: HistoryFrom::Latest,
            },
            serde_json::json!({
                "state": "loading",
                "from": { "type": "latest" },
            }),
        ),
        (
            UiRoomHistoryBackfillState::Loading {
                from: HistoryFrom::Token {
                    token: "t0".to_owned(),
                },
            },
            serde_json::json!({
                "state": "loading",
                "from": { "type": "token", "token": "t0" },
            }),
        ),
        (
            UiRoomHistoryBackfillState::Failed {
                from: HistoryFrom::Token {
                    token: "t1".to_owned(),
                },
                message: "request failed".to_owned(),
            },
            serde_json::json!({
                "state": "failed",
                "from": { "type": "token", "token": "t1" },
                "message": "request failed",
            }),
        ),
        (
            UiRoomHistoryBackfillState::Exhausted,
            serde_json::json!({
                "state": "exhausted",
            }),
        ),
    ];

    for (state, json) in cases {
        assert_eq!(
            serde_json::to_value(&state).expect("serialize ui room history backfill state"),
            json.clone()
        );
        assert_eq!(
            serde_json::from_value::<UiRoomHistoryBackfillState>(json)
                .expect("deserialize ui room history backfill state"),
            state
        );
    }
}

#[test]
fn ui_room_history_backfill_state_rejects_unknown_fields() {
    for json in [
        serde_json::json!({
            "state": "idle",
            "next": { "type": "latest" },
            "unexpected": true,
        }),
        serde_json::json!({
            "state": "loading",
            "from": { "type": "token", "token": "token" },
            "unexpected": true,
        }),
        serde_json::json!({
            "state": "loading",
            "from": { "type": "token", "token": "token", "unexpected": true },
        }),
        serde_json::json!({
            "state": "failed",
            "from": { "type": "latest" },
            "message": "boom",
            "unexpected": true,
        }),
        serde_json::json!({
            "state": "loading",
            "from": null,
        }),
    ] {
        assert!(
            serde_json::from_value::<UiRoomHistoryBackfillState>(json).is_err(),
            "ui room history backfill state must reject unknown fields"
        );
    }
}

#[test]
fn room_activity_sent_message_round_trips_through_json() {
    let activity = RoomActivity {
        room_id: "!room:example".to_owned(),
        activity_id: "activity-1".to_owned(),
        timestamp_ms: 1_700_000_000_000,
        highlight: false,
        relation: None,
        reactions: Vec::new(),
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
            "highlight": false,
            "relation": null,
            "reactions": [],
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
        highlight: false,
        relation: None,
        reactions: Vec::new(),
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
            "highlight": false,
            "relation": null,
            "reactions": [],
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
        highlight: false,
        relation: None,
        reactions: Vec::new(),
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
            "highlight": false,
            "relation": null,
            "reactions": [],
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
        highlight: false,
        relation: None,
        reactions: Vec::new(),
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
        "highlight": false,
        "relation": null,
        "reactions": [],
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
        highlight: false,
        relation: None,
        reactions: Vec::new(),
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
        "highlight": false,
        "relation": null,
        "reactions": [],
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
        highlight: false,
        relation: None,
        reactions: Vec::new(),
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
            "highlight": false,
            "relation": null,
            "reactions": [],
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
        highlight: false,
        relation: None,
        reactions: Vec::new(),
        kind: RoomActivityKind::StateNotice(RoomStateNoticeActivity {
            event_id: "$member:example".to_owned(),
            sender: "@carol:example".to_owned(),
            state_key: "@carol:example".to_owned(),
            content: RoomStateNoticeContent::Member {
                change: RoomMemberChange::NickChanged {
                    old: "Carol".to_owned(),
                    new: "carol".to_owned(),
                },
                target_nick: Some("carol".to_owned()),
                reason: Some("rename".to_owned()),
            },
        }),
    };

    assert_eq!(
        serde_json::to_value(&activity).expect("serialize member state-notice activity"),
        serde_json::json!({
            "room_id": "!room:example",
            "activity_id": "activity-5",
            "timestamp_ms": 1700000240000u64,
            "highlight": false,
            "relation": null,
            "reactions": [],
            "kind": {
                "type": "state_notice",
                "event_id": "$member:example",
                "sender": "@carol:example",
                "state_key": "@carol:example",
                "content": {
                    "type": "member",
                    "change": {
                        "type": "nick_changed",
                        "old": "Carol",
                        "new": "carol"
                    },
                    "target_nick": "carol",
                    "reason": "rename"
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
fn room_activity_power_levels_state_notice_round_trips_through_json() {
    let activity = RoomActivity {
        room_id: "!room:example".to_owned(),
        activity_id: "state:$power:example".to_owned(),
        timestamp_ms: 1_700_000_250_000,
        highlight: false,
        relation: None,
        reactions: Vec::new(),
        kind: RoomActivityKind::StateNotice(RoomStateNoticeActivity {
            event_id: "$power:example".to_owned(),
            sender: "@admin:example".to_owned(),
            state_key: String::new(),
            content: RoomStateNoticeContent::PowerLevels {
                changes: vec![RoomPowerLevelUserChange {
                    user_id: "@bob:example".to_owned(),
                    old: 0,
                    new: 50,
                }],
                users_default: Some(RoomPowerLevelValueChange { old: 0, new: 10 }),
                other_permissions_changed: true,
            },
        }),
    };
    let json = serde_json::json!({
        "room_id": "!room:example",
        "activity_id": "state:$power:example",
        "timestamp_ms": 1700000250000u64,
        "highlight": false,
        "relation": null,
        "reactions": [],
        "kind": {
            "type": "state_notice",
            "event_id": "$power:example",
            "sender": "@admin:example",
            "state_key": "",
            "content": {
                "type": "power_levels",
                "changes": [{ "user_id": "@bob:example", "old": 0, "new": 50 }],
                "users_default": { "old": 0, "new": 10 },
                "other_permissions_changed": true
            }
        }
    });

    assert_eq!(
        serde_json::to_value(&activity).expect("serialize power levels state-notice activity"),
        json
    );
    assert_eq!(
        serde_json::from_value::<RoomActivity>(json).expect("deserialize power levels notice"),
        activity
    );
    for content in [
        serde_json::json!({
            "type": "power_levels",
            "changes": [],
            "users_default": null
        }),
        serde_json::json!({
            "type": "power_levels",
            "changes": [{ "user_id": "@bob:example", "old": 0, "new": 50, "unexpected": true }],
            "users_default": null,
            "other_permissions_changed": false
        }),
        serde_json::json!({
            "type": "power_levels",
            "changes": [],
            "users_default": { "old": 0, "new": 10, "unexpected": true },
            "other_permissions_changed": false
        }),
    ] {
        assert!(
            serde_json::from_value::<RoomStateNoticeContent>(content.clone()).is_err(),
            "{content}"
        );
    }
}

#[test]
fn room_settings_state_notice_contents_round_trip_through_json() {
    for (content, json) in [
        (
            RoomStateNoticeContent::Name {
                name: Some("Ops".to_owned()),
            },
            serde_json::json!({ "type": "name", "name": "Ops" }),
        ),
        (
            RoomStateNoticeContent::Name { name: None },
            serde_json::json!({ "type": "name", "name": null }),
        ),
        (
            RoomStateNoticeContent::JoinRule {
                rule: RoomJoinRule::KnockRestricted,
            },
            serde_json::json!({ "type": "join_rule", "rule": "knock_restricted" }),
        ),
        (
            RoomStateNoticeContent::JoinRule {
                rule: RoomJoinRule::Other("org.example.vip".to_owned()),
            },
            serde_json::json!({ "type": "join_rule", "rule": { "other": "org.example.vip" } }),
        ),
        (
            RoomStateNoticeContent::Encryption,
            serde_json::json!({ "type": "encryption" }),
        ),
    ] {
        assert_eq!(serde_json::to_value(&content).expect("serialize"), json);
        assert_eq!(
            serde_json::from_value::<RoomStateNoticeContent>(json.clone()).expect("deserialize"),
            content
        );
        // The core's wire shape is the same.
        assert_eq!(
            serde_json::to_value(
                serde_json::from_value::<mirx_core::RoomStateNoticeContent>(json.clone())
                    .expect("core deserialize")
            )
            .expect("core serialize"),
            json
        );
    }
    for content in [
        serde_json::json!({ "type": "join_rule", "rule": "secret" }),
        serde_json::json!({ "type": "name", "name": "Ops", "unexpected": true }),
    ] {
        assert!(
            serde_json::from_value::<RoomStateNoticeContent>(content.clone()).is_err(),
            "{content}"
        );
    }
}

#[test]
fn room_activity_unsupported_message_round_trips_through_json() {
    let activity = RoomActivity {
        room_id: "!room:example".to_owned(),
        activity_id: "event:$custom1:example.org".to_owned(),
        timestamp_ms: 1_710_000_000_000,
        highlight: false,
        relation: None,
        reactions: Vec::new(),
        kind: RoomActivityKind::UnsupportedMessage(UnsupportedRoomMessageActivity {
            event_id: "$custom1:example.org".to_owned(),
            sender: "@alice:example.org".to_owned(),
            msgtype: "com.example.custom".to_owned(),
            body: "custom payload".to_owned(),
        }),
    };

    let json = serde_json::json!({
        "room_id": "!room:example",
        "activity_id": "event:$custom1:example.org",
        "timestamp_ms": 1710000000000u64,
        "highlight": false,
        "relation": null,
        "reactions": [],
        "kind": {
            "type": "unsupported_message",
            "event_id": "$custom1:example.org",
            "sender": "@alice:example.org",
            "msgtype": "com.example.custom",
            "body": "custom payload"
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
fn room_activity_unable_to_decrypt_round_trips_through_json() {
    let activity = RoomActivity {
        room_id: "!room:example".to_owned(),
        activity_id: "$encrypted1:example.org".to_owned(),
        timestamp_ms: 1_710_000_000_000,
        highlight: false,
        relation: None,
        reactions: Vec::new(),
        kind: RoomActivityKind::UnableToDecrypt(RoomUnableToDecryptActivity {
            event_id: "$encrypted1:example.org".to_owned(),
            sender: "@alice:example.org".to_owned(),
            session_id: Some("SESSIONID".to_owned()),
            cause: RoomUnableToDecryptCause::SentBeforeYouJoined,
        }),
    };
    let json = serde_json::json!({
        "room_id":"!room:example", "activity_id":"$encrypted1:example.org",
        "timestamp_ms":1710000000000u64, "highlight":false, "relation":null, "reactions":[],
        "kind":{"type":"unable_to_decrypt", "event_id":"$encrypted1:example.org",
        "sender":"@alice:example.org", "session_id":"SESSIONID",
        "cause":"sent_before_you_joined"}
    });

    assert_eq!(serde_json::to_value(&activity).unwrap(), json);
    assert_eq!(
        serde_json::from_value::<RoomActivity>(json).unwrap(),
        activity
    );
}

#[test]
fn room_activity_unable_to_decrypt_rejects_unknown_fields() {
    let json = serde_json::json!({
        "room_id":"!room:example", "activity_id":"$encrypted1:example.org",
        "timestamp_ms":1710000000000u64, "highlight":false, "relation":null, "reactions":[],
        "kind":{"type":"unable_to_decrypt", "event_id":"$encrypted1:example.org",
        "sender":"@alice:example.org", "session_id":"SESSIONID",
        "cause":"unknown", "unexpected":true}
    });

    assert!(serde_json::from_value::<RoomActivity>(json.clone()).is_err());
    assert!(serde_json::from_value::<CoreRoomActivity>(json).is_err());
}

/// The core wire names why a line could not be decrypted by its cause; the
/// old `reason` is refused, and every cause round-trips with the mirror.
#[test]
fn core_room_activity_unable_to_decrypt_carries_its_cause() {
    let core = |extra: serde_json::Value| {
        let mut kind = serde_json::json!({
            "type":"unable_to_decrypt", "event_id":"$encrypted1:example.org",
            "sender":{"user_id":"@alice:example.org", "nick":null,
                "nick_ambiguous":false},
            "session_id":"SESSIONID",
        });
        kind.as_object_mut()
            .unwrap()
            .extend(extra.as_object().unwrap().clone());
        serde_json::from_value::<CoreRoomActivity>(serde_json::json!({
            "room_id":"!room:example", "activity_id":"event:$encrypted1:example.org",
            "timestamp_ms":1710000000000u64, "highlight":false, "relation":null,
            "kind":kind,
        }))
    };
    assert!(core(serde_json::json!({"reason":"missing_megolm_session"})).is_err());
    for (wire, cause) in [
        ("unknown", RoomUnableToDecryptCause::Unknown),
        (
            "sent_before_you_joined",
            RoomUnableToDecryptCause::SentBeforeYouJoined,
        ),
        (
            "historical_no_backup",
            RoomUnableToDecryptCause::HistoricalNoBackup,
        ),
        (
            "historical_unverified",
            RoomUnableToDecryptCause::HistoricalUnverified,
        ),
        (
            "withheld_unverified",
            RoomUnableToDecryptCause::WithheldUnverified,
        ),
        (
            "withheld_by_sender",
            RoomUnableToDecryptCause::WithheldBySender,
        ),
        (
            "sender_identity_changed",
            RoomUnableToDecryptCause::SenderIdentityChanged,
        ),
        ("unsigned_device", RoomUnableToDecryptCause::UnsignedDevice),
        ("unknown_device", RoomUnableToDecryptCause::UnknownDevice),
    ] {
        let activity = core(serde_json::json!({ "cause": wire })).expect(wire);
        let mirx_core::RoomActivityKind::UnableToDecrypt(utd) = &activity.kind else {
            panic!("not a UTD: {activity:?}");
        };
        let core_wire = serde_json::to_value(utd.cause).unwrap();
        assert_eq!(core_wire, serde_json::json!(wire));
        assert_eq!(
            serde_json::from_value::<RoomUnableToDecryptCause>(core_wire).unwrap(),
            cause
        );
    }
}

#[test]
fn room_activity_image_round_trips_through_json() {
    let activity = RoomActivity {
        room_id: "!room:example".to_owned(),
        activity_id: "event:$image1:example.org".to_owned(),
        timestamp_ms: 1_710_000_000_000,
        highlight: false,
        relation: None,
        reactions: Vec::new(),
        kind: RoomActivityKind::Image(Box::new(sample_room_image_activity(None))),
    };

    let json = serde_json::json!({
        "room_id": "!room:example",
        "activity_id": "event:$image1:example.org",
        "timestamp_ms": 1710000000000u64,
        "highlight": false,
        "relation": null,
        "reactions": [],
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
fn room_activity_sticker_round_trips_through_json() {
    let sticker = RoomImageActivity {
        body: "a waving cat".to_owned(),
        filename: None,
        ..sample_room_image_activity(None)
    };
    let activity = RoomActivity {
        room_id: "!room:example".to_owned(),
        activity_id: "event:$image1:example.org".to_owned(),
        timestamp_ms: 1_710_000_000_000,
        highlight: false,
        relation: None,
        reactions: Vec::new(),
        kind: RoomActivityKind::Sticker(Box::new(sticker)),
    };
    let kind = serde_json::json!({
        "type": "sticker",
        "event_id": "$image1:example.org",
        "sender": "@alice:example.org",
        "body": "a waving cat",
        "filename": null,
        "source": { "type": "plain", "url": "mxc://example.org/diagram" },
        "info": {
            "mimetype": "image/png",
            "size": 12345u64,
            "width": 640u64,
            "height": 480u64,
            "is_animated": false,
            "thumbnail_source": null
        }
    });
    let mut dump_kind = kind.clone();
    dump_kind["media_state"] = serde_json::Value::Null;
    let json = serde_json::json!({
        "room_id": "!room:example",
        "activity_id": "event:$image1:example.org",
        "timestamp_ms": 1710000000000u64,
        "highlight": false,
        "relation": null,
        "reactions": [],
        "kind": dump_kind
    });
    assert_eq!(
        serde_json::to_value(&activity).expect("serialize sticker activity"),
        json
    );
    assert_eq!(
        serde_json::from_value::<RoomActivity>(json).expect("deserialize sticker activity"),
        activity
    );

    // The core's: its sender is a profile.
    let mut core_kind = kind;
    core_kind["sender"] = serde_json::json!({
        "user_id": "@alice:example.org",
        "nick": null,
        "nick_ambiguous": false
    });
    let core_json = serde_json::json!({
        "room_id": "!room:example",
        "activity_id": "event:$image1:example.org",
        "timestamp_ms": 1710000000000u64,
        "highlight": false,
        "relation": null,
        "kind": core_kind
    });
    let core = serde_json::from_value::<CoreRoomActivity>(core_json.clone())
        .expect("deserialize core sticker activity");
    assert!(
        matches!(&core.kind, CoreRoomActivityKind::Sticker(sticker) if sticker.body == "a waving cat"),
        "{core:?}"
    );
    assert_eq!(
        serde_json::to_value(&core).expect("serialize core sticker activity"),
        core_json
    );
}

#[test]
fn room_verification_request_activity_round_trips_through_json() {
    let kind = RoomActivityKind::VerificationRequest(RoomVerificationRequestActivity {
        event_id: "$e".to_owned(),
        sender: "@bob:example".to_owned(),
        to: "@carol:example".to_owned(),
    });
    let json = serde_json::json!({
        "type": "verification_request",
        "event_id": "$e",
        "sender": "@bob:example",
        "to": "@carol:example"
    });
    assert_eq!(
        serde_json::to_value(&kind).expect("serialize request"),
        json
    );
    assert_eq!(
        serde_json::from_value::<RoomActivityKind>(json).expect("deserialize request"),
        kind
    );
    let core_json = serde_json::json!({
        "type": "verification_request",
        "event_id": "$e",
        "sender": {
            "user_id": "@bob:example",
            "nick": null,
            "nick_ambiguous": false
        },
        "to": "@carol:example"
    });
    let core = serde_json::from_value::<CoreRoomActivityKind>(core_json.clone())
        .expect("deserialize core request");
    assert_eq!(
        serde_json::to_value(&core).expect("serialize core request"),
        core_json
    );
}

#[test]
fn room_poll_activities_round_trip_through_json() {
    for (content, json) in [
        (
            RoomPollContent::Start {
                question: "Lunch?".to_owned(),
                answers: vec!["Pizza".to_owned(), "Sushi".to_owned()],
                max_selections: 1,
                results_hidden: false,
            },
            serde_json::json!({
                "type": "start",
                "question": "Lunch?",
                "answers": ["Pizza", "Sushi"],
                "max_selections": 1,
                "results_hidden": false
            }),
        ),
        (
            RoomPollContent::End {
                poll_start_event_id: "$poll".to_owned(),
                text: "Pizza won".to_owned(),
            },
            serde_json::json!({
                "type": "end",
                "poll_start_event_id": "$poll",
                "text": "Pizza won"
            }),
        ),
    ] {
        let kind = RoomActivityKind::Poll(Box::new(RoomPollActivity {
            event_id: "$e".to_owned(),
            sender: "@bob:example".to_owned(),
            content,
        }));
        let kind_json = serde_json::json!({
            "type": "poll",
            "event_id": "$e",
            "sender": "@bob:example",
            "content": json.clone()
        });
        assert_eq!(
            serde_json::to_value(&kind).expect("serialize poll"),
            kind_json
        );
        assert_eq!(
            serde_json::from_value::<RoomActivityKind>(kind_json).expect("deserialize poll"),
            kind
        );

        // The core's: its sender is a profile.
        let core_json = serde_json::json!({
            "type": "poll",
            "event_id": "$e",
            "sender": {
                "user_id": "@bob:example",
                "nick": null,
                "nick_ambiguous": false
            },
            "content": json
        });
        let core = serde_json::from_value::<CoreRoomActivityKind>(core_json.clone())
            .expect("deserialize core poll");
        assert_eq!(
            serde_json::to_value(&core).expect("serialize core poll"),
            core_json
        );
    }
    assert!(
        serde_json::from_value::<RoomPollContent>(serde_json::json!({
            "type": "end",
            "poll_start_event_id": "$poll",
            "text": "Pizza won",
            "votes": 3
        }))
        .is_err()
    );
}

#[test]
fn room_activity_file_round_trips_through_json() {
    let activity = RoomActivity {
        room_id: "!room:example".to_owned(),
        activity_id: "event:$file1:example.org".to_owned(),
        timestamp_ms: 1_710_000_010_000,
        highlight: false,
        relation: None,
        reactions: Vec::new(),
        kind: RoomActivityKind::File(Box::new(sample_room_file_activity())),
    };

    let json = serde_json::json!({
        "room_id": "!room:example",
        "activity_id": "event:$file1:example.org",
        "timestamp_ms": 1710000010000u64,
        "highlight": false,
        "relation": null,
        "reactions": [],
        "kind": {
            "type": "file",
            "event_id": "$file1:example.org",
            "sender": "@alice:example.org",
            "body": "report.pdf",
            "filename": "incident-report.pdf",
            "source": {
                "type": "plain",
                "url": "mxc://example.org/report"
            },
            "info": {
                "mimetype": "application/pdf",
                "size": 54321u64
            }
        }
    });

    assert_eq!(
        serde_json::to_value(&activity).expect("serialize file activity"),
        json
    );
    assert_eq!(
        serde_json::from_value::<RoomActivity>(json).expect("deserialize file activity"),
        activity
    );
}

#[test]
fn room_activity_audio_round_trips_through_json() {
    let activity = RoomActivity {
        room_id: "!room:example".to_owned(),
        activity_id: "event:$audio1:example.org".to_owned(),
        timestamp_ms: 1_710_000_020_000,
        highlight: false,
        relation: None,
        reactions: Vec::new(),
        kind: RoomActivityKind::Audio(Box::new(sample_room_audio_activity())),
    };

    let json = serde_json::json!({
        "room_id": "!room:example",
        "activity_id": "event:$audio1:example.org",
        "timestamp_ms": 1710000020000u64,
        "highlight": false,
        "relation": null,
        "reactions": [],
        "kind": {
            "type": "audio",
            "event_id": "$audio1:example.org",
            "sender": "@alice:example.org",
            "body": "voice-note.ogg",
            "filename": "voice-note.ogg",
            "source": {
                "type": "encrypted",
                "url": "mxc://example.org/voice-note",
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
            "info": {
                "mimetype": "audio/ogg",
                "size": 9876u64,
                "duration_ms": 12345u64
            }
        }
    });

    assert_eq!(
        serde_json::to_value(&activity).expect("serialize audio activity"),
        json
    );
    assert_eq!(
        serde_json::from_value::<RoomActivity>(json).expect("deserialize audio activity"),
        activity
    );
}

#[test]
fn room_activity_video_round_trips_through_json() {
    let activity = RoomActivity {
        room_id: "!room:example".to_owned(),
        activity_id: "event:$custom1:example.org".to_owned(),
        timestamp_ms: 1_710_000_030_000,
        highlight: false,
        relation: None,
        reactions: Vec::new(),
        kind: RoomActivityKind::Video(Box::new(sample_room_video_activity(None))),
    };

    let json = serde_json::to_value(&activity).expect("serialize video activity");
    assert_eq!(json["kind"]["type"], "video");
    assert_eq!(json["kind"]["info"]["mimetype"], "video/mp4");
    assert_eq!(json["kind"]["info"]["duration_ms"], 12345u64);
    assert_eq!(json["kind"]["info"]["width"], 1920u64);
    assert_eq!(json["kind"]["info"]["height"], 1080u64);
    assert_eq!(
        serde_json::from_value::<RoomActivity>(json).expect("deserialize video activity"),
        activity
    );
}

#[test]
fn room_activity_video_media_states_round_trip_through_json() {
    for media_state in [
        RoomImageMediaState::Pending,
        RoomImageMediaState::Loaded {
            width: 64,
            height: 48,
        },
        RoomImageMediaState::Failed {
            message: "download failed".to_owned(),
        },
    ] {
        let video = sample_room_video_activity(Some(media_state));
        let json = serde_json::to_value(&video).expect("serialize video activity");
        assert_eq!(
            serde_json::from_value::<RoomVideoActivity>(json).expect("deserialize video activity"),
            video
        );
    }
}

#[test]
fn room_video_types_reject_unknown_fields() {
    assert!(
        serde_json::from_str::<RoomVideoActivity>(
            r#"{
            "event_id":"$video1:example.org",
            "sender":"@alice:example.org",
            "body":"clip.mp4",
            "filename":"clip.mp4",
            "source":{"type":"plain","url":"mxc://example.org/clip"},
            "info":null,
            "unexpected":true
        }"#
        )
        .is_err()
    );
    assert!(
        serde_json::from_str::<RoomVideoInfo>(
            r#"{
            "mimetype":"video/mp4",
            "size":54321,
            "duration_ms":12345,
            "width":1920,
            "height":1080,
            "thumbnail_source":null,
            "unexpected":true
        }"#
        )
        .is_err()
    );
}

#[test]
fn room_activity_image_pending_media_state_round_trips_through_json() {
    let activity = RoomActivity {
        room_id: "!room:example".to_owned(),
        activity_id: "event:$image1:example.org".to_owned(),
        timestamp_ms: 1_710_000_000_000,
        highlight: false,
        relation: None,
        reactions: Vec::new(),
        kind: RoomActivityKind::Image(Box::new(sample_room_image_activity(Some(
            RoomImageMediaState::Pending,
        )))),
    };

    let json = serde_json::json!({
        "room_id": "!room:example",
        "activity_id": "event:$image1:example.org",
        "timestamp_ms": 1710000000000u64,
        "highlight": false,
        "relation": null,
        "reactions": [],
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
        highlight: false,
        relation: None,
        reactions: Vec::new(),
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
        "highlight": false,
        "relation": null,
        "reactions": [],
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
        highlight: false,
        relation: None,
        reactions: Vec::new(),
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
        "highlight": false,
        "relation": null,
        "reactions": [],
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
        highlight: false,
        relation: None,
        reactions: Vec::new(),
        kind: RoomActivityKind::Redacted(RoomRedactedActivity {
            event_id: "$redacted:example.org".to_owned(),
        }),
    };

    let json = serde_json::json!({
        "room_id": "!room:example",
        "activity_id": "event:$redacted:example.org",
        "timestamp_ms": 1710000060000u64,
        "highlight": false,
        "relation": null,
        "reactions": [],
        "kind": {
            "type": "redacted",
            "event_id": "$redacted:example.org"
        }
    });

    assert_eq!(
        serde_json::to_value(&activity).expect("serialize redacted activity"),
        json.clone()
    );
    assert_eq!(
        serde_json::from_value::<RoomActivity>(json.clone())
            .expect("deserialize redacted activity"),
        activity
    );
    let core_json = serde_json::json!({
        "room_id": "!room:example",
        "activity_id": "event:$redacted:example.org",
        "timestamp_ms": 1710000060000u64,
        "highlight": false,
        "relation": null,
        "kind": {
            "type": "redacted",
            "event_id": "$redacted:example.org"
        }
    });

    assert_eq!(
        serde_json::to_value(CoreRoomActivity {
            room_id: "!room:example".to_owned(),
            activity_id: "event:$redacted:example.org".to_owned(),
            timestamp_ms: 1_710_000_060_000,
            highlight: false,
            relation: None,
            kind: CoreRoomActivityKind::Redacted(mirx_core::RoomRedactedActivity {
                event_id: "$redacted:example.org".to_owned(),
            }),
        })
        .expect("serialize core redacted activity"),
        core_json.clone()
    );
    assert_eq!(
        serde_json::from_value::<CoreRoomActivity>(core_json)
            .expect("deserialize core redacted activity"),
        CoreRoomActivity {
            room_id: "!room:example".to_owned(),
            activity_id: "event:$redacted:example.org".to_owned(),
            timestamp_ms: 1_710_000_060_000,
            highlight: false,
            relation: None,
            kind: CoreRoomActivityKind::Redacted(mirx_core::RoomRedactedActivity {
                event_id: "$redacted:example.org".to_owned(),
            }),
        }
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
        "highlight": false,
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
            highlight: false,
            relation: None,
            reactions: Vec::new(),
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
        highlight: false,
        relation: Some(RoomActivityRelation::Thread {
            thread_root_event_id: "$thread-root:example".to_owned(),
            in_reply_to_event_id: Some("$reply:example".to_owned()),
            is_falling_back: true,
        }),
        reactions: Vec::new(),
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
        "highlight": false,
        "relation": {
            "type": "thread",
            "thread_root_event_id": "$thread-root:example",
            "in_reply_to_event_id": "$reply:example",
            "is_falling_back": true,
        },
        "reactions": [],
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
fn room_activity_dump_reaction_summaries_round_trip_and_default() {
    let with_reactions = serde_json::json!({
        "room_id": "!room:example",
        "activity_id": "event:$event:example",
        "timestamp_ms": 1700000000000u64,
        "highlight": false,
        "relation": null,
        "reactions": [
            {
                "key": "+1",
                "count": 2u64,
                "senders": ["@alice:example", "@bob:example"],
                "reaction_event_ids": ["$reaction-1:example", "$reaction-2:example"]
            }
        ],
        "kind": {
            "type": "message",
            "event_id": "$event:example",
            "transaction_id": null,
            "sender": "@carol:example",
            "presentation": "text",
            "body": "hello",
            "send_state": {
                "state": "sent"
            }
        }
    });
    let activity_with_reactions = RoomActivity {
        room_id: "!room:example".to_owned(),
        activity_id: "event:$event:example".to_owned(),
        timestamp_ms: 1_700_000_000_000,
        highlight: false,
        relation: None,
        reactions: vec![RoomReactionSummary {
            key: "+1".to_owned(),
            count: 2,
            senders: vec!["@alice:example".to_owned(), "@bob:example".to_owned()],
            reaction_event_ids: vec![
                "$reaction-1:example".to_owned(),
                "$reaction-2:example".to_owned(),
            ],
        }],
        kind: RoomActivityKind::Message(RoomMessageActivity {
            event_id: Some("$event:example".to_owned()),
            transaction_id: None,
            sender: "@carol:example".to_owned(),
            presentation: RoomMessagePresentationKind::Text,
            body: "hello".to_owned(),
            send_state: RoomMessageSendState::Sent,
        }),
    };

    assert_eq!(
        serde_json::from_value::<RoomActivity>(with_reactions)
            .expect("deserialize activity dump with reactions"),
        activity_with_reactions.clone()
    );
    assert_eq!(
        serde_json::to_value(&activity_with_reactions)
            .expect("serialize activity dump with reactions"),
        serde_json::json!({
            "room_id": "!room:example",
            "activity_id": "event:$event:example",
            "timestamp_ms": 1700000000000u64,
            "highlight": false,
            "relation": null,
            "reactions": [
                {
                    "key": "+1",
                    "count": 2u64,
                    "senders": ["@alice:example", "@bob:example"],
                    "reaction_event_ids": ["$reaction-1:example", "$reaction-2:example"]
                }
            ],
            "kind": {
                "type": "message",
                "event_id": "$event:example",
                "transaction_id": null,
                "sender": "@carol:example",
                "presentation": "text",
                "body": "hello",
                "send_state": {
                    "state": "sent"
                }
            }
        })
    );

    let without_reactions = serde_json::json!({
        "room_id": "!room:example",
        "activity_id": "event:$event:example",
        "timestamp_ms": 1700000000000u64,
        "highlight": false,
        "kind": {
            "type": "message",
            "event_id": "$event:example",
            "transaction_id": null,
            "sender": "@carol:example",
            "presentation": "text",
            "body": "hello",
            "send_state": {
                "state": "sent"
            }
        }
    });
    let activity = serde_json::from_value::<RoomActivity>(without_reactions)
        .expect("deserialize activity dump without reactions");

    assert!(activity.reactions.is_empty());
    assert_eq!(
        serde_json::to_value(&activity).expect("serialize activity dump without reactions"),
        serde_json::json!({
            "room_id": "!room:example",
            "activity_id": "event:$event:example",
            "timestamp_ms": 1700000000000u64,
            "highlight": false,
            "relation": null,
            "reactions": [],
            "kind": {
                "type": "message",
                "event_id": "$event:example",
                "transaction_id": null,
                "sender": "@carol:example",
                "presentation": "text",
                "body": "hello",
                "send_state": {
                    "state": "sent"
                }
            }
        })
    );
    assert!(
        serde_json::from_value::<RoomReactionSummary>(serde_json::json!({
            "key": "+1",
            "count": 1u64,
            "senders": ["@alice:example"],
            "reaction_event_ids": ["$reaction-1:example"],
            "unexpected": true,
        }))
        .is_err()
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
            styles: Vec::new(),
            user_links: Vec::new(),
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
    let reaction_added = RoomActivityUpdate {
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
    };
    // A redaction of a reaction names the reaction.
    let reaction_redacted = RoomActivityUpdate {
        room_id: "!ops:example.org".to_owned(),
        target: RoomActivityUpdateTarget::Event {
            event_id: "$reaction:example.org".to_owned(),
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
            "body": "edited body",
            "styles": [],
            "user_links": []
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
    let reaction_added_json = serde_json::json!({
        "room_id": "!ops:example.org",
        "target": {
            "type": "event",
            "event_id": "$original:example.org"
        },
        "kind": {
            "type": "reaction_added",
            "reaction_event_id": "$reaction:example.org",
            "sender": "@alice:example.org",
            "timestamp_ms": 1700000001500u64,
            "key": "+1"
        }
    });
    let reaction_redacted_json = serde_json::json!({
        "room_id": "!ops:example.org",
        "target": {
            "type": "event",
            "event_id": "$reaction:example.org"
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
        serde_json::to_value(&reaction_added).expect("serialize reaction added update"),
        reaction_added_json
    );
    assert_eq!(
        serde_json::to_value(&reaction_redacted).expect("serialize reaction redacted update"),
        reaction_redacted_json
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
                presentation: mirx_core::RoomMessagePresentationKind::Text,
                body: "edited body".to_owned(),
                styles: Vec::new(),
                user_links: Vec::new(),
            },
        }
    );
    assert_eq!(
        serde_json::from_value::<CoreRoomActivityUpdate>(redacted_json.clone())
            .expect("deserialize redacted update as core"),
        CoreRoomActivityUpdate {
            room_id: "!ops:example.org".to_owned(),
            target: CoreRoomActivityUpdateTarget::Event {
                event_id: "$original:example.org".to_owned(),
            },
            kind: CoreRoomActivityUpdateKind::ActivityRedacted {
                redaction_event_id: "$redaction:example.org".to_owned(),
            },
        }
    );
    assert_eq!(
        serde_json::from_value::<CoreRoomActivityUpdate>(reaction_added_json.clone())
            .expect("deserialize reaction added update as core"),
        CoreRoomActivityUpdate {
            room_id: "!ops:example.org".to_owned(),
            target: CoreRoomActivityUpdateTarget::Event {
                event_id: "$original:example.org".to_owned(),
            },
            kind: CoreRoomActivityUpdateKind::ReactionAdded {
                reaction_event_id: "$reaction:example.org".to_owned(),
                sender: "@alice:example.org".to_owned(),
                timestamp_ms: 1_700_000_001_500,
                key: "+1".to_owned(),
            },
        }
    );
    assert_eq!(
        serde_json::from_value::<CoreRoomActivityUpdate>(reaction_redacted_json.clone())
            .expect("deserialize reaction redacted update as core"),
        CoreRoomActivityUpdate {
            room_id: "!ops:example.org".to_owned(),
            target: CoreRoomActivityUpdateTarget::Event {
                event_id: "$reaction:example.org".to_owned(),
            },
            kind: CoreRoomActivityUpdateKind::ActivityRedacted {
                redaction_event_id: "$redaction:example.org".to_owned(),
            },
        }
    );
    assert_eq!(
        serde_json::to_value(CoreRoomActivityUpdate {
            room_id: "!ops:example.org".to_owned(),
            target: CoreRoomActivityUpdateTarget::Event {
                event_id: "$original:example.org".to_owned(),
            },
            kind: CoreRoomActivityUpdateKind::ActivityRedacted {
                redaction_event_id: "$redaction:example.org".to_owned(),
            },
        })
        .expect("serialize redacted update as core"),
        redacted_json.clone()
    );
    assert_eq!(
        serde_json::to_value(CoreRoomActivityUpdate {
            room_id: "!ops:example.org".to_owned(),
            target: CoreRoomActivityUpdateTarget::Event {
                event_id: "$original:example.org".to_owned(),
            },
            kind: CoreRoomActivityUpdateKind::ReactionAdded {
                reaction_event_id: "$reaction:example.org".to_owned(),
                sender: "@alice:example.org".to_owned(),
                timestamp_ms: 1_700_000_001_500,
                key: "+1".to_owned(),
            },
        })
        .expect("serialize reaction added update as core"),
        reaction_added_json.clone()
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
    assert_eq!(
        serde_json::from_value::<RoomActivityUpdate>(reaction_added_json)
            .expect("deserialize reaction added update"),
        reaction_added
    );
    assert_eq!(
        serde_json::from_value::<RoomActivityUpdate>(reaction_redacted_json)
            .expect("deserialize reaction redacted update"),
        reaction_redacted
    );
}

#[test]
fn room_activity_redaction_contract_shapes_reject_invalid_payloads() {
    assert!(
        serde_json::from_str::<RoomActivityUpdateKind>(r#"{"type":"activity_redacted"}"#).is_err(),
        "activity redacted kind must require redaction_event_id"
    );
    assert!(
        serde_json::from_str::<CoreRoomActivityUpdateKind>(r#"{"type":"activity_redacted"}"#)
            .is_err(),
        "core activity redacted kind must require redaction_event_id"
    );
    assert!(
        serde_json::from_str::<RoomActivityUpdateKind>(
            r#"{"type":"activity_redacted","redaction_event_id":"$redaction:example.org","unexpected":"value"}"#
        )
        .is_err(),
        "activity redacted kind must reject unknown fields"
    );
    assert!(
        serde_json::from_str::<CoreRoomActivityUpdateKind>(
            r#"{"type":"activity_redacted","redaction_event_id":"$redaction:example.org","unexpected":"value"}"#
        )
        .is_err(),
        "core activity redacted kind must reject unknown fields"
    );
    for missing_field_json in [
        r#"{"type":"reaction_added","sender":"@alice:example.org","timestamp_ms":1700000001500,"key":"+1"}"#,
        r#"{"type":"reaction_added","reaction_event_id":"$reaction:example.org","timestamp_ms":1700000001500,"key":"+1"}"#,
        r#"{"type":"reaction_added","reaction_event_id":"$reaction:example.org","sender":"@alice:example.org","key":"+1"}"#,
        r#"{"type":"reaction_added","reaction_event_id":"$reaction:example.org","sender":"@alice:example.org","timestamp_ms":1700000001500}"#,
    ] {
        assert!(
            serde_json::from_str::<RoomActivityUpdateKind>(missing_field_json).is_err(),
            "reaction added kind must reject payload missing a required field: {missing_field_json}"
        );
        assert!(
            serde_json::from_str::<CoreRoomActivityUpdateKind>(missing_field_json).is_err(),
            "core reaction added kind must reject payload missing a required field: {missing_field_json}"
        );
    }
    assert!(
        serde_json::from_str::<RoomActivityUpdateKind>(
            r#"{"type":"reaction_added","reaction_event_id":"$reaction:example.org","sender":"@alice:example.org","timestamp_ms":1700000001500,"key":"+1","unexpected":"value"}"#
        )
        .is_err(),
        "reaction added kind must reject unknown fields"
    );
    assert!(
        serde_json::from_str::<CoreRoomActivityUpdateKind>(
            r#"{"type":"reaction_added","reaction_event_id":"$reaction:example.org","sender":"@alice:example.org","timestamp_ms":1700000001500,"key":"+1","unexpected":"value"}"#
        )
        .is_err(),
        "core reaction added kind must reject unknown fields"
    );
    // A redaction of a reaction is an `activity_redacted` of the reaction.
    let reaction_removed = r#"{"type":"reaction_removed","reaction_event_id":"$reaction:example.org","redaction_event_id":"$redaction:example.org","key":"+1"}"#;
    assert!(
        serde_json::from_str::<RoomActivityUpdateKind>(reaction_removed).is_err(),
        "reaction removed kind is no update kind"
    );
    assert!(
        serde_json::from_str::<CoreRoomActivityUpdateKind>(reaction_removed).is_err(),
        "core reaction removed kind is no update kind"
    );
    assert!(
        serde_json::from_str::<RoomActivityKind>(r#"{"type":"redacted"}"#).is_err(),
        "redacted activity must require event_id"
    );
    assert!(
        serde_json::from_str::<CoreRoomActivityKind>(r#"{"type":"redacted"}"#).is_err(),
        "core redacted activity must require event_id"
    );
    assert!(
        serde_json::from_str::<RoomActivityKind>(
            r#"{"type":"redacted","event_id":"$redacted:example.org","unexpected":"value"}"#
        )
        .is_err(),
        "redacted activity must reject unknown fields"
    );
    assert!(
        serde_json::from_str::<CoreRoomActivityKind>(
            r#"{"type":"redacted","event_id":"$redacted:example.org","unexpected":"value"}"#
        )
        .is_err(),
        "core redacted activity must reject unknown fields"
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
            r#"{"type":"message_replaced","replacement_event_id":"$edit:example.org","sender":"@alice:example.org","timestamp_ms":1700000001000,"presentation":"text","body":"edited body","styles":[],"user_links":[],"unexpected":"value"}"#
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
        fully_read_timestamp_ms: Some(1_700_000_000_000),
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
                "fully_read_timestamp_ms": 1700000000000u64,
                "marked_unread": true,
                "num_unread": 7,
                "num_notifications": 3,
                "num_mentions": 1
            }
        })
    );
}

/// An unknown read marker time may be left out.
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
            fully_read_timestamp_ms: None,
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
        serde_json::from_value::<CoreFetchRoomMediaRequest>(json)
            .expect("deserialize core image media request"),
        CoreFetchRoomMediaRequest {
            room_id: "!room:example".to_owned(),
            activity_id: "event:$image:example".to_owned(),
            event_id: "$image:example".to_owned(),
            source: CoreRoomMediaSource::Plain {
                url: "mxc://example.org/image".to_owned(),
            },
            format: CoreRoomMediaFormat::Thumbnail {
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
        serde_json::from_value::<CoreFetchRoomMediaRequest>(json)
            .expect("deserialize core encrypted image media request"),
        CoreFetchRoomMediaRequest {
            room_id: "!room:example".to_owned(),
            activity_id: "event:$image:example".to_owned(),
            event_id: "$image:example".to_owned(),
            source: CoreRoomMediaSource::Encrypted {
                url: "mxc://example.org/encrypted-image".to_owned(),
                key: mirx_core::RoomEncryptedFileKey {
                    kty: "oct".to_owned(),
                    key_ops: vec!["encrypt".to_owned(), "decrypt".to_owned()],
                    alg: "A256CTR".to_owned(),
                    k: "base64url-key".to_owned(),
                    ext: true,
                },
                iv: "base64-iv".to_owned(),
                hashes: mirx_core::RoomEncryptedFileHashes {
                    sha256: "base64-sha256".to_owned(),
                },
                v: "v2".to_owned(),
            },
            format: CoreRoomMediaFormat::Thumbnail {
                width: 160,
                height: 120,
            },
        }
    );
}

#[test]
fn backfill_room_history_request_round_trips_through_json_across_packages() {
    for (request, core_request, json) in [
        (
            BackfillRoomHistoryRequest {
                room_id: "!room:example".to_owned(),
                from: HistoryFrom::Latest,
                limit: 50,
            },
            CoreBackfillRoomHistoryRequest {
                room_id: "!room:example".to_owned(),
                from: CoreHistoryFrom::Latest,
                limit: 50,
            },
            serde_json::json!({
                "room_id": "!room:example",
                "from": { "type": "latest" },
                "limit": 50
            }),
        ),
        (
            BackfillRoomHistoryRequest {
                room_id: "!room:example".to_owned(),
                from: HistoryFrom::Token {
                    token: "token".to_owned(),
                },
                limit: 50,
            },
            CoreBackfillRoomHistoryRequest {
                room_id: "!room:example".to_owned(),
                from: CoreHistoryFrom::Token {
                    token: "token".to_owned(),
                },
                limit: 50,
            },
            serde_json::json!({
                "room_id": "!room:example",
                "from": { "type": "token", "token": "token" },
                "limit": 50
            }),
        ),
        (
            BackfillRoomHistoryRequest {
                room_id: "!room:example".to_owned(),
                from: HistoryFrom::BeforeEvent {
                    event_id: "$kept:example".to_owned(),
                },
                limit: 50,
            },
            CoreBackfillRoomHistoryRequest {
                room_id: "!room:example".to_owned(),
                from: CoreHistoryFrom::BeforeEvent {
                    event_id: "$kept:example".to_owned(),
                },
                limit: 50,
            },
            serde_json::json!({
                "room_id": "!room:example",
                "from": { "type": "before_event", "event_id": "$kept:example" },
                "limit": 50
            }),
        ),
    ] {
        assert_eq!(
            serde_json::to_value(&request).expect("serialize testkit backfill request"),
            json.clone()
        );
        assert_eq!(
            serde_json::from_value::<BackfillRoomHistoryRequest>(json.clone())
                .expect("deserialize testkit backfill request"),
            request
        );
        assert_eq!(
            serde_json::to_value(&core_request).expect("serialize core backfill request"),
            json.clone()
        );
        assert_eq!(
            serde_json::from_value::<CoreBackfillRoomHistoryRequest>(json)
                .expect("deserialize core backfill request"),
            core_request
        );
    }
}

#[test]
fn room_history_backfill_event_activity_ingests_core_json_and_defaults_reactions() {
    let core_json = serde_json::json!({
        "type": "activity",
        "room_id": "!ops:example.org",
        "activity_id": "event:$message:example.org",
        "timestamp_ms": 1700000000000u64,
        "highlight": false,
        "relation": null,
        "kind": {
            "type": "redacted",
            "event_id": "$message:example.org"
        }
    });
    let event = RoomHistoryBackfillEvent::Activity(Box::new(sample_room_history_activity()));
    let testkit_json = serde_json::json!({
        "type": "activity",
        "room_id": "!ops:example.org",
        "activity_id": "event:$message:example.org",
        "timestamp_ms": 1700000000000u64,
        "highlight": false,
        "relation": null,
        "reactions": [],
        "kind": {
            "type": "redacted",
            "event_id": "$message:example.org"
        }
    });

    assert_eq!(
        serde_json::from_value::<RoomHistoryBackfillEvent>(core_json.clone())
            .expect("deserialize core-shaped history activity event into testkit"),
        event
    );
    assert_eq!(
        serde_json::to_value(&event).expect("serialize testkit backfill activity event"),
        testkit_json
    );

    let core_event =
        CoreRoomHistoryBackfillEvent::Activity(Box::new(sample_core_room_history_activity()));

    assert_eq!(
        serde_json::to_value(&core_event).expect("serialize core backfill activity event"),
        core_json.clone()
    );
    assert_eq!(
        serde_json::from_value::<CoreRoomHistoryBackfillEvent>(core_json)
            .expect("deserialize core backfill activity event"),
        core_event
    );
}

#[test]
fn room_history_backfill_event_activity_update_round_trips_through_json_across_packages() {
    let event = RoomHistoryBackfillEvent::ActivityUpdate(sample_room_history_activity_update());
    let json = serde_json::json!({
        "type": "activity_update",
        "room_id": "!ops:example.org",
        "target": {
            "type": "event",
            "event_id": "$message:example.org"
        },
        "kind": {
            "type": "reaction_added",
            "reaction_event_id": "$reaction:example.org",
            "sender": "@bob:example.org",
            "timestamp_ms": 1700000000100u64,
            "key": "heart"
        }
    });

    assert_eq!(
        serde_json::to_value(&event).expect("serialize testkit backfill activity update event"),
        json.clone()
    );
    assert_eq!(
        serde_json::from_value::<RoomHistoryBackfillEvent>(json.clone())
            .expect("deserialize testkit backfill activity update event"),
        event
    );

    let core_event =
        CoreRoomHistoryBackfillEvent::ActivityUpdate(sample_core_room_history_activity_update());
    assert_eq!(
        serde_json::to_value(&core_event).expect("serialize core backfill activity update event"),
        json.clone()
    );
    assert_eq!(
        serde_json::from_value::<CoreRoomHistoryBackfillEvent>(json)
            .expect("deserialize core backfill activity update event"),
        core_event
    );
}

#[test]
fn room_history_backfill_with_updates_round_trips_through_json_across_packages() {
    let backfill = RoomHistoryBackfill {
        room_id: "!ops:example.org".to_owned(),
        end: Some("t4747337_219379_2603".to_owned()),
        events: vec![RoomHistoryBackfillEvent::ActivityUpdate(
            sample_room_history_activity_update(),
        )],
    };
    let json = serde_json::json!({
        "room_id": "!ops:example.org",
        "end": "t4747337_219379_2603",
        "events": [
            {
                "type": "activity_update",
                "room_id": "!ops:example.org",
                "target": {
                    "type": "event",
                    "event_id": "$message:example.org"
                },
                "kind": {
                    "type": "reaction_added",
                    "reaction_event_id": "$reaction:example.org",
                    "sender": "@bob:example.org",
                    "timestamp_ms": 1700000000100u64,
                    "key": "heart"
                }
            }
        ]
    });

    assert_eq!(
        serde_json::to_value(&backfill).expect("serialize testkit room history backfill"),
        json.clone()
    );
    assert_eq!(
        serde_json::from_value::<RoomHistoryBackfill>(json.clone())
            .expect("deserialize testkit room history backfill"),
        backfill
    );

    let core_backfill = CoreRoomHistoryBackfill {
        room_id: "!ops:example.org".to_owned(),
        end: Some("t4747337_219379_2603".to_owned()),
        events: vec![CoreRoomHistoryBackfillEvent::ActivityUpdate(
            sample_core_room_history_activity_update(),
        )],
    };

    assert_eq!(
        serde_json::to_value(&core_backfill).expect("serialize core room history backfill"),
        json.clone()
    );
    assert_eq!(
        serde_json::from_value::<CoreRoomHistoryBackfill>(json)
            .expect("deserialize core room history backfill"),
        core_backfill
    );
}

#[test]
fn room_history_backfill_with_activity_ingests_core_json_and_defaults_reactions() {
    let backfill = RoomHistoryBackfill {
        room_id: "!ops:example.org".to_owned(),
        end: Some("t4747337_219379_2603".to_owned()),
        events: vec![
            RoomHistoryBackfillEvent::Activity(Box::new(sample_room_history_activity())),
            RoomHistoryBackfillEvent::ActivityUpdate(sample_room_history_activity_update()),
        ],
    };
    let testkit_json = serde_json::json!({
        "room_id": "!ops:example.org",
        "end": "t4747337_219379_2603",
        "events": [
            {
                "type": "activity",
                "room_id": "!ops:example.org",
                "activity_id": "event:$message:example.org",
                "timestamp_ms": 1700000000000u64,
                "highlight": false,
                "relation": null,
                "reactions": [],
                "kind": {
                    "type": "redacted",
                    "event_id": "$message:example.org"
                }
            },
            {
                "type": "activity_update",
                "room_id": "!ops:example.org",
                "target": {
                    "type": "event",
                    "event_id": "$message:example.org"
                },
                "kind": {
                    "type": "reaction_added",
                    "reaction_event_id": "$reaction:example.org",
                    "sender": "@bob:example.org",
                    "timestamp_ms": 1700000000100u64,
                    "key": "heart"
                }
            }
        ]
    });
    let core_json = serde_json::json!({
        "room_id": "!ops:example.org",
        "end": "t4747337_219379_2603",
        "events": [
            {
                "type": "activity",
                "room_id": "!ops:example.org",
                "activity_id": "event:$message:example.org",
                "timestamp_ms": 1700000000000u64,
                "highlight": false,
                "relation": null,
                "kind": {
                    "type": "redacted",
                    "event_id": "$message:example.org"
                }
            },
            {
                "type": "activity_update",
                "room_id": "!ops:example.org",
                "target": {
                    "type": "event",
                    "event_id": "$message:example.org"
                },
                "kind": {
                    "type": "reaction_added",
                    "reaction_event_id": "$reaction:example.org",
                    "sender": "@bob:example.org",
                    "timestamp_ms": 1700000000100u64,
                    "key": "heart"
                }
            }
        ]
    });

    assert_eq!(
        serde_json::from_value::<RoomHistoryBackfill>(core_json.clone())
            .expect("deserialize core-shaped room history backfill into testkit"),
        backfill
    );
    assert_eq!(
        serde_json::to_value(&backfill).expect("serialize testkit room history backfill"),
        testkit_json
    );

    let core_backfill = CoreRoomHistoryBackfill {
        room_id: "!ops:example.org".to_owned(),
        end: Some("t4747337_219379_2603".to_owned()),
        events: vec![
            CoreRoomHistoryBackfillEvent::Activity(Box::new(sample_core_room_history_activity())),
            CoreRoomHistoryBackfillEvent::ActivityUpdate(sample_core_room_history_activity_update()),
        ],
    };

    assert_eq!(
        serde_json::to_value(&core_backfill).expect("serialize core room history backfill"),
        core_json.clone()
    );
    assert_eq!(
        serde_json::from_value::<CoreRoomHistoryBackfill>(core_json)
            .expect("deserialize core room history backfill"),
        core_backfill
    );
}

#[test]
fn backfill_room_history_request_rejects_unknown_fields_in_testkit_and_core() {
    let json =
        r#"{"room_id":"!room:example","from":{"type":"latest"},"limit":50,"unexpected":"value"}"#;

    assert!(
        serde_json::from_str::<BackfillRoomHistoryRequest>(json).is_err(),
        "testkit backfill request must reject unknown fields"
    );
    assert!(
        serde_json::from_str::<CoreBackfillRoomHistoryRequest>(json).is_err(),
        "core backfill request must reject unknown fields"
    );
}

#[test]
fn room_history_backfill_rejects_unknown_fields_in_testkit_and_core() {
    let json = r#"{"room_id":"!room:example.org","end":null,"events":[],"unexpected":"value"}"#;

    assert!(
        serde_json::from_str::<RoomHistoryBackfill>(json).is_err(),
        "testkit room history backfill must reject unknown fields"
    );
    assert!(
        serde_json::from_str::<CoreRoomHistoryBackfill>(json).is_err(),
        "core room history backfill must reject unknown fields"
    );
}

#[test]
fn room_history_backfill_event_rejects_invalid_tags_in_testkit_and_core() {
    let json = r#"{"type":"unknown","room_id":"!room:example.org"}"#;

    assert!(
        serde_json::from_str::<RoomHistoryBackfillEvent>(json).is_err(),
        "testkit room history backfill event must reject unknown tags"
    );
    assert!(
        serde_json::from_str::<CoreRoomHistoryBackfillEvent>(json).is_err(),
        "core room history backfill event must reject unknown tags"
    );
}

#[test]
fn room_history_backfill_event_rejects_unknown_nested_fields_in_testkit_and_core() {
    let json = r#"{"type":"activity_update","room_id":"!room:example.org","target":{"type":"event","event_id":"$event:example.org"},"kind":{"type":"reaction_added","reaction_event_id":"$reaction:example.org","sender":"@alice:example.org","timestamp_ms":1700000000000,"key":"+1","unexpected":"value"}}"#;

    assert!(
        serde_json::from_str::<RoomHistoryBackfillEvent>(json).is_err(),
        "testkit room history backfill event must reject unknown nested fields"
    );
    assert!(
        serde_json::from_str::<CoreRoomHistoryBackfillEvent>(json).is_err(),
        "core room history backfill event must reject unknown nested fields"
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
        serde_json::from_value::<CoreRoomMedia>(loaded_json)
            .expect("deserialize core loaded room image media"),
        CoreRoomMedia {
            room_id: "!room:example".to_owned(),
            activity_id: "event:$image:example".to_owned(),
            event_id: "$image:example".to_owned(),
            kind: CoreRoomMediaKind::Loaded {
                format: CoreRoomMediaFormat::Thumbnail {
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
        serde_json::from_value::<CoreRoomMedia>(failed_json)
            .expect("deserialize core failed room image media"),
        CoreRoomMedia {
            room_id: "!room:example".to_owned(),
            activity_id: "event:$image:example".to_owned(),
            event_id: "$image:example".to_owned(),
            kind: CoreRoomMediaKind::Failed {
                format: CoreRoomMediaFormat::File,
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

    assert!(serde_json::from_str::<RoomImageMediaKind>(
        r#"{"type":"failed","format":{"type":"file"},"message":"unavailable","unexpected":true}"#
    )
    .is_err());
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
        highlight: false,
        relation: None,
        reactions: Vec::new(),
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
        activity_id: "event:$custom1:example.org".to_owned(),
        timestamp_ms: 1_710_000_000_000,
        highlight: false,
        relation: None,
        reactions: Vec::new(),
        kind: RoomActivityKind::UnsupportedMessage(UnsupportedRoomMessageActivity {
            event_id: "$custom1:example.org".to_owned(),
            sender: "@alice:example.org".to_owned(),
            msgtype: "com.example.custom".to_owned(),
            body: "custom payload".to_owned(),
        }),
    };

    assert!(activity.contains("\"type\":\"unsupported_message\""));
    assert!(activity.contains("\"msgtype\":\"com.example.custom\""));
    assert!(activity.contains("custom payload"));
}

#[test]
fn room_activity_contains_matches_image_fields() {
    let activity = RoomActivity {
        room_id: "!room:example".to_owned(),
        activity_id: "event:$image1:example.org".to_owned(),
        timestamp_ms: 1_710_000_000_000,
        highlight: false,
        relation: None,
        reactions: Vec::new(),
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
        connection: "syncing".to_owned(),
        reconnect_attempts: 0,
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
        last_e2ee_recovery_setup_result: None,
        last_verification: None,
        submitted_membership_commands: Vec::new(),
        submitted_member_moderation_commands: Vec::new(),
        submitted_room_settings_commands: Vec::new(),
        submitted_profile_commands: Vec::new(),
        submitted_away_commands: Vec::new(),
        submitted_typing_commands: Vec::new(),
        submitted_user_commands: Vec::new(),
        submitted_direct_room_commands: Vec::new(),
        submitted_room_topic_commands: Vec::new(),
        submitted_reaction_commands: Vec::new(),
        submitted_edit_commands: Vec::new(),
        submitted_reply_commands: Vec::new(),
        submitted_message_commands: Vec::new(),
        submitted_file_commands: Vec::new(),
        submitted_notification_commands: Vec::new(),
        submitted_redaction_commands: Vec::new(),
        submitted_history_commands: Vec::new(),
        submitted_verification_actions: Vec::new(),
        submitted_verification_requests: Vec::new(),
        identity_warnings: Vec::new(),
        submitted_identity_reset_commands: Vec::new(),
        submitted_room_key_transfers: Vec::new(),
        submitted_session_commands: Vec::new(),
        submitted_recovery_commands: Vec::new(),
        submitted_setup_recovery_commands: Vec::new(),
        submitted_room_list_requests: Vec::new(),
        opened_links: Vec::new(),
        shown_tips: Vec::new(),
        last_error: None,
        selected_room: Some("!room:example".to_owned()),
    };

    assert_eq!(
        serde_json::to_value(&model).expect("serialize model with last_activity"),
        serde_json::json!({
            "status": "synced",
            "lifecycle_state": "ready",
            "connection": "syncing",
            "reconnect_attempts": 0,
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
        connection: "syncing".to_owned(),
        reconnect_attempts: 0,
        rooms: vec![],
        last_activity: None,
        e2ee_status: None,
        last_e2ee_recovery_setup_result: None,
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
        submitted_member_moderation_commands: Vec::new(),
        submitted_room_settings_commands: Vec::new(),
        submitted_profile_commands: Vec::new(),
        submitted_away_commands: Vec::new(),
        submitted_typing_commands: Vec::new(),
        submitted_user_commands: Vec::new(),
        submitted_direct_room_commands: Vec::new(),
        submitted_room_topic_commands: Vec::new(),
        submitted_reaction_commands: Vec::new(),
        submitted_edit_commands: Vec::new(),
        submitted_reply_commands: Vec::new(),
        submitted_message_commands: Vec::new(),
        submitted_file_commands: Vec::new(),
        submitted_notification_commands: Vec::new(),
        submitted_redaction_commands: Vec::new(),
        submitted_history_commands: Vec::new(),
        submitted_verification_actions: Vec::new(),
        submitted_verification_requests: Vec::new(),
        identity_warnings: Vec::new(),
        submitted_identity_reset_commands: Vec::new(),
        submitted_room_key_transfers: Vec::new(),
        submitted_session_commands: Vec::new(),
        submitted_recovery_commands: Vec::new(),
        submitted_setup_recovery_commands: Vec::new(),
        submitted_room_list_requests: Vec::new(),
        opened_links: Vec::new(),
        shown_tips: Vec::new(),
        last_error: None,
        selected_room: None,
    };

    assert_eq!(
        serde_json::to_value(&model).expect("serialize model with membership commands"),
        serde_json::json!({
            "status": "synced",
            "lifecycle_state": "ready",
            "connection": "syncing",
            "reconnect_attempts": 0,
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
            "connection": "syncing",
            "reconnect_attempts": 0,
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
fn submitted_power_level_and_profile_commands_have_strict_json_contracts() {
    for (role, wire) in [
        (MemberPowerRole::Moderator, "moderator"),
        (MemberPowerRole::Voiced, "voiced"),
        (MemberPowerRole::Default, "default"),
    ] {
        let command = SubmittedRoomSettingsCommand::Update {
            room_id: "!ops:example.org".to_owned(),
            topic_by_operators: None,
            moderated: Some(true),
            member_roles: vec![SubmittedPowerLevelChange {
                user_id: "@bob:example.org".to_owned(),
                role,
            }],
            invite_only: None,
            name: None,
            topic: None,
            listed_in_directory: None,
            unban: Vec::new(),
        };
        let value = serde_json::json!({
            "action": "update",
            "room_id": "!ops:example.org",
            "topic_by_operators": null,
            "moderated": true,
            "member_roles": [{ "user_id": "@bob:example.org", "role": wire }],
            "invite_only": null,
            "name": null,
            "topic": null,
            "listed_in_directory": null,
            "unban": [],
        });
        assert_eq!(serde_json::to_value(&command).unwrap(), value);
        assert_eq!(
            serde_json::from_value::<SubmittedRoomSettingsCommand>(value).unwrap(),
            command
        );
    }
    assert!(
        serde_json::from_value::<SubmittedPowerLevelChange>(serde_json::json!({
            "user_id": "@bob:example.org", "role": "admin"
        }))
        .is_err()
    );
    assert!(
        serde_json::from_value::<SubmittedPowerLevelChange>(serde_json::json!({
            "user_id": "@bob:example.org", "role": "voiced", "extra": 1
        }))
        .is_err()
    );
    assert_eq!(
        serde_json::from_value::<SubmittedProfileCommand>(serde_json::json!({
            "display_name": "New Name"
        }))
        .unwrap(),
        SubmittedProfileCommand {
            display_name: "New Name".to_owned(),
        }
    );
    assert!(
        serde_json::from_value::<SubmittedProfileCommand>(serde_json::json!({
            "display_name": "New Name", "extra": 1
        }))
        .is_err()
    );
    assert!(serde_json::from_value::<SubmittedProfileCommand>(serde_json::json!({})).is_err());
    assert_eq!(
        serde_json::from_value::<SubmittedUserCommand>(serde_json::json!({
            "action": "fetch_profile", "user_id": "@bob:example.org"
        }))
        .unwrap(),
        SubmittedUserCommand {
            action: UserCommandAction::FetchProfile,
            user_id: "@bob:example.org".to_owned(),
        }
    );
    for action in ["ignore", "unignore", "list_devices"] {
        assert!(
            serde_json::from_value::<SubmittedUserCommand>(serde_json::json!({
                "action": action, "user_id": "@bob:example.org"
            }))
            .is_ok()
        );
    }
    assert!(
        serde_json::from_value::<SubmittedUserCommand>(serde_json::json!({
            "action": "whois", "user_id": "@bob:example.org"
        }))
        .is_err()
    );
    assert!(
        serde_json::from_value::<SubmittedUserCommand>(serde_json::json!({
            "action": "ignore", "user_id": "@bob:example.org", "extra": 1
        }))
        .is_err()
    );
}

#[test]
fn model_submitted_member_moderation_commands_round_trip_and_default_to_empty() {
    let value = serde_json::json!({
        "status": "synced",
        "lifecycle_state": "ready",
        "connection": "syncing",
        "reconnect_attempts": 0,
        "rooms": [],
        "last_activity": null,
        "e2ee_status": null,
        "last_verification": null,
        "submitted_member_moderation_commands": [
            {"action": "kick", "room_id": "!ops:example.org", "user_id": "@bob:example.org", "reason": null},
            {"action": "ban", "room_id": "!ops:example.org", "user_id": "@carol:example.org", "reason": null}
        ],
        "last_error": null,
        "selected_room": null
    });
    let model = serde_json::from_value::<Model>(value.clone()).unwrap();
    assert_eq!(serde_json::to_value(&model).unwrap(), value);
    assert_eq!(
        model.submitted_member_moderation_commands,
        vec![
            SubmittedMemberModerationCommand {
                action: MemberModerationAction::Kick,
                room_id: "!ops:example.org".to_owned(),
                user_id: "@bob:example.org".to_owned(),
                reason: None,
            },
            SubmittedMemberModerationCommand {
                action: MemberModerationAction::Ban,
                room_id: "!ops:example.org".to_owned(),
                user_id: "@carol:example.org".to_owned(),
                reason: None,
            },
        ]
    );

    let model = serde_json::from_value::<Model>(serde_json::json!({
        "status": "synced",
        "lifecycle_state": "ready",
        "connection": "syncing",
        "reconnect_attempts": 0,
        "rooms": [],
        "last_activity": null,
        "e2ee_status": null,
        "last_verification": null,
        "last_error": null,
        "selected_room": null
    }))
    .unwrap();
    assert!(model.submitted_member_moderation_commands.is_empty());
}

#[test]
fn model_submitted_direct_room_commands_round_trip_through_json() {
    let model = Model {
        status: "synced".to_owned(),
        lifecycle_state: "ready".to_owned(),
        connection: "syncing".to_owned(),
        reconnect_attempts: 0,
        rooms: vec![],
        last_activity: None,
        e2ee_status: None,
        last_e2ee_recovery_setup_result: None,
        last_verification: None,
        submitted_membership_commands: vec![SubmittedMembershipCommand {
            action: MembershipCommandAction::Join,
            target: "#ops:example.org".to_owned(),
        }],
        submitted_member_moderation_commands: Vec::new(),
        submitted_room_settings_commands: Vec::new(),
        submitted_profile_commands: Vec::new(),
        submitted_away_commands: Vec::new(),
        submitted_typing_commands: Vec::new(),
        submitted_user_commands: Vec::new(),
        submitted_direct_room_commands: vec![SubmittedDirectRoomCommand {
            user_id: "@alice:example.org".to_owned(),
        }],
        submitted_room_topic_commands: Vec::new(),
        submitted_reaction_commands: Vec::new(),
        submitted_edit_commands: Vec::new(),
        submitted_reply_commands: Vec::new(),
        submitted_message_commands: Vec::new(),
        submitted_file_commands: Vec::new(),
        submitted_notification_commands: Vec::new(),
        submitted_redaction_commands: Vec::new(),
        submitted_history_commands: Vec::new(),
        submitted_verification_actions: Vec::new(),
        submitted_verification_requests: Vec::new(),
        identity_warnings: Vec::new(),
        submitted_identity_reset_commands: Vec::new(),
        submitted_room_key_transfers: Vec::new(),
        submitted_session_commands: Vec::new(),
        submitted_recovery_commands: Vec::new(),
        submitted_setup_recovery_commands: Vec::new(),
        submitted_room_list_requests: Vec::new(),
        opened_links: Vec::new(),
        shown_tips: Vec::new(),
        last_error: None,
        selected_room: None,
    };

    assert_eq!(
        serde_json::to_value(&model).expect("serialize model with direct room commands"),
        serde_json::json!({
            "status": "synced",
            "lifecycle_state": "ready",
            "connection": "syncing",
            "reconnect_attempts": 0,
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
            "connection": "syncing",
            "reconnect_attempts": 0,
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
        connection: "syncing".to_owned(),
        reconnect_attempts: 0,
        rooms: vec![],
        last_activity: None,
        e2ee_status: None,
        last_e2ee_recovery_setup_result: None,
        last_verification: None,
        submitted_membership_commands: vec![SubmittedMembershipCommand {
            action: MembershipCommandAction::Join,
            target: "#ops:example.org".to_owned(),
        }],
        submitted_member_moderation_commands: Vec::new(),
        submitted_room_settings_commands: Vec::new(),
        submitted_profile_commands: Vec::new(),
        submitted_away_commands: Vec::new(),
        submitted_typing_commands: Vec::new(),
        submitted_user_commands: Vec::new(),
        submitted_direct_room_commands: vec![SubmittedDirectRoomCommand {
            user_id: "@alice:example.org".to_owned(),
        }],
        submitted_room_topic_commands: vec![SubmittedRoomTopicCommand {
            room_id: "!ops:example.org".to_owned(),
            topic: "incident review".to_owned(),
        }],
        submitted_reaction_commands: Vec::new(),
        submitted_edit_commands: Vec::new(),
        submitted_reply_commands: Vec::new(),
        submitted_message_commands: Vec::new(),
        submitted_file_commands: Vec::new(),
        submitted_notification_commands: Vec::new(),
        submitted_redaction_commands: Vec::new(),
        submitted_history_commands: Vec::new(),
        submitted_verification_actions: Vec::new(),
        submitted_verification_requests: Vec::new(),
        identity_warnings: Vec::new(),
        submitted_identity_reset_commands: Vec::new(),
        submitted_room_key_transfers: Vec::new(),
        submitted_session_commands: Vec::new(),
        submitted_recovery_commands: Vec::new(),
        submitted_setup_recovery_commands: Vec::new(),
        submitted_room_list_requests: Vec::new(),
        opened_links: Vec::new(),
        shown_tips: Vec::new(),
        last_error: None,
        selected_room: None,
    };

    assert_eq!(
        serde_json::to_value(&model).expect("serialize model with room topic commands"),
        serde_json::json!({
            "status": "synced",
            "lifecycle_state": "ready",
            "connection": "syncing",
            "reconnect_attempts": 0,
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
            "connection": "syncing",
            "reconnect_attempts": 0,
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
fn model_submitted_reaction_commands_round_trip_through_json() {
    let model = Model {
        status: "synced".to_owned(),
        lifecycle_state: "ready".to_owned(),
        connection: "syncing".to_owned(),
        reconnect_attempts: 0,
        rooms: vec![],
        last_activity: None,
        e2ee_status: None,
        last_e2ee_recovery_setup_result: None,
        last_verification: None,
        submitted_membership_commands: Vec::new(),
        submitted_member_moderation_commands: Vec::new(),
        submitted_room_settings_commands: Vec::new(),
        submitted_profile_commands: Vec::new(),
        submitted_away_commands: Vec::new(),
        submitted_typing_commands: Vec::new(),
        submitted_user_commands: Vec::new(),
        submitted_direct_room_commands: Vec::new(),
        submitted_room_topic_commands: Vec::new(),
        submitted_reaction_commands: vec![SubmittedReactionCommand {
            room_id: "!ops:example.org".to_owned(),
            target_event_id: "$target:example.org".to_owned(),
            key: "+1".to_owned(),
        }],
        submitted_edit_commands: Vec::new(),
        submitted_reply_commands: Vec::new(),
        submitted_message_commands: Vec::new(),
        submitted_file_commands: Vec::new(),
        submitted_notification_commands: Vec::new(),
        submitted_redaction_commands: Vec::new(),
        submitted_history_commands: Vec::new(),
        submitted_verification_actions: Vec::new(),
        submitted_verification_requests: Vec::new(),
        identity_warnings: Vec::new(),
        submitted_identity_reset_commands: Vec::new(),
        submitted_room_key_transfers: Vec::new(),
        submitted_session_commands: Vec::new(),
        submitted_recovery_commands: Vec::new(),
        submitted_setup_recovery_commands: Vec::new(),
        submitted_room_list_requests: Vec::new(),
        opened_links: Vec::new(),
        shown_tips: Vec::new(),
        last_error: None,
        selected_room: None,
    };

    assert_eq!(
        serde_json::to_value(&model).expect("serialize model with reaction commands"),
        serde_json::json!({
            "status": "synced",
            "lifecycle_state": "ready",
            "connection": "syncing",
            "reconnect_attempts": 0,
            "rooms": [],
            "last_activity": null,
            "e2ee_status": null,
            "last_verification": null,
            "submitted_reaction_commands": [
                {
                    "room_id": "!ops:example.org",
                    "target_event_id": "$target:example.org",
                    "key": "+1"
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
            "connection": "syncing",
            "reconnect_attempts": 0,
            "rooms": [],
            "last_activity": null,
            "e2ee_status": null,
            "last_verification": null,
            "submitted_reaction_commands": [
                {
                    "room_id": "!ops:example.org",
                    "target_event_id": "$target:example.org",
                    "key": "+1"
                }
            ],
            "last_error": null,
            "selected_room": null
        }))
        .expect("deserialize model with reaction commands"),
        model
    );
}

#[test]
fn model_submitted_message_commands_round_trip_with_their_styles() {
    let model = Model {
        status: "synced".to_owned(),
        lifecycle_state: "ready".to_owned(),
        connection: "syncing".to_owned(),
        reconnect_attempts: 0,
        rooms: vec![],
        last_activity: None,
        e2ee_status: None,
        last_e2ee_recovery_setup_result: None,
        last_verification: None,
        submitted_membership_commands: Vec::new(),
        submitted_member_moderation_commands: Vec::new(),
        submitted_room_settings_commands: Vec::new(),
        submitted_profile_commands: Vec::new(),
        submitted_away_commands: Vec::new(),
        submitted_typing_commands: Vec::new(),
        submitted_user_commands: Vec::new(),
        submitted_direct_room_commands: Vec::new(),
        submitted_room_topic_commands: Vec::new(),
        submitted_reaction_commands: Vec::new(),
        submitted_edit_commands: Vec::new(),
        submitted_reply_commands: Vec::new(),
        submitted_message_commands: vec![SubmittedMessageCommand {
            room_id: "!ops:example.org".to_owned(),
            presentation: RoomMessagePresentationKind::Emote,
            body: "waves hard".to_owned(),
            styles: vec![mirx_core::TextStyleSpan {
                start: 6,
                end: 10,
                style: mirx_core::TextStyle {
                    bold: true,
                    ..mirx_core::TextStyle::default()
                },
            }],
            thread: None,
        }],
        submitted_file_commands: Vec::new(),
        submitted_notification_commands: Vec::new(),
        submitted_redaction_commands: Vec::new(),
        submitted_history_commands: Vec::new(),
        submitted_verification_actions: Vec::new(),
        submitted_verification_requests: Vec::new(),
        identity_warnings: Vec::new(),
        submitted_identity_reset_commands: Vec::new(),
        submitted_room_key_transfers: Vec::new(),
        submitted_session_commands: Vec::new(),
        submitted_recovery_commands: Vec::new(),
        submitted_setup_recovery_commands: Vec::new(),
        submitted_room_list_requests: Vec::new(),
        opened_links: Vec::new(),
        shown_tips: Vec::new(),
        last_error: None,
        selected_room: None,
    };
    let json = serde_json::json!({
        "status": "synced",
        "lifecycle_state": "ready",
        "connection": "syncing",
        "reconnect_attempts": 0,
        "rooms": [],
        "last_activity": null,
        "e2ee_status": null,
        "last_verification": null,
        "submitted_message_commands": [
            {
                "room_id": "!ops:example.org",
                "presentation": "emote",
                "body": "waves hard",
                "styles": [
                    {
                        "start": 6,
                        "end": 10,
                        "style": {
                            "bold": true,
                            "italic": false,
                            "underline": false,
                            "strikethrough": false,
                            "foreground": null,
                            "background": null
                        }
                    }
                ]
            }
        ],
        "last_error": null,
        "selected_room": null
    });

    assert_eq!(serde_json::to_value(&model).expect("serialize model"), json);
    assert_eq!(
        serde_json::from_value::<Model>(json).expect("deserialize model"),
        model
    );
}

#[test]
fn model_submitted_reply_commands_round_trip_through_json_without_thread() {
    let model = Model {
        status: "synced".to_owned(),
        lifecycle_state: "ready".to_owned(),
        connection: "syncing".to_owned(),
        reconnect_attempts: 0,
        rooms: vec![],
        last_activity: None,
        e2ee_status: None,
        last_e2ee_recovery_setup_result: None,
        last_verification: None,
        submitted_membership_commands: Vec::new(),
        submitted_member_moderation_commands: Vec::new(),
        submitted_room_settings_commands: Vec::new(),
        submitted_profile_commands: Vec::new(),
        submitted_away_commands: Vec::new(),
        submitted_typing_commands: Vec::new(),
        submitted_user_commands: Vec::new(),
        submitted_direct_room_commands: Vec::new(),
        submitted_room_topic_commands: Vec::new(),
        submitted_reaction_commands: Vec::new(),
        submitted_edit_commands: Vec::new(),
        submitted_reply_commands: vec![SubmittedReplyCommand {
            room_id: "!ops:example.org".to_owned(),
            target_event_id: "$target:example.org".to_owned(),
            sender_user_id: "@alice:example.org".to_owned(),
            body: "follow-up".to_owned(),
            styles: Vec::new(),
            thread: None,
        }],
        submitted_message_commands: Vec::new(),
        submitted_file_commands: Vec::new(),
        submitted_notification_commands: Vec::new(),
        submitted_redaction_commands: Vec::new(),
        submitted_history_commands: Vec::new(),
        submitted_verification_actions: Vec::new(),
        submitted_verification_requests: Vec::new(),
        identity_warnings: Vec::new(),
        submitted_identity_reset_commands: Vec::new(),
        submitted_room_key_transfers: Vec::new(),
        submitted_session_commands: Vec::new(),
        submitted_recovery_commands: Vec::new(),
        submitted_setup_recovery_commands: Vec::new(),
        submitted_room_list_requests: Vec::new(),
        opened_links: Vec::new(),
        shown_tips: Vec::new(),
        last_error: None,
        selected_room: None,
    };

    assert_eq!(
        serde_json::to_value(&model).expect("serialize model with reply commands"),
        serde_json::json!({
            "status": "synced",
            "lifecycle_state": "ready",
            "connection": "syncing",
            "reconnect_attempts": 0,
            "rooms": [],
            "last_activity": null,
            "e2ee_status": null,
            "last_verification": null,
            "submitted_reply_commands": [
                {
                    "room_id": "!ops:example.org",
                    "target_event_id": "$target:example.org",
                    "sender_user_id": "@alice:example.org",
                    "body": "follow-up",
                    "styles": []
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
            "connection": "syncing",
            "reconnect_attempts": 0,
            "rooms": [],
            "last_activity": null,
            "e2ee_status": null,
            "last_verification": null,
            "submitted_reply_commands": [
                {
                    "room_id": "!ops:example.org",
                    "target_event_id": "$target:example.org",
                    "sender_user_id": "@alice:example.org",
                    "body": "follow-up",
                    "styles": []
                }
            ],
            "last_error": null,
            "selected_room": null
        }))
        .expect("deserialize model with reply commands"),
        model
    );
}

#[test]
fn model_submitted_reply_commands_round_trip_through_json_with_thread() {
    let model = Model {
        status: "synced".to_owned(),
        lifecycle_state: "ready".to_owned(),
        connection: "syncing".to_owned(),
        reconnect_attempts: 0,
        rooms: vec![],
        last_activity: None,
        e2ee_status: None,
        last_e2ee_recovery_setup_result: None,
        last_verification: None,
        submitted_membership_commands: Vec::new(),
        submitted_member_moderation_commands: Vec::new(),
        submitted_room_settings_commands: Vec::new(),
        submitted_profile_commands: Vec::new(),
        submitted_away_commands: Vec::new(),
        submitted_typing_commands: Vec::new(),
        submitted_user_commands: Vec::new(),
        submitted_direct_room_commands: Vec::new(),
        submitted_room_topic_commands: Vec::new(),
        submitted_reaction_commands: Vec::new(),
        submitted_edit_commands: Vec::new(),
        submitted_reply_commands: vec![SubmittedReplyCommand {
            room_id: "!ops:example.org".to_owned(),
            target_event_id: "$target:example.org".to_owned(),
            sender_user_id: "@alice:example.org".to_owned(),
            body: "thread follow-up".to_owned(),
            styles: Vec::new(),
            thread: Some(SubmittedReplyThreadTarget {
                thread_root_event_id: "$root:example.org".to_owned(),
                latest_event_id: "$latest:example.org".to_owned(),
            }),
        }],
        submitted_message_commands: Vec::new(),
        submitted_file_commands: Vec::new(),
        submitted_notification_commands: Vec::new(),
        submitted_redaction_commands: Vec::new(),
        submitted_history_commands: Vec::new(),
        submitted_verification_actions: Vec::new(),
        submitted_verification_requests: Vec::new(),
        identity_warnings: Vec::new(),
        submitted_identity_reset_commands: Vec::new(),
        submitted_room_key_transfers: Vec::new(),
        submitted_session_commands: Vec::new(),
        submitted_recovery_commands: Vec::new(),
        submitted_setup_recovery_commands: Vec::new(),
        submitted_room_list_requests: Vec::new(),
        opened_links: Vec::new(),
        shown_tips: Vec::new(),
        last_error: None,
        selected_room: None,
    };

    assert_eq!(
        serde_json::to_value(&model).expect("serialize model with threaded reply commands"),
        serde_json::json!({
            "status": "synced",
            "lifecycle_state": "ready",
            "connection": "syncing",
            "reconnect_attempts": 0,
            "rooms": [],
            "last_activity": null,
            "e2ee_status": null,
            "last_verification": null,
            "submitted_reply_commands": [
                {
                    "room_id": "!ops:example.org",
                    "target_event_id": "$target:example.org",
                    "sender_user_id": "@alice:example.org",
                    "body": "thread follow-up",
                    "styles": [],
                    "thread": {
                        "thread_root_event_id": "$root:example.org",
                        "latest_event_id": "$latest:example.org"
                    }
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
            "connection": "syncing",
            "reconnect_attempts": 0,
            "rooms": [],
            "last_activity": null,
            "e2ee_status": null,
            "last_verification": null,
            "submitted_reply_commands": [
                {
                    "room_id": "!ops:example.org",
                    "target_event_id": "$target:example.org",
                    "sender_user_id": "@alice:example.org",
                    "body": "thread follow-up",
                    "styles": [],
                    "thread": {
                        "thread_root_event_id": "$root:example.org",
                        "latest_event_id": "$latest:example.org"
                    }
                }
            ],
            "last_error": null,
            "selected_room": null
        }))
        .expect("deserialize model with threaded reply commands"),
        model
    );
}

#[test]
fn model_submitted_redaction_commands_round_trip_through_json() {
    let model = Model {
        status: "synced".to_owned(),
        lifecycle_state: "ready".to_owned(),
        connection: "syncing".to_owned(),
        reconnect_attempts: 0,
        rooms: vec![],
        last_activity: None,
        e2ee_status: None,
        last_e2ee_recovery_setup_result: None,
        last_verification: None,
        submitted_membership_commands: Vec::new(),
        submitted_member_moderation_commands: Vec::new(),
        submitted_room_settings_commands: Vec::new(),
        submitted_profile_commands: Vec::new(),
        submitted_away_commands: Vec::new(),
        submitted_typing_commands: Vec::new(),
        submitted_user_commands: Vec::new(),
        submitted_direct_room_commands: Vec::new(),
        submitted_room_topic_commands: Vec::new(),
        submitted_reaction_commands: Vec::new(),
        submitted_edit_commands: Vec::new(),
        submitted_reply_commands: Vec::new(),
        submitted_message_commands: Vec::new(),
        submitted_file_commands: Vec::new(),
        submitted_notification_commands: Vec::new(),
        submitted_redaction_commands: vec![SubmittedRedactionCommand {
            room_id: "!ops:example.org".to_owned(),
            event_id: "$reaction:example.org".to_owned(),
        }],
        submitted_history_commands: Vec::new(),
        submitted_verification_actions: Vec::new(),
        submitted_verification_requests: Vec::new(),
        identity_warnings: Vec::new(),
        submitted_identity_reset_commands: Vec::new(),
        submitted_room_key_transfers: Vec::new(),
        submitted_session_commands: Vec::new(),
        submitted_recovery_commands: Vec::new(),
        submitted_setup_recovery_commands: Vec::new(),
        submitted_room_list_requests: Vec::new(),
        opened_links: Vec::new(),
        shown_tips: Vec::new(),
        last_error: None,
        selected_room: None,
    };

    assert_eq!(
        serde_json::to_value(&model).expect("serialize model with redaction commands"),
        serde_json::json!({
            "status": "synced",
            "lifecycle_state": "ready",
            "connection": "syncing",
            "reconnect_attempts": 0,
            "rooms": [],
            "last_activity": null,
            "e2ee_status": null,
            "last_verification": null,
            "submitted_redaction_commands": [
                {
                    "room_id": "!ops:example.org",
                    "event_id": "$reaction:example.org"
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
            "connection": "syncing",
            "reconnect_attempts": 0,
            "rooms": [],
            "last_activity": null,
            "e2ee_status": null,
            "last_verification": null,
            "submitted_redaction_commands": [
                {
                    "room_id": "!ops:example.org",
                    "event_id": "$reaction:example.org"
                }
            ],
            "last_error": null,
            "selected_room": null
        }))
        .expect("deserialize model with redaction commands"),
        model
    );
}

#[test]
fn model_full_payload_round_trips_submitted_reply_reaction_and_redaction_command_fields() {
    let input_json = serde_json::json!({
        "status": "synced",
        "lifecycle_state": "ready",
        "connection": "syncing",
        "reconnect_attempts": 0,
        "rooms": [],
        "last_activity": null,
        "e2ee_status": null,
        "last_verification": null,
        "submitted_membership_commands": [],
        "submitted_direct_room_commands": [],
        "submitted_room_topic_commands": [],
        "submitted_reply_commands": [
            {
                "room_id": "!ops:example.org",
                "target_event_id": "$target:example.org",
                "sender_user_id": "@alice:example.org",
                "body": "thread follow-up",
                "styles": [],
                "thread": {
                    "thread_root_event_id": "$root:example.org",
                    "latest_event_id": "$latest:example.org"
                }
            }
        ],
        "submitted_reaction_commands": [
            {
                "room_id": "!ops:example.org",
                "target_event_id": "$target:example.org",
                "key": "+1"
            }
        ],
        "submitted_redaction_commands": [
            {
                "room_id": "!ops:example.org",
                "event_id": "$reaction:example.org"
            }
        ],
        "submitted_verification_actions": [],
        "last_error": null,
        "selected_room": null
    });

    let model = serde_json::from_value::<Model>(input_json).expect(
        "deserialize full model payload with reply, reaction, and redaction command fields",
    );
    assert_eq!(
        model,
        Model {
            status: "synced".to_owned(),
            lifecycle_state: "ready".to_owned(),
            connection: "syncing".to_owned(),
            reconnect_attempts: 0,
            rooms: vec![],
            last_activity: None,
            e2ee_status: None,
            last_e2ee_recovery_setup_result: None,
            last_verification: None,
            submitted_membership_commands: Vec::new(),
            submitted_member_moderation_commands: Vec::new(),
            submitted_room_settings_commands: Vec::new(),
            submitted_profile_commands: Vec::new(),
            submitted_away_commands: Vec::new(),
            submitted_typing_commands: Vec::new(),
            submitted_user_commands: Vec::new(),
            submitted_direct_room_commands: Vec::new(),
            submitted_room_topic_commands: Vec::new(),
            submitted_reaction_commands: vec![SubmittedReactionCommand {
                room_id: "!ops:example.org".to_owned(),
                target_event_id: "$target:example.org".to_owned(),
                key: "+1".to_owned(),
            }],
            submitted_edit_commands: Vec::new(),
            submitted_reply_commands: vec![SubmittedReplyCommand {
                room_id: "!ops:example.org".to_owned(),
                target_event_id: "$target:example.org".to_owned(),
                sender_user_id: "@alice:example.org".to_owned(),
                body: "thread follow-up".to_owned(),
                styles: Vec::new(),
                thread: Some(SubmittedReplyThreadTarget {
                    thread_root_event_id: "$root:example.org".to_owned(),
                    latest_event_id: "$latest:example.org".to_owned(),
                }),
            }],
            submitted_message_commands: Vec::new(),
            submitted_file_commands: Vec::new(),
            submitted_notification_commands: Vec::new(),
            submitted_redaction_commands: vec![SubmittedRedactionCommand {
                room_id: "!ops:example.org".to_owned(),
                event_id: "$reaction:example.org".to_owned(),
            }],
            submitted_history_commands: Vec::new(),
            submitted_verification_actions: Vec::new(),
            submitted_verification_requests: Vec::new(),
            identity_warnings: Vec::new(),
            submitted_identity_reset_commands: Vec::new(),
            submitted_room_key_transfers: Vec::new(),
            submitted_session_commands: Vec::new(),
            submitted_recovery_commands: Vec::new(),
            submitted_setup_recovery_commands: Vec::new(),
            submitted_room_list_requests: Vec::new(),
            opened_links: Vec::new(),
            shown_tips: Vec::new(),
            last_error: None,
            selected_room: None,
        }
    );
    assert_eq!(
        serde_json::to_value(&model).unwrap(),
        serde_json::json!({
            "status": "synced",
            "lifecycle_state": "ready",
            "connection": "syncing",
            "reconnect_attempts": 0,
            "rooms": [],
            "last_activity": null,
            "e2ee_status": null,
            "last_verification": null,
            "submitted_reply_commands": [
                {
                    "room_id": "!ops:example.org",
                    "target_event_id": "$target:example.org",
                    "sender_user_id": "@alice:example.org",
                    "body": "thread follow-up",
                    "styles": [],
                    "thread": {
                        "thread_root_event_id": "$root:example.org",
                        "latest_event_id": "$latest:example.org"
                    }
                }
            ],
            "submitted_reaction_commands": [
                {
                    "room_id": "!ops:example.org",
                    "target_event_id": "$target:example.org",
                    "key": "+1"
                }
            ],
            "submitted_redaction_commands": [
                {
                    "room_id": "!ops:example.org",
                    "event_id": "$reaction:example.org"
                }
            ],
            "last_error": null,
            "selected_room": null
        })
    );
}

#[test]
fn model_submitted_history_commands_round_trip_through_json() {
    let model = Model {
        status: "synced".to_owned(),
        lifecycle_state: "ready".to_owned(),
        connection: "syncing".to_owned(),
        reconnect_attempts: 0,
        rooms: vec![],
        last_activity: None,
        e2ee_status: None,
        last_e2ee_recovery_setup_result: None,
        last_verification: None,
        submitted_membership_commands: Vec::new(),
        submitted_member_moderation_commands: Vec::new(),
        submitted_room_settings_commands: Vec::new(),
        submitted_profile_commands: Vec::new(),
        submitted_away_commands: Vec::new(),
        submitted_typing_commands: Vec::new(),
        submitted_user_commands: Vec::new(),
        submitted_direct_room_commands: Vec::new(),
        submitted_room_topic_commands: Vec::new(),
        submitted_reaction_commands: Vec::new(),
        submitted_edit_commands: Vec::new(),
        submitted_reply_commands: Vec::new(),
        submitted_message_commands: Vec::new(),
        submitted_file_commands: Vec::new(),
        submitted_notification_commands: Vec::new(),
        submitted_redaction_commands: Vec::new(),
        submitted_history_commands: vec![
            SubmittedHistoryCommand {
                room_id: "!room:example".to_owned(),
                from: HistoryFrom::Latest,
                limit: 50,
            },
            SubmittedHistoryCommand {
                room_id: "!room:example".to_owned(),
                from: HistoryFrom::Token {
                    token: "token".to_owned(),
                },
                limit: 50,
            },
        ],
        submitted_verification_actions: Vec::new(),
        submitted_verification_requests: Vec::new(),
        identity_warnings: Vec::new(),
        submitted_identity_reset_commands: Vec::new(),
        submitted_room_key_transfers: Vec::new(),
        submitted_session_commands: Vec::new(),
        submitted_recovery_commands: Vec::new(),
        submitted_setup_recovery_commands: Vec::new(),
        submitted_room_list_requests: Vec::new(),
        opened_links: Vec::new(),
        shown_tips: Vec::new(),
        last_error: None,
        selected_room: None,
    };

    assert_eq!(
        serde_json::to_value(&model).expect("serialize model with history commands"),
        serde_json::json!({
            "status": "synced",
            "lifecycle_state": "ready",
            "connection": "syncing",
            "reconnect_attempts": 0,
            "rooms": [],
            "last_activity": null,
            "e2ee_status": null,
            "last_verification": null,
            "submitted_history_commands": [
                {
                    "room_id": "!room:example",
                    "from": { "type": "latest" },
                    "limit": 50
                },
                {
                    "room_id": "!room:example",
                    "from": { "type": "token", "token": "token" },
                    "limit": 50
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
            "connection": "syncing",
            "reconnect_attempts": 0,
            "rooms": [],
            "last_activity": null,
            "e2ee_status": null,
            "last_verification": null,
            "submitted_history_commands": [
                {
                    "room_id": "!room:example",
                    "from": { "type": "latest" },
                    "limit": 50
                },
                {
                    "room_id": "!room:example",
                    "from": { "type": "token", "token": "token" },
                    "limit": 50
                }
            ],
            "last_error": null,
            "selected_room": null
        }))
        .expect("deserialize model with history commands"),
        model
    );
}

#[test]
fn model_submitted_history_commands_default_to_empty_when_omitted() {
    let model = serde_json::from_value::<Model>(serde_json::json!({
        "status": "synced",
        "lifecycle_state": "ready",
        "connection": "syncing",
        "reconnect_attempts": 0,
        "rooms": [],
        "last_activity": null,
        "e2ee_status": null,
        "last_verification": null,
        "last_error": null,
        "selected_room": null
    }))
    .expect("deserialize model without history commands field");

    assert_eq!(
        model,
        Model {
            status: "synced".to_owned(),
            lifecycle_state: "ready".to_owned(),
            connection: "syncing".to_owned(),
            reconnect_attempts: 0,
            rooms: vec![],
            last_activity: None,
            e2ee_status: None,
            last_e2ee_recovery_setup_result: None,
            last_verification: None,
            submitted_membership_commands: Vec::new(),
            submitted_member_moderation_commands: Vec::new(),
            submitted_room_settings_commands: Vec::new(),
            submitted_profile_commands: Vec::new(),
            submitted_away_commands: Vec::new(),
            submitted_typing_commands: Vec::new(),
            submitted_user_commands: Vec::new(),
            submitted_direct_room_commands: Vec::new(),
            submitted_room_topic_commands: Vec::new(),
            submitted_reaction_commands: Vec::new(),
            submitted_edit_commands: Vec::new(),
            submitted_reply_commands: Vec::new(),
            submitted_message_commands: Vec::new(),
            submitted_file_commands: Vec::new(),
            submitted_notification_commands: Vec::new(),
            submitted_redaction_commands: Vec::new(),
            submitted_history_commands: Vec::new(),
            submitted_verification_actions: Vec::new(),
            submitted_verification_requests: Vec::new(),
            identity_warnings: Vec::new(),
            submitted_identity_reset_commands: Vec::new(),
            submitted_room_key_transfers: Vec::new(),
            submitted_session_commands: Vec::new(),
            submitted_recovery_commands: Vec::new(),
            submitted_setup_recovery_commands: Vec::new(),
            submitted_room_list_requests: Vec::new(),
            opened_links: Vec::new(),
            shown_tips: Vec::new(),
            last_error: None,
            selected_room: None,
        }
    );
}

#[test]
fn model_submitted_membership_commands_reject_unknown_nested_fields() {
    let error = serde_json::from_value::<Model>(serde_json::json!({
        "status": "synced",
        "lifecycle_state": "ready",
        "connection": "syncing",
        "reconnect_attempts": 0,
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
        "submitted_reaction_commands": [],
        "submitted_redaction_commands": [],
        "last_error": null,
        "selected_room": null
    }))
    .unwrap_err();

    assert!(error.to_string().contains("unknown field `unexpected`"));
}

#[test]
fn model_submitted_reply_commands_reject_unknown_nested_fields() {
    let error = serde_json::from_value::<Model>(serde_json::json!({
        "status": "synced",
        "lifecycle_state": "ready",
        "connection": "syncing",
        "reconnect_attempts": 0,
        "rooms": [],
        "last_activity": null,
        "e2ee_status": null,
        "last_verification": null,
        "submitted_membership_commands": [],
        "submitted_direct_room_commands": [],
        "submitted_room_topic_commands": [],
        "submitted_reply_commands": [
            {
                "room_id": "!ops:example.org",
                "target_event_id": "$target:example.org",
                "sender_user_id": "@alice:example.org",
                "body": "follow-up",
                "styles": [],
                "unexpected": true
            }
        ],
        "submitted_reaction_commands": [],
        "submitted_redaction_commands": [],
        "last_error": null,
        "selected_room": null
    }))
    .unwrap_err();

    assert!(error.to_string().contains("unknown field `unexpected`"));
}

#[test]
fn model_submitted_reply_command_threads_reject_unknown_nested_fields() {
    let error = serde_json::from_value::<Model>(serde_json::json!({
        "status": "synced",
        "lifecycle_state": "ready",
        "connection": "syncing",
        "reconnect_attempts": 0,
        "rooms": [],
        "last_activity": null,
        "e2ee_status": null,
        "last_verification": null,
        "submitted_membership_commands": [],
        "submitted_direct_room_commands": [],
        "submitted_room_topic_commands": [],
        "submitted_reply_commands": [
            {
                "room_id": "!ops:example.org",
                "target_event_id": "$target:example.org",
                "sender_user_id": "@alice:example.org",
                "body": "thread follow-up",
                "styles": [],
                "thread": {
                    "thread_root_event_id": "$root:example.org",
                    "latest_event_id": "$latest:example.org",
                    "unexpected": true
                }
            }
        ],
        "submitted_reaction_commands": [],
        "submitted_redaction_commands": [],
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
        "connection": "syncing",
        "reconnect_attempts": 0,
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
        "submitted_reaction_commands": [],
        "submitted_redaction_commands": [],
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
        "connection": "syncing",
        "reconnect_attempts": 0,
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
        "submitted_reaction_commands": [],
        "submitted_redaction_commands": [],
        "last_error": null,
        "selected_room": null
    }))
    .unwrap_err();

    assert!(error.to_string().contains("unknown field `unexpected`"));
}

#[test]
fn model_submitted_reaction_commands_reject_unknown_nested_fields() {
    let error = serde_json::from_value::<Model>(serde_json::json!({
        "status": "synced",
        "lifecycle_state": "ready",
        "connection": "syncing",
        "reconnect_attempts": 0,
        "rooms": [],
        "last_activity": null,
        "e2ee_status": null,
        "last_verification": null,
        "submitted_membership_commands": [],
        "submitted_direct_room_commands": [],
        "submitted_room_topic_commands": [],
        "submitted_reaction_commands": [
            {
                "room_id": "!ops:example.org",
                "target_event_id": "$target:example.org",
                "key": "+1",
                "unexpected": true
            }
        ],
        "submitted_redaction_commands": [],
        "last_error": null,
        "selected_room": null
    }))
    .unwrap_err();

    assert!(error.to_string().contains("unknown field `unexpected`"));
}

#[test]
fn model_submitted_redaction_commands_reject_unknown_nested_fields() {
    let error = serde_json::from_value::<Model>(serde_json::json!({
        "status": "synced",
        "lifecycle_state": "ready",
        "connection": "syncing",
        "reconnect_attempts": 0,
        "rooms": [],
        "last_activity": null,
        "e2ee_status": null,
        "last_verification": null,
        "submitted_membership_commands": [],
        "submitted_direct_room_commands": [],
        "submitted_room_topic_commands": [],
        "submitted_reaction_commands": [],
        "submitted_redaction_commands": [
            {
                "room_id": "!ops:example.org",
                "event_id": "$reaction:example.org",
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
fn model_submitted_history_commands_reject_unknown_nested_fields() {
    let error = serde_json::from_value::<Model>(serde_json::json!({
        "status": "synced",
        "lifecycle_state": "ready",
        "connection": "syncing",
        "reconnect_attempts": 0,
        "rooms": [],
        "last_activity": null,
        "e2ee_status": null,
        "last_verification": null,
        "submitted_membership_commands": [],
        "submitted_direct_room_commands": [],
        "submitted_room_topic_commands": [],
        "submitted_reaction_commands": [],
        "submitted_history_commands": [
            {
                "room_id": "!room:example",
                "from": { "type": "token", "token": "token" },
                "limit": 50,
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
            "connection":"syncing", "reconnect_attempts":0,
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
            "protocol_version":18,
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
            "auth":"Matrix",
            "unexpected":true
        }"#
        )
        .is_err()
    );
    assert!(
        serde_json::from_str::<mirx_testkit::Room>(
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
            "highlight":false,
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
        serde_json::from_str::<RoomFileActivity>(
            r#"{
            "event_id":"$file1:example.org",
            "sender":"@alice:example.org",
            "body":"report.pdf",
            "filename":"incident-report.pdf",
            "source":{"type":"plain","url":"mxc://example.org/report"},
            "info":{"mimetype":"application/pdf","size":54321},
            "unexpected":true
        }"#
        )
        .is_err()
    );
    assert!(
        serde_json::from_str::<RoomAudioActivity>(
            r#"{
            "event_id":"$audio1:example.org",
            "sender":"@alice:example.org",
            "body":"voice-note.ogg",
            "filename":"voice-note.ogg",
            "source":{"type":"plain","url":"mxc://example.org/voice-note"},
            "info":{"mimetype":"audio/ogg","size":9876,"duration_ms":12345},
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
        serde_json::from_str::<RoomFileInfo>(
            r#"{
            "mimetype":"application/pdf",
            "size":54321,
            "unexpected":true
        }"#
        )
        .is_err()
    );
    assert!(
        serde_json::from_str::<RoomAudioInfo>(
            r#"{
            "mimetype":"audio/ogg",
            "size":9876,
            "duration_ms":12345,
            "unexpected":true
        }"#
        )
        .is_err()
    );
    assert!(
        serde_json::from_str::<UnsupportedRoomMessageActivity>(
            r#"{
            "event_id":"$video1:example.org",
            "sender":"@alice:example.org",
            "msgtype":"m.video",
            "body":"clip.mp4",
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
            "event_id":"$video1:example.org",
            "sender":"@alice:example.org",
            "msgtype":"m.video",
            "body":"clip.mp4",
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
            "change":{"type":"joined"},
            "target_nick":"carol",
            "reason":null,
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
            "device_display_name":"mirx-ui",
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
                "refresh_token":null,
                "auth":"Matrix"
            },
            "unexpected":true
        }"#
        )
        .is_err()
    );
    assert!(
        serde_json::from_str::<UiState>(
            r#"{
            "navigation":{"text":"mirx","children":[]},
            "mdi":{"active":{"kind":"status"},"views":[]},
            "status_bar":{"text":"ready"},
            "switchbar":{"items":[]},
            "active_find":null,
            "rooms":[],
            "unexpected":true
        }"#
        )
        .is_err()
    );
    assert!(
        serde_json::from_str::<UiState>(
            r#"{
            "navigation":{"text":"mirx","children":[]},
            "mdi":{"active":{"kind":"status"},"views":[]},
            "status_bar":{"text":"ready"},
            "switchbar":{"items":[]},
            "active_find":null
        }"#
        )
        .is_err()
    );
    assert!(
        serde_json::from_str::<UiRoomState>(
            r#"{
            "room_id":"!room:example",
            "display_name":"Room",
            "membership":"join",
            "encrypted":true,
            "is_space":false,
            "is_direct":false,
            "direct_user_ids":[],
            "tags":[],
            "space_children":[],
            "activity":null,
            "read_target_event_id":null,
            "read_state":null,
            "history_backfill":{"state":"idle","next":{"type":"latest"}},
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
            "text":"mirx",
            "activity":null,
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
            "placement":{"state":"minimized","x":0,"y":0,"width":320,"height":200},
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
            "nick":"Alice",
            "label":"Alice",
            "rank":"member",
            "can_kick":true,
            "can_ban":false,
            "unexpected":true
        }"#
        )
        .is_err()
    );
    assert!(
        serde_json::from_str::<RoomMemberView>(
            r#"{
            "user_id":"@alice:example",
            "nick":"Alice",
            "label":"Alice",
            "rank":"member",
            "can_ban":false
        }"#
        )
        .is_err()
    );
    assert!(
        serde_json::from_str::<RoomMemberView>(
            r#"{
            "user_id":"@alice:example",
            "nick":"Alice",
            "label":"Alice",
            "rank":"member",
            "can_kick":true
        }"#
        )
        .is_err()
    );
    assert!(
        serde_json::from_str::<RoomMemberView>(
            r#"{
            "user_id":"@alice:example",
            "nick":"Alice",
            "label":"Alice",
            "can_kick":true,
            "can_ban":false
        }"#
        )
        .is_err(),
        "room member view must require rank"
    );
    assert_eq!(
        serde_json::from_str::<RoomMemberView>(
            r##"{
            "user_id":"@alice:example",
            "nick":"Alice",
            "label":"Alice",
            "rank":"creator",
            "color":"#7f7f7f",
            "can_kick":true,
            "can_ban":false
        }"##
        )
        .expect("complete room member view")
        .rank,
        RoomMemberRank::Creator
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
            "activity":null,
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
        "connection":"syncing", "reconnect_attempts":0,
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
            "connection":"syncing", "reconnect_attempts":0,
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
        "connection":"syncing", "reconnect_attempts":0,
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
            },
            "session":"verified"
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
            "connection":"syncing", "reconnect_attempts":0,
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
                },
                "session":"verified"
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
            "connection":"syncing", "reconnect_attempts":0,
            "rooms":[],
            "last_activity":null,
            "e2ee_status":{
                "recovery":"incomplete",
                "backup":{"state":"enabled","enabled_locally":true,"exists_on_server":true},
                "cross_signing":{"has_master":true,"has_self_signing":true,"has_user_signing":false,"complete":false},
                "session":"verified",
                "unexpected":true
            },
            "last_verification":null,
            "last_error":null,
            "selected_room":null
        }),
        serde_json::json!({
            "status":"e2ee status updated",
            "lifecycle_state":"ready",
            "connection":"syncing", "reconnect_attempts":0,
            "rooms":[],
            "last_activity":null,
            "e2ee_status":{
                "recovery":"incomplete",
                "backup":{"state":"enabled","enabled_locally":true,"exists_on_server":true,"unexpected":true},
                "cross_signing":{"has_master":true,"has_self_signing":true,"has_user_signing":false,"complete":false},
                "session":"verified"
            },
            "last_verification":null,
            "last_error":null,
            "selected_room":null
        }),
        serde_json::json!({
            "status":"e2ee status updated",
            "lifecycle_state":"ready",
            "connection":"syncing", "reconnect_attempts":0,
            "rooms":[],
            "last_activity":null,
            "e2ee_status":{
                "recovery":"incomplete",
                "backup":{"state":"enabled","enabled_locally":true,"exists_on_server":true},
                "cross_signing":{"has_master":true,"has_self_signing":true,"has_user_signing":false,"complete":false,"unexpected":true},
                "session":"verified"
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
            "change":{"type":"joined"},
            "target_nick":"alice",
            "reason":null
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
                "change":{"type":"joined"},
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
            text: "Status".to_owned(),
            view: Some(ViewIdentity::Status),
            space_id: None,
            expanded: None,
            activity: None,
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
            rows: 1,
            items: vec![
                SwitchbarItem {
                    identity: ViewIdentity::Status,
                    title: "Status".to_owned(),
                    active: false,
                    activity: None,
                    rect: UiRect {
                        x: 0,
                        y: 0,
                        width: 120,
                        height: 36,
                    },
                },
                SwitchbarItem {
                    identity: ViewIdentity::Room {
                        room_id: "!room:example".to_owned(),
                    },
                    title: "Room".to_owned(),
                    active: true,
                    activity: Some(ViewActivityLevel::Message),
                    rect: UiRect {
                        x: 120,
                        y: 0,
                        width: 120,
                        height: 36,
                    },
                },
            ],
        },
        toolbar: vec![UiToolbarButton {
            command_id: 40030,
            label: "Connect".to_owned(),
            hidden: false,
            enabled: true,
            rect: UiRect {
                x: 0,
                y: 2,
                width: 23,
                height: 22,
            },
        }],
        frame_layout: UiFrameLayout {
            toolbar: Some(UiRect {
                x: 0,
                y: 0,
                width: 1008,
                height: 26,
            }),
            switchbar: Some(UiRect {
                x: 0,
                y: 0,
                width: 1008,
                height: 28,
            }),
            treebar: Some(UiRect {
                x: 0,
                y: 28,
                width: 240,
                height: 600,
            }),
            mdi_client: UiRect {
                x: 240,
                y: 28,
                width: 768,
                height: 600,
            },
        },
        dpi: 96,
        active_find: None,
        rooms: vec![],
        text_font: UiTextFont {
            face: "Fixedsys".to_owned(),
            points: 9,
            bold: false,
            line_height: 15,
            edit_height: 25,
            average_char_width: 8,
            fallback_faces: vec!["Tahoma".to_owned(), "Noto Emoji".to_owned()],
        },
        timestamp: UiTimestamp {
            show: true,
            format: "[HH:nn]".to_owned(),
        },
        colors: UiColors {
            items: vec![UiColorItem {
                item: "background".to_owned(),
                index: 0,
                color: "#ffffff".to_owned(),
            }],
            switchbar: UiActivityColors {
                event: 5,
                message: 4,
                highlight: 12,
            },
        },
        active_system_menu_items: vec![],
        active_system_menu_checked: vec![],
        active_system_menu_disabled: vec![],
        menu_bar: vec![],
        compose_target: None,
        room_list: None,
        options_dialog: None,
        colors_dialog: None,
        address_book: None,
        channel_central: None,
        color_index: None,
        tray: UiTray {
            icon: false,
            hidden: false,
            menu: Vec::new(),
        },
        active_timeline: None,
        ignored_users: None,
        aliases: Vec::new(),
        refresh_counters: UiRefreshCounters {
            timeline_row_builds: 0,
            rows_wrapped: 0,
            treebar_rebuilds: 0,
            switchbar_rebuilds: 0,
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
fn navigation_node_rejects_legacy_activity_flag() {
    let error = serde_json::from_str::<NavigationNode>(
        r#"{"text":"mirx","has_activity":true,"activity":null,"children":[]}"#,
    )
    .unwrap_err();

    assert!(
        error.to_string().contains("unknown field `has_activity`"),
        "{error}"
    );
}

#[test]
fn view_activity_levels_serialize_in_snake_case() {
    assert_eq!(
        serde_json::to_value([
            ViewActivityLevel::Event,
            ViewActivityLevel::Message,
            ViewActivityLevel::Highlight,
        ])
        .unwrap(),
        serde_json::json!(["event", "message", "highlight"])
    );
}

fn room_view(room_id: &str) -> ViewIdentity {
    ViewIdentity::Room {
        room_id: room_id.to_owned(),
    }
}

fn nav_node(
    text: &str,
    view: Option<ViewIdentity>,
    children: Vec<NavigationNode>,
) -> NavigationNode {
    NavigationNode {
        text: text.to_owned(),
        space_id: view
            .is_none()
            .then(|| format!("!{}:example", text.to_lowercase())),
        expanded: (!children.is_empty()).then_some(true),
        view,
        activity: None,
        children,
    }
}

/// `Status` > (`Work` > `Shared`, `Ops`), (`Social` > `Shared`), `Shared`:
/// one room listed under two spaces, and a different room with its label.
fn duplicate_view_navigation() -> NavigationNode {
    nav_node(
        "Status",
        Some(ViewIdentity::Status),
        vec![
            nav_node(
                "Work",
                None,
                vec![
                    nav_node("Shared", Some(room_view("!shared:example")), vec![]),
                    nav_node("Ops", Some(room_view("!ops:example")), vec![]),
                ],
            ),
            nav_node(
                "Social",
                None,
                vec![nav_node(
                    "Shared",
                    Some(room_view("!shared:example")),
                    vec![],
                )],
            ),
            nav_node("Shared", Some(room_view("!other:example")), vec![]),
        ],
    )
}

#[test]
fn navigation_node_find_view_returns_every_node_of_the_view() {
    let navigation = duplicate_view_navigation();

    let shared = navigation.find_view(&room_view("!shared:example"));
    assert_eq!(shared.len(), 2);
    assert!(shared.iter().all(|node| node.text == "Shared"));
    assert_eq!(navigation.find_view(&ViewIdentity::Status).len(), 1);
    assert!(
        navigation
            .find_view(&room_view("!missing:example"))
            .is_empty()
    );
}

#[test]
fn navigation_node_view_paths_index_duplicate_sibling_labels() {
    let navigation = duplicate_view_navigation();
    let segment = |text: &str, same_text_index: u32| NavigationPathSegment {
        text: text.to_owned(),
        same_text_index,
    };

    assert_eq!(
        navigation.view_paths(&room_view("!shared:example")),
        vec![
            vec![
                segment("Status", 0),
                segment("Work", 0),
                segment("Shared", 0)
            ],
            vec![
                segment("Status", 0),
                segment("Social", 0),
                segment("Shared", 0)
            ],
        ]
    );
    assert_eq!(
        navigation.view_paths(&ViewIdentity::Status),
        vec![vec![segment("Status", 0)]]
    );
    assert_eq!(
        navigation.view_paths(&room_view("!other:example")),
        vec![vec![segment("Status", 0), segment("Shared", 0)]]
    );
}

#[test]
fn compose_key_round_trips_and_rejects_invalid_shapes() {
    for (key, json) in [
        (ComposeKey::Return, r#"{"kind":"return"}"#),
        (ComposeKey::Up, r#"{"kind":"up"}"#),
        (ComposeKey::Down, r#"{"kind":"down"}"#),
        (ComposeKey::Tab, r#"{"kind":"tab"}"#),
        (ComposeKey::ShiftTab, r#"{"kind":"shift_tab"}"#),
        (ComposeKey::Escape, r#"{"kind":"escape"}"#),
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
        r#"{"kind":"shift-tab"}"#,
        r#"{"kind":"shiftTab"}"#,
        r#"{"kind":"tab","shift":true}"#,
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
            device_display_name: "mirx-ui".to_owned(),
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
fn setup_e2ee_recovery_args_round_trip_and_redact_debug() {
    let args = SetupE2eeRecoveryArgs {
        passphrase: Some("sample setup passphrase".to_owned()),
        wait_for_backups_to_upload: true,
    };
    assert_eq!(
        serde_json::from_value::<SetupE2eeRecoveryArgs>(serde_json::to_value(&args).unwrap())
            .unwrap(),
        args
    );
    assert!(!format!("{args:?}").contains("sample setup passphrase"));
    assert!(
        serde_json::from_value::<SetupE2eeRecoveryArgs>(serde_json::json!({
            "passphrase":"sample setup passphrase",
            "wait_for_backups_to_upload":false,
            "unexpected":true,
        }))
        .is_err()
    );
}

#[test]
fn setup_e2ee_recovery_dump_dtos_round_trip_and_reject_unknown_fields() {
    let submission = SubmittedSetupRecoveryCommand {
        passphrase_redacted: true,
        wait_for_backups_to_upload: true,
    };
    assert_eq!(
        serde_json::from_value::<SubmittedSetupRecoveryCommand>(
            serde_json::to_value(submission).unwrap()
        )
        .unwrap(),
        submission
    );
    assert!(
        serde_json::from_value::<SubmittedSetupRecoveryCommand>(serde_json::json!({
            "passphrase_redacted":true,
            "wait_for_backups_to_upload":true,
            "unexpected":true,
        }))
        .is_err()
    );

    let result = E2eeRecoverySetupResult {
        recovery_key_present: true,
    };
    assert_eq!(
        serde_json::from_value::<E2eeRecoverySetupResult>(serde_json::to_value(result).unwrap())
            .unwrap(),
        result
    );
    assert!(
        serde_json::from_value::<E2eeRecoverySetupResult>(serde_json::json!({
            "recovery_key_present":true,
            "unexpected":true,
        }))
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
    let error = MirxApp::find(Duration::from_millis(1)).expect_err("find must fail off Windows");
    assert_eq!(
        error.to_string(),
        "mirx-testkit requires the Windows target"
    );

    let error = MirxApp::find_by_pid(7, Duration::from_millis(1))
        .expect_err("find_by_pid must fail off Windows");
    assert_eq!(
        error.to_string(),
        "mirx-testkit requires the Windows target"
    );

    let launch_error = MirxApp::launch("mirx-ui.exe", &[OsString::from("--automation")])
        .expect_err("launch must fail off Windows");
    assert_eq!(
        launch_error.to_string(),
        "mirx-testkit requires the Windows target"
    );

    let launch_with_options_error = MirxApp::launch_with_options(
        "mirx-ui.exe",
        &[OsString::from("--automation")],
        LaunchOptions {
            startup_timeout: Duration::from_secs(1),
            local_app_data: Some(PathBuf::from("/var/tmp/mirx-testkit")),
            frame_size: None,
        },
    )
    .expect_err("launch_with_options must fail off Windows");
    assert_eq!(
        launch_with_options_error.to_string(),
        "mirx-testkit requires the Windows target"
    );

    let app = MirxApp::find(Duration::from_millis(1)).expect_err("find must fail off Windows");
    assert_eq!(app.to_string(), "mirx-testkit requires the Windows target");
}

#[cfg(not(windows))]
#[test]
fn non_windows_platform_reports_unsupported_for_new_public_methods() {
    let app: MirxApp = unsafe { std::mem::MaybeUninit::zeroed().assume_init() };

    let unsupported = |result: Result<(), TestkitError>| {
        let error = result.expect_err("method must fail off Windows");
        assert_eq!(
            error.to_string(),
            "mirx-testkit requires the Windows target"
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
    unsupported(app.select_timeline_event("$event:example"));
    unsupported(app.accept_invite_via_room_menu());
    unsupported(app.reject_invite_via_room_menu());
    unsupported(app.part_room_via_room_menu());
    unsupported(app.reply_to_selected_message_via_menu());
    unsupported(app.redact_selected_message_via_menu());
    assert!(matches!(
        app.dump_member_menu_state(),
        Err(TestkitError::UnsupportedPlatform(_))
    ));
    unsupported(app.select_member("@bob:example"));
    unsupported(app.query_selected_member_via_menu());
    unsupported(app.mention_selected_member_via_menu());
    unsupported(app.open_member_context_menu("@bob:example", ContextMenuCommand::QueryMember));
    unsupported(app.open_selected_member_context_menu(ContextMenuCommand::MentionMember));
}

#[cfg(not(windows))]
#[test]
fn non_windows_new_methods_report_unsupported_driver() {
    let app: MirxApp = unsafe { std::mem::MaybeUninit::zeroed().assume_init() };

    let error = app
        .dump_ui_state()
        .expect_err("dump_ui_state must fail off Windows");
    assert_eq!(
        error.to_string(),
        "mirx-testkit requires the Windows target"
    );

    for result in [
        app.dump_room_menu_state().map(|_| ()),
        app.dump_message_menu_state().map(|_| ()),
    ] {
        let error = result.expect_err("menu state dump must fail off Windows");
        assert_eq!(
            error.to_string(),
            "mirx-testkit requires the Windows target"
        );
    }

    let error = app
        .select_view(ViewIdentity::Status)
        .expect_err("select_view must fail off Windows");
    assert_eq!(
        error.to_string(),
        "mirx-testkit requires the Windows target"
    );

    let error = app
        .compose_text()
        .expect_err("compose_text must fail off Windows");
    assert_eq!(
        error.to_string(),
        "mirx-testkit requires the Windows target"
    );

    let error = app
        .select_status_view()
        .expect_err("select_status_view must fail off Windows");
    assert_eq!(
        error.to_string(),
        "mirx-testkit requires the Windows target"
    );

    let error = app
        .find_active_view("needle")
        .expect_err("find_active_view must fail off Windows");
    assert_eq!(
        error.to_string(),
        "mirx-testkit requires the Windows target"
    );

    let error = app
        .select_navigation_path(&[NavigationPathSegment::first("mirx")])
        .expect_err("select_navigation_path must fail off Windows");
    assert_eq!(
        error.to_string(),
        "mirx-testkit requires the Windows target"
    );

    let error = app
        .select_navigation_view(&ViewIdentity::Status)
        .expect_err("select_navigation_view must fail off Windows");
    assert_eq!(
        error.to_string(),
        "mirx-testkit requires the Windows target"
    );

    let error = app
        .open_dialog(AutomationDialog::Options)
        .expect_err("open_dialog must fail off Windows");
    assert_eq!(
        error.to_string(),
        "mirx-testkit requires the Windows target"
    );

    let error = app
        .select_options_page("IRC/Messages")
        .expect_err("select_options_page must fail off Windows");
    assert_eq!(
        error.to_string(),
        "mirx-testkit requires the Windows target"
    );

    let error = app
        .choose_tray_menu_item("Status")
        .expect_err("choose_tray_menu_item must fail off Windows");
    assert_eq!(
        error.to_string(),
        "mirx-testkit requires the Windows target"
    );

    let error = app.crash("boom").expect_err("crash must fail off Windows");
    assert_eq!(
        error.to_string(),
        "mirx-testkit requires the Windows target"
    );

    let error = app
        .selected_navigation_path()
        .expect_err("selected_navigation_path must fail off Windows");
    assert_eq!(
        error.to_string(),
        "mirx-testkit requires the Windows target"
    );

    let error = app
        .selected_navigation_view()
        .expect_err("selected_navigation_view must fail off Windows");
    assert_eq!(
        error.to_string(),
        "mirx-testkit requires the Windows target"
    );

    let error = app
        .send_text_target(None, "hello")
        .expect_err("send_text_target must fail off Windows");
    assert_eq!(
        error.to_string(),
        "mirx-testkit requires the Windows target"
    );
}

#[test]
fn automation_dialog_round_trips_and_rejects_unknown_values() {
    for (dialog, text) in [
        (AutomationDialog::Options, "options"),
        (AutomationDialog::AddressBook, "address_book"),
        (AutomationDialog::ChannelCentral, "channel_central"),
        (AutomationDialog::SelectServer, "select_server"),
        (AutomationDialog::About, "about"),
        (AutomationDialog::CommandsJoin, "commands_join"),
        (AutomationDialog::RecoverE2ee, "recover_e2ee"),
        (AutomationDialog::SetupE2eeRecovery, "setup_e2ee_recovery"),
        (AutomationDialog::ResetE2ee, "reset_e2ee"),
        (AutomationDialog::CloseActiveView, "close_active_view"),
    ] {
        assert_eq!(
            serde_json::to_value(dialog).unwrap(),
            serde_json::json!(text)
        );
        assert_eq!(
            serde_json::from_value::<AutomationDialog>(serde_json::json!(text)).unwrap(),
            dialog
        );
    }
    assert!(
        serde_json::from_value::<AutomationDialog>(serde_json::json!("select-server")).is_err()
    );
}

#[test]
fn mdi_arrangement_and_window_state_round_trip_and_reject_unknown_values() {
    for (arrangement, text) in [
        (MdiArrangement::Cascade, "cascade"),
        (MdiArrangement::TileHorizontal, "tile_horizontal"),
        (MdiArrangement::TileVertical, "tile_vertical"),
        (MdiArrangement::ArrangeIcons, "arrange_icons"),
    ] {
        assert_eq!(
            serde_json::to_value(arrangement).unwrap(),
            serde_json::json!(text)
        );
        assert_eq!(
            serde_json::from_value::<MdiArrangement>(serde_json::json!(text)).unwrap(),
            arrangement
        );
    }
    assert!(serde_json::from_value::<MdiArrangement>(serde_json::json!("tile")).is_err());

    for (state, text) in [
        (MdiWindowState::Maximized, "maximized"),
        (MdiWindowState::Normal, "normal"),
        (MdiWindowState::Minimized, "minimized"),
    ] {
        assert_eq!(
            serde_json::to_value(state).unwrap(),
            serde_json::json!(text)
        );
        assert_eq!(
            serde_json::from_value::<MdiWindowState>(serde_json::json!(text)).unwrap(),
            state
        );
    }
    assert!(serde_json::from_value::<MdiWindowState>(serde_json::json!("hidden")).is_err());
}
