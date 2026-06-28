use std::collections::BTreeSet;
use std::error::Error;
use std::fmt::{self, Display, Formatter};

use matrix_sdk::{
    Client, LoopCtrl, Room, SessionChange, SessionMeta, SessionTokens,
    authentication::matrix::MatrixSession,
    config::SyncSettings,
    deserialized_responses::SyncOrStrippedState,
    encryption::{BackupDownloadStrategy, EncryptionSettings},
    ruma::{
        MilliSecondsSinceUnixEpoch, OwnedDeviceId, OwnedUserId,
        events::{
            AnyMessageLikeEventContent, SyncStateEvent,
            room::{
                member::{OriginalSyncRoomMemberEvent, SyncRoomMemberEvent},
                message::{MessageType, OriginalSyncRoomMessageEvent, RoomMessageEventContent},
                topic::{OriginalSyncRoomTopicEvent, SyncRoomTopicEvent},
            },
            space::child::{SpaceChildEventContent, SpaceChildOrd},
            typing::SyncTypingEvent,
        },
    },
    send_queue::{LocalEcho, LocalEchoContent, RoomSendQueueUpdate, SendQueueUpdate},
    store::{RoomLoadSettings, SerializableEventContent},
};
use tokio::task::JoinHandle;

use crate::api::{
    CoreConfig, CoreErrorEvent, Event, LoginRequest, Operation, RoomActivity, RoomActivityKind,
    RoomActivityUpdate, RoomActivityUpdateKind, RoomMessageActivity, RoomMessageSendState,
    RoomStateNoticeActivity, RoomStateNoticeContent, RoomSummary, RoomTyping, SendTextRequest,
    SpaceChildSummary, StoredSession,
};
use crate::runtime::{EventSender, emit_event};

#[derive(Debug)]
pub(crate) struct MatrixCoreError {
    message: String,
}

impl MatrixCoreError {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl Display for MatrixCoreError {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl Error for MatrixCoreError {}

pub(crate) async fn build_client(
    config: &CoreConfig,
    homeserver_url: &str,
) -> Result<Client, MatrixCoreError> {
    let store_path = config.profile_dir.join("matrix-store");
    std::fs::create_dir_all(&store_path)
        .map_err(|err| MatrixCoreError::new(format!("failed to create store directory: {err}")))?;

    Client::builder()
        .homeserver_url(homeserver_url)
        .sqlite_store(&store_path, Some(config.store_passphrase.as_str()))
        .with_encryption_settings(encryption_settings())
        .handle_refresh_tokens()
        .build()
        .await
        .map_err(|err| MatrixCoreError::new(format!("failed to build Matrix client: {err}")))
}

pub(crate) fn encryption_settings() -> EncryptionSettings {
    EncryptionSettings {
        auto_enable_cross_signing: true,
        auto_enable_backups: true,
        backup_download_strategy: BackupDownloadStrategy::AfterDecryptionFailure,
    }
}

pub(crate) async fn restore_session(
    client: &Client,
    dto: StoredSession,
) -> Result<(), MatrixCoreError> {
    let session = MatrixSession {
        meta: SessionMeta {
            user_id: OwnedUserId::try_from(dto.user_id)
                .map_err(|err| MatrixCoreError::new(format!("invalid session user id: {err}")))?,
            device_id: OwnedDeviceId::from(dto.device_id),
        },
        tokens: SessionTokens {
            access_token: dto.access_token,
            refresh_token: dto.refresh_token,
        },
    };

    client
        .matrix_auth()
        .restore_session(session, RoomLoadSettings::default())
        .await
        .map_err(|err| MatrixCoreError::new(format!("failed to restore Matrix session: {err}")))
}

pub(crate) async fn login(
    client: &Client,
    request: &LoginRequest,
) -> Result<StoredSession, MatrixCoreError> {
    let response = client
        .matrix_auth()
        .login_username(&request.username, &request.password)
        .initial_device_display_name(&request.device_display_name)
        .request_refresh_token()
        .send()
        .await
        .map_err(|err| MatrixCoreError::new(format!("failed to log in to Matrix: {err}")))?;

    Ok(stored_session_from_matrix_session(MatrixSession::from(
        &response,
    )))
}

pub(crate) async fn initial_room_sync(
    client: &Client,
) -> Result<Vec<RoomSummary>, MatrixCoreError> {
    client
        .sync_once(SyncSettings::default())
        .await
        .map_err(|err| MatrixCoreError::new(format!("initial Matrix sync failed: {err}")))?;

    summarize_joined_rooms(client).await
}

pub(crate) async fn summarize_joined_rooms(
    client: &Client,
) -> Result<Vec<RoomSummary>, MatrixCoreError> {
    let joined_rooms = client.joined_rooms();
    let joined_room_ids = joined_rooms
        .iter()
        .map(|room| room.room_id().to_string())
        .collect::<BTreeSet<_>>();
    let mut rooms = Vec::with_capacity(joined_rooms.len());

    for room in joined_rooms {
        let display_name = room.display_name().await.map_err(|err| {
            MatrixCoreError::new(format!("failed to read room display name: {err}"))
        })?;
        let encrypted = room.latest_encryption_state().await.map_err(|err| {
            MatrixCoreError::new(format!("failed to read room encryption state: {err}"))
        })?;
        let is_space = room.is_space();
        let space_children = if is_space {
            summarize_space_children(&room, &joined_room_ids).await?
        } else {
            Vec::new()
        };

        rooms.push(RoomSummary {
            room_id: room.room_id().to_string(),
            display_name: display_name.to_string(),
            encrypted: encrypted.is_encrypted(),
            is_space,
            space_children,
        });
    }

    Ok(rooms)
}

async fn summarize_space_children(
    room: &Room,
    joined_room_ids: &BTreeSet<String>,
) -> Result<Vec<SpaceChildSummary>, MatrixCoreError> {
    let raw_events = room
        .get_state_events_static::<SpaceChildEventContent>()
        .await
        .map_err(|err| MatrixCoreError::new(format!("failed to read space child state: {err}")))?;

    let mut child_events = Vec::new();
    for raw_event in raw_events {
        let event = raw_event.deserialize().map_err(|err| {
            MatrixCoreError::new(format!("failed to deserialize space child state: {err}"))
        })?;
        let SyncOrStrippedState::Sync(SyncStateEvent::Original(event)) = event else {
            continue;
        };
        if joined_room_ids.contains(event.state_key.as_str()) {
            child_events.push(event);
        }
    }

    child_events.sort_by(SpaceChildOrd::cmp_space_child);

    Ok(child_events
        .into_iter()
        .map(|event| SpaceChildSummary {
            room_id: event.state_key.to_string(),
            suggested: event.content.suggested,
            order: event.content.order.map(|order| order.to_string()),
        })
        .collect())
}

pub(crate) fn register_event_handlers(client: &Client, events: EventSender) {
    let timeline_events = events.clone();
    client.add_event_handler(move |event: OriginalSyncRoomMessageEvent, room: Room| {
        let events = timeline_events.clone();
        async move {
            if let Some(activity) = room_activity_from_sync_message(room.room_id().as_str(), &event)
            {
                emit_event(&events, Event::RoomActivity(activity));
            }
        }
    });

    let topic_events = events.clone();
    client.add_event_handler(move |event: SyncRoomTopicEvent, room: Room| {
        let events = topic_events.clone();
        async move {
            if let SyncRoomTopicEvent::Original(event) = event {
                emit_event(
                    &events,
                    Event::RoomActivity(room_activity_from_sync_topic(
                        room.room_id().as_str(),
                        &event,
                    )),
                );
            }
        }
    });

    let member_events = events.clone();
    client.add_event_handler(move |event: SyncRoomMemberEvent, room: Room| {
        let events = member_events.clone();
        async move {
            if let SyncRoomMemberEvent::Original(event) = event {
                emit_event(
                    &events,
                    Event::RoomActivity(room_activity_from_sync_member(
                        room.room_id().as_str(),
                        &event,
                    )),
                );
            }
        }
    });

    let typing_events = events.clone();
    client.add_event_handler(move |event: SyncTypingEvent, room: Room| {
        let events = typing_events.clone();
        async move {
            emit_event(
                &events,
                Event::RoomTyping(room_typing_from_sync_typing(room.room_id().as_str(), &event)),
            );
        }
    });
}

fn room_activity_from_sync_message(
    room_id: &str,
    event: &OriginalSyncRoomMessageEvent,
) -> Option<RoomActivity> {
    let MessageType::Text(text) = &event.content.msgtype else {
        return None;
    };

    Some(RoomActivity {
        room_id: room_id.to_owned(),
        activity_id: format!("event:{}", event.event_id),
        timestamp_ms: event.origin_server_ts.get().into(),
        kind: RoomActivityKind::Message(RoomMessageActivity {
            event_id: Some(event.event_id.to_string()),
            transaction_id: event
                .unsigned
                .transaction_id
                .as_ref()
                .map(ToString::to_string),
            sender: event.sender.to_string(),
            body: text.body.to_string(),
            send_state: RoomMessageSendState::Sent,
        }),
    })
}

fn room_activity_from_sync_topic(
    room_id: &str,
    event: &OriginalSyncRoomTopicEvent,
) -> RoomActivity {
    RoomActivity {
        room_id: room_id.to_owned(),
        activity_id: format!("state:{}", event.event_id),
        timestamp_ms: event.origin_server_ts.get().into(),
        kind: RoomActivityKind::StateNotice(RoomStateNoticeActivity {
            event_id: event.event_id.to_string(),
            sender: event.sender.to_string(),
            state_key: String::new(),
            content: RoomStateNoticeContent::Topic {
                topic: event.content.topic.to_string(),
            },
        }),
    }
}

fn room_activity_from_sync_member(
    room_id: &str,
    event: &OriginalSyncRoomMemberEvent,
) -> RoomActivity {
    RoomActivity {
        room_id: room_id.to_owned(),
        activity_id: format!("state:{}", event.event_id),
        timestamp_ms: event.origin_server_ts.get().into(),
        kind: RoomActivityKind::StateNotice(RoomStateNoticeActivity {
            event_id: event.event_id.to_string(),
            sender: event.sender.to_string(),
            state_key: event.state_key.to_string(),
            content: RoomStateNoticeContent::Member {
                membership: event.content.membership.to_string(),
                displayname: event.content.displayname.clone(),
            },
        }),
    }
}

fn room_typing_from_sync_typing(room_id: &str, event: &SyncTypingEvent) -> RoomTyping {
    RoomTyping {
        room_id: room_id.to_owned(),
        user_ids: event
            .content
            .user_ids
            .iter()
            .map(ToString::to_string)
            .collect(),
    }
}

pub(crate) fn start_sync_task(client: Client, events: EventSender) -> JoinHandle<()> {
    tokio::task::spawn_local(async move {
        if let Err(err) = client
            .sync_with_callback(SyncSettings::default(), |_| async { LoopCtrl::Continue })
            .await
        {
            emit_event(
                &events,
                Event::Error(crate::api::CoreErrorEvent {
                    operation: Operation::StartSync,
                    message: format!("continuous Matrix sync failed: {err}"),
                }),
            );
        }
    })
}

pub(crate) fn start_session_update_task(client: Client, events: EventSender) -> JoinHandle<()> {
    let mut session_changes = client.subscribe_to_session_changes();

    tokio::task::spawn_local(async move {
        loop {
            match session_changes.recv().await {
                Ok(SessionChange::TokensRefreshed) => {
                    let session = client
                        .matrix_auth()
                        .session()
                        .expect("refreshed tokens require an active Matrix session");
                    emit_event(
                        &events,
                        Event::SessionUpdated {
                            session: stored_session_from_matrix_session(session),
                        },
                    );
                }
                Ok(SessionChange::UnknownToken { .. }) => {}
                Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => emit_event(
                    &events,
                    Event::Error(CoreErrorEvent {
                        operation: Operation::SessionUpdate,
                        message: "missed Matrix session change notification".to_owned(),
                    }),
                ),
            }
        }
    })
}

pub(crate) fn start_send_queue_update_task(
    client: Client,
    current_user_id: String,
    events: EventSender,
) -> JoinHandle<()> {
    let mut updates = client.send_queue().subscribe();

    tokio::task::spawn_local(async move {
        match hydrate_send_queue_local_echoes(&client, &current_user_id).await {
            Ok(initial_events) => {
                for event in initial_events {
                    emit_event(&events, event);
                }
            }
            Err(err) => emit_event(
                &events,
                Event::Error(CoreErrorEvent {
                    operation: Operation::SendText,
                    message: format!("failed to hydrate Matrix send queue local echoes: {err}"),
                }),
            ),
        }

        loop {
            match updates.recv().await {
                Ok(update) => match map_send_queue_update(update, &current_user_id) {
                    Ok(mapped_events) => {
                        for event in mapped_events {
                            emit_event(&events, event);
                        }
                    }
                    Err(err) => emit_event(
                        &events,
                        Event::Error(CoreErrorEvent {
                            operation: Operation::SendText,
                            message: format!("failed to map Matrix send queue update: {err}"),
                        }),
                    ),
                },
                Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => emit_event(
                    &events,
                    Event::Error(CoreErrorEvent {
                        operation: Operation::SendText,
                        message: "missed Matrix send queue update".to_owned(),
                    }),
                ),
            }
        }
    })
}

fn map_send_queue_update(
    update: SendQueueUpdate,
    current_user_id: &str,
) -> Result<Vec<Event>, MatrixCoreError> {
    let room_id = update.room_id.to_string();

    match update.update {
        RoomSendQueueUpdate::NewLocalEvent(local_echo) => {
            map_local_echo(room_id, local_echo, current_user_id)
        }
        RoomSendQueueUpdate::SentEvent {
            transaction_id,
            event_id,
        } => Ok(vec![Event::RoomActivityUpdate(RoomActivityUpdate {
            room_id,
            transaction_id: transaction_id.to_string(),
            kind: RoomActivityUpdateKind::MessageSent {
                event_id: event_id.to_string(),
            },
        })]),
        RoomSendQueueUpdate::SendError {
            transaction_id,
            error,
            is_recoverable,
        } => {
            let message = error.to_string();
            Ok(vec![
                Event::RoomActivityUpdate(RoomActivityUpdate {
                    room_id,
                    transaction_id: transaction_id.to_string(),
                    kind: RoomActivityUpdateKind::MessageFailed {
                        message: message.clone(),
                        recoverable: is_recoverable,
                    },
                }),
                Event::Error(CoreErrorEvent {
                    operation: Operation::SendText,
                    message,
                }),
            ])
        }
        RoomSendQueueUpdate::CancelledLocalEvent { .. }
        | RoomSendQueueUpdate::ReplacedLocalEvent { .. }
        | RoomSendQueueUpdate::RetryEvent { .. }
        | RoomSendQueueUpdate::MediaUpload { .. } => Ok(Vec::new()),
    }
}

async fn hydrate_send_queue_local_echoes(
    client: &Client,
    current_user_id: &str,
) -> Result<Vec<Event>, MatrixCoreError> {
    let local_echoes = client.send_queue().local_echoes().await.map_err(|err| {
        MatrixCoreError::new(format!("failed to load Matrix local echoes: {err}"))
    })?;

    let mut events = Vec::new();
    for (room_id, room_local_echoes) in local_echoes {
        for local_echo in room_local_echoes {
            events.extend(map_local_echo(
                room_id.to_string(),
                local_echo,
                current_user_id,
            )?);
        }
    }

    Ok(events)
}

fn map_local_echo(
    room_id: String,
    local_echo: LocalEcho,
    current_user_id: &str,
) -> Result<Vec<Event>, MatrixCoreError> {
    let transaction_id = local_echo.transaction_id.to_string();
    let LocalEchoContent::Event {
        serialized_event,
        send_handle,
        send_error,
    } = local_echo.content
    else {
        return Ok(Vec::new());
    };

    outgoing_events_from_local_echo_event(
        room_id,
        transaction_id,
        current_user_id,
        send_handle.created_at,
        &serialized_event,
        send_error.as_ref().map(ToString::to_string),
    )
}

fn outgoing_events_from_local_echo_event(
    room_id: String,
    transaction_id: String,
    current_user_id: &str,
    created_at: MilliSecondsSinceUnixEpoch,
    serialized_event: &SerializableEventContent,
    send_error: Option<String>,
) -> Result<Vec<Event>, MatrixCoreError> {
    let Some(activity) = map_local_message_activity(
        room_id.clone(),
        transaction_id.clone(),
        current_user_id,
        created_at,
        serialized_event,
    )?
    else {
        return Ok(Vec::new());
    };

    let mut events = vec![Event::RoomActivity(activity)];
    if let Some(send_error) = send_error {
        events.push(Event::RoomActivityUpdate(RoomActivityUpdate {
            room_id,
            transaction_id,
            kind: RoomActivityUpdateKind::MessageFailed {
                message: send_error.clone(),
                recoverable: false,
            },
        }));
        events.push(Event::Error(CoreErrorEvent {
            operation: Operation::SendText,
            message: send_error,
        }));
    }

    Ok(events)
}

fn map_local_message_activity(
    room_id: String,
    transaction_id: String,
    current_user_id: &str,
    created_at: MilliSecondsSinceUnixEpoch,
    serialized_event: &SerializableEventContent,
) -> Result<Option<RoomActivity>, MatrixCoreError> {
    let Some(body) = text_body_from_serialized_event(serialized_event)? else {
        return Ok(None);
    };

    Ok(Some(RoomActivity {
        room_id,
        activity_id: format!("txn:{transaction_id}"),
        timestamp_ms: created_at.get().into(),
        kind: RoomActivityKind::Message(RoomMessageActivity {
            event_id: None,
            transaction_id: Some(transaction_id),
            sender: current_user_id.to_owned(),
            body,
            send_state: RoomMessageSendState::Pending,
        }),
    }))
}

fn text_body_from_serialized_event(
    serialized_event: &SerializableEventContent,
) -> Result<Option<String>, MatrixCoreError> {
    let AnyMessageLikeEventContent::RoomMessage(content) = serialized_event
        .deserialize()
        .map_err(|err| MatrixCoreError::new(format!("failed to deserialize local echo: {err}")))?
    else {
        return Ok(None);
    };
    let MessageType::Text(text) = content.msgtype else {
        return Ok(None);
    };

    Ok(Some(text.body))
}

pub(crate) async fn send_text(
    client: &Client,
    request: SendTextRequest,
) -> Result<(), MatrixCoreError> {
    let room = client
        .joined_rooms()
        .into_iter()
        .find(|room| room.room_id().as_str() == request.room_id)
        .ok_or_else(|| {
            MatrixCoreError::new(format!("joined room not found: {}", request.room_id))
        })?;

    let content = RoomMessageEventContent::text_plain(request.body);

    room.send_queue()
        .send(AnyMessageLikeEventContent::RoomMessage(content))
        .await
        .map_err(|err| MatrixCoreError::new(format!("failed to send Matrix text: {err}")))?;

    Ok(())
}

pub(crate) async fn logout(client: Client) -> Result<(), MatrixCoreError> {
    client
        .logout()
        .await
        .map_err(|err| MatrixCoreError::new(format!("failed to log out of Matrix: {err}")))
}

pub(crate) fn stored_session_from_matrix_session(session: MatrixSession) -> StoredSession {
    StoredSession {
        user_id: session.meta.user_id.to_string(),
        device_id: session.meta.device_id.to_string(),
        access_token: session.tokens.access_token,
        refresh_token: session.tokens.refresh_token,
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::{
        map_local_message_activity, map_send_queue_update, outgoing_events_from_local_echo_event,
        room_activity_from_sync_member, room_activity_from_sync_message,
        room_activity_from_sync_topic, room_typing_from_sync_typing,
        text_body_from_serialized_event,
    };
    use crate::api::{
        CoreErrorEvent, Event, Operation, RoomActivity, RoomActivityKind, RoomActivityUpdate,
        RoomActivityUpdateKind, RoomMessageActivity, RoomMessageSendState, RoomStateNoticeActivity,
        RoomStateNoticeContent, RoomTyping,
    };
    use matrix_sdk::{
        ruma::{
            MilliSecondsSinceUnixEpoch, OwnedEventId, OwnedRoomId, OwnedTransactionId,
            events::{
                AnyMessageLikeEventContent,
                room::{
                    member::OriginalSyncRoomMemberEvent,
                    message::{OriginalSyncRoomMessageEvent, RoomMessageEventContent},
                    topic::OriginalSyncRoomTopicEvent,
                },
                typing::SyncTypingEvent,
            },
        },
        send_queue::{RoomSendQueueUpdate, SendQueueUpdate},
        store::SerializableEventContent,
    };

    fn room_id() -> OwnedRoomId {
        OwnedRoomId::try_from("!room:example.org").expect("valid room id")
    }

    fn transaction_id() -> OwnedTransactionId {
        "txn-1".into()
    }

    fn event_id() -> OwnedEventId {
        OwnedEventId::try_from("$event:example.org").expect("valid event id")
    }

    #[test]
    fn room_activity_from_sync_message_copies_event_identity() {
        let event: OriginalSyncRoomMessageEvent = serde_json::from_value(serde_json::json!({
            "type": "m.room.message",
            "content": {
                "msgtype": "m.text",
                "body": "hello"
            },
            "event_id": "$event:example.org",
            "sender": "@alice:example.org",
            "origin_server_ts": 1700000000000u64,
            "unsigned": {
                "transaction_id": "txn-1"
            }
        }))
        .expect("deserialize sync message event");

        assert_eq!(
            room_activity_from_sync_message("!room:example.org", &event),
            Some(RoomActivity {
                room_id: "!room:example.org".to_owned(),
                activity_id: "event:$event:example.org".to_owned(),
                timestamp_ms: 1_700_000_000_000,
                kind: RoomActivityKind::Message(RoomMessageActivity {
                    event_id: Some("$event:example.org".to_owned()),
                    transaction_id: Some("txn-1".to_owned()),
                    sender: "@alice:example.org".to_owned(),
                    body: "hello".to_owned(),
                    send_state: RoomMessageSendState::Sent,
                }),
            })
        );
    }

    #[test]
    fn non_text_sync_message_mapping_is_ignored() {
        let event: OriginalSyncRoomMessageEvent = serde_json::from_value(serde_json::json!({
            "type": "m.room.message",
            "content": {
                "msgtype": "m.notice",
                "body": "heads up"
            },
            "event_id": "$event:example.org",
            "sender": "@alice:example.org",
            "origin_server_ts": 1700000000000u64
        }))
        .expect("deserialize sync notice event");

        assert_eq!(
            room_activity_from_sync_message("!room:example.org", &event),
            None
        );
    }

    #[test]
    fn topic_state_mapping_produces_state_notice_activity() {
        let event: OriginalSyncRoomTopicEvent = serde_json::from_value(serde_json::json!({
            "type": "m.room.topic",
            "content": {
                "topic": "incident review"
            },
            "event_id": "$topic:example.org",
            "sender": "@alice:example.org",
            "origin_server_ts": 1700000000001u64,
            "state_key": ""
        }))
        .expect("deserialize topic event");

        assert_eq!(
            room_activity_from_sync_topic("!room:example.org", &event),
            RoomActivity {
                room_id: "!room:example.org".to_owned(),
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
            }
        );
    }

    #[test]
    fn member_state_mapping_produces_state_notice_activity() {
        let event: OriginalSyncRoomMemberEvent = serde_json::from_value(serde_json::json!({
            "type": "m.room.member",
            "content": {
                "membership": "join",
                "displayname": "carol"
            },
            "event_id": "$member:example.org",
            "sender": "@alice:example.org",
            "origin_server_ts": 1700000000002u64,
            "state_key": "@carol:example.org"
        }))
        .expect("deserialize member event");

        assert_eq!(
            room_activity_from_sync_member("!room:example.org", &event),
            RoomActivity {
                room_id: "!room:example.org".to_owned(),
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
            }
        );
    }

    #[test]
    fn typing_event_mapping_replaces_user_list_for_room() {
        let event: SyncTypingEvent = serde_json::from_value(serde_json::json!({
            "type": "m.typing",
            "content": {
                "user_ids": ["@alice:example.org", "@bob:example.org"]
            }
        }))
        .expect("deserialize typing event");

        assert_eq!(
            room_typing_from_sync_typing("!room:example.org", &event),
            RoomTyping {
                room_id: "!room:example.org".to_owned(),
                user_ids: vec![
                    "@alice:example.org".to_owned(),
                    "@bob:example.org".to_owned(),
                ],
            }
        );
    }

    #[test]
    fn local_text_queue_mapping_extracts_public_metadata() {
        let created_at = MilliSecondsSinceUnixEpoch::now();
        let serialized_event = SerializableEventContent::new(
            &AnyMessageLikeEventContent::RoomMessage(RoomMessageEventContent::text_plain("hello")),
        )
        .expect("serialize text event");

        let activity = map_local_message_activity(
            "!room:example.org".to_owned(),
            "txn-1".to_owned(),
            "@alice:example.org",
            created_at,
            &serialized_event,
        )
        .expect("map local text")
        .expect("text local echo");

        assert_eq!(
            activity,
            RoomActivity {
                room_id: "!room:example.org".to_owned(),
                activity_id: "txn:txn-1".to_owned(),
                timestamp_ms: created_at.get().into(),
                kind: RoomActivityKind::Message(RoomMessageActivity {
                    event_id: None,
                    transaction_id: Some("txn-1".to_owned()),
                    sender: "@alice:example.org".to_owned(),
                    body: "hello".to_owned(),
                    send_state: RoomMessageSendState::Pending,
                }),
            }
        );
    }

    #[test]
    fn text_body_extraction_ignores_non_text_local_echoes() {
        let serialized_event =
            SerializableEventContent::new(&AnyMessageLikeEventContent::RoomMessage(
                RoomMessageEventContent::notice_plain("heads up"),
            ))
            .expect("serialize notice event");

        assert_eq!(
            text_body_from_serialized_event(&serialized_event).expect("read local echo body"),
            None
        );
    }

    #[test]
    fn send_queue_sent_update_maps_to_room_activity_update() {
        let events = map_send_queue_update(
            SendQueueUpdate {
                room_id: room_id(),
                update: RoomSendQueueUpdate::SentEvent {
                    transaction_id: transaction_id(),
                    event_id: event_id(),
                },
            },
            "@alice:example.org",
        )
        .expect("map sent update");

        assert_eq!(
            events,
            vec![Event::RoomActivityUpdate(RoomActivityUpdate {
                room_id: "!room:example.org".to_owned(),
                transaction_id: "txn-1".to_owned(),
                kind: RoomActivityUpdateKind::MessageSent {
                    event_id: "$event:example.org".to_owned(),
                },
            })]
        );
    }

    #[test]
    fn restored_local_echo_hydrates_to_pending_room_activity() {
        let created_at = MilliSecondsSinceUnixEpoch::now();
        let serialized_event = SerializableEventContent::new(
            &AnyMessageLikeEventContent::RoomMessage(RoomMessageEventContent::text_plain("hello")),
        )
        .expect("serialize text event");

        let events = outgoing_events_from_local_echo_event(
            "!room:example.org".to_owned(),
            "txn-1".to_owned(),
            "@alice:example.org",
            created_at,
            &serialized_event,
            None,
        )
        .expect("map restored local echo");

        assert_eq!(
            events,
            vec![Event::RoomActivity(RoomActivity {
                room_id: "!room:example.org".to_owned(),
                activity_id: "txn:txn-1".to_owned(),
                timestamp_ms: created_at.get().into(),
                kind: RoomActivityKind::Message(RoomMessageActivity {
                    event_id: None,
                    transaction_id: Some("txn-1".to_owned()),
                    sender: "@alice:example.org".to_owned(),
                    body: "hello".to_owned(),
                    send_state: RoomMessageSendState::Pending,
                }),
            })]
        );
    }

    #[test]
    fn restored_failed_local_echo_re_emits_error_surface() {
        let created_at = MilliSecondsSinceUnixEpoch::now();
        let serialized_event = SerializableEventContent::new(
            &AnyMessageLikeEventContent::RoomMessage(RoomMessageEventContent::text_plain("hello")),
        )
        .expect("serialize text event");

        let events = outgoing_events_from_local_echo_event(
            "!room:example.org".to_owned(),
            "txn-1".to_owned(),
            "@alice:example.org",
            created_at,
            &serialized_event,
            Some("Other unrecoverable error: wedged".to_owned()),
        )
        .expect("map restored failed local echo");

        assert_eq!(
            events,
            vec![
                Event::RoomActivity(RoomActivity {
                    room_id: "!room:example.org".to_owned(),
                    activity_id: "txn:txn-1".to_owned(),
                    timestamp_ms: created_at.get().into(),
                    kind: RoomActivityKind::Message(RoomMessageActivity {
                        event_id: None,
                        transaction_id: Some("txn-1".to_owned()),
                        sender: "@alice:example.org".to_owned(),
                        body: "hello".to_owned(),
                        send_state: RoomMessageSendState::Pending,
                    }),
                }),
                Event::RoomActivityUpdate(RoomActivityUpdate {
                    room_id: "!room:example.org".to_owned(),
                    transaction_id: "txn-1".to_owned(),
                    kind: RoomActivityUpdateKind::MessageFailed {
                        message: "Other unrecoverable error: wedged".to_owned(),
                        recoverable: false,
                    },
                }),
                Event::Error(CoreErrorEvent {
                    operation: Operation::SendText,
                    message: "Other unrecoverable error: wedged".to_owned(),
                }),
            ]
        );
    }

    #[test]
    fn send_queue_error_maps_to_failed_activity_update_and_error_surface() {
        let events = map_send_queue_update(
            SendQueueUpdate {
                room_id: room_id(),
                update: RoomSendQueueUpdate::SendError {
                    transaction_id: transaction_id(),
                    error: Arc::new(matrix_sdk::Error::from(std::io::Error::other(
                        "send failed",
                    ))),
                    is_recoverable: true,
                },
            },
            "@alice:example.org",
        )
        .expect("map send error");

        assert_eq!(
            events,
            vec![
                Event::RoomActivityUpdate(RoomActivityUpdate {
                    room_id: "!room:example.org".to_owned(),
                    transaction_id: "txn-1".to_owned(),
                    kind: RoomActivityUpdateKind::MessageFailed {
                        message: "send failed".to_owned(),
                        recoverable: true,
                    },
                }),
                Event::Error(CoreErrorEvent {
                    operation: Operation::SendText,
                    message: "send failed".to_owned(),
                }),
            ]
        );
    }
}
