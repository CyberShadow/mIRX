//! Test builders and fixtures shared by the win32 modules' tests.

use std::env;
use std::fs;
use std::sync::LazyLock;
use std::sync::Mutex as StdMutex;
use std::sync::atomic::{AtomicU64, Ordering};

use mirx_core::{RoomInviteSummary, RoomMemberRank, RoomStateNoticeActivity, RoomUserProfile};
use tokio::sync::mpsc::UnboundedReceiver;

use super::*;

pub(super) fn user_profile(user_id: &str) -> RoomUserProfile {
    RoomUserProfile {
        user_id: user_id.to_owned(),
        nick: None,
        nick_ambiguous: false,
    }
}

static LOCAL_APP_DATA_LOCK: StdMutex<()> = StdMutex::new(());
static TEST_TEMP_COUNTER: AtomicU64 = AtomicU64::new(1);

fn unique_test_temp_path(prefix: &str) -> PathBuf {
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock must be after unix epoch")
        .as_nanos();
    let counter = TEST_TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
    env::temp_dir().join(format!(
        "{prefix}-{}-{timestamp}-{counter}",
        std::process::id()
    ))
}

pub(super) fn room(room_id: &str, display_name: &str, encrypted: bool) -> RoomSummary {
    RoomSummary {
        room_id: room_id.to_owned(),
        display_name: display_name.to_owned(),
        topic: None,
        membership: RoomMembership::Join,
        encrypted,
        is_space: false,
        is_direct: false,
        direct_user_ids: Vec::new(),
        tags: Vec::new(),
        space_children: Vec::new(),
        notification_mode: Some(RoomNotificationMode::AllMessages),
        canonical_alias: None,
        alt_aliases: Vec::new(),
        invite: None,
    }
}

pub(super) fn invited_room(room_id: &str, display_name: &str) -> RoomSummary {
    RoomSummary {
        room_id: room_id.to_owned(),
        display_name: display_name.to_owned(),
        topic: None,
        membership: RoomMembership::Invite,
        encrypted: false,
        is_space: false,
        is_direct: false,
        direct_user_ids: Vec::new(),
        tags: Vec::new(),
        space_children: Vec::new(),
        notification_mode: None,
        canonical_alias: None,
        alt_aliases: Vec::new(),
        invite: Some(RoomInviteSummary {
            inviter: RoomUserProfile {
                user_id: "@bob:example".to_owned(),
                nick: Some("Bob".to_owned()),
                nick_ambiguous: false,
            },
            direct: false,
        }),
    }
}

pub(super) fn space(room_id: &str, display_name: &str) -> RoomSummary {
    RoomSummary {
        room_id: room_id.to_owned(),
        display_name: display_name.to_owned(),
        topic: None,
        membership: RoomMembership::Join,
        encrypted: false,
        is_space: true,
        is_direct: false,
        direct_user_ids: Vec::new(),
        tags: Vec::new(),
        space_children: Vec::new(),
        notification_mode: Some(RoomNotificationMode::AllMessages),
        canonical_alias: None,
        alt_aliases: Vec::new(),
        invite: None,
    }
}

pub(super) fn e2ee_status(
    recovery: E2eeRecoveryState,
    backup_state: E2eeBackupState,
    backup_enabled_local: bool,
    backup_exists_on_server: Option<bool>,
    cross_signing_complete: Option<bool>,
) -> E2eeStatus {
    E2eeStatus {
        recovery,
        backup: mirx_core::E2eeBackupStatus {
            state: backup_state,
            enabled_locally: backup_enabled_local,
            exists_on_server: backup_exists_on_server,
        },
        cross_signing: cross_signing_complete.map(|complete| E2eeCrossSigningStatus {
            has_master: true,
            has_self_signing: complete,
            has_user_signing: complete,
            complete,
        }),
        session: E2eeSessionState::Verified,
    }
}

pub(super) fn sent_message_activity_with_presentation(
    room_id: &str,
    event_id: &str,
    transaction_id: Option<&str>,
    presentation: RoomMessagePresentationKind,
    body: &str,
    timestamp_ms: u64,
) -> RoomActivity {
    RoomActivity {
        room_id: room_id.to_owned(),
        activity_id: format!("event:{event_id}"),
        timestamp_ms,
        highlight: false,
        relation: None,
        kind: RoomActivityKind::Message(RoomMessageActivity {
            event_id: Some(event_id.to_owned()),
            transaction_id: transaction_id.map(str::to_owned),
            sender: user_profile("@alice:example"),
            presentation,
            body: body.to_owned(),
            styles: Vec::new(),
            user_links: Vec::new(),
            source_body: None,
            send_state: RoomMessageSendState::Sent,
        }),
    }
}

pub(super) fn sent_message_activity_with_relation_and_presentation(
    room_id: &str,
    event_id: &str,
    transaction_id: Option<&str>,
    relation: Option<RoomActivityRelation>,
    presentation: RoomMessagePresentationKind,
    body: &str,
    timestamp_ms: u64,
) -> RoomActivity {
    RoomActivity {
        relation,
        ..sent_message_activity_with_presentation(
            room_id,
            event_id,
            transaction_id,
            presentation,
            body,
            timestamp_ms,
        )
    }
}

pub(super) fn sent_message_activity(
    room_id: &str,
    event_id: &str,
    transaction_id: Option<&str>,
    body: &str,
    timestamp_ms: u64,
) -> RoomActivity {
    sent_message_activity_with_presentation(
        room_id,
        event_id,
        transaction_id,
        RoomMessagePresentationKind::Text,
        body,
        timestamp_ms,
    )
}

pub(super) fn thread_relation(thread_root_event_id: &str) -> RoomActivityRelation {
    RoomActivityRelation::Thread {
        thread_root_event_id: thread_root_event_id.to_owned(),
        in_reply_to_event_id: Some(thread_root_event_id.to_owned()),
        is_falling_back: false,
    }
}

pub(super) fn reply_relation(in_reply_to_event_id: &str) -> RoomActivityRelation {
    RoomActivityRelation::Reply {
        in_reply_to_event_id: in_reply_to_event_id.to_owned(),
    }
}

pub(super) fn timeline(room_id: &str, body: &str) -> Event {
    let event_id = format!("${room_id}:{}", body.replace(' ', "_"));
    Event::RoomActivity(Box::new(sent_message_activity(
        room_id,
        &event_id,
        None,
        body,
        1_700_000_000_000,
    )))
}

pub(super) fn timeline_with_relation_and_presentation(
    room_id: &str,
    event_id: &str,
    transaction_id: Option<&str>,
    relation: Option<RoomActivityRelation>,
    presentation: RoomMessagePresentationKind,
    body: &str,
    timestamp_ms: u64,
) -> Event {
    Event::RoomActivity(Box::new(
        sent_message_activity_with_relation_and_presentation(
            room_id,
            event_id,
            transaction_id,
            relation,
            presentation,
            body,
            timestamp_ms,
        ),
    ))
}

pub(super) fn thread_timeline(
    room_id: &str,
    event_id: &str,
    root_event_id: &str,
    body: &str,
) -> Event {
    timeline_with_relation_and_presentation(
        room_id,
        event_id,
        None,
        Some(thread_relation(root_event_id)),
        RoomMessagePresentationKind::Text,
        body,
        1_700_000_120_000,
    )
}

pub(super) fn timeline_with_transaction_id_with_presentation(
    room_id: &str,
    event_id: &str,
    transaction_id: Option<&str>,
    presentation: RoomMessagePresentationKind,
    body: &str,
    timestamp_ms: u64,
) -> Event {
    Event::RoomActivity(Box::new(sent_message_activity_with_presentation(
        room_id,
        event_id,
        transaction_id,
        presentation,
        body,
        timestamp_ms,
    )))
}

pub(super) fn timeline_with_transaction_id(
    room_id: &str,
    event_id: &str,
    transaction_id: Option<&str>,
    body: &str,
    timestamp_ms: u64,
) -> Event {
    timeline_with_transaction_id_with_presentation(
        room_id,
        event_id,
        transaction_id,
        RoomMessagePresentationKind::Text,
        body,
        timestamp_ms,
    )
}

pub(super) fn pending_message_activity_with_presentation(
    room_id: &str,
    transaction_id: &str,
    presentation: RoomMessagePresentationKind,
    body: &str,
) -> RoomActivity {
    RoomActivity {
        room_id: room_id.to_owned(),
        activity_id: format!("txn:{transaction_id}"),
        timestamp_ms: 1_700_000_000_000,
        highlight: false,
        relation: None,
        kind: RoomActivityKind::Message(RoomMessageActivity {
            event_id: None,
            transaction_id: Some(transaction_id.to_owned()),
            sender: user_profile("@alice:example"),
            presentation,
            body: body.to_owned(),
            styles: Vec::new(),
            user_links: Vec::new(),
            source_body: None,
            send_state: RoomMessageSendState::Pending,
        }),
    }
}

fn pending_message_activity_with_relation_and_presentation(
    room_id: &str,
    transaction_id: &str,
    relation: Option<RoomActivityRelation>,
    presentation: RoomMessagePresentationKind,
    body: &str,
) -> RoomActivity {
    let mut activity =
        pending_message_activity_with_presentation(room_id, transaction_id, presentation, body);
    activity.relation = relation;
    activity
}

pub(super) fn pending_message_activity(
    room_id: &str,
    transaction_id: &str,
    body: &str,
) -> RoomActivity {
    pending_message_activity_with_presentation(
        room_id,
        transaction_id,
        RoomMessagePresentationKind::Text,
        body,
    )
}

pub(super) fn outgoing_queued(room_id: &str, transaction_id: &str, body: &str) -> Event {
    Event::RoomActivity(Box::new(pending_message_activity(
        room_id,
        transaction_id,
        body,
    )))
}

pub(super) fn outgoing_queued_in_thread(
    room_id: &str,
    transaction_id: &str,
    root_event_id: &str,
) -> Event {
    Event::RoomActivity(Box::new(
        pending_message_activity_with_relation_and_presentation(
            room_id,
            transaction_id,
            Some(thread_relation(root_event_id)),
            RoomMessagePresentationKind::Text,
            "pending thread reply",
        ),
    ))
}

pub(super) fn outgoing_sent(room_id: &str, transaction_id: &str, event_id: &str) -> Event {
    Event::RoomActivityUpdate(RoomActivityUpdate {
        room_id: room_id.to_owned(),
        target: RoomActivityUpdateTarget::Transaction {
            transaction_id: transaction_id.to_owned(),
        },
        kind: RoomActivityUpdateKind::MessageSent {
            event_id: event_id.to_owned(),
        },
    })
}

/// Alice's edit of `target_event_id`.
pub(super) fn incoming_replaced(
    room_id: &str,
    target_event_id: &str,
    replacement_event_id: &str,
    presentation: RoomMessagePresentationKind,
    body: &str,
) -> Event {
    Event::RoomActivityUpdate(edit_by(
        room_id,
        target_event_id,
        replacement_event_id,
        "@alice:example",
        1_700_000_000_500,
        presentation,
        body,
    ))
}

pub(super) fn edit_by(
    room_id: &str,
    target_event_id: &str,
    replacement_event_id: &str,
    sender: &str,
    timestamp_ms: u64,
    presentation: RoomMessagePresentationKind,
    body: &str,
) -> RoomActivityUpdate {
    RoomActivityUpdate {
        room_id: room_id.to_owned(),
        target: RoomActivityUpdateTarget::Event {
            event_id: target_event_id.to_owned(),
        },
        kind: RoomActivityUpdateKind::MessageReplaced {
            replacement_event_id: replacement_event_id.to_owned(),
            sender: sender.to_owned(),
            timestamp_ms,
            presentation,
            body: body.to_owned(),
            styles: Vec::new(),
            user_links: Vec::new(),
            source_body: None,
        },
    }
}

pub(super) fn incoming_redacted(
    room_id: &str,
    target_event_id: &str,
    redaction_event_id: &str,
) -> Event {
    Event::RoomActivityUpdate(RoomActivityUpdate {
        room_id: room_id.to_owned(),
        target: RoomActivityUpdateTarget::Event {
            event_id: target_event_id.to_owned(),
        },
        kind: RoomActivityUpdateKind::ActivityRedacted {
            redaction_event_id: redaction_event_id.to_owned(),
        },
    })
}

pub(super) fn incoming_reaction_added(
    room_id: &str,
    target_event_id: &str,
    reaction_event_id: &str,
    sender: &str,
    key: &str,
    timestamp_ms: u64,
) -> Event {
    Event::RoomActivityUpdate(RoomActivityUpdate {
        room_id: room_id.to_owned(),
        target: RoomActivityUpdateTarget::Event {
            event_id: target_event_id.to_owned(),
        },
        kind: RoomActivityUpdateKind::ReactionAdded {
            reaction_event_id: reaction_event_id.to_owned(),
            sender: sender.to_owned(),
            timestamp_ms,
            key: key.to_owned(),
        },
    })
}

pub(super) fn room_read_state(
    room_id: &str,
    latest_active_event_id: Option<&str>,
    fully_read_event_id: Option<&str>,
    marked_unread: bool,
    num_unread: u64,
    num_notifications: u64,
    num_mentions: u64,
) -> Event {
    Event::RoomReadState(RoomReadState {
        room_id: room_id.to_owned(),
        latest_active_event_id: latest_active_event_id.map(str::to_owned),
        fully_read_event_id: fully_read_event_id.map(str::to_owned),
        fully_read_timestamp_ms: None,
        marked_unread,
        num_unread,
        num_notifications,
        num_mentions,
    })
}

pub(super) fn room_history_backfill(
    room_id: &str,
    end: Option<&str>,
    events: Vec<RoomHistoryBackfillEvent>,
) -> Event {
    Event::RoomHistoryBackfill(RoomHistoryBackfill {
        room_id: room_id.to_owned(),
        end: end.map(str::to_owned),
        events,
    })
}

pub(super) fn failed_message_activity_with_presentation(
    room_id: &str,
    transaction_id: &str,
    presentation: RoomMessagePresentationKind,
    body: &str,
    message: &str,
) -> RoomActivity {
    RoomActivity {
        room_id: room_id.to_owned(),
        activity_id: format!("txn:{transaction_id}"),
        timestamp_ms: 1_700_000_000_000,
        highlight: false,
        relation: None,
        kind: RoomActivityKind::Message(RoomMessageActivity {
            event_id: None,
            transaction_id: Some(transaction_id.to_owned()),
            sender: user_profile("@alice:example"),
            presentation,
            body: body.to_owned(),
            styles: Vec::new(),
            user_links: Vec::new(),
            source_body: None,
            send_state: RoomMessageSendState::Failed {
                message: message.to_owned(),
                recoverable: false,
            },
        }),
    }
}

pub(super) fn failed_message_with_presentation(
    room_id: &str,
    transaction_id: &str,
    presentation: RoomMessagePresentationKind,
    body: &str,
    message: &str,
) -> Event {
    Event::RoomActivity(Box::new(failed_message_activity_with_presentation(
        room_id,
        transaction_id,
        presentation,
        body,
        message,
    )))
}

pub(super) fn failed_message(
    room_id: &str,
    transaction_id: &str,
    body: &str,
    message: &str,
) -> Event {
    failed_message_with_presentation(
        room_id,
        transaction_id,
        RoomMessagePresentationKind::Text,
        body,
        message,
    )
}

pub(super) fn topic_state(room_id: &str, topic: &str) -> Event {
    Event::RoomActivity(Box::new(RoomActivity {
        room_id: room_id.to_owned(),
        activity_id: format!("state:$topic:{room_id}:{topic}"),
        timestamp_ms: 1_700_000_060_000,
        highlight: false,
        relation: None,
        kind: RoomActivityKind::StateNotice(RoomStateNoticeActivity {
            event_id: format!("$topic:{room_id}:{topic}"),
            sender: user_profile("@alice:example"),
            content: RoomStateNoticeContent::Topic {
                topic: topic.to_owned(),
            },
        }),
    }))
}

pub(super) fn unsupported_message_activity(
    room_id: &str,
    event_id: &str,
    sender: &str,
    timestamp_ms: u64,
    msgtype: &str,
    body: &str,
) -> RoomActivity {
    RoomActivity {
        room_id: room_id.to_owned(),
        activity_id: format!("event:{event_id}"),
        timestamp_ms,
        highlight: false,
        relation: None,
        kind: RoomActivityKind::UnsupportedMessage(mirx_core::UnsupportedRoomMessageActivity {
            event_id: event_id.to_owned(),
            sender: user_profile(sender),
            msgtype: msgtype.to_owned(),
            body: body.to_owned(),
        }),
    }
}

pub(super) fn unsupported_message(
    room_id: &str,
    event_id: &str,
    sender: &str,
    timestamp_ms: u64,
    msgtype: &str,
    body: &str,
) -> Event {
    Event::RoomActivity(Box::new(unsupported_message_activity(
        room_id,
        event_id,
        sender,
        timestamp_ms,
        msgtype,
        body,
    )))
}

pub(super) fn unable_to_decrypt_activity(
    room_id: &str,
    event_id: &str,
    sender: &str,
    timestamp_ms: u64,
) -> Event {
    Event::RoomActivity(Box::new(RoomActivity {
        room_id: room_id.to_owned(),
        activity_id: format!("event:{event_id}"),
        timestamp_ms,
        highlight: false,
        relation: None,
        kind: RoomActivityKind::UnableToDecrypt(mirx_core::RoomUnableToDecryptActivity {
            event_id: event_id.to_owned(),
            sender: user_profile(sender),
            session_id: Some("SESSIONID".to_owned()),
            cause: mirx_core::RoomUnableToDecryptCause::Unknown,
        }),
    }))
}

pub(super) fn image_activity(
    room_id: &str,
    event_id: &str,
    sender: &str,
    timestamp_ms: u64,
    body: &str,
    filename: Option<&str>,
    info: Option<RoomImageInfo>,
) -> RoomActivity {
    RoomActivity {
        room_id: room_id.to_owned(),
        activity_id: format!("event:{event_id}"),
        timestamp_ms,
        highlight: false,
        relation: None,
        kind: RoomActivityKind::Image(Box::new(mirx_core::RoomImageActivity {
            event_id: event_id.to_owned(),
            sender: user_profile(sender),
            body: body.to_owned(),
            filename: filename.map(str::to_owned),
            source: mirx_core::RoomMediaSource::Plain {
                url: "mxc://example.org/diagram".to_owned(),
            },
            info,
        })),
    }
}

/// Bob's poll line: a start or an end.
pub(super) fn poll_activity(
    room_id: &str,
    event_id: &str,
    timestamp_ms: u64,
    content: RoomPollContent,
) -> RoomActivity {
    RoomActivity {
        room_id: room_id.to_owned(),
        activity_id: format!("event:{event_id}"),
        timestamp_ms,
        highlight: false,
        relation: None,
        kind: RoomActivityKind::Poll(Box::new(RoomPollActivity {
            event_id: event_id.to_owned(),
            sender: user_profile("@bob:example"),
            content,
        })),
    }
}

pub(super) fn poll_start(question: &str, answers: &[&str]) -> RoomPollContent {
    RoomPollContent::Start {
        question: question.to_owned(),
        answers: answers.iter().map(|answer| (*answer).to_owned()).collect(),
        max_selections: 1,
        results_hidden: false,
    }
}

pub(super) fn poll_end(poll_start_event_id: &str, text: &str) -> RoomPollContent {
    RoomPollContent::End {
        poll_start_event_id: poll_start_event_id.to_owned(),
        text: text.to_owned(),
    }
}

/// Bob's request that `to` verify, in the room.
pub(super) fn verification_request_activity(
    room_id: &str,
    event_id: &str,
    timestamp_ms: u64,
    to: &str,
) -> RoomActivity {
    RoomActivity {
        room_id: room_id.to_owned(),
        activity_id: format!("event:{event_id}"),
        timestamp_ms,
        highlight: false,
        relation: None,
        kind: RoomActivityKind::VerificationRequest(mirx_core::RoomVerificationRequestActivity {
            event_id: event_id.to_owned(),
            sender: user_profile("@bob:example"),
            to: to.to_owned(),
        }),
    }
}

/// A sticker with a plain source and no thumbnail.
pub(super) fn sticker_activity(
    room_id: &str,
    event_id: &str,
    sender: &str,
    timestamp_ms: u64,
    body: &str,
) -> RoomActivity {
    RoomActivity {
        room_id: room_id.to_owned(),
        activity_id: format!("event:{event_id}"),
        timestamp_ms,
        highlight: false,
        relation: None,
        kind: RoomActivityKind::Sticker(Box::new(mirx_core::RoomImageActivity {
            event_id: event_id.to_owned(),
            sender: user_profile(sender),
            body: body.to_owned(),
            filename: None,
            source: mirx_core::RoomMediaSource::Plain {
                url: "mxc://example.org/sticker".to_owned(),
            },
            info: None,
        })),
    }
}

pub(super) fn file_activity(
    room_id: &str,
    event_id: &str,
    sender: &str,
    timestamp_ms: u64,
    body: &str,
    filename: Option<&str>,
    info: Option<RoomFileInfo>,
) -> RoomActivity {
    RoomActivity {
        room_id: room_id.to_owned(),
        activity_id: format!("event:{event_id}"),
        timestamp_ms,
        highlight: false,
        relation: None,
        kind: RoomActivityKind::File(Box::new(mirx_core::RoomFileActivity {
            event_id: event_id.to_owned(),
            sender: user_profile(sender),
            body: body.to_owned(),
            filename: filename.map(str::to_owned),
            source: mirx_core::RoomMediaSource::Plain {
                url: "mxc://example.org/report".to_owned(),
            },
            info,
        })),
    }
}

pub(super) fn audio_activity(
    room_id: &str,
    event_id: &str,
    sender: &str,
    timestamp_ms: u64,
    body: &str,
    filename: Option<&str>,
    info: Option<RoomAudioInfo>,
) -> RoomActivity {
    RoomActivity {
        room_id: room_id.to_owned(),
        activity_id: format!("event:{event_id}"),
        timestamp_ms,
        highlight: false,
        relation: None,
        kind: RoomActivityKind::Audio(Box::new(mirx_core::RoomAudioActivity {
            event_id: event_id.to_owned(),
            sender: user_profile(sender),
            body: body.to_owned(),
            filename: filename.map(str::to_owned),
            source: mirx_core::RoomMediaSource::Plain {
                url: "mxc://example.org/audio".to_owned(),
            },
            info,
        })),
    }
}

pub(super) fn image_message(
    room_id: &str,
    event_id: &str,
    sender: &str,
    timestamp_ms: u64,
    body: &str,
    filename: Option<&str>,
    info: Option<RoomImageInfo>,
) -> Event {
    Event::RoomActivity(Box::new(image_activity(
        room_id,
        event_id,
        sender,
        timestamp_ms,
        body,
        filename,
        info,
    )))
}

pub(super) fn video_message(room_id: &str, event_id: &str) -> Event {
    video_message_with_thumbnail(room_id, event_id, true)
}

pub(super) fn video_message_with_thumbnail(
    room_id: &str,
    event_id: &str,
    has_thumbnail: bool,
) -> Event {
    Event::RoomActivity(Box::new(RoomActivity {
        room_id: room_id.to_owned(),
        activity_id: format!("event:{event_id}"),
        timestamp_ms: 1_710_000_000_000,
        highlight: false,
        relation: None,
        kind: RoomActivityKind::Video(Box::new(mirx_core::RoomVideoActivity {
            event_id: event_id.to_owned(),
            sender: user_profile("@alice:example.org"),
            body: "clip.mp4".to_owned(),
            filename: Some("clip.mp4".to_owned()),
            source: RoomMediaSource::Plain {
                url: "mxc://example.org/video".to_owned(),
            },
            info: Some(RoomVideoInfo {
                mimetype: Some("video/mp4".to_owned()),
                size: Some(34_567),
                duration_ms: Some(12_000),
                width: Some(1920),
                height: Some(1080),
                thumbnail_source: has_thumbnail.then(|| RoomMediaSource::Plain {
                    url: "mxc://example.org/video-thumbnail".to_owned(),
                }),
            }),
        })),
    }))
}

pub(super) fn room_preview_media_event(
    room_id: &str,
    activity_id: &str,
    event_id: &str,
    kind: RoomMediaKind,
) -> Event {
    Event::RoomMedia(RoomMedia {
        room_id: room_id.to_owned(),
        activity_id: activity_id.to_owned(),
        event_id: event_id.to_owned(),
        kind,
    })
}

pub(super) fn tiny_png_bytes() -> Vec<u8> {
    vec![
        137, 80, 78, 71, 13, 10, 26, 10, 0, 0, 0, 13, 73, 72, 68, 82, 0, 0, 0, 1, 0, 0, 0, 1, 8, 4,
        0, 0, 0, 181, 28, 12, 2, 0, 0, 0, 11, 73, 68, 65, 84, 120, 218, 99, 252, 255, 31, 0, 3, 3,
        2, 0, 239, 191, 107, 212, 0, 0, 0, 0, 73, 69, 78, 68, 174, 66, 96, 130,
    ]
}

pub(super) fn typing_event(room_id: &str, user_ids: &[&str]) -> Event {
    Event::RoomTyping(RoomTyping {
        room_id: room_id.to_owned(),
        user_ids: user_ids
            .iter()
            .map(|user_id| (*user_id).to_owned())
            .collect(),
    })
}

pub(super) fn stored_session() -> StoredSession {
    StoredSession {
        user_id: "@alice:example".to_owned(),
        device_id: "DEVICE".to_owned(),
        access_token: "secret-token".to_owned(),
        refresh_token: Some("refresh-token".to_owned()),
        auth: SessionAuth::Matrix,
    }
}

pub(super) fn saved_session_payload() -> SavedSession {
    SavedSession {
        version: SAVED_SESSION_VERSION,
        homeserver_url: "https://matrix.example".to_owned(),
        session: stored_session(),
    }
}

pub(super) fn session_file_path(root: &Path) -> PathBuf {
    root.join(PROFILE_SUBDIR).join(SESSION_FILE_NAME)
}

pub(super) fn assert_bytes_do_not_contain(bytes: &[u8], needle: &str) {
    assert!(
        !bytes
            .windows(needle.len())
            .any(|window| window == needle.as_bytes()),
        "bytes unexpectedly contained {needle:?}"
    );
}

pub(super) fn with_local_app_data<T>(test: impl FnOnce(PathBuf) -> T) -> T {
    let _guard = match LOCAL_APP_DATA_LOCK.lock() {
        Ok(guard) => guard,
        Err(poisoned) => {
            LOCAL_APP_DATA_LOCK.clear_poison();
            poisoned.into_inner()
        }
    };
    let root = unique_test_temp_path("mirx-ui-local-app-data");
    fs::create_dir_all(&root).expect("test LOCALAPPDATA root must be created");
    let previous = env::var_os("LOCALAPPDATA");
    unsafe {
        env::set_var("LOCALAPPDATA", &root);
    }
    let result = test(root.clone());
    match previous {
        Some(previous) => unsafe {
            env::set_var("LOCALAPPDATA", previous);
        },
        None => unsafe {
            env::remove_var("LOCALAPPDATA");
        },
    }
    let _ = fs::remove_dir_all(root);
    result
}

pub(super) fn test_state() -> WindowState {
    let (gdiplus_session, gdiplus_startup_error) = initialize_gdiplus_state();
    WindowState {
        controls: Controls::default(),
        event_queue: Arc::new(Mutex::new(VecDeque::new())),
        command_sender: None,
        core_thread: None,
        harness: Harness::default(),
        clock: UiClock::Fixed(RENDER_STATE_CLOCK_MS),
        settings: SettingsStore::in_memory(),
        aliases: AliasStore::in_memory(),
        local_time: timestamp::utc_local_time,
        text_font: unsafe { TextFont::create(&FontSetting::default(), Dpi::STANDARD).unwrap() },
        scheme_brushes: unsafe { SchemeBrushes::create(&Default::default()) },
        exit_code: Arc::new(AtomicI32::new(0)),
        event_generation: 0,
        core_answers: AnsweredCommands::default(),
        script_runs: ScriptRuns::default(),
        tree_items: BTreeMap::new(),
        primary_tree_items: BTreeMap::new(),
        views: BTreeMap::new(),
        hwnd_views: BTreeMap::new(),
        active_view: ViewId::Status,
        selected_timeline_event: None,
        find_states: BTreeMap::new(),
        pending_jump: None,
        switchbar_views: Vec::new(),
        switchbar_faces: Vec::new(),
        applied_navigation: None,
        room_activity: BTreeMap::new(),
        sent_receipts: Default::default(),
        closed_queries: BTreeSet::new(),
        quiet_joins: BTreeSet::new(),
        pending_activation: None,
        nick_completion: None,
        edit_window_proc: None,
        model: initial_model(),
        decoded_images: BTreeMap::new(),
        gdiplus_session,
        gdiplus_startup_error,
        typing_notices: super::typing::TypingNotices::default(),
        pending_attachment_saves: BTreeMap::new(),
        channel_central_dialog: None,
        sessions_dialog: None,
        tray: TrayState::default(),
        reconnect: connection::Reconnect::default(),
        catch_up_flash: false,
        notify_watch: notify::NotifyWatch::default(),
        room_count_due: false,
        log_out_warning: None,
        painted_presence_revision: 0,
        treebar_drag: None,
    }
}

/// Runs `test` on a stand-in frame whose active view is a stand-in MDI
/// child holding the compose editbox.
pub(super) unsafe fn with_test_compose_window<T>(
    state: WindowState,
    test: impl FnOnce(HWND) -> T,
) -> T {
    init_common_controls().unwrap();
    let create = |class_name: &str, style: u32, parent: HWND| {
        let hwnd = CreateWindowExW(
            0,
            wide_null(class_name).as_ptr(),
            wide_null("").as_ptr(),
            style,
            0,
            0,
            0,
            0,
            parent,
            null_mut(),
            GetModuleHandleW(null()),
            null_mut(),
        );
        assert!(!hwnd.is_null(), "test {class_name} control must be created");
        hwnd
    };
    let frame = create(STATIC_CLASS_NAME, 0, null_mut());
    let child = create(STATIC_CLASS_NAME, WS_CHILD, frame);
    // The real editbox's style: without ES_AUTOHSCROLL, an edit box drops
    // what does not fit its (here empty) width.
    let compose = create(EDIT_CLASS_NAME, COMPOSE_EDIT_STYLE, child);

    let status_bar = CreateWindowExW(
        0,
        STATUSCLASSNAMEW,
        null(),
        0,
        0,
        0,
        0,
        0,
        null_mut(),
        null_mut(),
        GetModuleHandleW(null()),
        null_mut(),
    );
    assert!(
        !status_bar.is_null(),
        "test status bar control must be created"
    );

    let mut data = Box::new(window_data::WindowData::new(state));
    window_data::lend(frame, &mut *data);
    let state = state_mut(frame);
    state.controls.status_bar = status_bar;
    let active_view = state.active_view.clone();
    state.hwnd_views.insert(child as isize, active_view.clone());
    state.views.insert(
        active_view,
        MdiViewState {
            child,
            content: null_mut(),
            compose: Some(ViewCompose {
                edit: compose,
                history: ComposeHistory::default(),
                target: None,
            }),
            log: ViewLog::Off,
            rows_key: None,
            nick_list: Vec::new(),
        },
    );

    let result = test(frame);
    DestroyWindow(frame);
    DestroyWindow(status_bar);
    drop(data);
    result
}

/// Gives the stand-in active view the content window its kind has: the
/// timeline itself for Status, a room content window otherwise.
pub(super) unsafe fn install_test_view_content(hwnd: HWND) {
    let instance = GetModuleHandleW(null());
    register_room_content_class(instance).unwrap();
    register_timeline_class(instance).unwrap();
    let active_view = state_ref(hwnd).active_view.clone();
    let class_name = match active_view {
        ViewId::Status => TIMELINE_CLASS_NAME,
        ViewId::Room(_) | ViewId::Thread { .. } => ROOM_CONTENT_CLASS_NAME,
        ViewId::RoomList => unreachable!("test windows have a timeline"),
    };
    let content = CreateWindowExW(
        0,
        wide_null(class_name).as_ptr(),
        wide_null("").as_ptr(),
        WS_CHILD | WS_VISIBLE,
        0,
        0,
        0,
        0,
        hwnd,
        null_mut(),
        instance,
        null_mut(),
    );
    assert!(!content.is_null(), "test view content must be created");
    state_mut(hwnd)
        .views
        .get_mut(&active_view)
        .expect("the test compose window must track the active view")
        .content = content;
    set_test_timeline_font(hwnd, &active_view, content);
}

/// Gives the stand-in view's timeline the frame's text font, as
/// creating a real window does.
pub(super) unsafe fn set_test_timeline_font(hwnd: HWND, view_id: &ViewId, content: HWND) {
    SendMessageW(
        view_timeline(view_id, content),
        WM_SETFONT,
        state_ref(hwnd).text_font.hfont() as WPARAM,
        0,
    );
}

/// Types `input` into the active editbox and presses Return.
pub(super) unsafe fn type_compose_line(hwnd: HWND, input: &str) {
    set_compose_text(hwnd, input).unwrap();
    submit_compose_from_edit(hwnd).unwrap();
}

/// The default timestamps, in UTC.
pub(super) fn utc_timestamps() -> TimestampStyle<'static> {
    static SETTING: LazyLock<TimestampSetting> = LazyLock::new(TimestampSetting::default);
    TimestampStyle::new(&SETTING, timestamp::utc_local_time)
}

pub(super) fn tracked_view(hwnd_value: isize) -> MdiViewState {
    MdiViewState {
        child: hwnd_value as HWND,
        content: (hwnd_value + 1) as HWND,
        compose: Some(ViewCompose {
            edit: (hwnd_value + 2) as HWND,
            history: ComposeHistory::default(),
            target: None,
        }),
        log: ViewLog::Off,
        rows_key: None,
        nick_list: Vec::new(),
    }
}

pub(super) fn room_view_id(room_id: &str) -> ViewId {
    ViewId::Room(room_id.to_owned())
}

pub(super) fn thread_view_id(room_id: &str, thread_root_event_id: &str) -> ViewId {
    ViewId::Thread {
        room_id: room_id.to_owned(),
        thread_root_event_id: thread_root_event_id.to_owned(),
    }
}

/// A sender whose commands wait in the returned receiver, for tests of
/// what the UI submits: a real core would dial the test homeserver.
pub(super) fn inert_command_sender() -> (CommandSender, UnboundedReceiver<(CommandId, Command)>) {
    let (sender, receiver) = unbounded_channel();
    (CommandSender::new(sender), receiver)
}

pub(super) fn start_test_core() -> CoreHandle {
    let profile_dir = unique_test_temp_path("mirx-ui-win32-tests");
    fs::create_dir_all(&profile_dir).expect("test profile dir must be created");

    start(CoreConfig {
        profile_dir,
        store_passphrase: "mirx-ui-win32-tests".to_owned(),
        save_session: Arc::new(save_session),
    })
    .expect("test core must start")
}

pub(super) fn test_state_with_command_sender() -> (WindowState, CoreHandle) {
    let mut state = test_state();
    let handle = start_test_core();
    state.command_sender = Some(handle.command_sender().clone());
    (state, handle)
}

pub(super) fn stop_test_core(handle: CoreHandle) {
    handle
        .command_sender()
        .send(Command::Shutdown)
        .expect("shutdown command must enqueue");
    handle.join().expect("test core must stop cleanly");
}

/// The Ops room with `$q` ("are we deploying today?") and Bob's reply
/// to `target`.
pub(super) fn reply_quote_state(target: &str) -> WindowState {
    let mut state = test_state();
    state.model.rooms = vec![room("!ops:example", "Ops", false)];
    apply_core_event(
        &mut state,
        timeline_with_transaction_id(
            "!ops:example",
            "$q",
            None,
            "are we deploying today?\nsecond line",
            1_700_000_000_000,
        ),
    )
    .unwrap();
    let mut reply = sent_message_activity_with_relation_and_presentation(
        "!ops:example",
        "$a",
        None,
        Some(reply_relation(target)),
        RoomMessagePresentationKind::Text,
        "yes, after lunch",
        1_700_000_060_000,
    );
    let RoomActivityKind::Message(message) = &mut reply.kind else {
        unreachable!()
    };
    message.sender = user_profile("@bob:example");
    apply_core_event(&mut state, Event::RoomActivity(Box::new(reply))).unwrap();
    state
}

pub(super) fn bold_style() -> mirx_core::TextStyle {
    mirx_core::TextStyle {
        bold: true,
        ..mirx_core::TextStyle::default()
    }
}

pub(super) fn member_summary(user_id: &str, nick: &str, nick_ambiguous: bool) -> RoomMemberSummary {
    RoomMemberSummary {
        user_id: user_id.to_owned(),
        nick: nick.to_owned(),
        nick_ambiguous,
        is_own_user: false,
        rank: RoomMemberRank::Member,
        can_kick: false,
        can_ban: false,
    }
}

pub(super) fn status_log_lines(state: &WindowState) -> Vec<String> {
    project_timeline_rows(&build_status_timeline_rows(
        &state.model.status_log,
        state.timestamps(),
    ))
}

pub(super) fn query(room_id: &str, display_name: &str, user_id: &str) -> RoomSummary {
    let mut room = room(room_id, display_name, true);
    room.is_direct = true;
    room.direct_user_ids = vec![user_id.to_owned()];
    room
}

/// A view's item; a room's is a room's (`nav_direct`, `nav_invite` for
/// the others).
pub(super) fn nav_view(label: &str, view: ViewId, children: Vec<NavItem>) -> NavItem {
    let icon = match view {
        ViewId::Status => WindowIcon::Status,
        ViewId::Room(_) => WindowIcon::Room,
        ViewId::Thread { .. } => WindowIcon::Thread,
        ViewId::RoomList => WindowIcon::RoomList,
    };
    NavItem {
        label: label.to_owned(),
        icon,
        node: NavNode::View(view),
        children,
    }
}

pub(super) fn nav_room(label: &str, room_id: &str) -> NavItem {
    nav_view(label, room_view_id(room_id), Vec::new())
}

pub(super) fn nav_direct(label: &str, room_id: &str) -> NavItem {
    NavItem {
        icon: WindowIcon::Direct,
        ..nav_room(label, room_id)
    }
}

pub(super) fn nav_invite(label: &str, room_id: &str) -> NavItem {
    NavItem {
        icon: WindowIcon::Invite,
        ..nav_room(label, room_id)
    }
}

pub(super) fn highlighted(mut activity: RoomActivity) -> RoomActivity {
    activity.highlight = true;
    activity
}

/// The Ops room, active, with Alice's question `$q`, the user's text
/// `$mine` and emote `$wave`, and a sender whose commands wait.
pub(super) fn compose_target_state() -> WindowState {
    let mut state = test_state();
    let (sender, commands) = inert_command_sender();
    // Kept open for the state's lifetime: sends must not fail.
    std::mem::forget(commands);
    state.command_sender = Some(sender);
    state.model.own_account = Some(OwnAccount::matrix("@me:example".to_owned()));
    state.model.rooms = vec![room("!ops:example", "Ops", false)];
    let mine = |event_id: &str, presentation, body: &str, timestamp_ms| {
        let mut activity = sent_message_activity_with_presentation(
            "!ops:example",
            event_id,
            None,
            presentation,
            body,
            timestamp_ms,
        );
        let RoomActivityKind::Message(message) = &mut activity.kind else {
            unreachable!()
        };
        message.sender = user_profile("@me:example");
        Event::RoomActivity(Box::new(activity))
    };
    for event in [
        timeline_with_transaction_id(
            "!ops:example",
            "$q",
            None,
            "are we deploying today?",
            1_700_000_000_000,
        ),
        mine(
            "$mine",
            RoomMessagePresentationKind::Text,
            "deploy at 14:00",
            1_700_000_060_000,
        ),
        mine(
            "$wave",
            RoomMessagePresentationKind::Emote,
            "waves",
            1_700_000_120_000,
        ),
    ] {
        apply_core_event(&mut state, event).unwrap();
    }
    state.active_view = room_view_id("!ops:example");
    state
}

pub(super) unsafe fn with_compose_target_window(test: impl FnOnce(HWND)) {
    with_test_compose_window(compose_target_state(), |hwnd| {
        install_test_view_content(hwnd);
        refresh_view_content(hwnd, &room_view_id("!ops:example")).unwrap();
        test(hwnd);
    });
}

pub(super) unsafe fn active_compose_target(hwnd: HWND) -> Option<ComposeTarget> {
    let state = state_ref(hwnd);
    state.views[&state.active_view].compose().target.clone()
}

/// The active window's lines as its timeline holds them.
pub(super) unsafe fn active_timeline_lines(hwnd: HWND) -> Vec<String> {
    let state = state_ref(hwnd);
    let content = state.views[&state.active_view].content;
    project_timeline_rows(&timeline_state_ref(room_content_timeline(content)).source_rows)
}

pub(super) fn reply_command(presentation: RoomMessagePresentationKind, body: &str) -> Command {
    Command::SendMessage(SendMessageRequest {
        room_id: "!ops:example".to_owned(),
        presentation,
        body: body.to_owned(),
        format: ComposeFormat::Markdown,
        thread: None,
        reply: Some(SendMessageReplyTarget {
            event_id: "$q".to_owned(),
            sender_user_id: "@alice:example".to_owned(),
        }),
    })
}

/// Runs `test` on a real frame `width` x `height` with its controls and
/// the Status window, and destroys it afterwards.
pub(super) unsafe fn with_test_frame<T>(
    width: i32,
    height: i32,
    test: impl FnOnce(HWND) -> T,
) -> T {
    let instance = GetModuleHandleW(null());
    init_common_controls().unwrap();
    register_frame_class(instance).unwrap();
    register_mdi_child_class(instance).unwrap();
    register_room_content_class(instance).unwrap();
    register_timeline_class(instance).unwrap();
    let hwnd = create_main_window(
        instance,
        Box::new(test_state()),
        FrameSize { width, height },
    )
    .unwrap();
    let result = test(hwnd);
    DestroyWindow(hwnd);
    result
}
