use std::collections::BTreeSet;
use std::error::Error;
use std::fmt::{self, Display, Formatter};

use futures_util::StreamExt;
use matrix_sdk::{
    Client, LoopCtrl, Room, SessionChange, SessionMeta, SessionTokens,
    authentication::matrix::MatrixSession,
    config::SyncSettings,
    deserialized_responses::SyncOrStrippedState,
    encryption::{
        BackupDownloadStrategy, EncryptionSettings,
        verification::{
            CancelInfo, Emoji as MatrixEmoji, EmojiShortAuthString, SasState,
            SasVerification as MatrixSasVerification,
            VerificationRequest as MatrixVerificationRequest, VerificationRequestState,
        },
    },
    room::Receipts,
    ruma::{
        EventId, MilliSecondsSinceUnixEpoch, OwnedDeviceId, OwnedUserId, RoomId,
        events::{
            AnyMessageLikeEventContent, SyncStateEvent, ToDeviceEvent,
            key::verification::{
                VerificationMethod, request::ToDeviceKeyVerificationRequestEventContent,
            },
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
use matrix_sdk_base::{RoomInfoNotableUpdateReasons, read_receipts::RoomReadReceipts};
use tokio::task::JoinHandle;

use crate::api::{
    CoreConfig, CoreErrorEvent, Event, LoginRequest, MarkRoomReadRequest, Operation, RoomActivity,
    RoomActivityKind, RoomActivityUpdate, RoomActivityUpdateKind, RoomMessageActivity,
    RoomMessagePresentationKind, RoomMessageSendState, RoomReadState, RoomStateNoticeActivity,
    RoomStateNoticeContent, RoomSummary, RoomTyping, SendMessageRequest, SpaceChildSummary,
    StoredSession, VerificationDecimals, VerificationEmoji, VerificationFlowId,
    VerificationRequest, VerificationState, VerificationStateEvent,
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

pub(crate) fn room_read_update_is_relevant(reasons: RoomInfoNotableUpdateReasons) -> bool {
    reasons.intersects(
        RoomInfoNotableUpdateReasons::READ_RECEIPT
            | RoomInfoNotableUpdateReasons::FULLY_READ
            | RoomInfoNotableUpdateReasons::UNREAD_MARKER,
    )
}

pub(crate) fn room_read_state_from_room(room: &Room) -> RoomReadState {
    let receipts = room.read_receipts();
    let fully_read_event_id = room.fully_read_event_id();
    room_read_state_from_parts(
        room.room_id().as_str(),
        &receipts,
        fully_read_event_id.as_deref(),
        room.is_marked_unread(),
    )
}

pub(crate) fn room_read_state_from_parts(
    room_id: &str,
    receipts: &RoomReadReceipts,
    fully_read_event_id: Option<&EventId>,
    marked_unread: bool,
) -> RoomReadState {
    RoomReadState {
        room_id: room_id.to_owned(),
        latest_active_event_id: receipts
            .latest_active
            .as_ref()
            .map(|receipt| receipt.event_id.to_string()),
        fully_read_event_id: fully_read_event_id.map(ToString::to_string),
        marked_unread,
        num_unread: receipts.num_unread,
        num_notifications: receipts.num_notifications,
        num_mentions: receipts.num_mentions,
    }
}

pub(crate) fn summarize_joined_room_read_states(client: &Client) -> Vec<RoomReadState> {
    client
        .joined_rooms()
        .into_iter()
        .map(|room| room_read_state_from_room(&room))
        .collect()
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
                Event::RoomTyping(room_typing_from_sync_typing(
                    room.room_id().as_str(),
                    &event,
                )),
            );
        }
    });

    let verification_events = events.clone();
    client.add_event_handler(
        move |event: ToDeviceEvent<ToDeviceKeyVerificationRequestEventContent>| {
            let events = verification_events.clone();
            async move {
                let flow = verification_flow_id_from_to_device(&event);
                emit_event(
                    &events,
                    Event::VerificationRequest(verification_request_from_to_device(&event)),
                );
                emit_event(
                    &events,
                    Event::VerificationState(verification_requested_state_event(flow.clone())),
                );
            }
        },
    );
}

fn room_activity_from_sync_message(
    room_id: &str,
    event: &OriginalSyncRoomMessageEvent,
) -> Option<RoomActivity> {
    let (presentation, body) =
        room_message_presentation_and_body_from_message_type(&event.content.msgtype)?;

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
            presentation,
            body,
            send_state: RoomMessageSendState::Sent,
        }),
    })
}

fn room_message_presentation_and_body_from_message_type(
    message_type: &MessageType,
) -> Option<(RoomMessagePresentationKind, String)> {
    match message_type {
        MessageType::Text(text) => Some((RoomMessagePresentationKind::Text, text.body.clone())),
        MessageType::Notice(notice) => {
            Some((RoomMessagePresentationKind::Notice, notice.body.clone()))
        }
        MessageType::Emote(emote) => Some((RoomMessagePresentationKind::Emote, emote.body.clone())),
        _ => None,
    }
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

fn verification_request_from_to_device(
    event: &ToDeviceEvent<ToDeviceKeyVerificationRequestEventContent>,
) -> VerificationRequest {
    VerificationRequest {
        flow_id: event.content.transaction_id.to_string(),
        user_id: event.sender.to_string(),
    }
}

fn verification_flow_id_from_to_device(
    event: &ToDeviceEvent<ToDeviceKeyVerificationRequestEventContent>,
) -> VerificationFlowId {
    VerificationFlowId {
        flow_id: event.content.transaction_id.to_string(),
        user_id: event.sender.to_string(),
    }
}

fn verification_requested_state_event(flow: VerificationFlowId) -> VerificationStateEvent {
    verification_state_event(&flow, VerificationState::Requested)
}

fn verification_state_event(
    flow: &VerificationFlowId,
    state: VerificationState,
) -> VerificationStateEvent {
    VerificationStateEvent {
        flow_id: flow.flow_id.clone(),
        user_id: flow.user_id.clone(),
        state,
    }
}

pub(crate) fn parse_verification_user_id(user_id: &str) -> Result<OwnedUserId, MatrixCoreError> {
    OwnedUserId::try_from(user_id.to_owned())
        .map_err(|err| MatrixCoreError::new(format!("invalid verification user id: {err}")))
}

pub(crate) async fn lookup_verification_request(
    client: &Client,
    flow: &VerificationFlowId,
) -> Result<MatrixVerificationRequest, MatrixCoreError> {
    let user_id = parse_verification_user_id(&flow.user_id)?;
    client
        .encryption()
        .get_verification_request(&user_id, &flow.flow_id)
        .await
        .ok_or_else(|| {
            MatrixCoreError::new(format!(
                "verification request not found for user {} and flow {}",
                flow.user_id, flow.flow_id
            ))
        })
}

pub(crate) async fn lookup_sas_verification(
    client: &Client,
    flow: &VerificationFlowId,
) -> Result<MatrixSasVerification, MatrixCoreError> {
    let user_id = parse_verification_user_id(&flow.user_id)?;
    client
        .encryption()
        .get_verification(&user_id, &flow.flow_id)
        .await
        .and_then(|verification| verification.sas())
        .ok_or_else(|| {
            MatrixCoreError::new(format!(
                "SAS verification not found for user {} and flow {}",
                flow.user_id, flow.flow_id
            ))
        })
}

pub(crate) async fn accept_verification(
    client: &Client,
    flow: &VerificationFlowId,
) -> Result<VerificationStateEvent, MatrixCoreError> {
    let request = lookup_verification_request(client, flow).await?;
    request
        .accept_with_methods(vec![VerificationMethod::SasV1])
        .await
        .map_err(|err| MatrixCoreError::new(format!("failed to accept verification: {err}")))?;

    match request.state() {
        VerificationRequestState::Ready { .. } => {
            request.start_sas().await.map_err(|err| {
                MatrixCoreError::new(format!("failed to start SAS verification: {err}"))
            })?;
            Ok(verification_state_event(flow, VerificationState::Ready))
        }
        VerificationRequestState::Done => {
            Ok(verification_state_event(flow, VerificationState::Done))
        }
        VerificationRequestState::Cancelled(cancel_info) => {
            Ok(verification_state_event(flow, map_cancel_info(cancel_info)))
        }
        VerificationRequestState::Created { .. }
        | VerificationRequestState::Requested { .. }
        | VerificationRequestState::Transitioned { .. } => {
            Ok(verification_state_event(flow, VerificationState::Accepted))
        }
    }
}

pub(crate) async fn cancel_verification(
    client: &Client,
    flow: &VerificationFlowId,
) -> Result<VerificationStateEvent, MatrixCoreError> {
    if let Ok(sas) = lookup_sas_verification(client, flow).await {
        sas.cancel().await.map_err(|err| {
            MatrixCoreError::new(format!("failed to cancel SAS verification: {err}"))
        })?;
        return cancelled_sas_state_event(flow, &sas);
    }

    let request = lookup_verification_request(client, flow).await?;
    request.cancel().await.map_err(|err| {
        MatrixCoreError::new(format!("failed to cancel verification request: {err}"))
    })?;
    cancelled_request_state_event(flow, &request)
}

pub(crate) async fn confirm_verification_sas(
    client: &Client,
    flow: &VerificationFlowId,
) -> Result<VerificationStateEvent, MatrixCoreError> {
    let sas = lookup_sas_verification(client, flow).await?;
    sas.confirm().await.map_err(|err| {
        MatrixCoreError::new(format!("failed to confirm SAS verification: {err}"))
    })?;

    match sas.state() {
        SasState::Done { .. } => Ok(verification_state_event(flow, VerificationState::Done)),
        SasState::Cancelled(cancel_info) => {
            Ok(verification_state_event(flow, map_cancel_info(cancel_info)))
        }
        _ => Ok(verification_state_event(flow, VerificationState::Confirmed)),
    }
}

pub(crate) async fn reject_verification_sas(
    client: &Client,
    flow: &VerificationFlowId,
) -> Result<VerificationStateEvent, MatrixCoreError> {
    let sas = lookup_sas_verification(client, flow).await?;
    sas.mismatch()
        .await
        .map_err(|err| MatrixCoreError::new(format!("failed to reject SAS verification: {err}")))?;
    cancelled_sas_state_event(flow, &sas)
}

pub(crate) async fn watch_verification_request(
    request: MatrixVerificationRequest,
    flow: VerificationFlowId,
    events: EventSender,
) {
    if let Some(event) = map_request_state(&flow, request.state())
        && event.state != VerificationState::Requested
    {
        emit_event(&events, Event::VerificationState(event));
    }

    if hand_off_request_state(&request, &flow, &events, request.state()).await {
        return;
    }

    let mut changes = request.changes();
    while let Some(state) = changes.next().await {
        if let Some(event) = map_request_state(&flow, state.clone()) {
            emit_event(&events, Event::VerificationState(event));
        }

        if hand_off_request_state(&request, &flow, &events, state).await {
            return;
        }
    }
}

pub(crate) async fn watch_sas_verification(
    sas: MatrixSasVerification,
    flow: VerificationFlowId,
    events: EventSender,
) {
    if let Some(event) = map_sas_verification(&flow, &sas) {
        emit_event(&events, Event::VerificationState(event.clone()));
        if matches!(
            event.state,
            VerificationState::Done | VerificationState::Cancelled { .. }
        ) {
            return;
        }
    }

    let mut changes = sas.changes();
    while let Some(state) = changes.next().await {
        if let Some(event) = map_sas_state(&flow, state) {
            let terminal = matches!(
                event.state,
                VerificationState::Done | VerificationState::Cancelled { .. }
            );
            emit_event(&events, Event::VerificationState(event));
            if terminal {
                return;
            }
        }
    }
}

async fn hand_off_request_state(
    request: &MatrixVerificationRequest,
    flow: &VerificationFlowId,
    events: &EventSender,
    state: VerificationRequestState,
) -> bool {
    match state {
        VerificationRequestState::Ready { .. } => {
            match request.start_sas().await {
                Ok(Some(sas)) => {
                    if let Some(event) = map_sas_verification(flow, &sas) {
                        emit_event(events, Event::VerificationState(event.clone()));
                        if matches!(
                            event.state,
                            VerificationState::Done | VerificationState::Cancelled { .. }
                        ) {
                            return true;
                        }
                    }
                    watch_sas_verification(sas, flow.clone(), events.clone()).await;
                }
                Ok(None) => {}
                Err(err) => {
                    emit_event(
                        events,
                        Event::Error(CoreErrorEvent {
                            operation: Operation::Verification,
                            message: format!("failed to start SAS verification: {err}"),
                        }),
                    );
                }
            }
            true
        }
        VerificationRequestState::Transitioned { verification } => {
            if let Some(sas) = verification.sas() {
                if let Some(event) = map_sas_verification(flow, &sas) {
                    emit_event(events, Event::VerificationState(event.clone()));
                    if matches!(
                        event.state,
                        VerificationState::Done | VerificationState::Cancelled { .. }
                    ) {
                        return true;
                    }
                }
                watch_sas_verification(sas, flow.clone(), events.clone()).await;
                return true;
            }
            false
        }
        VerificationRequestState::Done | VerificationRequestState::Cancelled(_) => true,
        VerificationRequestState::Created { .. } | VerificationRequestState::Requested { .. } => {
            false
        }
    }
}

fn cancelled_request_state_event(
    flow: &VerificationFlowId,
    request: &MatrixVerificationRequest,
) -> Result<VerificationStateEvent, MatrixCoreError> {
    match request.state() {
        VerificationRequestState::Cancelled(cancel_info) => {
            Ok(verification_state_event(flow, map_cancel_info(cancel_info)))
        }
        _ => request
            .cancel_info()
            .map(|cancel_info| verification_state_event(flow, map_cancel_info(cancel_info)))
            .ok_or_else(|| {
                MatrixCoreError::new(
                    "verification request missing cancelled state after cancellation",
                )
            }),
    }
}

fn cancelled_sas_state_event(
    flow: &VerificationFlowId,
    sas: &MatrixSasVerification,
) -> Result<VerificationStateEvent, MatrixCoreError> {
    match sas.state() {
        SasState::Cancelled(cancel_info) => {
            Ok(verification_state_event(flow, map_cancel_info(cancel_info)))
        }
        _ => sas
            .cancel_info()
            .map(|cancel_info| verification_state_event(flow, map_cancel_info(cancel_info)))
            .ok_or_else(|| {
                MatrixCoreError::new("SAS verification missing cancelled state after cancellation")
            }),
    }
}

fn map_request_state(
    flow: &VerificationFlowId,
    state: VerificationRequestState,
) -> Option<VerificationStateEvent> {
    match state {
        VerificationRequestState::Requested { .. } => {
            Some(verification_requested_state_event(flow.clone()))
        }
        VerificationRequestState::Ready { .. } => {
            Some(verification_state_event(flow, VerificationState::Ready))
        }
        VerificationRequestState::Done => {
            Some(verification_state_event(flow, VerificationState::Done))
        }
        VerificationRequestState::Cancelled(cancel_info) => {
            Some(verification_state_event(flow, map_cancel_info(cancel_info)))
        }
        VerificationRequestState::Created { .. }
        | VerificationRequestState::Transitioned { .. } => None,
    }
}

fn map_sas_verification(
    flow: &VerificationFlowId,
    sas: &MatrixSasVerification,
) -> Option<VerificationStateEvent> {
    if sas.can_be_presented() {
        let decimals = sas.decimals().map(map_decimals)?;
        let emojis = sas.emoji().map(map_emoji_array).unwrap_or_default();
        return Some(verification_state_event(
            flow,
            VerificationState::SasReady { emojis, decimals },
        ));
    }

    match sas.state() {
        SasState::Confirmed => Some(verification_state_event(flow, VerificationState::Confirmed)),
        SasState::Done { .. } => Some(verification_state_event(flow, VerificationState::Done)),
        SasState::Cancelled(cancel_info) => {
            Some(verification_state_event(flow, map_cancel_info(cancel_info)))
        }
        _ => None,
    }
}

fn map_sas_state(flow: &VerificationFlowId, state: SasState) -> Option<VerificationStateEvent> {
    match state {
        SasState::KeysExchanged { emojis, decimals } => Some(verification_state_event(
            flow,
            VerificationState::SasReady {
                emojis: emojis.map(map_emoji_short_auth_string).unwrap_or_default(),
                decimals: map_decimals(decimals),
            },
        )),
        SasState::Confirmed => Some(verification_state_event(flow, VerificationState::Confirmed)),
        SasState::Done { .. } => Some(verification_state_event(flow, VerificationState::Done)),
        SasState::Cancelled(cancel_info) => {
            Some(verification_state_event(flow, map_cancel_info(cancel_info)))
        }
        SasState::Created { .. } | SasState::Started { .. } | SasState::Accepted { .. } => None,
    }
}

fn map_cancel_info(cancel_info: CancelInfo) -> VerificationState {
    cancelled_state(
        cancel_info.reason(),
        cancel_info.cancel_code().as_str(),
        cancel_info.cancelled_by_us(),
    )
}

fn cancelled_state(reason: &str, code: &str, cancelled_by_us: bool) -> VerificationState {
    VerificationState::Cancelled {
        reason: reason.to_owned(),
        code: code.to_owned(),
        cancelled_by_us,
    }
}

fn map_emoji_short_auth_string(emojis: EmojiShortAuthString) -> Vec<VerificationEmoji> {
    emojis.emojis.into_iter().map(map_emoji).collect()
}

fn map_emoji_array(emojis: [MatrixEmoji; 7]) -> Vec<VerificationEmoji> {
    emojis.into_iter().map(map_emoji).collect()
}

fn map_emoji(emoji: MatrixEmoji) -> VerificationEmoji {
    VerificationEmoji {
        symbol: emoji.symbol.to_owned(),
        description: emoji.description.to_owned(),
    }
}

fn map_decimals((first, second, third): (u16, u16, u16)) -> VerificationDecimals {
    VerificationDecimals {
        first,
        second,
        third,
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

pub(crate) fn start_room_read_state_update_task(
    client: Client,
    events: EventSender,
) -> Result<JoinHandle<()>, matrix_sdk::event_cache::EventCacheError> {
    client.event_cache().subscribe()?;
    let mut updates = client.room_info_notable_update_receiver();

    Ok(tokio::task::spawn_local(async move {
        loop {
            match updates.recv().await {
                Ok(update) => {
                    if !room_read_update_is_relevant(update.reasons) {
                        continue;
                    }

                    let Some(room) = client.get_room(&update.room_id) else {
                        continue;
                    };

                    if events
                        .send(Event::RoomReadState(room_read_state_from_room(&room)))
                        .is_err()
                    {
                        break;
                    }
                }
                Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {
                    if events
                        .send(Event::Error(CoreErrorEvent {
                            operation: Operation::RoomReadState,
                            message: "missed Matrix room read-state update".to_owned(),
                        }))
                        .is_err()
                    {
                        break;
                    }
                }
            }
        }
    }))
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
                    operation: Operation::SendMessage,
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
                            operation: Operation::SendMessage,
                            message: format!("failed to map Matrix send queue update: {err}"),
                        }),
                    ),
                },
                Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => emit_event(
                    &events,
                    Event::Error(CoreErrorEvent {
                        operation: Operation::SendMessage,
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
                    operation: Operation::SendMessage,
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
            operation: Operation::SendMessage,
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
    let Some((presentation, body)) =
        message_presentation_and_body_from_serialized_event(serialized_event)?
    else {
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
            presentation,
            body,
            send_state: RoomMessageSendState::Pending,
        }),
    }))
}

fn message_presentation_and_body_from_serialized_event(
    serialized_event: &SerializableEventContent,
) -> Result<Option<(RoomMessagePresentationKind, String)>, MatrixCoreError> {
    let AnyMessageLikeEventContent::RoomMessage(content) = serialized_event
        .deserialize()
        .map_err(|err| MatrixCoreError::new(format!("failed to deserialize local echo: {err}")))?
    else {
        return Ok(None);
    };

    Ok(room_message_presentation_and_body_from_message_type(
        &content.msgtype,
    ))
}

fn room_message_event_content(
    presentation: RoomMessagePresentationKind,
    body: String,
) -> RoomMessageEventContent {
    match presentation {
        RoomMessagePresentationKind::Text => RoomMessageEventContent::text_plain(body),
        RoomMessagePresentationKind::Notice => RoomMessageEventContent::notice_plain(body),
        RoomMessagePresentationKind::Emote => RoomMessageEventContent::emote_plain(body),
    }
}

pub(crate) async fn send_message(
    client: &Client,
    request: SendMessageRequest,
) -> Result<(), MatrixCoreError> {
    let room = client
        .joined_rooms()
        .into_iter()
        .find(|room| room.room_id().as_str() == request.room_id)
        .ok_or_else(|| {
            MatrixCoreError::new(format!("joined room not found: {}", request.room_id))
        })?;

    let content = room_message_event_content(request.presentation, request.body);

    room.send_queue()
        .send(AnyMessageLikeEventContent::RoomMessage(content))
        .await
        .map_err(|err| MatrixCoreError::new(format!("failed to send Matrix message: {err}")))?;

    Ok(())
}

pub(crate) async fn mark_room_read(
    client: &Client,
    request: &MarkRoomReadRequest,
) -> Result<(), MatrixCoreError> {
    let room_id = <&RoomId>::try_from(request.room_id.as_str())
        .map_err(|err| MatrixCoreError::new(format!("invalid Matrix room id: {err}")))?;
    let event_id = <&EventId>::try_from(request.event_id.as_str())
        .map_err(|err| MatrixCoreError::new(format!("invalid Matrix event id: {err}")))?
        .to_owned();

    let room = client
        .joined_rooms()
        .into_iter()
        .find(|room| room.room_id().as_str() == room_id.as_str())
        .ok_or_else(|| MatrixCoreError::new(format!("joined room not found: {room_id}")))?;

    room.send_multiple_receipts(
        Receipts::new()
            .fully_read_marker(event_id.clone())
            .public_read_receipt(event_id),
    )
    .await
    .map_err(|err| MatrixCoreError::new(format!("failed to send Matrix read receipts: {err}")))?;

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
        cancelled_state, map_decimals, map_emoji_array, map_emoji_short_auth_string,
        map_local_message_activity, map_send_queue_update,
        message_presentation_and_body_from_serialized_event, outgoing_events_from_local_echo_event,
        room_read_state_from_parts, room_read_update_is_relevant,
        room_activity_from_sync_member, room_activity_from_sync_message,
        room_activity_from_sync_topic, room_message_event_content, room_typing_from_sync_typing,
        verification_flow_id_from_to_device, verification_request_from_to_device,
        verification_requested_state_event,
    };
    use crate::api::{
        CoreErrorEvent, Event, Operation, RoomActivity, RoomActivityKind, RoomActivityUpdate,
        RoomActivityUpdateKind, RoomMessageActivity, RoomMessagePresentationKind,
        RoomMessageSendState, RoomReadState, RoomStateNoticeActivity,
        RoomStateNoticeContent, RoomTyping, VerificationDecimals, VerificationEmoji,
        VerificationRequest, VerificationState, VerificationStateEvent,
    };
    use matrix_sdk::{
        encryption::verification::{Emoji as MatrixEmoji, EmojiShortAuthString},
        ruma::{
            MilliSecondsSinceUnixEpoch, OwnedEventId, OwnedRoomId, OwnedTransactionId,
            events::{
                AnyMessageLikeEventContent, ToDeviceEvent,
                key::verification::request::ToDeviceKeyVerificationRequestEventContent,
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
    use matrix_sdk_base::{
        RoomInfoNotableUpdateReasons,
        read_receipts::{LatestReadReceipt, RoomReadReceipts},
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

    fn latest_event_id() -> OwnedEventId {
        OwnedEventId::try_from("$latest:example.org").expect("valid latest event id")
    }

    fn fully_read_event_id() -> OwnedEventId {
        OwnedEventId::try_from("$fully-read:example.org").expect("valid fully read event id")
    }

    fn matrix_emojis() -> [MatrixEmoji; 7] {
        [
            MatrixEmoji {
                symbol: "🐶",
                description: "Dog",
            },
            MatrixEmoji {
                symbol: "🐱",
                description: "Cat",
            },
            MatrixEmoji {
                symbol: "🦁",
                description: "Lion",
            },
            MatrixEmoji {
                symbol: "🐎",
                description: "Horse",
            },
            MatrixEmoji {
                symbol: "🦄",
                description: "Unicorn",
            },
            MatrixEmoji {
                symbol: "🐷",
                description: "Pig",
            },
            MatrixEmoji {
                symbol: "🐘",
                description: "Elephant",
            },
        ]
    }

    fn expected_verification_emojis() -> Vec<VerificationEmoji> {
        vec![
            VerificationEmoji {
                symbol: "🐶".to_owned(),
                description: "Dog".to_owned(),
            },
            VerificationEmoji {
                symbol: "🐱".to_owned(),
                description: "Cat".to_owned(),
            },
            VerificationEmoji {
                symbol: "🦁".to_owned(),
                description: "Lion".to_owned(),
            },
            VerificationEmoji {
                symbol: "🐎".to_owned(),
                description: "Horse".to_owned(),
            },
            VerificationEmoji {
                symbol: "🦄".to_owned(),
                description: "Unicorn".to_owned(),
            },
            VerificationEmoji {
                symbol: "🐷".to_owned(),
                description: "Pig".to_owned(),
            },
            VerificationEmoji {
                symbol: "🐘".to_owned(),
                description: "Elephant".to_owned(),
            },
        ]
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
                    presentation: RoomMessagePresentationKind::Text,
                    body: "hello".to_owned(),
                    send_state: RoomMessageSendState::Sent,
                }),
            })
        );
    }

    #[test]
    fn notice_sync_message_mapping_preserves_activity_fields() {
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
            Some(RoomActivity {
                room_id: "!room:example.org".to_owned(),
                activity_id: "event:$event:example.org".to_owned(),
                timestamp_ms: 1_700_000_000_000,
                kind: RoomActivityKind::Message(RoomMessageActivity {
                    event_id: Some("$event:example.org".to_owned()),
                    transaction_id: None,
                    sender: "@alice:example.org".to_owned(),
                    presentation: RoomMessagePresentationKind::Notice,
                    body: "heads up".to_owned(),
                    send_state: RoomMessageSendState::Sent,
                }),
            })
        );
    }

    #[test]
    fn emote_sync_message_mapping_preserves_activity_fields() {
        let event: OriginalSyncRoomMessageEvent = serde_json::from_value(serde_json::json!({
            "type": "m.room.message",
            "content": {
                "msgtype": "m.emote",
                "body": "waves"
            },
            "event_id": "$event:example.org",
            "sender": "@alice:example.org",
            "origin_server_ts": 1700000000000u64
        }))
        .expect("deserialize sync emote event");

        assert_eq!(
            room_activity_from_sync_message("!room:example.org", &event),
            Some(RoomActivity {
                room_id: "!room:example.org".to_owned(),
                activity_id: "event:$event:example.org".to_owned(),
                timestamp_ms: 1_700_000_000_000,
                kind: RoomActivityKind::Message(RoomMessageActivity {
                    event_id: Some("$event:example.org".to_owned()),
                    transaction_id: None,
                    sender: "@alice:example.org".to_owned(),
                    presentation: RoomMessagePresentationKind::Emote,
                    body: "waves".to_owned(),
                    send_state: RoomMessageSendState::Sent,
                }),
            })
        );
    }

    #[test]
    fn unsupported_sync_message_mapping_is_ignored() {
        let event: OriginalSyncRoomMessageEvent = serde_json::from_value(serde_json::json!({
            "type": "m.room.message",
            "content": {
                "msgtype": "m.image",
                "body": "ignored",
                "url": "mxc://example.org/image"
            },
            "event_id": "$event:example.org",
            "sender": "@alice:example.org",
            "origin_server_ts": 1700000000000u64
        }))
        .expect("deserialize sync image event");

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
    fn to_device_verification_request_mapping_uses_transaction_id_and_sender() {
        let event: ToDeviceEvent<ToDeviceKeyVerificationRequestEventContent> =
            serde_json::from_value(serde_json::json!({
                "type": "m.key.verification.request",
                "sender": "@alice:example.org",
                "content": {
                    "from_device": "ALICEDEVICE",
                    "transaction_id": "txn-123",
                    "methods": ["m.sas.v1"],
                    "timestamp": 1700000000003u64
                }
            }))
            .expect("deserialize to-device verification request");

        assert_eq!(
            verification_request_from_to_device(&event),
            VerificationRequest {
                flow_id: "txn-123".to_owned(),
                user_id: "@alice:example.org".to_owned(),
            }
        );
    }

    #[test]
    fn to_device_verification_request_mapping_derives_requested_state() {
        let event: ToDeviceEvent<ToDeviceKeyVerificationRequestEventContent> =
            serde_json::from_value(serde_json::json!({
                "type": "m.key.verification.request",
                "sender": "@alice:example.org",
                "content": {
                    "from_device": "ALICEDEVICE",
                    "transaction_id": "txn-123",
                    "methods": ["m.sas.v1"],
                    "timestamp": 1700000000003u64
                }
            }))
            .expect("deserialize to-device verification request");

        assert_eq!(
            verification_requested_state_event(verification_flow_id_from_to_device(&event)),
            VerificationStateEvent {
                flow_id: "txn-123".to_owned(),
                user_id: "@alice:example.org".to_owned(),
                state: VerificationState::Requested,
            }
        );
    }

    #[test]
    fn decimal_mapping_preserves_each_value() {
        assert_eq!(
            map_decimals((1234, 5678, 9012)),
            VerificationDecimals {
                first: 1234,
                second: 5678,
                third: 9012,
            }
        );
    }

    #[test]
    fn emoji_short_auth_string_mapping_preserves_symbols_and_descriptions() {
        let sas = EmojiShortAuthString {
            indices: [0, 1, 2, 3, 4, 5, 6],
            emojis: matrix_emojis(),
        };

        assert_eq!(
            map_emoji_short_auth_string(sas),
            expected_verification_emojis()
        );
    }

    #[test]
    fn emoji_array_mapping_preserves_symbols_and_descriptions() {
        assert_eq!(
            map_emoji_array(matrix_emojis()),
            expected_verification_emojis()
        );
    }

    #[test]
    fn cancelled_state_preserves_reason_code_and_origin() {
        assert_eq!(
            cancelled_state("The user cancelled the verification.", "m.user", true),
            VerificationState::Cancelled {
                reason: "The user cancelled the verification.".to_owned(),
                code: "m.user".to_owned(),
                cancelled_by_us: true,
            }
        );
    }

    #[test]
    fn room_read_update_relevance_tracks_read_state_reasons() {
        for reasons in [
            RoomInfoNotableUpdateReasons::READ_RECEIPT,
            RoomInfoNotableUpdateReasons::FULLY_READ,
            RoomInfoNotableUpdateReasons::UNREAD_MARKER,
        ] {
            assert!(room_read_update_is_relevant(reasons), "{reasons:?} should be relevant");
        }
    }

    #[test]
    fn room_read_update_relevance_ignores_display_name() {
        assert!(!room_read_update_is_relevant(
            RoomInfoNotableUpdateReasons::DISPLAY_NAME
        ));
    }

    #[test]
    fn room_read_state_from_parts_maps_all_public_fields() {
        let receipts = RoomReadReceipts {
            num_unread: 7,
            num_notifications: 3,
            num_mentions: 1,
            latest_active: Some(LatestReadReceipt {
                event_id: latest_event_id(),
            }),
            ..RoomReadReceipts::default()
        };

        assert_eq!(
            room_read_state_from_parts(
                "!room:example.org",
                &receipts,
                Some(fully_read_event_id().as_ref()),
                true,
            ),
            RoomReadState {
                room_id: "!room:example.org".to_owned(),
                latest_active_event_id: Some("$latest:example.org".to_owned()),
                fully_read_event_id: Some("$fully-read:example.org".to_owned()),
                marked_unread: true,
                num_unread: 7,
                num_notifications: 3,
                num_mentions: 1,
            }
        );
    }

    #[test]
    fn room_read_state_from_parts_preserves_absent_markers() {
        let receipts = RoomReadReceipts::default();

        assert_eq!(
            room_read_state_from_parts("!room:example.org", &receipts, None, false),
            RoomReadState {
                room_id: "!room:example.org".to_owned(),
                latest_active_event_id: None,
                fully_read_event_id: None,
                marked_unread: false,
                num_unread: 0,
                num_notifications: 0,
                num_mentions: 0,
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
                    presentation: RoomMessagePresentationKind::Text,
                    body: "hello".to_owned(),
                    send_state: RoomMessageSendState::Pending,
                }),
            }
        );
    }

    #[test]
    fn local_notice_queue_mapping_extracts_public_metadata() {
        let created_at = MilliSecondsSinceUnixEpoch::now();
        let serialized_event =
            SerializableEventContent::new(&AnyMessageLikeEventContent::RoomMessage(
                RoomMessageEventContent::notice_plain("heads up"),
            ))
            .expect("serialize notice event");

        let activity = map_local_message_activity(
            "!room:example.org".to_owned(),
            "txn-1".to_owned(),
            "@alice:example.org",
            created_at,
            &serialized_event,
        )
        .expect("map local notice")
        .expect("notice local echo");

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
                    presentation: RoomMessagePresentationKind::Notice,
                    body: "heads up".to_owned(),
                    send_state: RoomMessageSendState::Pending,
                }),
            }
        );
    }

    #[test]
    fn local_emote_queue_mapping_extracts_public_metadata() {
        let created_at = MilliSecondsSinceUnixEpoch::now();
        let serialized_event = SerializableEventContent::new(
            &AnyMessageLikeEventContent::RoomMessage(RoomMessageEventContent::emote_plain("waves")),
        )
        .expect("serialize emote event");

        let activity = map_local_message_activity(
            "!room:example.org".to_owned(),
            "txn-1".to_owned(),
            "@alice:example.org",
            created_at,
            &serialized_event,
        )
        .expect("map local emote")
        .expect("emote local echo");

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
                    presentation: RoomMessagePresentationKind::Emote,
                    body: "waves".to_owned(),
                    send_state: RoomMessageSendState::Pending,
                }),
            }
        );
    }

    #[test]
    fn unsupported_local_echo_message_is_ignored() {
        let serialized_event =
            SerializableEventContent::new(&AnyMessageLikeEventContent::RoomMessage(
                serde_json::from_value(serde_json::json!({
                    "msgtype": "m.image",
                    "body": "ignored",
                    "url": "mxc://example.org/image"
                }))
                .expect("deserialize image content"),
            ))
            .expect("serialize image event");

        assert_eq!(
            message_presentation_and_body_from_serialized_event(&serialized_event)
                .expect("read local echo body"),
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
                    presentation: RoomMessagePresentationKind::Text,
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
                        presentation: RoomMessagePresentationKind::Text,
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
                    operation: Operation::SendMessage,
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
                    operation: Operation::SendMessage,
                    message: "send failed".to_owned(),
                }),
            ]
        );
    }

    #[test]
    fn room_message_event_content_serializes_expected_msgtypes() {
        let text = serde_json::to_value(room_message_event_content(
            RoomMessagePresentationKind::Text,
            "hello".to_owned(),
        ))
        .expect("serialize text content");
        let notice = serde_json::to_value(room_message_event_content(
            RoomMessagePresentationKind::Notice,
            "heads up".to_owned(),
        ))
        .expect("serialize notice content");
        let emote = serde_json::to_value(room_message_event_content(
            RoomMessagePresentationKind::Emote,
            "waves".to_owned(),
        ))
        .expect("serialize emote content");

        assert_eq!(text["msgtype"], "m.text");
        assert_eq!(notice["msgtype"], "m.notice");
        assert_eq!(emote["msgtype"], "m.emote");
    }
}
