use std::collections::{BTreeMap, BTreeSet};
use std::error::Error;
use std::fmt::{self, Display, Formatter};
use std::str::FromStr;
use std::sync::{Arc, Mutex};

use futures_util::StreamExt;
use matrix_sdk::{
    Client, LoopCtrl, Room, RoomMemberships, SessionChange, SessionMeta, SessionTokens,
    attachment::{AttachmentConfig, AttachmentInfo, BaseFileInfo},
    authentication::matrix::MatrixSession,
    config::SyncSettings,
    deserialized_responses::{
        SyncOrStrippedState, TimelineEvent, TimelineEventKind, UnableToDecryptInfo,
        UnableToDecryptReason,
    },
    encryption::{
        BackupDownloadStrategy, EncryptionSettings,
        backups::BackupState,
        recovery::RecoveryState,
        verification::{
            CancelInfo, Emoji as MatrixEmoji, EmojiShortAuthString, SasState,
            SasVerification as MatrixSasVerification,
            VerificationRequest as MatrixVerificationRequest, VerificationRequestState,
        },
    },
    media::{MediaFormat, MediaRequestParameters, MediaThumbnailSettings},
    room::edit::EditedContent,
    room::{MessagesOptions, Receipts},
    ruma::{
        EventId, MilliSecondsSinceUnixEpoch, MxcUri, OwnedDeviceId, OwnedEventId, OwnedUserId,
        RoomId, RoomOrAliasId, UInt, UserId,
        api::client::media::get_content_thumbnail::v3::Method,
        events::{
            AnyMessageLikeEventContent, SyncStateEvent, ToDeviceEvent,
            direct::{DirectEvent, DirectEventContent},
            key::verification::{
                VerificationMethod, request::ToDeviceKeyVerificationRequestEventContent,
            },
            reaction::{OriginalSyncReactionEvent, ReactionEventContent},
            relation::{Annotation, Reply, Thread},
            room::{
                EncryptedFile, ImageInfo, MediaSource,
                member::{OriginalSyncRoomMemberEvent, SyncRoomMemberEvent},
                message::{
                    AudioInfo, FileInfo, MessageType, OriginalSyncRoomMessageEvent,
                    Relation as MessageContentRelation, Relation as MessageRelation,
                    RoomMessageEventContent, RoomMessageEventContentWithoutRelation, VideoInfo,
                },
                redaction::OriginalSyncRoomRedactionEvent,
                topic::{OriginalSyncRoomTopicEvent, SyncRoomTopicEvent},
            },
            space::child::{SpaceChildEventContent, SpaceChildOrd},
            tag::{TagEvent, TagEventContent, Tags},
            typing::SyncTypingEvent,
        },
    },
    send_queue::{LocalEcho, LocalEchoContent, RoomSendQueueUpdate, SendQueueUpdate},
    store::{RoomLoadSettings, SerializableEventContent},
};
use matrix_sdk_base::{RoomInfoNotableUpdateReasons, read_receipts::RoomReadReceipts};
use matrix_sdk_crypto::CrossSigningStatus;
use mime::Mime;
use tokio::task::JoinHandle;

use crate::api::{
    BackfillRoomHistoryRequest, BanRoomMemberRequest, CoreConfig, CoreErrorEvent, E2eeBackupState,
    E2eeBackupStatus, E2eeCrossSigningStatus, E2eeRecoveryState, E2eeStatus, EditMessageRequest,
    Event, FetchRoomMediaRequest, KickRoomMemberRequest, LoginRequest, MarkRoomReadRequest,
    Operation, ROOM_HISTORY_BACKFILL_LIMIT_MAX, RecoverE2eeRequest, RedactEventRequest,
    RoomActivity, RoomActivityKind, RoomActivityRelation, RoomActivityUpdate,
    RoomActivityUpdateKind, RoomActivityUpdateTarget, RoomAudioActivity, RoomAudioInfo,
    RoomEncryptedFileHashes, RoomEncryptedFileKey, RoomFileActivity, RoomFileInfo,
    RoomHistoryBackfill, RoomHistoryBackfillEvent, RoomImageActivity, RoomImageInfo, RoomMedia,
    RoomMediaFormat, RoomMediaKind, RoomMediaSource, RoomMemberSummary, RoomMembers,
    RoomMembership, RoomMessageActivity, RoomMessagePresentationKind, RoomMessageSendState,
    RoomReadState, RoomStateNoticeActivity, RoomStateNoticeContent, RoomSummary, RoomTagSummary,
    RoomTyping, RoomUnableToDecryptActivity, RoomUnableToDecryptReason, RoomVideoActivity,
    RoomVideoInfo, SendFileRequest, SendMessageReplyTarget, SendMessageRequest,
    SendMessageThreadTarget, SendReactionRequest, SetRoomTopicRequest, SetupE2eeRecoveryRequest,
    SpaceChildSummary, StoredSession, UnsupportedRoomMessageActivity, VerificationDecimals,
    VerificationEmoji, VerificationFlowId, VerificationRequest, VerificationState,
    VerificationStateEvent,
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

#[derive(Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct MatrixEncryptedFileDto {
    url: String,
    key: RoomEncryptedFileKey,
    iv: String,
    hashes: RoomEncryptedFileHashes,
    v: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BackupServerStatusMode {
    Fetch,
    Cached,
}

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

    summarize_rooms(client).await
}

pub(crate) fn map_e2ee_recovery_state(state: RecoveryState) -> E2eeRecoveryState {
    match state {
        RecoveryState::Unknown => E2eeRecoveryState::Unknown,
        RecoveryState::Enabled => E2eeRecoveryState::Enabled,
        RecoveryState::Disabled => E2eeRecoveryState::Disabled,
        RecoveryState::Incomplete => E2eeRecoveryState::Incomplete,
    }
}

pub(crate) fn map_e2ee_backup_state(state: BackupState) -> E2eeBackupState {
    match state {
        BackupState::Unknown => E2eeBackupState::Unknown,
        BackupState::Creating => E2eeBackupState::Creating,
        BackupState::Enabling => E2eeBackupState::Enabling,
        BackupState::Resuming => E2eeBackupState::Resuming,
        BackupState::Enabled => E2eeBackupState::Enabled,
        BackupState::Downloading => E2eeBackupState::Downloading,
        BackupState::Disabling => E2eeBackupState::Disabling,
    }
}

pub(crate) fn map_cross_signing_status(status: CrossSigningStatus) -> E2eeCrossSigningStatus {
    E2eeCrossSigningStatus {
        has_master: status.has_master,
        has_self_signing: status.has_self_signing,
        has_user_signing: status.has_user_signing,
        complete: status.is_complete(),
    }
}

pub(crate) async fn summarize_e2ee_status(
    client: &Client,
    server_status_mode: BackupServerStatusMode,
) -> Result<E2eeStatus, MatrixCoreError> {
    let encryption = client.encryption();
    let recovery = encryption.recovery();
    let backups = encryption.backups();

    let exists_on_server = match server_status_mode {
        BackupServerStatusMode::Fetch => {
            Some(backups.fetch_exists_on_server().await.map_err(|err| {
                MatrixCoreError::new(format!(
                    "failed to fetch Matrix backup server status: {err}"
                ))
            })?)
        }
        // In matrix-sdk 0.18, exists_on_server() can fall through to a network fetch when the
        // in-memory cache is empty, so watcher-driven cached summaries must not call it.
        BackupServerStatusMode::Cached => None,
    };

    Ok(E2eeStatus {
        recovery: map_e2ee_recovery_state(recovery.state()),
        backup: E2eeBackupStatus {
            state: map_e2ee_backup_state(backups.state()),
            enabled_locally: backups.are_enabled().await,
            exists_on_server,
        },
        cross_signing: encryption
            .cross_signing_status()
            .await
            .map(map_cross_signing_status),
    })
}

pub(crate) async fn setup_e2ee_recovery(
    client: &Client,
    request: &SetupE2eeRecoveryRequest,
) -> Result<(String, E2eeStatus), MatrixCoreError> {
    let encryption = client.encryption();
    let recovery = encryption.recovery();
    let enable = recovery.enable();
    let enable = if let Some(passphrase) = request.passphrase.as_deref() {
        enable.with_passphrase(passphrase)
    } else {
        enable
    };
    let enable = if request.wait_for_backups_to_upload {
        enable.wait_for_backups_to_upload()
    } else {
        enable
    };
    let recovery_key = enable
        .await
        .map_err(|err| MatrixCoreError::new(format!("failed to set up E2EE recovery: {err}")))?;

    let status = summarize_e2ee_status(client, BackupServerStatusMode::Fetch).await?;
    Ok((recovery_key, status))
}

pub(crate) async fn recover_e2ee(
    client: &Client,
    request: &RecoverE2eeRequest,
) -> Result<E2eeStatus, MatrixCoreError> {
    client
        .encryption()
        .recovery()
        .recover_and_fix_backup(&request.recovery_secret)
        .await
        .map_err(|err| MatrixCoreError::new(format!("failed to recover E2EE secrets: {err}")))?;

    summarize_e2ee_status(client, BackupServerStatusMode::Fetch).await
}

fn direct_user_ids_by_room_from_content(
    content: Option<&DirectEventContent>,
) -> BTreeMap<String, Vec<String>> {
    let Some(content) = content else {
        return BTreeMap::new();
    };

    let mut direct_user_ids_by_room = BTreeMap::new();
    for (direct_user_id, room_ids) in content.iter() {
        for room_id in room_ids {
            direct_user_ids_by_room
                .entry(room_id.to_string())
                .or_insert_with(Vec::new)
                .push(direct_user_id.to_string());
        }
    }

    for direct_user_ids in direct_user_ids_by_room.values_mut() {
        direct_user_ids.sort();
        direct_user_ids.dedup();
    }

    direct_user_ids_by_room
}

fn room_tag_summaries_from_tags(tags: Option<Tags>) -> Vec<RoomTagSummary> {
    tags.unwrap_or_default()
        .into_iter()
        .map(|(tag, info)| RoomTagSummary {
            tag: tag.to_string(),
            order: info.order.map(|order| order.to_string()),
        })
        .collect()
}

pub(crate) async fn summarize_rooms(client: &Client) -> Result<Vec<RoomSummary>, MatrixCoreError> {
    let direct_content = client
        .account()
        .account_data::<DirectEventContent>()
        .await
        .map_err(|err| {
            MatrixCoreError::new(format!("failed to read direct-room account data: {err}"))
        })?
        .map(|event| event.deserialize())
        .transpose()
        .map_err(|err| {
            MatrixCoreError::new(format!("failed to decode direct-room account data: {err}"))
        })?;
    let direct_user_ids_by_room = direct_user_ids_by_room_from_content(direct_content.as_ref());
    let joined_rooms = client.joined_rooms();
    let joined_room_ids = joined_rooms
        .iter()
        .map(|room| room.room_id().to_string())
        .collect::<BTreeSet<_>>();
    let invited_rooms = client.invited_rooms();
    let mut rooms = Vec::with_capacity(joined_rooms.len() + invited_rooms.len());

    for room in joined_rooms {
        let display_name = room.display_name().await.map_err(|err| {
            MatrixCoreError::new(format!("failed to read room display name: {err}"))
        })?;
        let topic = room.topic();
        let encrypted = room.latest_encryption_state().await.map_err(|err| {
            MatrixCoreError::new(format!("failed to read room encryption state: {err}"))
        })?;
        let is_space = room.is_space();
        let space_children = if is_space {
            summarize_space_children(&room, &joined_room_ids).await?
        } else {
            Vec::new()
        };
        let tag_content = room
            .account_data_static::<TagEventContent>()
            .await
            .map_err(|err| MatrixCoreError::new(format!("failed to read room tags: {err}")))?
            .map(|event| event.deserialize().map(|event| event.content.tags))
            .transpose()
            .map_err(|err| MatrixCoreError::new(format!("failed to decode room tags: {err}")))?;
        let direct_user_ids = direct_user_ids_by_room
            .get(room.room_id().as_str())
            .cloned()
            .unwrap_or_default();
        let tags = room_tag_summaries_from_tags(tag_content);

        rooms.push(RoomSummary {
            room_id: room.room_id().to_string(),
            display_name: display_name.to_string(),
            topic,
            membership: RoomMembership::Join,
            encrypted: encrypted.is_encrypted(),
            is_space,
            is_direct: !direct_user_ids.is_empty(),
            direct_user_ids,
            tags,
            space_children,
        });
    }

    let mut invited_summaries = Vec::with_capacity(invited_rooms.len());
    for room in invited_rooms {
        let display_name = room.display_name().await.map_err(|err| {
            MatrixCoreError::new(format!("failed to read room display name: {err}"))
        })?;
        let encrypted = room.latest_encryption_state().await.map_err(|err| {
            MatrixCoreError::new(format!("failed to read room encryption state: {err}"))
        })?;
        let direct_user_ids = direct_user_ids_by_room
            .get(room.room_id().as_str())
            .cloned()
            .unwrap_or_default();

        invited_summaries.push(RoomSummary {
            room_id: room.room_id().to_string(),
            display_name: display_name.to_string(),
            topic: None,
            membership: RoomMembership::Invite,
            encrypted: encrypted.is_encrypted(),
            is_space: room.is_space(),
            is_direct: !direct_user_ids.is_empty(),
            direct_user_ids,
            tags: Vec::new(),
            space_children: Vec::new(),
        });
    }
    invited_summaries.sort_by(|left, right| {
        left.display_name
            .cmp(&right.display_name)
            .then_with(|| left.room_id.cmp(&right.room_id))
    });
    rooms.extend(invited_summaries);

    Ok(rooms)
}

pub(crate) async fn join_room(client: &Client, room_or_alias: &str) -> Result<(), MatrixCoreError> {
    let room_or_alias = <&RoomOrAliasId>::try_from(room_or_alias)
        .map_err(|err| MatrixCoreError::new(format!("invalid Matrix room id or alias: {err}")))?;

    client
        .join_room_by_id_or_alias(room_or_alias, &[])
        .await
        .map_err(|err| MatrixCoreError::new(format!("failed to join Matrix room: {err}")))?;

    Ok(())
}

pub(crate) async fn accept_room_invite(
    client: &Client,
    room_id: &str,
) -> Result<(), MatrixCoreError> {
    let room_id = <&RoomId>::try_from(room_id)
        .map_err(|err| MatrixCoreError::new(format!("invalid Matrix room id: {err}")))?;

    let room = client
        .invited_rooms()
        .into_iter()
        .find(|room| room.room_id().as_str() == room_id.as_str())
        .ok_or_else(|| MatrixCoreError::new(format!("invited room not found: {room_id}")))?;

    room.join().await.map_err(|err| {
        MatrixCoreError::new(format!("failed to accept Matrix room invitation: {err}"))
    })?;

    Ok(())
}

pub(crate) async fn create_direct_room(
    client: &Client,
    user_id: &str,
) -> Result<(), MatrixCoreError> {
    let user_id = <&UserId>::try_from(user_id)
        .map_err(|err| MatrixCoreError::new(format!("invalid Matrix user id: {err}")))?;

    client.create_dm(user_id).await.map_err(|err| {
        MatrixCoreError::new(format!("failed to create Matrix direct room: {err}"))
    })?;

    Ok(())
}

pub(crate) async fn leave_joined_room(
    client: &Client,
    room_id: &str,
) -> Result<(), MatrixCoreError> {
    let room_id = <&RoomId>::try_from(room_id)
        .map_err(|err| MatrixCoreError::new(format!("invalid Matrix room id: {err}")))?;

    let room = client
        .joined_rooms()
        .into_iter()
        .find(|room| room.room_id().as_str() == room_id.as_str())
        .ok_or_else(|| MatrixCoreError::new(format!("joined room not found: {room_id}")))?;

    room.leave()
        .await
        .map_err(|err| MatrixCoreError::new(format!("failed to leave Matrix room: {err}")))?;

    Ok(())
}

pub(crate) async fn kick_room_member(
    client: &Client,
    request: &KickRoomMemberRequest,
) -> Result<(), MatrixCoreError> {
    let room_id = <&RoomId>::try_from(request.room_id.as_str())
        .map_err(|err| MatrixCoreError::new(format!("invalid Matrix room id: {err}")))?;
    let user_id = <&UserId>::try_from(request.user_id.as_str())
        .map_err(|err| MatrixCoreError::new(format!("invalid Matrix user id: {err}")))?;
    let room = joined_room_by_id(client, room_id.as_str())?;

    room.kick_user(user_id, None)
        .await
        .map_err(|err| MatrixCoreError::new(format!("failed to kick Matrix room member: {err}")))
}

pub(crate) async fn ban_room_member(
    client: &Client,
    request: &BanRoomMemberRequest,
) -> Result<(), MatrixCoreError> {
    let room_id = <&RoomId>::try_from(request.room_id.as_str())
        .map_err(|err| MatrixCoreError::new(format!("invalid Matrix room id: {err}")))?;
    let user_id = <&UserId>::try_from(request.user_id.as_str())
        .map_err(|err| MatrixCoreError::new(format!("invalid Matrix user id: {err}")))?;
    let room = joined_room_by_id(client, room_id.as_str())?;

    room.ban_user(user_id, None)
        .await
        .map_err(|err| MatrixCoreError::new(format!("failed to ban Matrix room member: {err}")))
}

pub(crate) async fn reject_room_invite(
    client: &Client,
    room_id: &str,
) -> Result<(), MatrixCoreError> {
    let room_id = <&RoomId>::try_from(room_id)
        .map_err(|err| MatrixCoreError::new(format!("invalid Matrix room id: {err}")))?;

    let room = client
        .invited_rooms()
        .into_iter()
        .find(|room| room.room_id().as_str() == room_id.as_str())
        .ok_or_else(|| MatrixCoreError::new(format!("invited room not found: {room_id}")))?;

    room.leave().await.map_err(|err| {
        MatrixCoreError::new(format!("failed to reject Matrix room invitation: {err}"))
    })?;

    Ok(())
}

pub(crate) async fn set_room_topic(
    client: &Client,
    request: SetRoomTopicRequest,
) -> Result<(), MatrixCoreError> {
    let room_id = <&RoomId>::try_from(request.room_id.as_str())
        .map_err(|err| MatrixCoreError::new(format!("invalid Matrix room id: {err}")))?;

    let room = client
        .joined_rooms()
        .into_iter()
        .find(|room| room.room_id().as_str() == room_id.as_str())
        .ok_or_else(|| MatrixCoreError::new(format!("joined room not found: {room_id}")))?;

    room.set_room_topic(&request.topic)
        .await
        .map_err(|err| MatrixCoreError::new(format!("failed to set Matrix room topic: {err}")))?;

    Ok(())
}

pub(crate) async fn emit_room_snapshot(
    client: &Client,
    event_tx: &EventSender,
) -> Result<(), MatrixCoreError> {
    emit_event(event_tx, Event::SyncedRooms(summarize_rooms(client).await?));
    Ok(())
}

pub(crate) async fn emit_joined_room_members_snapshot(
    client: &Client,
    event_tx: &EventSender,
    room_id: &str,
) -> Result<(), MatrixCoreError> {
    let authenticated_user_id = client.user_id().ok_or_else(|| {
        MatrixCoreError::new("cannot summarize joined room members without an authenticated user")
    })?;
    let room = joined_room_by_id(client, room_id)?;
    let members = joined_room_members(authenticated_user_id, &room).await?;
    emit_event(event_tx, Event::RoomMembers(members));
    Ok(())
}

async fn emit_room_snapshot_or_room_list_error(client: &Client, events: &EventSender) {
    if let Err(err) = emit_room_snapshot(client, events).await {
        emit_event(
            events,
            Event::Error(CoreErrorEvent {
                operation: Operation::RoomList,
                message: err.to_string(),
            }),
        );
    }
}

fn room_member_display_label(member: &RoomMemberSummary) -> &str {
    match member.display_name.as_deref() {
        Some(display_name) if !display_name.is_empty() => display_name,
        _ => member.user_id.as_str(),
    }
}

#[cfg(test)]
fn sorted_room_member_summaries_from_parts(
    members: Vec<(String, Option<String>)>,
) -> Vec<RoomMemberSummary> {
    let members = members
        .into_iter()
        .map(|(user_id, display_name)| RoomMemberSummary {
            user_id,
            display_name,
            can_kick: false,
            can_ban: false,
        })
        .collect::<Vec<_>>();
    room_members_from_summaries("", members).members
}

#[cfg(test)]
fn room_members_from_parts(room_id: &str, members: Vec<(String, Option<String>)>) -> RoomMembers {
    RoomMembers {
        room_id: room_id.to_owned(),
        members: sorted_room_member_summaries_from_parts(members),
    }
}

fn room_members_from_summaries(room_id: &str, mut members: Vec<RoomMemberSummary>) -> RoomMembers {
    members.sort_by(|left, right| {
        room_member_display_label(left)
            .to_lowercase()
            .cmp(&room_member_display_label(right).to_lowercase())
            .then_with(|| left.user_id.cmp(&right.user_id))
    });
    RoomMembers {
        room_id: room_id.to_owned(),
        members,
    }
}

fn moderation_capabilities<T: PartialOrd>(
    actor_user_id: &UserId,
    target_user_id: &UserId,
    actor_can_kick: bool,
    actor_can_ban: bool,
    actor_power_level: &T,
    target_power_level: &T,
) -> (bool, bool) {
    let eligible_target = actor_user_id != target_user_id && actor_power_level > target_power_level;
    (
        eligible_target && actor_can_kick,
        eligible_target && actor_can_ban,
    )
}

fn room_members_from_matrix_members(
    room_id: &str,
    authenticated_user_id: &UserId,
    members: Vec<matrix_sdk::room::RoomMember>,
) -> Result<RoomMembers, MatrixCoreError> {
    let actor = members
        .iter()
        .find(|member| member.user_id() == authenticated_user_id)
        .ok_or_else(|| {
            MatrixCoreError::new(format!(
                "authenticated user is not a joined member of room: {room_id}"
            ))
        })?;
    let actor_can_kick = actor.can_kick();
    let actor_can_ban = actor.can_ban();
    let actor_power_level = actor.power_level();

    Ok(room_members_from_summaries(
        room_id,
        members
            .into_iter()
            .map(|member| {
                let (can_kick, can_ban) = moderation_capabilities(
                    authenticated_user_id,
                    member.user_id(),
                    actor_can_kick,
                    actor_can_ban,
                    &actor_power_level,
                    &member.power_level(),
                );
                RoomMemberSummary {
                    user_id: member.user_id().to_string(),
                    display_name: member.display_name().map(str::to_owned),
                    can_kick,
                    can_ban,
                }
            })
            .collect(),
    ))
}

async fn joined_room_members(
    authenticated_user_id: &UserId,
    room: &Room,
) -> Result<RoomMembers, MatrixCoreError> {
    let members = room.members(RoomMemberships::JOIN).await.map_err(|err| {
        MatrixCoreError::new(format!("failed to read joined Matrix room members: {err}"))
    })?;
    room_members_from_matrix_members(room.room_id().as_str(), authenticated_user_id, members)
}

async fn joined_room_members_no_sync(
    authenticated_user_id: &UserId,
    room: &Room,
) -> Result<RoomMembers, MatrixCoreError> {
    let members = room
        .members_no_sync(RoomMemberships::JOIN)
        .await
        .map_err(|err| {
            MatrixCoreError::new(format!(
                "failed to read cached joined Matrix room members: {err}"
            ))
        })?;
    room_members_from_matrix_members(room.room_id().as_str(), authenticated_user_id, members)
}

pub(crate) async fn summarize_joined_room_members(
    client: &Client,
) -> Result<Vec<RoomMembers>, MatrixCoreError> {
    let authenticated_user_id = client.user_id().ok_or_else(|| {
        MatrixCoreError::new("cannot summarize joined room members without an authenticated user")
    })?;
    let joined_rooms = client.joined_rooms();
    let mut room_members = Vec::with_capacity(joined_rooms.len());

    for room in joined_rooms {
        room_members.push(joined_room_members(authenticated_user_id, &room).await?);
    }

    Ok(room_members)
}

pub(crate) fn room_read_update_is_relevant(reasons: RoomInfoNotableUpdateReasons) -> bool {
    reasons.intersects(
        RoomInfoNotableUpdateReasons::READ_RECEIPT
            | RoomInfoNotableUpdateReasons::FULLY_READ
            | RoomInfoNotableUpdateReasons::UNREAD_MARKER,
    )
}

pub(crate) fn room_list_update_is_relevant(reasons: RoomInfoNotableUpdateReasons) -> bool {
    reasons.intersects(
        RoomInfoNotableUpdateReasons::MEMBERSHIP
            | RoomInfoNotableUpdateReasons::DISPLAY_NAME
            | RoomInfoNotableUpdateReasons::NONE,
    )
}

pub(crate) fn start_e2ee_recovery_state_task(
    client: Client,
    events: EventSender,
) -> JoinHandle<()> {
    let mut states = client.encryption().recovery().state_stream();

    tokio::task::spawn_local(async move {
        while states.next().await.is_some() {
            match summarize_e2ee_status(&client, BackupServerStatusMode::Cached).await {
                Ok(status) => emit_event(&events, Event::E2eeStatus(status)),
                Err(err) => emit_event(
                    &events,
                    Event::Error(CoreErrorEvent {
                        operation: Operation::E2eeStatus,
                        message: err.to_string(),
                    }),
                ),
            }
        }
    })
}

pub(crate) fn start_e2ee_backup_state_task(client: Client, events: EventSender) -> JoinHandle<()> {
    let mut states = client.encryption().backups().state_stream();

    tokio::task::spawn_local(async move {
        while let Some(state) = states.next().await {
            match state {
                Ok(_) => match summarize_e2ee_status(&client, BackupServerStatusMode::Cached).await
                {
                    Ok(status) => emit_event(&events, Event::E2eeStatus(status)),
                    Err(err) => emit_event(
                        &events,
                        Event::Error(CoreErrorEvent {
                            operation: Operation::E2eeStatus,
                            message: err.to_string(),
                        }),
                    ),
                },
                Err(_) => emit_event(
                    &events,
                    Event::Error(CoreErrorEvent {
                        operation: Operation::E2eeStatus,
                        message: "missed Matrix backup state update".to_owned(),
                    }),
                ),
            }
        }
    })
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

pub(crate) async fn fetch_room_media(client: &Client, request: FetchRoomMediaRequest) -> RoomMedia {
    let FetchRoomMediaRequest {
        room_id,
        activity_id,
        event_id,
        source,
        format,
    } = request;

    let params = match room_media_request_parameters(&source, &format) {
        Ok(params) => params,
        Err(err) => {
            return RoomMedia {
                room_id,
                activity_id,
                event_id,
                kind: RoomMediaKind::Failed {
                    format,
                    message: err.to_string(),
                },
            };
        }
    };

    match client.media().get_media_content(&params, true).await {
        Ok(bytes) => RoomMedia {
            room_id,
            activity_id,
            event_id,
            kind: RoomMediaKind::Loaded { format, bytes },
        },
        Err(err) => RoomMedia {
            room_id,
            activity_id,
            event_id,
            kind: RoomMediaKind::Failed {
                format,
                message: format!("failed to fetch room media: {err}"),
            },
        },
    }
}

fn room_media_request_parameters(
    source: &RoomMediaSource,
    format: &RoomMediaFormat,
) -> Result<MediaRequestParameters, MatrixCoreError> {
    let source = media_source_from_room_media_source(source)?;

    let format = match format {
        RoomMediaFormat::Thumbnail { width, height } => {
            if *width == 0 || *height == 0 {
                return Err(MatrixCoreError::new(
                    "thumbnail width and height must be greater than zero",
                ));
            }

            let width = UInt::new(u64::from(*width))
                .ok_or_else(|| MatrixCoreError::new("thumbnail width is out of range"))?;
            let height = UInt::new(u64::from(*height))
                .ok_or_else(|| MatrixCoreError::new("thumbnail height is out of range"))?;

            MediaFormat::Thumbnail(MediaThumbnailSettings::with_method(
                Method::Scale,
                width,
                height,
            ))
        }
        RoomMediaFormat::File => MediaFormat::File,
    };

    Ok(MediaRequestParameters { source, format })
}

fn media_source_from_room_media_source(
    source: &RoomMediaSource,
) -> Result<MediaSource, MatrixCoreError> {
    match source {
        RoomMediaSource::Plain { url } => {
            let uri = <&MxcUri>::from(url.as_str());
            uri.validate()
                .map_err(|err| MatrixCoreError::new(format!("invalid media MXC URI: {err}")))?;
            Ok(MediaSource::Plain(uri.to_owned()))
        }
        RoomMediaSource::Encrypted {
            url,
            key,
            iv,
            hashes,
            v,
        } => {
            let file: EncryptedFile = serde_json::from_value(serde_json::json!({
                "url": url,
                "key": {
                    "kty": key.kty,
                    "key_ops": key.key_ops,
                    "alg": key.alg,
                    "k": key.k,
                    "ext": key.ext,
                },
                "iv": iv,
                "hashes": {
                    "sha256": hashes.sha256,
                },
                "v": v,
            }))
            .map_err(|err| {
                MatrixCoreError::new(format!("invalid encrypted media metadata: {err}"))
            })?;

            Ok(MediaSource::Encrypted(Box::new(file)))
        }
    }
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

#[derive(Default)]
struct ReactionEventIndex {
    live: BTreeMap<String, ReactionEventIndexEntry>,
    pending_redactions: BTreeMap<String, String>,
}

struct ReactionEventIndexEntry {
    target_event_id: String,
    key: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum PendingOutgoingKind {
    Message,
    MessageEdit {
        target_event_id: String,
    },
    Reaction {
        target_event_id: String,
        key: String,
    },
    Redaction {
        redacts: String,
    },
}

type PendingOutgoingRegistry = Arc<Mutex<BTreeMap<String, PendingOutgoingKind>>>;

pub(crate) fn register_event_handlers(client: &Client, events: EventSender) {
    let direct_account_data_events = events.clone();
    client.add_event_handler(move |_event: DirectEvent, client: Client| {
        let events = direct_account_data_events.clone();
        async move {
            emit_room_snapshot_or_room_list_error(&client, &events).await;
        }
    });

    let tag_account_data_events = events.clone();
    client.add_event_handler(move |_event: TagEvent, _room: Room, client: Client| {
        let events = tag_account_data_events.clone();
        async move {
            emit_room_snapshot_or_room_list_error(&client, &events).await;
        }
    });

    let timeline_events = events.clone();
    client.add_event_handler(move |event: OriginalSyncRoomMessageEvent, room: Room| {
        let events = timeline_events.clone();
        async move {
            if let Some(event) =
                room_timeline_event_from_sync_message(room.room_id().as_str(), &event)
            {
                emit_event(&events, event);
            }
        }
    });

    let reaction_index = Arc::new(Mutex::new(ReactionEventIndex::default()));

    let redaction_events = events.clone();
    let redaction_reaction_index = reaction_index.clone();
    client.add_event_handler(move |event: OriginalSyncRoomRedactionEvent, room: Room| {
        let events = redaction_events.clone();
        let reaction_index = redaction_reaction_index.clone();
        async move {
            let rules = room.clone_info().room_version_rules_or_default().redaction;
            let redacted_event_id = event.redacts(&rules).to_string();
            let mut reaction_index = reaction_index.lock().expect("lock reaction event index");
            if let Some(entry) = reaction_index.live.remove(redacted_event_id.as_str()) {
                emit_event(
                    &events,
                    Event::RoomActivityUpdate(
                        room_activity_update_from_indexed_reaction_redaction(
                            room.room_id().as_str(),
                            redacted_event_id,
                            &event,
                            entry,
                        ),
                    ),
                );
                return;
            }

            reaction_index
                .pending_redactions
                .insert(redacted_event_id.clone(), event.event_id.to_string());
            emit_event(
                &events,
                Event::RoomActivityUpdate(room_activity_update_from_sync_redaction(
                    room.room_id().as_str(),
                    &event,
                    redacted_event_id,
                )),
            );
        }
    });

    let reaction_events = events.clone();
    let reaction_event_index = reaction_index.clone();
    client.add_event_handler(move |event: OriginalSyncReactionEvent, room: Room| {
        let events = reaction_events.clone();
        let reaction_index = reaction_event_index.clone();
        async move {
            let reaction_event_id = event.event_id.to_string();
            let mut reaction_index = reaction_index.lock().expect("lock reaction event index");
            if reaction_index
                .pending_redactions
                .remove(reaction_event_id.as_str())
                .is_some()
            {
                return;
            }

            reaction_index.live.insert(
                reaction_event_id.clone(),
                ReactionEventIndexEntry {
                    target_event_id: event.content.relates_to.event_id.to_string(),
                    key: event.content.relates_to.key.clone(),
                },
            );

            emit_event(
                &events,
                Event::RoomActivityUpdate(room_activity_update_from_sync_reaction(
                    room.room_id().as_str(),
                    &event,
                )),
            );
        }
    });

    let topic_events = events.clone();
    client.add_event_handler(move |event: SyncRoomTopicEvent, room: Room| {
        let events = topic_events.clone();
        async move {
            if let SyncRoomTopicEvent::Original(event) = event {
                emit_event(
                    &events,
                    Event::RoomActivity(Box::new(room_activity_from_sync_topic(
                        room.room_id().as_str(),
                        &event,
                    ))),
                );
            }
        }
    });

    let member_events = events.clone();
    let member_client = client.clone();
    client.add_event_handler(move |event: SyncRoomMemberEvent, room: Room| {
        let events = member_events.clone();
        let client = member_client.clone();
        async move {
            if let SyncRoomMemberEvent::Original(event) = event {
                emit_event(
                    &events,
                    Event::RoomActivity(Box::new(room_activity_from_sync_member(
                        room.room_id().as_str(),
                        &event,
                    ))),
                );
            }

            if let Some(authenticated_user_id) = client.user_id()
                && let Ok(snapshot) =
                    joined_room_members_no_sync(authenticated_user_id, &room).await
            {
                emit_event(&events, Event::RoomMembers(snapshot));
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

fn room_timeline_event_from_sync_message(
    room_id: &str,
    event: &OriginalSyncRoomMessageEvent,
) -> Option<Event> {
    if let Some(MessageRelation::Replacement(replacement)) = event.content.relates_to.as_ref() {
        return room_activity_update_from_sync_message_replacement(room_id, event, replacement)
            .map(Event::RoomActivityUpdate);
    }

    room_activity_from_sync_message(room_id, event)
        .map(|activity| Event::RoomActivity(Box::new(activity)))
}

fn room_activity_from_sync_message(
    room_id: &str,
    event: &OriginalSyncRoomMessageEvent,
) -> Option<RoomActivity> {
    let relation = room_activity_relation_from_message(event);

    if let Some((presentation, body)) =
        room_message_presentation_and_body_from_message_type(&event.content.msgtype)
    {
        return Some(RoomActivity {
            room_id: room_id.to_owned(),
            activity_id: format!("event:{}", event.event_id),
            timestamp_ms: event.origin_server_ts.get().into(),
            relation,
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
        });
    }

    if let MessageType::Image(image) = &event.content.msgtype {
        return Some(RoomActivity {
            room_id: room_id.to_owned(),
            activity_id: format!("event:{}", event.event_id),
            timestamp_ms: event.origin_server_ts.get().into(),
            relation: relation.clone(),
            kind: RoomActivityKind::Image(Box::new(RoomImageActivity {
                event_id: event.event_id.to_string(),
                sender: event.sender.to_string(),
                body: image.body.clone(),
                filename: image.filename.clone(),
                source: room_media_source_from_matrix(&image.source),
                info: image.info.as_deref().map(room_image_info_from_matrix),
            })),
        });
    }

    if let MessageType::File(file) = &event.content.msgtype {
        return Some(RoomActivity {
            room_id: room_id.to_owned(),
            activity_id: format!("event:{}", event.event_id),
            timestamp_ms: event.origin_server_ts.get().into(),
            relation: relation.clone(),
            kind: RoomActivityKind::File(Box::new(RoomFileActivity {
                event_id: event.event_id.to_string(),
                sender: event.sender.to_string(),
                body: file.body.clone(),
                filename: file.filename.clone(),
                source: room_media_source_from_matrix(&file.source),
                info: file.info.as_deref().map(room_file_info_from_matrix),
            })),
        });
    }

    if let MessageType::Audio(audio) = &event.content.msgtype {
        return Some(RoomActivity {
            room_id: room_id.to_owned(),
            activity_id: format!("event:{}", event.event_id),
            timestamp_ms: event.origin_server_ts.get().into(),
            relation: relation.clone(),
            kind: RoomActivityKind::Audio(Box::new(RoomAudioActivity {
                event_id: event.event_id.to_string(),
                sender: event.sender.to_string(),
                body: audio.body.clone(),
                filename: audio.filename.clone(),
                source: room_media_source_from_matrix(&audio.source),
                info: audio.info.as_deref().map(room_audio_info_from_matrix),
            })),
        });
    }

    if let MessageType::Video(video) = &event.content.msgtype {
        return Some(RoomActivity {
            room_id: room_id.to_owned(),
            activity_id: format!("event:{}", event.event_id),
            timestamp_ms: event.origin_server_ts.get().into(),
            relation: relation.clone(),
            kind: RoomActivityKind::Video(Box::new(RoomVideoActivity {
                event_id: event.event_id.to_string(),
                sender: event.sender.to_string(),
                body: video.body.clone(),
                filename: video.filename.clone(),
                source: room_media_source_from_matrix(&video.source),
                info: video.info.as_deref().map(room_video_info_from_matrix),
            })),
        });
    }

    let (msgtype, body) = unsupported_room_message_body(&event.content.msgtype)?;

    Some(RoomActivity {
        room_id: room_id.to_owned(),
        activity_id: format!("event:{}", event.event_id),
        timestamp_ms: event.origin_server_ts.get().into(),
        relation,
        kind: RoomActivityKind::UnsupportedMessage(UnsupportedRoomMessageActivity {
            event_id: event.event_id.to_string(),
            sender: event.sender.to_string(),
            msgtype,
            body,
        }),
    })
}

fn room_activity_update_from_sync_message_replacement(
    room_id: &str,
    event: &OriginalSyncRoomMessageEvent,
    replacement: &matrix_sdk::ruma::events::relation::Replacement<
        RoomMessageEventContentWithoutRelation,
    >,
) -> Option<RoomActivityUpdate> {
    let (presentation, body) =
        room_message_presentation_and_body_from_message_type(&replacement.new_content.msgtype)?;

    Some(RoomActivityUpdate {
        room_id: room_id.to_owned(),
        target: RoomActivityUpdateTarget::Event {
            event_id: replacement.event_id.to_string(),
        },
        kind: RoomActivityUpdateKind::MessageReplaced {
            replacement_event_id: event.event_id.to_string(),
            sender: event.sender.to_string(),
            timestamp_ms: event.origin_server_ts.get().into(),
            presentation,
            body,
        },
    })
}

fn room_activity_update_from_sync_reaction(
    room_id: &str,
    event: &OriginalSyncReactionEvent,
) -> RoomActivityUpdate {
    RoomActivityUpdate {
        room_id: room_id.to_owned(),
        target: RoomActivityUpdateTarget::Event {
            event_id: event.content.relates_to.event_id.to_string(),
        },
        kind: RoomActivityUpdateKind::ReactionAdded {
            reaction_event_id: event.event_id.to_string(),
            sender: event.sender.to_string(),
            timestamp_ms: event.origin_server_ts.get().into(),
            key: event.content.relates_to.key.clone(),
        },
    }
}

fn room_activity_update_from_indexed_reaction_redaction(
    room_id: &str,
    reaction_event_id: String,
    event: &OriginalSyncRoomRedactionEvent,
    entry: ReactionEventIndexEntry,
) -> RoomActivityUpdate {
    RoomActivityUpdate {
        room_id: room_id.to_owned(),
        target: RoomActivityUpdateTarget::Event {
            event_id: entry.target_event_id,
        },
        kind: RoomActivityUpdateKind::ReactionRemoved {
            reaction_event_id,
            redaction_event_id: event.event_id.to_string(),
            key: entry.key,
        },
    }
}

fn room_activity_update_from_sync_redaction(
    room_id: &str,
    event: &OriginalSyncRoomRedactionEvent,
    redacted_event_id: String,
) -> RoomActivityUpdate {
    RoomActivityUpdate {
        room_id: room_id.to_owned(),
        target: RoomActivityUpdateTarget::Event {
            event_id: redacted_event_id,
        },
        kind: RoomActivityUpdateKind::ActivityRedacted {
            redaction_event_id: event.event_id.to_string(),
        },
    }
}

fn room_activity_relation_from_message(
    event: &OriginalSyncRoomMessageEvent,
) -> Option<RoomActivityRelation> {
    match event.content.relates_to.as_ref()? {
        MessageRelation::Reply(reply) => Some(RoomActivityRelation::Reply {
            in_reply_to_event_id: reply.in_reply_to.event_id.to_string(),
        }),
        MessageRelation::Thread(thread) => Some(RoomActivityRelation::Thread {
            thread_root_event_id: thread.event_id.to_string(),
            in_reply_to_event_id: thread
                .in_reply_to
                .as_ref()
                .map(|in_reply_to| in_reply_to.event_id.to_string()),
            is_falling_back: thread.is_falling_back,
        }),
        _ => None,
    }
}

fn room_media_source_from_matrix(source: &MediaSource) -> RoomMediaSource {
    match source {
        MediaSource::Plain(url) => RoomMediaSource::Plain {
            url: url.as_str().to_owned(),
        },
        MediaSource::Encrypted(file) => {
            let file: MatrixEncryptedFileDto = serde_json::from_value(
                serde_json::to_value(file).expect("serialize Matrix encrypted file"),
            )
            .expect("deserialize Matrix encrypted file into core DTO");

            RoomMediaSource::Encrypted {
                url: file.url,
                key: file.key,
                iv: file.iv,
                hashes: file.hashes,
                v: file.v,
            }
        }
    }
}

fn room_image_info_from_matrix(info: &ImageInfo) -> RoomImageInfo {
    RoomImageInfo {
        mimetype: info.mimetype.as_deref().map(str::to_owned),
        size: info.size.map(u64::from),
        width: info.width.map(u64::from),
        height: info.height.map(u64::from),
        is_animated: info.is_animated,
        thumbnail_source: info
            .thumbnail_source
            .as_ref()
            .map(room_media_source_from_matrix),
    }
}

fn room_file_info_from_matrix(info: &FileInfo) -> RoomFileInfo {
    RoomFileInfo {
        mimetype: info.mimetype.as_deref().map(str::to_owned),
        size: info.size.map(u64::from),
    }
}

fn room_audio_info_from_matrix(info: &AudioInfo) -> RoomAudioInfo {
    RoomAudioInfo {
        mimetype: info.mimetype.as_deref().map(str::to_owned),
        size: info.size.map(u64::from),
        duration_ms: info.duration.map(|duration| {
            u64::try_from(duration.as_millis())
                .expect("Matrix audio duration exceeds u64 milliseconds")
        }),
    }
}

fn room_video_info_from_matrix(info: &VideoInfo) -> RoomVideoInfo {
    RoomVideoInfo {
        mimetype: info.mimetype.as_deref().map(str::to_owned),
        size: info.size.map(u64::from),
        duration_ms: info.duration.map(|duration| {
            u64::try_from(duration.as_millis())
                .expect("Matrix video duration exceeds u64 milliseconds")
        }),
        width: info.width.map(u64::from),
        height: info.height.map(u64::from),
        thumbnail_source: info
            .thumbnail_source
            .as_ref()
            .map(room_media_source_from_matrix),
    }
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
        MessageType::Image(_)
        | MessageType::Audio(_)
        | MessageType::File(_)
        | MessageType::Video(_)
        | MessageType::Location(_)
        | MessageType::VerificationRequest(_)
        | MessageType::_Custom(_) => None,
        // ruma marks MessageType as non_exhaustive, so a final fallback is required.
        _ => None,
    }
}

fn unsupported_room_message_body(message_type: &MessageType) -> Option<(String, String)> {
    match message_type {
        MessageType::_Custom(_) => Some((
            message_type.msgtype().to_owned(),
            message_type.body().to_owned(),
        )),
        MessageType::Text(_)
        | MessageType::Notice(_)
        | MessageType::Emote(_)
        | MessageType::Image(_)
        | MessageType::Audio(_)
        | MessageType::File(_)
        | MessageType::Video(_)
        | MessageType::Location(_)
        | MessageType::VerificationRequest(_) => None,
        // ruma marks MessageType as non_exhaustive, so a final fallback is required.
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
        relation: None,
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
        relation: None,
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

                    let Some(room) = client
                        .joined_rooms()
                        .into_iter()
                        .find(|room| room.room_id() == update.room_id)
                    else {
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

pub(crate) fn start_room_list_update_task(
    client: Client,
    events: EventSender,
) -> Result<JoinHandle<()>, matrix_sdk::event_cache::EventCacheError> {
    client.event_cache().subscribe()?;
    let mut updates = client.room_info_notable_update_receiver();

    Ok(tokio::task::spawn_local(async move {
        loop {
            match updates.recv().await {
                Ok(update) => {
                    if !room_list_update_is_relevant(update.reasons) {
                        continue;
                    }

                    emit_room_snapshot_or_room_list_error(&client, &events).await;
                }
                Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => emit_event(
                    &events,
                    Event::Error(CoreErrorEvent {
                        operation: Operation::RoomList,
                        message: "missed Matrix room-list update".to_owned(),
                    }),
                ),
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
    let pending = Arc::new(Mutex::new(BTreeMap::new()));

    tokio::task::spawn_local(async move {
        match hydrate_send_queue_local_echoes(&client, &current_user_id, &pending).await {
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
                Ok(update) => match map_send_queue_update(update, &current_user_id, &pending) {
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
    pending: &PendingOutgoingRegistry,
) -> Result<Vec<Event>, MatrixCoreError> {
    let room_id = update.room_id.to_string();

    match update.update {
        RoomSendQueueUpdate::NewLocalEvent(local_echo) => {
            map_local_echo(room_id, local_echo, current_user_id, pending)
        }
        RoomSendQueueUpdate::SentEvent {
            transaction_id,
            event_id,
        } => {
            let transaction_id = transaction_id.to_string();
            Ok(match take_pending_outgoing_kind(pending, &transaction_id) {
                Some(PendingOutgoingKind::Message) => {
                    vec![Event::RoomActivityUpdate(RoomActivityUpdate {
                        room_id,
                        target: RoomActivityUpdateTarget::Transaction { transaction_id },
                        kind: RoomActivityUpdateKind::MessageSent {
                            event_id: event_id.to_string(),
                        },
                    })]
                }
                Some(PendingOutgoingKind::MessageEdit { .. }) => Vec::new(),
                Some(PendingOutgoingKind::Reaction { .. })
                | Some(PendingOutgoingKind::Redaction { .. })
                | None => Vec::new(),
            })
        }
        RoomSendQueueUpdate::SendError {
            transaction_id,
            error,
            is_recoverable,
        } => {
            let transaction_id = transaction_id.to_string();
            let message = error.to_string();
            Ok(match take_pending_outgoing_kind(pending, &transaction_id) {
                Some(PendingOutgoingKind::Message) => vec![
                    Event::RoomActivityUpdate(RoomActivityUpdate {
                        room_id,
                        target: RoomActivityUpdateTarget::Transaction { transaction_id },
                        kind: RoomActivityUpdateKind::MessageFailed {
                            message: message.clone(),
                            recoverable: is_recoverable,
                        },
                    }),
                    Event::Error(CoreErrorEvent {
                        operation: Operation::SendMessage,
                        message,
                    }),
                ],
                Some(PendingOutgoingKind::MessageEdit { .. }) => {
                    vec![Event::Error(CoreErrorEvent {
                        operation: Operation::EditMessage,
                        message,
                    })]
                }
                Some(PendingOutgoingKind::Reaction { .. }) => vec![Event::Error(CoreErrorEvent {
                    operation: Operation::SendReaction,
                    message,
                })],
                Some(PendingOutgoingKind::Redaction { .. }) => vec![Event::Error(CoreErrorEvent {
                    operation: Operation::RedactEvent,
                    message,
                })],
                None => vec![Event::Error(CoreErrorEvent {
                    operation: Operation::SendMessage,
                    message,
                })],
            })
        }
        RoomSendQueueUpdate::CancelledLocalEvent { transaction_id } => {
            take_pending_outgoing_kind(pending, transaction_id.as_ref());
            Ok(Vec::new())
        }
        RoomSendQueueUpdate::ReplacedLocalEvent { .. }
        | RoomSendQueueUpdate::RetryEvent { .. }
        | RoomSendQueueUpdate::MediaUpload { .. } => Ok(Vec::new()),
    }
}

async fn hydrate_send_queue_local_echoes(
    client: &Client,
    current_user_id: &str,
    pending: &PendingOutgoingRegistry,
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
                pending,
            )?);
        }
    }

    Ok(events)
}

fn map_local_echo(
    room_id: String,
    local_echo: LocalEcho,
    current_user_id: &str,
    pending: &PendingOutgoingRegistry,
) -> Result<Vec<Event>, MatrixCoreError> {
    let transaction_id = local_echo.transaction_id.to_string();
    match local_echo.content {
        LocalEchoContent::Event {
            serialized_event,
            send_handle,
            send_error,
        } => outgoing_events_from_local_echo_event(
            room_id,
            transaction_id,
            current_user_id,
            send_handle.created_at,
            &serialized_event,
            send_error.as_ref().map(ToString::to_string),
            pending,
        ),
        LocalEchoContent::React {
            key, applies_to, ..
        } => {
            insert_pending_outgoing_kind(
                pending,
                transaction_id,
                PendingOutgoingKind::Reaction {
                    target_event_id: applies_to.to_string(),
                    key,
                },
            );
            Ok(Vec::new())
        }
        LocalEchoContent::Redaction {
            redacts,
            send_error,
            ..
        } => {
            insert_pending_outgoing_kind(
                pending,
                transaction_id,
                PendingOutgoingKind::Redaction {
                    redacts: redacts.to_string(),
                },
            );

            let mut events = Vec::new();
            if let Some(send_error) = send_error {
                events.push(Event::Error(CoreErrorEvent {
                    operation: Operation::RedactEvent,
                    message: send_error.to_string(),
                }));
            }
            Ok(events)
        }
    }
}

fn outgoing_events_from_local_echo_event(
    room_id: String,
    transaction_id: String,
    current_user_id: &str,
    created_at: MilliSecondsSinceUnixEpoch,
    serialized_event: &SerializableEventContent,
    send_error: Option<String>,
    pending: &PendingOutgoingRegistry,
) -> Result<Vec<Event>, MatrixCoreError> {
    if let Some(target_event_id) =
        edited_message_target_event_id_from_serialized_event(serialized_event)?
    {
        insert_pending_outgoing_kind(
            pending,
            transaction_id,
            PendingOutgoingKind::MessageEdit { target_event_id },
        );

        let mut events = Vec::new();
        if let Some(send_error) = send_error {
            events.push(Event::Error(CoreErrorEvent {
                operation: Operation::EditMessage,
                message: send_error,
            }));
        }

        return Ok(events);
    }

    if let Some(activity) = map_local_message_activity(
        room_id.clone(),
        transaction_id.clone(),
        current_user_id,
        created_at,
        serialized_event,
    )? {
        insert_pending_outgoing_kind(
            pending,
            transaction_id.clone(),
            PendingOutgoingKind::Message,
        );

        let mut events = vec![Event::RoomActivity(Box::new(activity))];
        if let Some(send_error) = send_error {
            events.push(Event::RoomActivityUpdate(RoomActivityUpdate {
                room_id,
                target: RoomActivityUpdateTarget::Transaction { transaction_id },
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

        return Ok(events);
    }

    let Some((target_event_id, key)) =
        reaction_target_and_key_from_serialized_event(serialized_event)?
    else {
        return Ok(Vec::new());
    };

    insert_pending_outgoing_kind(
        pending,
        transaction_id,
        PendingOutgoingKind::Reaction {
            target_event_id,
            key,
        },
    );

    let mut events = Vec::new();
    if let Some(send_error) = send_error {
        events.push(Event::Error(CoreErrorEvent {
            operation: Operation::SendReaction,
            message: send_error,
        }));
    }

    Ok(events)
}

fn insert_pending_outgoing_kind(
    pending: &PendingOutgoingRegistry,
    transaction_id: String,
    kind: PendingOutgoingKind,
) {
    pending
        .lock()
        .expect("lock pending outgoing registry")
        .insert(transaction_id, kind);
}

fn take_pending_outgoing_kind(
    pending: &PendingOutgoingRegistry,
    transaction_id: &str,
) -> Option<PendingOutgoingKind> {
    pending
        .lock()
        .expect("lock pending outgoing registry")
        .remove(transaction_id)
}

#[derive(Debug, PartialEq, Eq)]
struct LocalEchoMessageDetails {
    presentation: RoomMessagePresentationKind,
    body: String,
    relation: Option<RoomActivityRelation>,
}

fn map_local_message_activity(
    room_id: String,
    transaction_id: String,
    current_user_id: &str,
    created_at: MilliSecondsSinceUnixEpoch,
    serialized_event: &SerializableEventContent,
) -> Result<Option<RoomActivity>, MatrixCoreError> {
    let Some(LocalEchoMessageDetails {
        presentation,
        body,
        relation,
    }) = message_presentation_and_body_from_serialized_event(serialized_event)?
    else {
        return Ok(None);
    };

    Ok(Some(RoomActivity {
        room_id,
        activity_id: format!("txn:{transaction_id}"),
        timestamp_ms: created_at.get().into(),
        relation,
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
) -> Result<Option<LocalEchoMessageDetails>, MatrixCoreError> {
    let AnyMessageLikeEventContent::RoomMessage(content) = serialized_event
        .deserialize()
        .map_err(|err| MatrixCoreError::new(format!("failed to deserialize local echo: {err}")))?
    else {
        return Ok(None);
    };

    let Some((presentation, body)) =
        room_message_presentation_and_body_from_message_type(&content.msgtype)
    else {
        return Ok(None);
    };

    let relation = match content.relates_to.as_ref() {
        Some(MessageContentRelation::Reply(reply)) => Some(RoomActivityRelation::Reply {
            in_reply_to_event_id: reply.in_reply_to.event_id.to_string(),
        }),
        Some(MessageContentRelation::Thread(thread)) => Some(RoomActivityRelation::Thread {
            thread_root_event_id: thread.event_id.to_string(),
            in_reply_to_event_id: thread
                .in_reply_to
                .as_ref()
                .map(|in_reply_to| in_reply_to.event_id.to_string()),
            is_falling_back: thread.is_falling_back,
        }),
        _ => None,
    };

    Ok(Some(LocalEchoMessageDetails {
        presentation,
        body,
        relation,
    }))
}

fn edited_message_target_event_id_from_serialized_event(
    serialized_event: &SerializableEventContent,
) -> Result<Option<String>, MatrixCoreError> {
    let AnyMessageLikeEventContent::RoomMessage(content) = serialized_event
        .deserialize()
        .map_err(|err| MatrixCoreError::new(format!("failed to deserialize local echo: {err}")))?
    else {
        return Ok(None);
    };

    match content.relates_to {
        Some(MessageContentRelation::Replacement(replacement)) => {
            Ok(Some(replacement.event_id.to_string()))
        }
        _ => Ok(None),
    }
}

fn reaction_target_and_key_from_serialized_event(
    serialized_event: &SerializableEventContent,
) -> Result<Option<(String, String)>, MatrixCoreError> {
    let AnyMessageLikeEventContent::Reaction(content) = serialized_event
        .deserialize()
        .map_err(|err| MatrixCoreError::new(format!("failed to deserialize local echo: {err}")))?
    else {
        return Ok(None);
    };

    Ok(Some((
        content.relates_to.event_id.to_string(),
        content.relates_to.key,
    )))
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

fn room_message_event_content_without_relation(
    presentation: RoomMessagePresentationKind,
    body: String,
) -> RoomMessageEventContentWithoutRelation {
    match presentation {
        RoomMessagePresentationKind::Text => {
            RoomMessageEventContentWithoutRelation::text_plain(body)
        }
        RoomMessagePresentationKind::Notice => {
            RoomMessageEventContentWithoutRelation::notice_plain(body)
        }
        RoomMessagePresentationKind::Emote => {
            RoomMessageEventContentWithoutRelation::emote_plain(body)
        }
    }
}

fn attach_thread_relation(
    content: &mut RoomMessageEventContent,
    thread: SendMessageThreadTarget,
) -> Result<(), MatrixCoreError> {
    let thread_root_event_id = owned_event_id_from_str(&thread.thread_root_event_id)?;
    let latest_event_id = owned_event_id_from_str(&thread.latest_event_id)?;

    content.relates_to = Some(MessageContentRelation::Thread(Thread::plain(
        thread_root_event_id,
        latest_event_id,
    )));

    Ok(())
}

fn attach_reply_relation(
    content: &mut RoomMessageEventContent,
    reply: SendMessageReplyTarget,
) -> Result<(), MatrixCoreError> {
    let reply_event_id = owned_event_id_from_str(&reply.event_id)?;
    content.relates_to = Some(MessageContentRelation::Reply(Reply::with_event_id(
        reply_event_id,
    )));
    Ok(())
}

fn attach_thread_reply_relation(
    content: &mut RoomMessageEventContent,
    thread: SendMessageThreadTarget,
    reply: SendMessageReplyTarget,
) -> Result<(), MatrixCoreError> {
    let thread_root_event_id = owned_event_id_from_str(&thread.thread_root_event_id)?;
    let _latest_event_id = owned_event_id_from_str(&thread.latest_event_id)?;
    let reply_event_id = owned_event_id_from_str(&reply.event_id)?;
    content.relates_to = Some(MessageContentRelation::Thread(Thread::reply(
        thread_root_event_id,
        reply_event_id,
    )));
    Ok(())
}

fn joined_room_by_id(client: &Client, room_id: &str) -> Result<Room, MatrixCoreError> {
    client
        .joined_rooms()
        .into_iter()
        .find(|room| room.room_id().as_str() == room_id)
        .ok_or_else(|| MatrixCoreError::new(format!("joined room not found: {room_id}")))
}

fn reaction_event_content(
    target_event_id: String,
    key: String,
) -> Result<ReactionEventContent, MatrixCoreError> {
    let target_event_id = <&EventId>::try_from(target_event_id.as_str())
        .map_err(|err| MatrixCoreError::new(format!("invalid Matrix event id: {err}")))?
        .to_owned();

    Ok(ReactionEventContent::new(Annotation::new(
        target_event_id,
        key,
    )))
}

fn owned_event_id_from_str(event_id: &str) -> Result<OwnedEventId, MatrixCoreError> {
    <&EventId>::try_from(event_id)
        .map_err(|err| MatrixCoreError::new(format!("invalid Matrix event id: {err}")))
        .map(|event_id| event_id.to_owned())
}

pub(crate) async fn send_message(
    client: &Client,
    request: SendMessageRequest,
) -> Result<(), MatrixCoreError> {
    let room = joined_room_by_id(client, &request.room_id)?;

    let mut content = room_message_event_content(request.presentation, request.body);
    match (request.thread, request.reply) {
        (None, None) => {}
        (Some(thread), None) => attach_thread_relation(&mut content, thread)?,
        (None, Some(reply)) => attach_reply_relation(&mut content, reply)?,
        (Some(thread), Some(reply)) => attach_thread_reply_relation(&mut content, thread, reply)?,
    }

    room.send_queue()
        .send(AnyMessageLikeEventContent::RoomMessage(content))
        .await
        .map_err(|err| MatrixCoreError::new(format!("failed to send Matrix message: {err}")))?;

    Ok(())
}

pub(crate) async fn send_file(
    client: &Client,
    request: SendFileRequest,
) -> Result<(), MatrixCoreError> {
    let room = joined_room_by_id(client, &request.room_id)?;
    let path = request.path.clone();
    let (bytes, size) = tokio::task::spawn_blocking(move || read_file_bytes(&path))
        .await
        .map_err(|err| MatrixCoreError::new(format!("failed to read file path: {err}")))??;

    let filename = match request.filename {
        Some(filename) => filename,
        None => request
            .path
            .file_name()
            .and_then(|name| name.to_str())
            .map(str::to_owned)
            .ok_or_else(|| {
                MatrixCoreError::new(format!(
                    "file path does not include a file name: {}",
                    request.path.display()
                ))
            })?,
    };
    let mime = match request.content_type {
        Some(content_type) => Mime::from_str(&content_type)
            .map_err(|err| MatrixCoreError::new(format!("invalid file content type: {err}")))?,
        None => mime::APPLICATION_OCTET_STREAM,
    };
    let size = UInt::new(size)
        .ok_or_else(|| MatrixCoreError::new(format!("file size cannot fit Matrix UInt: {size}")))?;
    let config =
        AttachmentConfig::new().info(AttachmentInfo::File(BaseFileInfo { size: Some(size) }));

    room.send_attachment(filename, &mime, bytes, config)
        .await
        .map_err(|err| MatrixCoreError::new(format!("failed to send Matrix file: {err}")))?;

    Ok(())
}

fn read_file_bytes(path: &std::path::Path) -> Result<(Vec<u8>, u64), MatrixCoreError> {
    let metadata = std::fs::metadata(path).map_err(|err| {
        MatrixCoreError::new(format!(
            "failed to read file path {}: {err}",
            path.display()
        ))
    })?;
    if !metadata.is_file() {
        return Err(MatrixCoreError::new(format!(
            "file path is not a regular file: {}",
            path.display()
        )));
    }
    let bytes = std::fs::read(path).map_err(|err| {
        MatrixCoreError::new(format!(
            "failed to read file path {}: {err}",
            path.display()
        ))
    })?;
    Ok((bytes, metadata.len()))
}

pub(crate) async fn send_reaction(
    client: &Client,
    request: SendReactionRequest,
) -> Result<(), MatrixCoreError> {
    let room = joined_room_by_id(client, &request.room_id)?;
    let content = reaction_event_content(request.target_event_id, request.key)?;

    room.send_queue()
        .send(AnyMessageLikeEventContent::Reaction(content))
        .await
        .map_err(|err| MatrixCoreError::new(format!("failed to send Matrix reaction: {err}")))?;

    Ok(())
}

pub(crate) async fn edit_message(
    client: &Client,
    request: EditMessageRequest,
) -> Result<(), MatrixCoreError> {
    let room = joined_room_by_id(client, &request.room_id)?;
    let target_event_id = <&EventId>::try_from(request.target_event_id.as_str())
        .map_err(|err| MatrixCoreError::new(format!("invalid Matrix event id: {err}")))?;
    let new_content =
        room_message_event_content_without_relation(request.presentation, request.body);
    let edit_event = room
        .make_edit_event(target_event_id, EditedContent::RoomMessage(new_content))
        .await
        .map_err(|err| {
            MatrixCoreError::new(format!("failed to create Matrix message edit: {err}"))
        })?;

    room.send_queue().send(edit_event).await.map_err(|err| {
        MatrixCoreError::new(format!("failed to send Matrix message edit: {err}"))
    })?;

    Ok(())
}

pub(crate) async fn redact_event(
    client: &Client,
    request: RedactEventRequest,
) -> Result<(), MatrixCoreError> {
    let room = joined_room_by_id(client, &request.room_id)?;
    let event_id = owned_event_id_from_str(&request.event_id)?;

    room.send_queue()
        .redact(event_id, None)
        .await
        .map_err(|err| MatrixCoreError::new(format!("failed to redact Matrix event: {err}")))?;

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

pub(crate) async fn backfill_room_history(
    client: &Client,
    request: BackfillRoomHistoryRequest,
) -> Result<RoomHistoryBackfill, MatrixCoreError> {
    if !(1..=ROOM_HISTORY_BACKFILL_LIMIT_MAX).contains(&request.limit) {
        return Err(MatrixCoreError::new(
            "room history backfill limit must be between 1 and 50",
        ));
    }

    let room = joined_room_by_id(client, &request.room_id)?;
    let limit = UInt::from(request.limit);
    let mut options = MessagesOptions::backward().from(request.from.as_deref());
    options.limit = limit;
    options.filter.limit = Some(limit);

    let response = room.messages(options).await.map_err(|err| {
        MatrixCoreError::new(format!("failed to backfill Matrix room history: {err}"))
    })?;
    let events = room_history_backfill_events_from_timeline(
        room.room_id().as_str(),
        response.chunk,
        room.clone_info().room_version_rules_or_default().redaction,
    );

    Ok(RoomHistoryBackfill {
        room_id: request.room_id,
        start: response.start,
        end: response.end,
        events,
    })
}

fn room_history_backfill_events_from_timeline(
    room_id: &str,
    chunk: Vec<TimelineEvent>,
    redaction_rules: matrix_sdk::ruma::room_version_rules::RedactionRules,
) -> Vec<RoomHistoryBackfillEvent> {
    let mut reaction_index = BTreeMap::new();

    chunk
        .into_iter()
        .rev()
        .filter_map(|event| {
            room_history_backfill_event_from_timeline_event(
                room_id,
                &event,
                &redaction_rules,
                &mut reaction_index,
            )
        })
        .collect()
}

fn room_activity_from_unable_to_decrypt_timeline_event(
    room_id: &str,
    event: &TimelineEvent,
    utd_info: &UnableToDecryptInfo,
) -> Option<RoomActivity> {
    let event_id = event.event_id()?.to_string();

    Some(RoomActivity {
        room_id: room_id.to_owned(),
        activity_id: event_id.clone(),
        timestamp_ms: event.timestamp()?.get().into(),
        relation: None,
        kind: RoomActivityKind::UnableToDecrypt(RoomUnableToDecryptActivity {
            event_id,
            sender: event.sender()?.to_string(),
            session_id: utd_info.session_id.clone(),
            reason: room_unable_to_decrypt_reason_from_sdk(&utd_info.reason),
        }),
    })
}

fn room_unable_to_decrypt_reason_from_sdk(
    reason: &UnableToDecryptReason,
) -> RoomUnableToDecryptReason {
    match reason {
        UnableToDecryptReason::Unknown => RoomUnableToDecryptReason::Unknown,
        UnableToDecryptReason::MalformedEncryptedEvent => {
            RoomUnableToDecryptReason::MalformedEncryptedEvent
        }
        UnableToDecryptReason::MissingMegolmSession { .. } => {
            RoomUnableToDecryptReason::MissingMegolmSession
        }
        UnableToDecryptReason::UnknownMegolmMessageIndex => {
            RoomUnableToDecryptReason::UnknownMegolmMessageIndex
        }
        UnableToDecryptReason::MegolmDecryptionFailure => {
            RoomUnableToDecryptReason::MegolmDecryptionFailure
        }
        UnableToDecryptReason::PayloadDeserializationFailure => {
            RoomUnableToDecryptReason::PayloadDeserializationFailure
        }
        UnableToDecryptReason::MismatchedIdentityKeys => {
            RoomUnableToDecryptReason::MismatchedIdentityKeys
        }
        UnableToDecryptReason::SenderIdentityNotTrusted(_) => {
            RoomUnableToDecryptReason::SenderIdentityNotTrusted
        }
    }
}

fn room_history_backfill_event_from_timeline_event(
    room_id: &str,
    event: &TimelineEvent,
    redaction_rules: &matrix_sdk::ruma::room_version_rules::RedactionRules,
    reaction_index: &mut BTreeMap<String, ReactionEventIndexEntry>,
) -> Option<RoomHistoryBackfillEvent> {
    if let TimelineEventKind::UnableToDecrypt { utd_info, .. } = &event.kind {
        return room_activity_from_unable_to_decrypt_timeline_event(room_id, event, utd_info)
            .map(|activity| RoomHistoryBackfillEvent::Activity(Box::new(activity)));
    }

    let raw = event.raw();

    if let Ok(event) = raw.deserialize_as_unchecked::<OriginalSyncRoomMessageEvent>() {
        return match room_timeline_event_from_sync_message(room_id, &event)? {
            Event::RoomActivity(activity) => Some(RoomHistoryBackfillEvent::Activity(activity)),
            Event::RoomActivityUpdate(update) => {
                Some(RoomHistoryBackfillEvent::ActivityUpdate(update))
            }
            _ => None,
        };
    }

    if let Ok(event) = raw.deserialize_as_unchecked::<OriginalSyncReactionEvent>() {
        reaction_index.insert(
            event.event_id.to_string(),
            ReactionEventIndexEntry {
                target_event_id: event.content.relates_to.event_id.to_string(),
                key: event.content.relates_to.key.clone(),
            },
        );
        return Some(RoomHistoryBackfillEvent::ActivityUpdate(
            room_activity_update_from_sync_reaction(room_id, &event),
        ));
    }

    if let Ok(event) = raw.deserialize_as_unchecked::<OriginalSyncRoomRedactionEvent>() {
        let redacted_event_id = event.redacts(redaction_rules).to_string();
        if let Some(entry) = reaction_index.remove(redacted_event_id.as_str()) {
            return Some(RoomHistoryBackfillEvent::ActivityUpdate(
                room_activity_update_from_indexed_reaction_redaction(
                    room_id,
                    redacted_event_id,
                    &event,
                    entry,
                ),
            ));
        }

        return Some(RoomHistoryBackfillEvent::ActivityUpdate(
            room_activity_update_from_sync_redaction(room_id, &event, redacted_event_id),
        ));
    }

    if let Ok(event) = raw.deserialize_as_unchecked::<OriginalSyncRoomTopicEvent>() {
        return Some(RoomHistoryBackfillEvent::Activity(Box::new(
            room_activity_from_sync_topic(room_id, &event),
        )));
    }

    if let Ok(event) = raw.deserialize_as_unchecked::<OriginalSyncRoomMemberEvent>() {
        return Some(RoomHistoryBackfillEvent::Activity(Box::new(
            room_activity_from_sync_member(room_id, &event),
        )));
    }

    None
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
    use std::collections::BTreeMap;
    use std::sync::{Arc, Mutex};

    use super::{
        LocalEchoMessageDetails, PendingOutgoingKind, PendingOutgoingRegistry, ReactionEventIndex,
        ReactionEventIndexEntry, attach_reply_relation, attach_thread_relation,
        attach_thread_reply_relation, backfill_room_history, cancelled_state,
        direct_user_ids_by_room_from_content, fetch_room_media, insert_pending_outgoing_kind,
        joined_room_members, joined_room_members_no_sync, map_cross_signing_status, map_decimals,
        map_e2ee_backup_state, map_e2ee_recovery_state, map_emoji_array,
        map_emoji_short_auth_string, map_local_echo, map_local_message_activity,
        map_send_queue_update, message_presentation_and_body_from_serialized_event,
        moderation_capabilities, outgoing_events_from_local_echo_event, owned_event_id_from_str,
        reaction_event_content, read_file_bytes, room_activity_from_sync_member,
        room_activity_from_sync_message, room_activity_from_sync_topic,
        room_activity_update_from_indexed_reaction_redaction,
        room_activity_update_from_sync_reaction, room_activity_update_from_sync_redaction,
        room_history_backfill_event_from_timeline_event,
        room_history_backfill_events_from_timeline, room_list_update_is_relevant,
        room_media_request_parameters, room_members_from_parts, room_message_event_content,
        room_message_event_content_without_relation, room_read_state_from_parts,
        room_read_update_is_relevant, room_tag_summaries_from_tags,
        room_timeline_event_from_sync_message, room_typing_from_sync_typing,
        room_unable_to_decrypt_reason_from_sdk, sorted_room_member_summaries_from_parts,
        verification_flow_id_from_to_device, verification_request_from_to_device,
        verification_requested_state_event,
    };

    #[test]
    fn moderation_capabilities_require_independent_sdk_permissions_and_lower_target_power() {
        let actor = <&matrix_sdk::ruma::UserId>::try_from("@actor:example.org")
            .expect("valid actor user id");
        let lower = <&matrix_sdk::ruma::UserId>::try_from("@lower:example.org")
            .expect("valid lower user id");
        let equal = <&matrix_sdk::ruma::UserId>::try_from("@equal:example.org")
            .expect("valid equal user id");
        let higher = <&matrix_sdk::ruma::UserId>::try_from("@higher:example.org")
            .expect("valid higher user id");

        assert_eq!(
            moderation_capabilities(actor, lower, true, false, &50, &0),
            (true, false)
        );
        assert_eq!(
            moderation_capabilities(actor, lower, false, true, &50, &0),
            (false, true)
        );
        assert_eq!(
            moderation_capabilities(actor, lower, false, false, &50, &0),
            (false, false)
        );
        assert_eq!(
            moderation_capabilities(actor, actor, true, true, &50, &50),
            (false, false)
        );
        assert_eq!(
            moderation_capabilities(actor, equal, true, true, &50, &50),
            (false, false)
        );
        assert_eq!(
            moderation_capabilities(actor, higher, true, true, &50, &100),
            (false, false)
        );
    }

    #[test]
    fn joined_member_snapshots_use_sdk_members_and_default_power_levels() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("build tokio runtime");

        runtime.block_on(async {
            let client = client_with_joined_members(serde_json::json!([
                {
                    "type": "m.room.member",
                    "state_key": "@alice:example.org",
                    "sender": "@alice:example.org",
                    "event_id": "$alice:example.org",
                    "origin_server_ts": 1u64,
                    "content": { "membership": "join", "displayname": "Alice" }
                },
                {
                    "type": "m.room.member",
                    "state_key": "@bob:example.org",
                    "sender": "@bob:example.org",
                    "event_id": "$bob:example.org",
                    "origin_server_ts": 2u64,
                    "content": { "membership": "join", "displayname": "Bob" }
                }
            ]))
            .await;
            let room = client
                .get_room(room_id().as_ref())
                .expect("joined room from sync response");
            let actor = client.user_id().expect("authenticated user");

            assert_eq!(
                joined_room_members(actor, &room)
                    .await
                    .expect("synced members"),
                joined_room_members_no_sync(actor, &room)
                    .await
                    .expect("cached members")
            );
            assert_eq!(
                joined_room_members_no_sync(actor, &room)
                    .await
                    .expect("cached members"),
                RoomMembers {
                    room_id: "!room:example.org".to_owned(),
                    members: vec![
                        RoomMemberSummary {
                            user_id: "@alice:example.org".to_owned(),
                            display_name: Some("Alice".to_owned()),
                            can_kick: false,
                            can_ban: false,
                        },
                        RoomMemberSummary {
                            user_id: "@bob:example.org".to_owned(),
                            display_name: Some("Bob".to_owned()),
                            can_kick: false,
                            can_ban: false,
                        },
                    ],
                }
            );
        });
    }

    #[test]
    fn joined_member_snapshots_reject_a_missing_authenticated_member() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("build tokio runtime");

        runtime.block_on(async {
            let client = client_with_joined_members(serde_json::json!([{
                "type": "m.room.member",
                "state_key": "@bob:example.org",
                "sender": "@bob:example.org",
                "event_id": "$bob:example.org",
                "origin_server_ts": 1u64,
                "content": { "membership": "join" }
            }]))
            .await;
            let room = client
                .get_room(room_id().as_ref())
                .expect("joined room from sync response");
            let err =
                joined_room_members_no_sync(client.user_id().expect("authenticated user"), &room)
                    .await
                    .expect_err("authenticated member must be present");

            assert_eq!(
                err.to_string(),
                "authenticated user is not a joined member of room: !room:example.org"
            );
        });
    }

    #[test]
    fn room_history_backfill_maps_unable_to_decrypt_event() {
        let event = TimelineEvent::from_utd(
            Raw::new(&serde_json::json!({
                "type": "m.room.encrypted",
                "content": {"algorithm": "m.megolm.v1.aes-sha2"},
                "event_id": "$encrypted1:example.org",
                "sender": "@alice:example.org",
                "origin_server_ts": 1700000000000u64
            }))
            .expect("serialize raw encrypted timeline event")
            .cast_unchecked(),
            UnableToDecryptInfo {
                session_id: Some("SESSIONID".to_owned()),
                reason: UnableToDecryptReason::MissingMegolmSession {
                    withheld_code: None,
                },
            },
        );

        assert_eq!(
            room_history_backfill_event_from_timeline_event(
                "!room:example.org",
                &event,
                &RedactionRules::V1,
                &mut BTreeMap::new(),
            ),
            Some(RoomHistoryBackfillEvent::Activity(Box::new(RoomActivity {
                room_id: "!room:example.org".to_owned(),
                activity_id: "$encrypted1:example.org".to_owned(),
                timestamp_ms: 1_700_000_000_000,
                relation: None,
                kind: RoomActivityKind::UnableToDecrypt(RoomUnableToDecryptActivity {
                    event_id: "$encrypted1:example.org".to_owned(),
                    sender: "@alice:example.org".to_owned(),
                    session_id: Some("SESSIONID".to_owned()),
                    reason: RoomUnableToDecryptReason::MissingMegolmSession,
                }),
            })))
        );
    }

    #[test]
    fn unable_to_decrypt_reason_mapping_covers_sdk_reasons() {
        assert_eq!(
            room_unable_to_decrypt_reason_from_sdk(&UnableToDecryptReason::MegolmDecryptionFailure),
            RoomUnableToDecryptReason::MegolmDecryptionFailure,
        );
    }

    #[test]
    fn room_history_backfill_events_deliver_unable_to_decrypt_activity() {
        let event = TimelineEvent::from_utd(
            Raw::new(&serde_json::json!({
                "type": "m.room.encrypted",
                "content": {"algorithm": "m.megolm.v1.aes-sha2"},
                "event_id": "$encrypted-vector:example.org",
                "sender": "@alice:example.org",
                "origin_server_ts": 1700000000000u64
            }))
            .expect("serialize raw encrypted timeline event")
            .cast_unchecked(),
            UnableToDecryptInfo {
                session_id: Some("SESSIONID".to_owned()),
                reason: UnableToDecryptReason::MissingMegolmSession {
                    withheld_code: None,
                },
            },
        );

        assert_eq!(
            room_history_backfill_events_from_timeline(
                "!room:example.org",
                vec![event],
                RedactionRules::V1,
            ),
            vec![RoomHistoryBackfillEvent::Activity(Box::new(RoomActivity {
                room_id: "!room:example.org".to_owned(),
                activity_id: "$encrypted-vector:example.org".to_owned(),
                timestamp_ms: 1_700_000_000_000,
                relation: None,
                kind: RoomActivityKind::UnableToDecrypt(RoomUnableToDecryptActivity {
                    event_id: "$encrypted-vector:example.org".to_owned(),
                    sender: "@alice:example.org".to_owned(),
                    session_id: Some("SESSIONID".to_owned()),
                    reason: RoomUnableToDecryptReason::MissingMegolmSession,
                }),
            }))]
        );
    }

    #[test]
    fn read_file_bytes_reads_regular_files_and_rejects_missing_and_directories() {
        let temp = tempfile::tempdir().expect("temporary directory");
        let file = temp.path().join("report.txt");
        std::fs::write(&file, b"report").expect("write file");
        assert_eq!(
            read_file_bytes(&file).expect("read file"),
            (b"report".to_vec(), 6)
        );

        let missing = read_file_bytes(&temp.path().join("missing.txt")).expect_err("missing file");
        assert!(missing.to_string().contains("failed to read file path"));

        let directory = read_file_bytes(temp.path()).expect_err("directory is not a file");
        assert!(
            directory
                .to_string()
                .contains("file path is not a regular file")
        );
    }
    use crate::api::{
        BackfillRoomHistoryRequest, CoreErrorEvent, E2eeBackupState, E2eeCrossSigningStatus,
        E2eeRecoveryState, Event, FetchRoomMediaRequest, Operation,
        ROOM_HISTORY_BACKFILL_LIMIT_MAX, RoomActivity, RoomActivityKind, RoomActivityRelation,
        RoomActivityUpdate, RoomActivityUpdateKind, RoomActivityUpdateTarget, RoomAudioActivity,
        RoomAudioInfo, RoomEncryptedFileHashes, RoomEncryptedFileKey, RoomFileActivity,
        RoomFileInfo, RoomHistoryBackfillEvent, RoomImageActivity, RoomImageInfo, RoomMediaFormat,
        RoomMediaKind, RoomMediaSource, RoomMemberSummary, RoomMembers, RoomMessageActivity,
        RoomMessagePresentationKind, RoomMessageSendState, RoomReadState, RoomStateNoticeActivity,
        RoomStateNoticeContent, RoomTagSummary, RoomTyping, RoomUnableToDecryptActivity,
        RoomUnableToDecryptReason, RoomVideoActivity, RoomVideoInfo, SendMessageReplyTarget,
        SendMessageThreadTarget, UnsupportedRoomMessageActivity, VerificationDecimals,
        VerificationEmoji, VerificationRequest, VerificationState, VerificationStateEvent,
    };
    use matrix_sdk::{
        Client, SessionMeta,
        deserialized_responses::{TimelineEvent, UnableToDecryptInfo, UnableToDecryptReason},
        encryption::{
            backups::BackupState,
            recovery::RecoveryState,
            verification::{Emoji as MatrixEmoji, EmojiShortAuthString},
        },
        media::{MediaFormat, MediaThumbnailSettings},
        ruma::{
            MilliSecondsSinceUnixEpoch, OwnedEventId, OwnedRoomId, OwnedTransactionId, OwnedUserId,
            UInt,
            api::client::media::get_content_thumbnail::v3::Method,
            events::{
                AnyMessageLikeEventContent, ToDeviceEvent,
                direct::DirectEventContent,
                key::verification::request::ToDeviceKeyVerificationRequestEventContent,
                reaction::OriginalSyncReactionEvent,
                room::MediaSource,
                room::{
                    member::OriginalSyncRoomMemberEvent,
                    message::{
                        MessageType, OriginalSyncRoomMessageEvent, ReplacementMetadata,
                        RoomMessageEventContent,
                    },
                    redaction::OriginalSyncRoomRedactionEvent,
                    topic::OriginalSyncRoomTopicEvent,
                },
                tag::{TagInfo, TagName, Tags},
                typing::SyncTypingEvent,
            },
            room_version_rules::RedactionRules,
            serde::Raw,
        },
        send_queue::{LocalEchoContent, RoomSendQueue, RoomSendQueueUpdate, SendQueueUpdate},
        store::SerializableEventContent,
    };
    use matrix_sdk_base::cross_process_lock::CrossProcessLockConfig;
    use matrix_sdk_base::{
        BaseClient, DmRoomDefinition, RoomInfoNotableUpdateReasons, RoomState, ThreadingSupport,
        read_receipts::{LatestReadReceipt, RoomReadReceipts},
        store::StoreConfig,
    };
    use matrix_sdk_crypto::CrossSigningStatus;
    use matrix_sdk_test::ruma_response_from_json;

    fn room_id() -> OwnedRoomId {
        OwnedRoomId::try_from("!room:example.org").expect("valid room id")
    }

    fn transaction_id() -> OwnedTransactionId {
        "txn-1".into()
    }

    fn pending_outgoing_registry() -> PendingOutgoingRegistry {
        Arc::new(Mutex::new(BTreeMap::new()))
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

    fn build_test_client() -> Client {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("build tokio runtime");

        runtime.block_on(async {
            Client::builder()
                .homeserver_url("http://localhost")
                .build()
                .await
                .expect("build matrix client")
        })
    }

    fn build_test_room_send_queue_context() -> (tokio::runtime::Runtime, Client, RoomSendQueue) {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("build tokio runtime");

        let (client, room_send_queue) = runtime.block_on(async {
            let base_client = BaseClient::new(
                StoreConfig::new(CrossProcessLockConfig::SingleProcess),
                ThreadingSupport::Disabled,
                DmRoomDefinition::MatrixSpec,
            );
            base_client
                .activate(
                    SessionMeta {
                        user_id: OwnedUserId::try_from("@alice:example.org")
                            .expect("valid user id"),
                        device_id: "DEVICE".into(),
                    },
                    matrix_sdk::store::RoomLoadSettings::default(),
                    None,
                )
                .await
                .expect("activate base client");
            base_client.get_or_create_room(room_id().as_ref(), RoomState::Joined);

            let client = Client::builder()
                .homeserver_url("http://localhost")
                .base_client(base_client)
                .build()
                .await
                .expect("build matrix client");
            let room = client.get_room(room_id().as_ref()).expect("joined room");

            (client, room.send_queue())
        });

        (runtime, client, room_send_queue)
    }

    async fn client_with_joined_members(member_events: serde_json::Value) -> Client {
        let base_client = BaseClient::new(
            StoreConfig::new(CrossProcessLockConfig::SingleProcess),
            ThreadingSupport::Disabled,
            DmRoomDefinition::MatrixSpec,
        );
        base_client
            .activate(
                SessionMeta {
                    user_id: OwnedUserId::try_from("@alice:example.org").expect("valid user id"),
                    device_id: "DEVICE".into(),
                },
                matrix_sdk::store::RoomLoadSettings::default(),
                None,
            )
            .await
            .expect("activate base client");
        let response = ruma_response_from_json(&serde_json::json!({
            "next_batch": "s1",
            "rooms": { "join": { "!room:example.org": {
                "state": { "events": member_events },
                "timeline": { "events": [], "limited": false, "prev_batch": "p1" }
            }}}
        }));
        base_client
            .receive_sync_response(response)
            .await
            .expect("receive sync response");
        Client::builder()
            .homeserver_url("http://localhost")
            .base_client(base_client)
            .build()
            .await
            .expect("build matrix client")
    }

    fn room_media_request(source: RoomMediaSource) -> FetchRoomMediaRequest {
        FetchRoomMediaRequest {
            room_id: "!room:example.org".to_owned(),
            activity_id: "event:$image:example.org".to_owned(),
            event_id: "$image:example.org".to_owned(),
            source,
            format: RoomMediaFormat::Thumbnail {
                width: 160,
                height: 120,
            },
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

    fn timeline_event(event: serde_json::Value) -> TimelineEvent {
        TimelineEvent::from_plaintext(
            Raw::new(&event)
                .expect("serialize raw timeline event")
                .cast_unchecked(),
        )
    }

    #[test]
    fn e2ee_recovery_state_mapping_covers_all_sdk_variants() {
        for (state, expected) in [
            (RecoveryState::Unknown, E2eeRecoveryState::Unknown),
            (RecoveryState::Enabled, E2eeRecoveryState::Enabled),
            (RecoveryState::Disabled, E2eeRecoveryState::Disabled),
            (RecoveryState::Incomplete, E2eeRecoveryState::Incomplete),
        ] {
            assert_eq!(map_e2ee_recovery_state(state), expected);
        }
    }

    #[test]
    fn e2ee_backup_state_mapping_covers_all_sdk_variants() {
        for (state, expected) in [
            (BackupState::Unknown, E2eeBackupState::Unknown),
            (BackupState::Creating, E2eeBackupState::Creating),
            (BackupState::Enabling, E2eeBackupState::Enabling),
            (BackupState::Resuming, E2eeBackupState::Resuming),
            (BackupState::Enabled, E2eeBackupState::Enabled),
            (BackupState::Downloading, E2eeBackupState::Downloading),
            (BackupState::Disabling, E2eeBackupState::Disabling),
        ] {
            assert_eq!(map_e2ee_backup_state(state), expected);
        }
    }

    #[test]
    fn room_media_request_parameters_map_plain_thumbnail_requests() {
        let params = room_media_request_parameters(
            &RoomMediaSource::Plain {
                url: "mxc://example.org/image".to_owned(),
            },
            &RoomMediaFormat::Thumbnail {
                width: 160,
                height: 120,
            },
        )
        .expect("build media request parameters");

        match params.source {
            MediaSource::Plain(uri) => assert_eq!(uri.as_str(), "mxc://example.org/image"),
            other => panic!("expected plain MXC source, got {other:?}"),
        }
        match params.format {
            MediaFormat::Thumbnail(settings) => {
                assert_eq!(
                    settings.method,
                    MediaThumbnailSettings::with_method(
                        Method::Scale,
                        UInt::new(160).expect("valid width"),
                        UInt::new(120).expect("valid height"),
                    )
                    .method
                );
                assert_eq!(settings.width, UInt::new(160).expect("valid width"));
                assert_eq!(settings.height, UInt::new(120).expect("valid height"));
                assert!(!settings.animated);
            }
            other => panic!("expected thumbnail media format, got {other:?}"),
        }
    }

    #[test]
    fn room_media_request_parameters_map_plain_file_requests() {
        let params = room_media_request_parameters(
            &RoomMediaSource::Plain {
                url: "mxc://example.org/file".to_owned(),
            },
            &RoomMediaFormat::File,
        )
        .expect("build media request parameters");

        match params.source {
            MediaSource::Plain(uri) => assert_eq!(uri.as_str(), "mxc://example.org/file"),
            other => panic!("expected plain MXC source, got {other:?}"),
        }
        assert!(matches!(params.format, MediaFormat::File));
    }

    #[test]
    fn fetch_room_media_returns_typed_failure_for_invalid_mxc() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("build tokio runtime");
        let client = build_test_client();

        let media = runtime.block_on(async {
            fetch_room_media(
                &client,
                room_media_request(RoomMediaSource::Plain {
                    url: "not-an-mxc-uri".to_owned(),
                }),
            )
            .await
        });

        assert_eq!(media.room_id, "!room:example.org");
        assert_eq!(media.activity_id, "event:$image:example.org");
        assert_eq!(media.event_id, "$image:example.org");
        match media.kind {
            RoomMediaKind::Failed { format, message } => {
                assert_eq!(
                    format,
                    RoomMediaFormat::Thumbnail {
                        width: 160,
                        height: 120,
                    }
                );
                assert!(
                    message.starts_with("invalid media MXC URI:"),
                    "unexpected error message: {message}"
                );
            }
            other => panic!("expected failed room media, got {other:?}"),
        }
    }

    #[test]
    fn room_media_request_parameters_map_encrypted_thumbnail_requests() {
        let params = room_media_request_parameters(
            &encrypted_media_source("mxc://example.org/image"),
            &RoomMediaFormat::Thumbnail {
                width: 160,
                height: 120,
            },
        )
        .expect("build encrypted media request parameters");

        match params.source {
            MediaSource::Encrypted(file) => {
                let json = serde_json::to_value(file.as_ref())
                    .expect("serialize encrypted media request source");
                assert_eq!(json["url"], "mxc://example.org/image");
                assert_eq!(json["v"], "v2");
                assert_eq!(json["key"]["kty"], "oct");
                assert_eq!(json["key"]["alg"], "A256CTR");
                assert_eq!(
                    json["key"]["k"],
                    "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA"
                );
                assert_eq!(
                    json["hashes"]["sha256"],
                    "AQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQE"
                );
            }
            other => panic!("expected encrypted media source, got {other:?}"),
        }
    }

    #[test]
    fn room_media_request_parameters_map_encrypted_file_requests() {
        let params = room_media_request_parameters(
            &encrypted_media_source("mxc://example.org/audio"),
            &RoomMediaFormat::File,
        )
        .expect("build encrypted media request parameters");

        assert!(matches!(params.format, MediaFormat::File));
        match params.source {
            MediaSource::Encrypted(file) => {
                let json = serde_json::to_value(file.as_ref())
                    .expect("serialize encrypted media request source");
                assert_eq!(json["url"], "mxc://example.org/audio");
                assert_eq!(json["v"], "v2");
                assert_eq!(json["key"]["kty"], "oct");
                assert_eq!(json["key"]["alg"], "A256CTR");
                assert_eq!(
                    json["key"]["k"],
                    "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA"
                );
                assert_eq!(
                    json["hashes"]["sha256"],
                    "AQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQE"
                );
            }
            other => panic!("expected encrypted media source, got {other:?}"),
        }
    }

    #[test]
    fn fetch_room_media_file_returns_typed_failure_for_invalid_mxc() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("build tokio runtime");
        let client = build_test_client();

        let media = runtime.block_on(async {
            fetch_room_media(
                &client,
                FetchRoomMediaRequest {
                    room_id: "!room:example.org".to_owned(),
                    activity_id: "event:$file:example.org".to_owned(),
                    event_id: "$file:example.org".to_owned(),
                    source: RoomMediaSource::Plain {
                        url: "not-an-mxc-uri".to_owned(),
                    },
                    format: RoomMediaFormat::File,
                },
            )
            .await
        });

        assert_eq!(media.room_id, "!room:example.org");
        assert_eq!(media.activity_id, "event:$file:example.org");
        assert_eq!(media.event_id, "$file:example.org");
        match media.kind {
            RoomMediaKind::Failed { format, message } => {
                assert_eq!(format, RoomMediaFormat::File);
                assert!(
                    message.starts_with("invalid media MXC URI:"),
                    "unexpected error message: {message}"
                );
            }
            other => panic!("expected failed room media, got {other:?}"),
        }
    }

    #[test]
    fn room_media_request_parameters_reject_zero_thumbnail_dimensions() {
        for format in [
            RoomMediaFormat::Thumbnail {
                width: 0,
                height: 120,
            },
            RoomMediaFormat::Thumbnail {
                width: 160,
                height: 0,
            },
        ] {
            let err = room_media_request_parameters(
                &RoomMediaSource::Plain {
                    url: "mxc://example.org/image".to_owned(),
                },
                &format,
            )
            .expect_err("reject zero thumbnail dimensions");

            assert_eq!(
                err.to_string(),
                "thumbnail width and height must be greater than zero"
            );
        }
    }

    #[test]
    fn encrypted_media_request_parameters_reject_invalid_metadata() {
        let err = room_media_request_parameters(
            &RoomMediaSource::Encrypted {
                url: "mxc://example.org/image".to_owned(),
                key: RoomEncryptedFileKey {
                    kty: "oct".to_owned(),
                    key_ops: vec!["encrypt".to_owned(), "decrypt".to_owned()],
                    alg: "A256CTR".to_owned(),
                    k: "not-valid-base64".to_owned(),
                    ext: true,
                },
                iv: "AgICAgICAgICAgICAgICAg".to_owned(),
                hashes: encrypted_file_hashes(),
                v: "v2".to_owned(),
            },
            &RoomMediaFormat::Thumbnail {
                width: 160,
                height: 120,
            },
        )
        .expect_err("reject invalid encrypted media metadata");

        assert!(
            err.to_string()
                .starts_with("invalid encrypted media metadata:"),
            "unexpected error message: {err}"
        );
    }

    #[test]
    fn cross_signing_status_mapping_copies_flags_and_completeness() {
        for (status, expected) in [
            (
                CrossSigningStatus {
                    has_master: true,
                    has_self_signing: true,
                    has_user_signing: true,
                },
                E2eeCrossSigningStatus {
                    has_master: true,
                    has_self_signing: true,
                    has_user_signing: true,
                    complete: true,
                },
            ),
            (
                CrossSigningStatus {
                    has_master: true,
                    has_self_signing: false,
                    has_user_signing: true,
                },
                E2eeCrossSigningStatus {
                    has_master: true,
                    has_self_signing: false,
                    has_user_signing: true,
                    complete: false,
                },
            ),
        ] {
            assert_eq!(map_cross_signing_status(status), expected);
        }
    }

    #[test]
    fn room_member_sorting_uses_case_insensitive_display_name_then_user_id() {
        assert_eq!(
            sorted_room_member_summaries_from_parts(vec![
                ("@charlie:example.org".to_owned(), None),
                ("@delta:example.org".to_owned(), Some("beta".to_owned())),
                ("@echo:example.org".to_owned(), Some("alice".to_owned())),
                ("@beta:example.org".to_owned(), Some(String::new())),
                ("@alpha:example.org".to_owned(), Some("ALICE".to_owned())),
            ]),
            vec![
                RoomMemberSummary {
                    user_id: "@beta:example.org".to_owned(),
                    display_name: Some(String::new()),
                    can_kick: false,
                    can_ban: false,
                },
                RoomMemberSummary {
                    user_id: "@charlie:example.org".to_owned(),
                    display_name: None,
                    can_kick: false,
                    can_ban: false,
                },
                RoomMemberSummary {
                    user_id: "@alpha:example.org".to_owned(),
                    display_name: Some("ALICE".to_owned()),
                    can_kick: false,
                    can_ban: false,
                },
                RoomMemberSummary {
                    user_id: "@echo:example.org".to_owned(),
                    display_name: Some("alice".to_owned()),
                    can_kick: false,
                    can_ban: false,
                },
                RoomMemberSummary {
                    user_id: "@delta:example.org".to_owned(),
                    display_name: Some("beta".to_owned()),
                    can_kick: false,
                    can_ban: false,
                },
            ]
        );
    }

    #[test]
    fn room_members_from_parts_breaks_duplicate_display_names_by_user_id() {
        assert_eq!(
            room_members_from_parts(
                "!room:example.org",
                vec![
                    ("@zulu:example.org".to_owned(), Some("Same".to_owned())),
                    ("@alpha:example.org".to_owned(), Some("same".to_owned())),
                    ("@mike:example.org".to_owned(), Some("same".to_owned())),
                ],
            )
            .members,
            vec![
                RoomMemberSummary {
                    user_id: "@alpha:example.org".to_owned(),
                    display_name: Some("same".to_owned()),
                    can_kick: false,
                    can_ban: false,
                },
                RoomMemberSummary {
                    user_id: "@mike:example.org".to_owned(),
                    display_name: Some("same".to_owned()),
                    can_kick: false,
                    can_ban: false,
                },
                RoomMemberSummary {
                    user_id: "@zulu:example.org".to_owned(),
                    display_name: Some("Same".to_owned()),
                    can_kick: false,
                    can_ban: false,
                },
            ]
        );
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
                relation: None,
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
                relation: None,
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
                relation: None,
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
    fn text_sync_message_replacement_maps_to_room_activity_update() {
        let event: OriginalSyncRoomMessageEvent = serde_json::from_value(serde_json::json!({
            "type": "m.room.message",
            "content": {
                "msgtype": "m.text",
                "body": "* edited body",
                "m.new_content": {
                    "msgtype": "m.text",
                    "body": "edited body"
                },
                "m.relates_to": {
                    "rel_type": "m.replace",
                    "event_id": "$original:example.org"
                }
            },
            "event_id": "$edit:example.org",
            "sender": "@alice:example.org",
            "origin_server_ts": 1700000001000u64
        }))
        .expect("deserialize sync text replacement event");

        assert_eq!(
            room_timeline_event_from_sync_message("!room:example.org", &event),
            Some(Event::RoomActivityUpdate(RoomActivityUpdate {
                room_id: "!room:example.org".to_owned(),
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
            }))
        );
    }

    #[test]
    fn notice_and_emote_sync_message_replacements_preserve_presentation() {
        let notice_event: OriginalSyncRoomMessageEvent =
            serde_json::from_value(serde_json::json!({
                "type": "m.room.message",
                "content": {
                    "msgtype": "m.notice",
                    "body": "* heads up",
                    "m.new_content": {
                        "msgtype": "m.notice",
                        "body": "heads up"
                    },
                    "m.relates_to": {
                        "rel_type": "m.replace",
                        "event_id": "$notice:example.org"
                    }
                },
                "event_id": "$notice-edit:example.org",
                "sender": "@alice:example.org",
                "origin_server_ts": 1700000001001u64
            }))
            .expect("deserialize sync notice replacement event");
        let emote_event: OriginalSyncRoomMessageEvent = serde_json::from_value(serde_json::json!({
            "type": "m.room.message",
            "content": {
                "msgtype": "m.emote",
                "body": "* waves",
                "m.new_content": {
                    "msgtype": "m.emote",
                    "body": "waves"
                },
                "m.relates_to": {
                    "rel_type": "m.replace",
                    "event_id": "$emote:example.org"
                }
            },
            "event_id": "$emote-edit:example.org",
            "sender": "@alice:example.org",
            "origin_server_ts": 1700000001002u64
        }))
        .expect("deserialize sync emote replacement event");

        assert_eq!(
            room_timeline_event_from_sync_message("!room:example.org", &notice_event),
            Some(Event::RoomActivityUpdate(RoomActivityUpdate {
                room_id: "!room:example.org".to_owned(),
                target: RoomActivityUpdateTarget::Event {
                    event_id: "$notice:example.org".to_owned(),
                },
                kind: RoomActivityUpdateKind::MessageReplaced {
                    replacement_event_id: "$notice-edit:example.org".to_owned(),
                    sender: "@alice:example.org".to_owned(),
                    timestamp_ms: 1_700_000_001_001,
                    presentation: RoomMessagePresentationKind::Notice,
                    body: "heads up".to_owned(),
                },
            }))
        );
        assert_eq!(
            room_timeline_event_from_sync_message("!room:example.org", &emote_event),
            Some(Event::RoomActivityUpdate(RoomActivityUpdate {
                room_id: "!room:example.org".to_owned(),
                target: RoomActivityUpdateTarget::Event {
                    event_id: "$emote:example.org".to_owned(),
                },
                kind: RoomActivityUpdateKind::MessageReplaced {
                    replacement_event_id: "$emote-edit:example.org".to_owned(),
                    sender: "@alice:example.org".to_owned(),
                    timestamp_ms: 1_700_000_001_002,
                    presentation: RoomMessagePresentationKind::Emote,
                    body: "waves".to_owned(),
                },
            }))
        );
    }

    #[test]
    fn replacement_sync_message_with_thread_fallback_maps_only_to_update() {
        let event: OriginalSyncRoomMessageEvent = serde_json::from_value(serde_json::json!({
            "type": "m.room.message",
            "content": {
                "msgtype": "m.text",
                "body": "* thread reply edited",
                "m.new_content": {
                    "msgtype": "m.text",
                    "body": "thread reply edited",
                    "m.relates_to": {
                        "rel_type": "m.thread",
                        "event_id": "$thread-root:example.org",
                        "is_falling_back": false,
                        "m.in_reply_to": {
                            "event_id": "$reply-target:example.org"
                        }
                    }
                },
                "m.relates_to": {
                    "rel_type": "m.replace",
                    "event_id": "$thread-reply:example.org"
                }
            },
            "event_id": "$thread-edit:example.org",
            "sender": "@alice:example.org",
            "origin_server_ts": 1700000001003u64
        }))
        .expect("deserialize sync threaded replacement event");

        assert_eq!(
            room_timeline_event_from_sync_message("!room:example.org", &event),
            Some(Event::RoomActivityUpdate(RoomActivityUpdate {
                room_id: "!room:example.org".to_owned(),
                target: RoomActivityUpdateTarget::Event {
                    event_id: "$thread-reply:example.org".to_owned(),
                },
                kind: RoomActivityUpdateKind::MessageReplaced {
                    replacement_event_id: "$thread-edit:example.org".to_owned(),
                    sender: "@alice:example.org".to_owned(),
                    timestamp_ms: 1_700_000_001_003,
                    presentation: RoomMessagePresentationKind::Text,
                    body: "thread reply edited".to_owned(),
                },
            }))
        );
    }

    #[test]
    fn sync_reaction_maps_to_room_activity_update() {
        let event: OriginalSyncReactionEvent = serde_json::from_value(serde_json::json!({
            "type": "m.reaction",
            "content": {
                "m.relates_to": {
                    "rel_type": "m.annotation",
                    "event_id": "$original:example.org",
                    "key": "+1"
                }
            },
            "event_id": "$reaction:example.org",
            "sender": "@alice:example.org",
            "origin_server_ts": 1700000001500u64
        }))
        .expect("deserialize sync reaction event");

        assert_eq!(
            room_activity_update_from_sync_reaction("!room:example.org", &event),
            RoomActivityUpdate {
                room_id: "!room:example.org".to_owned(),
                target: RoomActivityUpdateTarget::Event {
                    event_id: "$original:example.org".to_owned(),
                },
                kind: RoomActivityUpdateKind::ReactionAdded {
                    reaction_event_id: "$reaction:example.org".to_owned(),
                    sender: "@alice:example.org".to_owned(),
                    timestamp_ms: 1_700_000_001_500,
                    key: "+1".to_owned(),
                },
            }
        );
    }

    #[test]
    fn room_backfill_message_with_top_level_room_id_maps_to_activity() {
        let event = timeline_event(serde_json::json!({
            "type": "m.room.message",
            "room_id": "!history:example.org",
            "content": {
                "msgtype": "m.text",
                "body": "older message"
            },
            "event_id": "$history:example.org",
            "sender": "@alice:example.org",
            "origin_server_ts": 1700000000100u64
        }));

        let mapped = room_history_backfill_event_from_timeline_event(
            "!room:example.org",
            &event,
            &RedactionRules::V1,
            &mut BTreeMap::new(),
        );

        assert_eq!(
            mapped,
            Some(RoomHistoryBackfillEvent::Activity(Box::new(RoomActivity {
                room_id: "!room:example.org".to_owned(),
                activity_id: "event:$history:example.org".to_owned(),
                timestamp_ms: 1_700_000_000_100,
                relation: None,
                kind: RoomActivityKind::Message(RoomMessageActivity {
                    event_id: Some("$history:example.org".to_owned()),
                    transaction_id: None,
                    sender: "@alice:example.org".to_owned(),
                    presentation: RoomMessagePresentationKind::Text,
                    body: "older message".to_owned(),
                    send_state: RoomMessageSendState::Sent,
                }),
            })))
        );
    }

    #[test]
    fn room_backfill_edit_maps_to_message_replaced_update() {
        let event = timeline_event(serde_json::json!({
            "type": "m.room.message",
            "room_id": "!history:example.org",
            "content": {
                "msgtype": "m.text",
                "body": "* edited body",
                "m.new_content": {
                    "msgtype": "m.text",
                    "body": "edited body"
                },
                "m.relates_to": {
                    "rel_type": "m.replace",
                    "event_id": "$original:example.org"
                }
            },
            "event_id": "$edit:example.org",
            "sender": "@alice:example.org",
            "origin_server_ts": 1700000001000u64
        }));

        let mapped = room_history_backfill_event_from_timeline_event(
            "!room:example.org",
            &event,
            &RedactionRules::V1,
            &mut BTreeMap::new(),
        );

        assert_eq!(
            mapped,
            Some(RoomHistoryBackfillEvent::ActivityUpdate(
                RoomActivityUpdate {
                    room_id: "!room:example.org".to_owned(),
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
                }
            ))
        );
    }

    #[test]
    fn room_backfill_reaction_maps_to_reaction_added_update() {
        let event = timeline_event(serde_json::json!({
            "type": "m.reaction",
            "room_id": "!history:example.org",
            "content": {
                "m.relates_to": {
                    "rel_type": "m.annotation",
                    "event_id": "$original:example.org",
                    "key": "heart"
                }
            },
            "event_id": "$reaction:example.org",
            "sender": "@alice:example.org",
            "origin_server_ts": 1700000001500u64
        }));

        let mapped = room_history_backfill_event_from_timeline_event(
            "!room:example.org",
            &event,
            &RedactionRules::V1,
            &mut BTreeMap::new(),
        );

        assert_eq!(
            mapped,
            Some(RoomHistoryBackfillEvent::ActivityUpdate(
                RoomActivityUpdate {
                    room_id: "!room:example.org".to_owned(),
                    target: RoomActivityUpdateTarget::Event {
                        event_id: "$original:example.org".to_owned(),
                    },
                    kind: RoomActivityUpdateKind::ReactionAdded {
                        reaction_event_id: "$reaction:example.org".to_owned(),
                        sender: "@alice:example.org".to_owned(),
                        timestamp_ms: 1_700_000_001_500,
                        key: "heart".to_owned(),
                    },
                }
            ))
        );
    }

    #[test]
    fn room_backfill_normalizes_backward_page_to_oldest_first() {
        let events = room_history_backfill_events_from_timeline(
            "!room:example.org",
            vec![
                timeline_event(serde_json::json!({
                    "type": "m.room.message",
                    "room_id": "!history:example.org",
                    "content": {
                        "msgtype": "m.text",
                        "body": "newer"
                    },
                    "event_id": "$newer:example.org",
                    "sender": "@alice:example.org",
                    "origin_server_ts": 1700000000200u64
                })),
                timeline_event(serde_json::json!({
                    "type": "m.room.message",
                    "room_id": "!history:example.org",
                    "content": {
                        "msgtype": "m.text",
                        "body": "older"
                    },
                    "event_id": "$older:example.org",
                    "sender": "@alice:example.org",
                    "origin_server_ts": 1700000000100u64
                })),
            ],
            RedactionRules::V1,
        );

        assert_eq!(
            events,
            vec![
                RoomHistoryBackfillEvent::Activity(Box::new(RoomActivity {
                    room_id: "!room:example.org".to_owned(),
                    activity_id: "event:$older:example.org".to_owned(),
                    timestamp_ms: 1_700_000_000_100,
                    relation: None,
                    kind: RoomActivityKind::Message(RoomMessageActivity {
                        event_id: Some("$older:example.org".to_owned()),
                        transaction_id: None,
                        sender: "@alice:example.org".to_owned(),
                        presentation: RoomMessagePresentationKind::Text,
                        body: "older".to_owned(),
                        send_state: RoomMessageSendState::Sent,
                    }),
                })),
                RoomHistoryBackfillEvent::Activity(Box::new(RoomActivity {
                    room_id: "!room:example.org".to_owned(),
                    activity_id: "event:$newer:example.org".to_owned(),
                    timestamp_ms: 1_700_000_000_200,
                    relation: None,
                    kind: RoomActivityKind::Message(RoomMessageActivity {
                        event_id: Some("$newer:example.org".to_owned()),
                        transaction_id: None,
                        sender: "@alice:example.org".to_owned(),
                        presentation: RoomMessagePresentationKind::Text,
                        body: "newer".to_owned(),
                        send_state: RoomMessageSendState::Sent,
                    }),
                })),
            ]
        );
    }

    #[test]
    fn reaction_redaction_maps_to_reaction_removed_when_indexed() {
        let reaction_event: OriginalSyncReactionEvent = serde_json::from_value(serde_json::json!({
            "type": "m.reaction",
            "content": {
                "m.relates_to": {
                    "rel_type": "m.annotation",
                    "event_id": "$original:example.org",
                    "key": "heart"
                }
            },
            "event_id": "$reaction:example.org",
            "sender": "@alice:example.org",
            "origin_server_ts": 1700000001501u64
        }))
        .expect("deserialize sync reaction event");
        let redaction_event: OriginalSyncRoomRedactionEvent =
            serde_json::from_value(serde_json::json!({
                "type": "m.room.redaction",
                "content": {},
                "redacts": "$reaction:example.org",
                "event_id": "$redaction:example.org",
                "sender": "@bob:example.org",
                "origin_server_ts": 1700000002003u64
            }))
            .expect("deserialize redaction event");

        let mut index = ReactionEventIndex::default();
        index.live.insert(
            reaction_event.event_id.to_string(),
            ReactionEventIndexEntry {
                target_event_id: reaction_event.content.relates_to.event_id.to_string(),
                key: reaction_event.content.relates_to.key.clone(),
            },
        );

        let reaction_event_id = redaction_event.redacts(&RedactionRules::V1).to_string();
        let entry = index
            .live
            .remove(reaction_event_id.as_str())
            .expect("reaction indexed");
        assert_eq!(
            room_activity_update_from_indexed_reaction_redaction(
                "!room:example.org",
                reaction_event_id,
                &redaction_event,
                entry,
            ),
            RoomActivityUpdate {
                room_id: "!room:example.org".to_owned(),
                target: RoomActivityUpdateTarget::Event {
                    event_id: "$original:example.org".to_owned(),
                },
                kind: RoomActivityUpdateKind::ReactionRemoved {
                    reaction_event_id: "$reaction:example.org".to_owned(),
                    redaction_event_id: "$redaction:example.org".to_owned(),
                    key: "heart".to_owned(),
                },
            }
        );
    }

    #[test]
    fn room_backfill_rejects_invalid_limits_before_matrix_request() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("build tokio runtime");
        let client = build_test_client();

        for limit in [0, ROOM_HISTORY_BACKFILL_LIMIT_MAX + 1] {
            let err = runtime
                .block_on(async {
                    backfill_room_history(
                        &client,
                        BackfillRoomHistoryRequest {
                            room_id: "!room:example.org".to_owned(),
                            from: None,
                            limit,
                        },
                    )
                    .await
                })
                .expect_err("reject invalid room history backfill limit");

            assert_eq!(
                err.to_string(),
                "room history backfill limit must be between 1 and 50"
            );
        }
    }

    #[test]
    fn reaction_redaction_before_reaction_suppresses_later_add() {
        let redaction_event: OriginalSyncRoomRedactionEvent =
            serde_json::from_value(serde_json::json!({
                "type": "m.room.redaction",
                "content": {},
                "redacts": "$reaction:example.org",
                "event_id": "$redaction:example.org",
                "sender": "@bob:example.org",
                "origin_server_ts": 1700000002004u64
            }))
            .expect("deserialize redaction event");
        let reaction_event: OriginalSyncReactionEvent = serde_json::from_value(serde_json::json!({
            "type": "m.reaction",
            "content": {
                "m.relates_to": {
                    "rel_type": "m.annotation",
                    "event_id": "$original:example.org",
                    "key": "eyes"
                }
            },
            "event_id": "$reaction:example.org",
            "sender": "@alice:example.org",
            "origin_server_ts": 1700000001502u64
        }))
        .expect("deserialize sync reaction event");

        let mut index = ReactionEventIndex::default();
        let redacted_event_id = redaction_event.redacts(&RedactionRules::V1).to_string();
        index
            .pending_redactions
            .insert(redacted_event_id, redaction_event.event_id.to_string());

        let reaction_event_id = reaction_event.event_id.to_string();
        let update = if index
            .pending_redactions
            .remove(reaction_event_id.as_str())
            .is_some()
        {
            None
        } else {
            index.live.insert(
                reaction_event_id,
                ReactionEventIndexEntry {
                    target_event_id: reaction_event.content.relates_to.event_id.to_string(),
                    key: reaction_event.content.relates_to.key.clone(),
                },
            );
            Some(room_activity_update_from_sync_reaction(
                "!room:example.org",
                &reaction_event,
            ))
        };

        assert_eq!(update, None);
        assert!(
            index.live.is_empty(),
            "suppressed reaction must not be indexed"
        );
        assert!(
            index.pending_redactions.is_empty(),
            "pending redaction tombstone must be cleared when the reaction arrives"
        );
    }

    #[test]
    fn unknown_redaction_still_maps_to_activity_redacted() {
        let redaction_event: OriginalSyncRoomRedactionEvent =
            serde_json::from_value(serde_json::json!({
                "type": "m.room.redaction",
                "content": {},
                "redacts": "$unknown:example.org",
                "event_id": "$redaction:example.org",
                "sender": "@alice:example.org",
                "origin_server_ts": 1700000002005u64
            }))
            .expect("deserialize redaction event");

        assert_eq!(
            room_activity_update_from_sync_redaction(
                "!room:example.org",
                &redaction_event,
                redaction_event.redacts(&RedactionRules::V1).to_string(),
            ),
            RoomActivityUpdate {
                room_id: "!room:example.org".to_owned(),
                target: RoomActivityUpdateTarget::Event {
                    event_id: "$unknown:example.org".to_owned(),
                },
                kind: RoomActivityUpdateKind::ActivityRedacted {
                    redaction_event_id: "$redaction:example.org".to_owned(),
                },
            }
        );
    }

    #[test]
    fn legacy_redaction_maps_to_room_activity_update() {
        let event: OriginalSyncRoomRedactionEvent = serde_json::from_value(serde_json::json!({
            "type": "m.room.redaction",
            "content": {},
            "redacts": "$original:example.org",
            "event_id": "$redaction:example.org",
            "sender": "@alice:example.org",
            "origin_server_ts": 1700000002000u64
        }))
        .expect("deserialize legacy redaction event");

        assert_eq!(
            room_activity_update_from_sync_redaction(
                "!room:example.org",
                &event,
                event.redacts(&RedactionRules::V1).to_string(),
            ),
            RoomActivityUpdate {
                room_id: "!room:example.org".to_owned(),
                target: RoomActivityUpdateTarget::Event {
                    event_id: "$original:example.org".to_owned(),
                },
                kind: RoomActivityUpdateKind::ActivityRedacted {
                    redaction_event_id: "$redaction:example.org".to_owned(),
                },
            }
        );
    }

    #[test]
    fn v11_redaction_maps_to_room_activity_update() {
        let event: OriginalSyncRoomRedactionEvent = serde_json::from_value(serde_json::json!({
            "type": "m.room.redaction",
            "content": {
                "redacts": "$original:example.org"
            },
            "event_id": "$redaction:example.org",
            "sender": "@alice:example.org",
            "origin_server_ts": 1700000002001u64
        }))
        .expect("deserialize v11 redaction event");

        assert_eq!(
            room_activity_update_from_sync_redaction(
                "!room:example.org",
                &event,
                event.redacts(&RedactionRules::V11).to_string(),
            ),
            RoomActivityUpdate {
                room_id: "!room:example.org".to_owned(),
                target: RoomActivityUpdateTarget::Event {
                    event_id: "$original:example.org".to_owned(),
                },
                kind: RoomActivityUpdateKind::ActivityRedacted {
                    redaction_event_id: "$redaction:example.org".to_owned(),
                },
            }
        );
    }

    #[test]
    fn redaction_targeting_edited_message_uses_original_event_id() {
        let replacement_event: OriginalSyncRoomMessageEvent =
            serde_json::from_value(serde_json::json!({
                "type": "m.room.message",
                "content": {
                    "msgtype": "m.text",
                    "body": "* edited body",
                    "m.new_content": {
                        "msgtype": "m.text",
                        "body": "edited body"
                    },
                    "m.relates_to": {
                        "rel_type": "m.replace",
                        "event_id": "$original:example.org"
                    }
                },
                "event_id": "$edit:example.org",
                "sender": "@alice:example.org",
                "origin_server_ts": 1700000001000u64
            }))
            .expect("deserialize replacement event");
        let redaction_event: OriginalSyncRoomRedactionEvent =
            serde_json::from_value(serde_json::json!({
                "type": "m.room.redaction",
                "content": {},
                "redacts": "$original:example.org",
                "event_id": "$redaction:example.org",
                "sender": "@alice:example.org",
                "origin_server_ts": 1700000002002u64
            }))
            .expect("deserialize redaction event");

        assert_eq!(
            room_timeline_event_from_sync_message("!room:example.org", &replacement_event),
            Some(Event::RoomActivityUpdate(RoomActivityUpdate {
                room_id: "!room:example.org".to_owned(),
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
            }))
        );
        assert_eq!(
            room_activity_update_from_sync_redaction(
                "!room:example.org",
                &redaction_event,
                redaction_event.redacts(&RedactionRules::V1).to_string(),
            ),
            RoomActivityUpdate {
                room_id: "!room:example.org".to_owned(),
                target: RoomActivityUpdateTarget::Event {
                    event_id: "$original:example.org".to_owned(),
                },
                kind: RoomActivityUpdateKind::ActivityRedacted {
                    redaction_event_id: "$redaction:example.org".to_owned(),
                },
            }
        );
    }

    #[test]
    fn unsupported_sync_message_replacement_is_ignored() {
        let image_event: OriginalSyncRoomMessageEvent = serde_json::from_value(serde_json::json!({
            "type": "m.room.message",
            "content": {
                "msgtype": "m.image",
                "body": "* diagram.png",
                "url": "mxc://example.org/fallback-image",
                "m.new_content": {
                    "msgtype": "m.image",
                    "body": "diagram.png",
                    "url": "mxc://example.org/image"
                },
                "m.relates_to": {
                    "rel_type": "m.replace",
                    "event_id": "$image:example.org"
                }
            },
            "event_id": "$image-edit:example.org",
            "sender": "@alice:example.org",
            "origin_server_ts": 1700000001004u64
        }))
        .expect("deserialize sync image replacement event");

        assert_eq!(
            room_timeline_event_from_sync_message("!room:example.org", &image_event),
            None
        );
    }

    #[test]
    fn thread_sync_text_message_maps_thread_relation() {
        let event: OriginalSyncRoomMessageEvent = serde_json::from_value(serde_json::json!({
            "type": "m.room.message",
            "content": {
                "msgtype": "m.text",
                "body": "hello from thread",
                "m.relates_to": {
                    "rel_type": "m.thread",
                    "event_id": "$thread-root:example.org",
                    "is_falling_back": true,
                    "m.in_reply_to": {
                        "event_id": "$reply-target:example.org"
                    }
                }
            },
            "event_id": "$threaded:example.org",
            "sender": "@alice:example.org",
            "origin_server_ts": 1700000000005u64
        }))
        .expect("deserialize threaded sync text event");

        assert_eq!(
            room_activity_from_sync_message("!room:example.org", &event),
            Some(RoomActivity {
                room_id: "!room:example.org".to_owned(),
                activity_id: "event:$threaded:example.org".to_owned(),
                timestamp_ms: 1_700_000_000_005,
                relation: Some(RoomActivityRelation::Thread {
                    thread_root_event_id: "$thread-root:example.org".to_owned(),
                    in_reply_to_event_id: Some("$reply-target:example.org".to_owned()),
                    is_falling_back: true,
                }),
                kind: RoomActivityKind::Message(RoomMessageActivity {
                    event_id: Some("$threaded:example.org".to_owned()),
                    transaction_id: None,
                    sender: "@alice:example.org".to_owned(),
                    presentation: RoomMessagePresentationKind::Text,
                    body: "hello from thread".to_owned(),
                    send_state: RoomMessageSendState::Sent,
                }),
            })
        );
    }

    #[test]
    fn ordinary_reply_sync_message_maps_reply_relation_not_thread() {
        let event: OriginalSyncRoomMessageEvent = serde_json::from_value(serde_json::json!({
            "type": "m.room.message",
            "content": {
                "msgtype": "m.text",
                "body": "reply only",
                "m.relates_to": {
                    "m.in_reply_to": {
                        "event_id": "$parent:example.org"
                    }
                }
            },
            "event_id": "$reply:example.org",
            "sender": "@alice:example.org",
            "origin_server_ts": 1700000000010u64
        }))
        .expect("deserialize reply sync text event");

        assert_eq!(
            room_activity_from_sync_message("!room:example.org", &event),
            Some(RoomActivity {
                room_id: "!room:example.org".to_owned(),
                activity_id: "event:$reply:example.org".to_owned(),
                timestamp_ms: 1_700_000_000_010,
                relation: Some(RoomActivityRelation::Reply {
                    in_reply_to_event_id: "$parent:example.org".to_owned(),
                }),
                kind: RoomActivityKind::Message(RoomMessageActivity {
                    event_id: Some("$reply:example.org".to_owned()),
                    transaction_id: None,
                    sender: "@alice:example.org".to_owned(),
                    presentation: RoomMessagePresentationKind::Text,
                    body: "reply only".to_owned(),
                    send_state: RoomMessageSendState::Sent,
                }),
            })
        );
    }

    #[test]
    fn threaded_sync_image_mapping_preserves_image_kind_and_relation() {
        let event: OriginalSyncRoomMessageEvent = serde_json::from_value(serde_json::json!({
            "type": "m.room.message",
            "content": {
                "msgtype": "m.image",
                "body": "diagram.png",
                "url": "mxc://example.org/image",
                "m.relates_to": {
                    "rel_type": "m.thread",
                    "event_id": "$thread-root:example.org",
                    "is_falling_back": false
                }
            },
            "event_id": "$image-thread:example.org",
            "sender": "@alice:example.org",
            "origin_server_ts": 1700000000015u64
        }))
        .expect("deserialize threaded sync image event");

        assert_eq!(
            room_activity_from_sync_message("!room:example.org", &event),
            Some(RoomActivity {
                room_id: "!room:example.org".to_owned(),
                activity_id: "event:$image-thread:example.org".to_owned(),
                timestamp_ms: 1_700_000_000_015,
                relation: Some(RoomActivityRelation::Thread {
                    thread_root_event_id: "$thread-root:example.org".to_owned(),
                    in_reply_to_event_id: None,
                    is_falling_back: false,
                }),
                kind: RoomActivityKind::Image(Box::new(RoomImageActivity {
                    event_id: "$image-thread:example.org".to_owned(),
                    sender: "@alice:example.org".to_owned(),
                    body: "diagram.png".to_owned(),
                    filename: None,
                    source: RoomMediaSource::Plain {
                        url: "mxc://example.org/image".to_owned(),
                    },
                    info: None,
                })),
            })
        );
    }

    #[test]
    fn received_sync_image_mapping_produces_typed_image_activity() {
        let event: OriginalSyncRoomMessageEvent = serde_json::from_value(serde_json::json!({
            "type": "m.room.message",
            "content": {
                "msgtype": "m.image",
                "body": "diagram.png",
                "url": "mxc://example.org/image",
                "info": {
                    "mimetype": "image/png",
                    "size": 4096u64,
                    "w": 800u64,
                    "h": 600u64,
                    "is_animated": true
                }
            },
            "event_id": "$event:example.org",
            "sender": "@alice:example.org",
            "origin_server_ts": 1700000000000u64
        }))
        .expect("deserialize sync image event");

        assert_eq!(
            room_activity_from_sync_message("!room:example.org", &event),
            Some(RoomActivity {
                room_id: "!room:example.org".to_owned(),
                activity_id: "event:$event:example.org".to_owned(),
                timestamp_ms: 1_700_000_000_000,
                relation: None,
                kind: RoomActivityKind::Image(Box::new(RoomImageActivity {
                    event_id: "$event:example.org".to_owned(),
                    sender: "@alice:example.org".to_owned(),
                    body: "diagram.png".to_owned(),
                    filename: None,
                    source: RoomMediaSource::Plain {
                        url: "mxc://example.org/image".to_owned(),
                    },
                    info: Some(RoomImageInfo {
                        mimetype: Some("image/png".to_owned()),
                        size: Some(4096),
                        width: Some(800),
                        height: Some(600),
                        is_animated: Some(true),
                        thumbnail_source: None,
                    }),
                })),
            })
        );
    }

    #[test]
    fn received_sync_image_mapping_preserves_distinct_filename() {
        let event: OriginalSyncRoomMessageEvent = serde_json::from_value(serde_json::json!({
            "type": "m.room.message",
            "content": {
                "msgtype": "m.image",
                "body": "Quarterly diagram",
                "filename": "diagram.png",
                "url": "mxc://example.org/image"
            },
            "event_id": "$image:example.org",
            "sender": "@alice:example.org",
            "origin_server_ts": 1700000000050u64
        }))
        .expect("deserialize sync image event with filename");

        assert_eq!(
            room_activity_from_sync_message("!room:example.org", &event),
            Some(RoomActivity {
                room_id: "!room:example.org".to_owned(),
                activity_id: "event:$image:example.org".to_owned(),
                timestamp_ms: 1_700_000_000_050,
                relation: None,
                kind: RoomActivityKind::Image(Box::new(RoomImageActivity {
                    event_id: "$image:example.org".to_owned(),
                    sender: "@alice:example.org".to_owned(),
                    body: "Quarterly diagram".to_owned(),
                    filename: Some("diagram.png".to_owned()),
                    source: RoomMediaSource::Plain {
                        url: "mxc://example.org/image".to_owned(),
                    },
                    info: None,
                })),
            })
        );
    }

    #[test]
    fn encrypted_sync_image_mapping_preserves_file_and_thumbnail_metadata() {
        let event: OriginalSyncRoomMessageEvent = serde_json::from_value(serde_json::json!({
            "type": "m.room.message",
            "content": {
                "msgtype": "m.image",
                "body": "secret.png",
                "file": {
                    "url": "mxc://example.org/full",
                    "key": {
                        "kty": "oct",
                        "key_ops": ["decrypt", "encrypt"],
                        "alg": "A256CTR",
                        "k": "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA",
                        "ext": true
                    },
                    "iv": "AgICAgICAgICAgICAgICAg",
                    "hashes": {
                        "sha256": "AQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQE"
                    },
                    "v": "v2"
                },
                "info": {
                    "thumbnail_file": {
                        "url": "mxc://example.org/thumb",
                        "key": {
                            "kty": "oct",
                            "key_ops": ["decrypt", "encrypt"],
                            "alg": "A256CTR",
                            "k": "AwMDAwMDAwMDAwMDAwMDAwMDAwMDAwMDAwMDAwMDAwM",
                            "ext": true
                        },
                        "iv": "BAQEBAQEBAQEBAQEBAQEBA",
                        "hashes": {
                            "sha256": "BAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQ"
                        },
                        "v": "v2"
                    }
                }
            },
            "event_id": "$encrypted:example.org",
            "sender": "@alice:example.org",
            "origin_server_ts": 1700000000060u64
        }))
        .expect("deserialize encrypted sync image event");

        assert_eq!(
            room_activity_from_sync_message("!room:example.org", &event),
            Some(RoomActivity {
                room_id: "!room:example.org".to_owned(),
                activity_id: "event:$encrypted:example.org".to_owned(),
                timestamp_ms: 1_700_000_000_060,
                relation: None,
                kind: RoomActivityKind::Image(Box::new(RoomImageActivity {
                    event_id: "$encrypted:example.org".to_owned(),
                    sender: "@alice:example.org".to_owned(),
                    body: "secret.png".to_owned(),
                    filename: None,
                    source: RoomMediaSource::Encrypted {
                        url: "mxc://example.org/full".to_owned(),
                        key: RoomEncryptedFileKey {
                            kty: "oct".to_owned(),
                            key_ops: vec!["decrypt".to_owned(), "encrypt".to_owned()],
                            alg: "A256CTR".to_owned(),
                            k: "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA".to_owned(),
                            ext: true,
                        },
                        iv: "AgICAgICAgICAgICAgICAg".to_owned(),
                        hashes: RoomEncryptedFileHashes {
                            sha256: "AQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQE".to_owned(),
                        },
                        v: "v2".to_owned(),
                    },
                    info: Some(RoomImageInfo {
                        mimetype: None,
                        size: None,
                        width: None,
                        height: None,
                        is_animated: None,
                        thumbnail_source: Some(RoomMediaSource::Encrypted {
                            url: "mxc://example.org/thumb".to_owned(),
                            key: RoomEncryptedFileKey {
                                kty: "oct".to_owned(),
                                key_ops: vec!["decrypt".to_owned(), "encrypt".to_owned()],
                                alg: "A256CTR".to_owned(),
                                k: "AwMDAwMDAwMDAwMDAwMDAwMDAwMDAwMDAwMDAwMDAwM".to_owned(),
                                ext: true,
                            },
                            iv: "BAQEBAQEBAQEBAQEBAQEBA".to_owned(),
                            hashes: RoomEncryptedFileHashes {
                                sha256: "BAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQ".to_owned(),
                            },
                            v: "v2".to_owned(),
                        }),
                    }),
                })),
            })
        );
    }

    #[test]
    fn plain_sync_image_mapping_preserves_thumbnail_source() {
        let event: OriginalSyncRoomMessageEvent = serde_json::from_value(serde_json::json!({
            "type": "m.room.message",
            "content": {
                "msgtype": "m.image",
                "body": "diagram.png",
                "url": "mxc://example.org/image",
                "info": {
                    "thumbnail_url": "mxc://example.org/thumb"
                }
            },
            "event_id": "$image-thumb:example.org",
            "sender": "@alice:example.org",
            "origin_server_ts": 1700000000070u64
        }))
        .expect("deserialize sync image event with thumbnail");

        assert_eq!(
            room_activity_from_sync_message("!room:example.org", &event),
            Some(RoomActivity {
                room_id: "!room:example.org".to_owned(),
                activity_id: "event:$image-thumb:example.org".to_owned(),
                timestamp_ms: 1_700_000_000_070,
                relation: None,
                kind: RoomActivityKind::Image(Box::new(RoomImageActivity {
                    event_id: "$image-thumb:example.org".to_owned(),
                    sender: "@alice:example.org".to_owned(),
                    body: "diagram.png".to_owned(),
                    filename: None,
                    source: RoomMediaSource::Plain {
                        url: "mxc://example.org/image".to_owned(),
                    },
                    info: Some(RoomImageInfo {
                        mimetype: None,
                        size: None,
                        width: None,
                        height: None,
                        is_animated: None,
                        thumbnail_source: Some(RoomMediaSource::Plain {
                            url: "mxc://example.org/thumb".to_owned(),
                        }),
                    }),
                })),
            })
        );
    }

    #[test]
    fn sync_audio_mapping_produces_typed_audio_activity() {
        let event: OriginalSyncRoomMessageEvent = serde_json::from_value(serde_json::json!({
            "type": "m.room.message",
            "content": {
                "msgtype": "m.audio",
                "body": "Voice note",
                "filename": "voice-note.ogg",
                "url": "mxc://example.org/audio",
                "info": {
                    "mimetype": "audio/ogg",
                    "size": 8192u64,
                    "duration": 12000u64
                }
            },
            "event_id": "$audio:example.org",
            "sender": "@alice:example.org",
            "origin_server_ts": 1700000000100u64
        }))
        .expect("deserialize sync audio event");

        assert_eq!(
            room_activity_from_sync_message("!room:example.org", &event),
            Some(RoomActivity {
                room_id: "!room:example.org".to_owned(),
                activity_id: "event:$audio:example.org".to_owned(),
                timestamp_ms: 1_700_000_000_100,
                relation: None,
                kind: RoomActivityKind::Audio(Box::new(RoomAudioActivity {
                    event_id: "$audio:example.org".to_owned(),
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
            })
        );
    }

    #[test]
    fn sync_file_mapping_produces_typed_file_activity() {
        let event: OriginalSyncRoomMessageEvent = serde_json::from_value(serde_json::json!({
            "type": "m.room.message",
            "content": {
                "msgtype": "m.file",
                "body": "Quarterly report",
                "filename": "report.pdf",
                "url": "mxc://example.org/file",
                "info": {
                    "mimetype": "application/pdf",
                    "size": 65536u64
                }
            },
            "event_id": "$file:example.org",
            "sender": "@alice:example.org",
            "origin_server_ts": 1700000000200u64
        }))
        .expect("deserialize sync file event");

        assert_eq!(
            room_activity_from_sync_message("!room:example.org", &event),
            Some(RoomActivity {
                room_id: "!room:example.org".to_owned(),
                activity_id: "event:$file:example.org".to_owned(),
                timestamp_ms: 1_700_000_000_200,
                relation: None,
                kind: RoomActivityKind::File(Box::new(RoomFileActivity {
                    event_id: "$file:example.org".to_owned(),
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
            })
        );
    }

    #[test]
    fn sync_video_mapping_produces_typed_activity() {
        let event: OriginalSyncRoomMessageEvent = serde_json::from_value(serde_json::json!({
            "type": "m.room.message",
            "content": {
                "msgtype": "m.video",
                "body": "demo.mp4",
                "filename": "clip.mp4",
                "url": "mxc://example.org/video",
                "info": {
                    "mimetype": "video/mp4",
                    "size": 12345u64,
                    "duration": 12000u64,
                    "w": 640u64,
                    "h": 480u64,
                    "thumbnail_url": "mxc://example.org/video-thumbnail"
                }
            },
            "event_id": "$video:example.org",
            "sender": "@alice:example.org",
            "origin_server_ts": 1700000000300u64
        }))
        .expect("deserialize sync video event");

        assert_eq!(
            room_activity_from_sync_message("!room:example.org", &event),
            Some(RoomActivity {
                room_id: "!room:example.org".to_owned(),
                activity_id: "event:$video:example.org".to_owned(),
                timestamp_ms: 1_700_000_000_300,
                relation: None,
                kind: RoomActivityKind::Video(Box::new(RoomVideoActivity {
                    event_id: "$video:example.org".to_owned(),
                    sender: "@alice:example.org".to_owned(),
                    body: "demo.mp4".to_owned(),
                    filename: Some("clip.mp4".to_owned()),
                    source: RoomMediaSource::Plain {
                        url: "mxc://example.org/video".to_owned()
                    },
                    info: Some(RoomVideoInfo {
                        mimetype: Some("video/mp4".to_owned()),
                        size: Some(12345),
                        duration_ms: Some(12000),
                        width: Some(640),
                        height: Some(480),
                        thumbnail_source: Some(RoomMediaSource::Plain {
                            url: "mxc://example.org/video-thumbnail".to_owned()
                        }),
                    }),
                })),
            })
        );
    }

    #[test]
    fn location_sync_message_mapping_remains_ignored() {
        let event: OriginalSyncRoomMessageEvent = serde_json::from_value(serde_json::json!({
            "type": "m.room.message",
            "content": {
                "msgtype": "m.location",
                "body": "geo:37.786971,-122.399677",
                "geo_uri": "geo:37.786971,-122.399677"
            },
            "event_id": "$location:example.org",
            "sender": "@alice:example.org",
            "origin_server_ts": 1700000000400u64
        }))
        .expect("deserialize sync location event");

        assert_eq!(
            room_activity_from_sync_message("!room:example.org", &event),
            None
        );
    }

    #[test]
    fn custom_sync_message_mapping_produces_unsupported_placeholder() {
        let custom_msgtype = MessageType::new(
            "com.example.custom",
            "Custom body".to_owned(),
            serde_json::Map::new(),
        )
        .expect("build custom message type");
        let content = serde_json::to_value(RoomMessageEventContent::new(custom_msgtype))
            .expect("serialize custom content");
        let event: OriginalSyncRoomMessageEvent = serde_json::from_value(serde_json::json!({
            "type": "m.room.message",
            "content": content,
            "event_id": "$custom:example.org",
            "sender": "@alice:example.org",
            "origin_server_ts": 1700000000450u64
        }))
        .expect("deserialize custom sync message event");

        assert_eq!(
            room_activity_from_sync_message("!room:example.org", &event),
            Some(RoomActivity {
                room_id: "!room:example.org".to_owned(),
                activity_id: "event:$custom:example.org".to_owned(),
                timestamp_ms: 1_700_000_000_450,
                relation: None,
                kind: RoomActivityKind::UnsupportedMessage(UnsupportedRoomMessageActivity {
                    event_id: "$custom:example.org".to_owned(),
                    sender: "@alice:example.org".to_owned(),
                    msgtype: "com.example.custom".to_owned(),
                    body: "Custom body".to_owned(),
                }),
            })
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
                relation: None,
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
            assert!(
                room_read_update_is_relevant(reasons),
                "{reasons:?} should be relevant"
            );
        }
    }

    #[test]
    fn room_read_update_relevance_ignores_room_summary_metadata_reasons() {
        for reasons in [
            RoomInfoNotableUpdateReasons::DISPLAY_NAME,
            RoomInfoNotableUpdateReasons::MEMBERSHIP,
            RoomInfoNotableUpdateReasons::NONE,
        ] {
            assert!(
                !room_read_update_is_relevant(reasons),
                "{reasons:?} should be ignored"
            );
        }
    }

    #[test]
    fn room_list_update_relevance_tracks_membership_display_name_and_catch_all_metadata() {
        for reasons in [
            RoomInfoNotableUpdateReasons::MEMBERSHIP,
            RoomInfoNotableUpdateReasons::DISPLAY_NAME,
            RoomInfoNotableUpdateReasons::NONE,
        ] {
            assert!(
                room_list_update_is_relevant(reasons),
                "{reasons:?} should be relevant"
            );
        }
    }

    #[test]
    fn room_list_update_relevance_ignores_non_room_list_reasons() {
        for reasons in [
            RoomInfoNotableUpdateReasons::READ_RECEIPT,
            RoomInfoNotableUpdateReasons::UNREAD_MARKER,
            RoomInfoNotableUpdateReasons::RECENCY_STAMP,
            RoomInfoNotableUpdateReasons::LATEST_EVENT,
        ] {
            assert!(
                !room_list_update_is_relevant(reasons),
                "{reasons:?} should be ignored"
            );
        }
    }

    #[test]
    fn direct_user_ids_by_room_from_content_inverts_m_direct() {
        let content = DirectEventContent::from_iter([
            (
                OwnedUserId::try_from("@bob:example.org")
                    .expect("valid user id")
                    .into(),
                vec![
                    OwnedRoomId::try_from("!shared:example.org").expect("valid room id"),
                    OwnedRoomId::try_from("!bob:example.org").expect("valid room id"),
                ],
            ),
            (
                OwnedUserId::try_from("@alice:example.org")
                    .expect("valid user id")
                    .into(),
                vec![
                    OwnedRoomId::try_from("!shared:example.org").expect("valid room id"),
                    OwnedRoomId::try_from("!shared:example.org").expect("valid room id"),
                    OwnedRoomId::try_from("!alice:example.org").expect("valid room id"),
                ],
            ),
        ]);

        assert_eq!(
            direct_user_ids_by_room_from_content(Some(&content)),
            BTreeMap::from([
                (
                    "!alice:example.org".to_owned(),
                    vec!["@alice:example.org".to_owned()],
                ),
                (
                    "!bob:example.org".to_owned(),
                    vec!["@bob:example.org".to_owned()],
                ),
                (
                    "!shared:example.org".to_owned(),
                    vec![
                        "@alice:example.org".to_owned(),
                        "@bob:example.org".to_owned(),
                    ],
                ),
            ])
        );
    }

    #[test]
    fn room_tag_summaries_from_tags_preserve_name_and_order() {
        let tags: Tags = BTreeMap::from([
            (
                TagName::Favorite,
                serde_json::from_value(serde_json::json!({ "order": 0.25 }))
                    .expect("favorite tag info"),
            ),
            (TagName::LowPriority, TagInfo::new()),
            (
                TagName::from("u.work"),
                serde_json::from_value(serde_json::json!({ "order": 0.75 }))
                    .expect("user tag info"),
            ),
        ]);

        assert_eq!(
            room_tag_summaries_from_tags(Some(tags)),
            vec![
                RoomTagSummary {
                    tag: "m.favourite".to_owned(),
                    order: Some("0.25".to_owned()),
                },
                RoomTagSummary {
                    tag: "m.lowpriority".to_owned(),
                    order: None,
                },
                RoomTagSummary {
                    tag: "u.work".to_owned(),
                    order: Some("0.75".to_owned()),
                },
            ]
        );
    }

    #[test]
    fn room_read_state_from_parts_preserves_fully_read_and_unread_reconciliation() {
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
                relation: None,
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
                relation: None,
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
                relation: None,
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
    fn threaded_local_echo_message_extracts_thread_relation() {
        let mut content = RoomMessageEventContent::text_plain("hello from thread");
        attach_thread_relation(
            &mut content,
            SendMessageThreadTarget {
                thread_root_event_id: "$thread-root:example.org".to_owned(),
                latest_event_id: "$thread-1:example.org".to_owned(),
            },
        )
        .expect("attach thread relation");
        let serialized_event =
            SerializableEventContent::new(&AnyMessageLikeEventContent::RoomMessage(content))
                .expect("serialize threaded text event");

        assert_eq!(
            message_presentation_and_body_from_serialized_event(&serialized_event)
                .expect("read local echo body"),
            Some(LocalEchoMessageDetails {
                presentation: RoomMessagePresentationKind::Text,
                body: "hello from thread".to_owned(),
                relation: Some(RoomActivityRelation::Thread {
                    thread_root_event_id: "$thread-root:example.org".to_owned(),
                    in_reply_to_event_id: Some("$thread-1:example.org".to_owned()),
                    is_falling_back: true,
                }),
            })
        );
    }

    #[test]
    fn reply_local_echo_message_extracts_reply_relation() {
        let mut content = RoomMessageEventContent::text_plain("hello in reply");
        attach_reply_relation(
            &mut content,
            SendMessageReplyTarget {
                event_id: "$parent:example.org".to_owned(),
            },
        )
        .expect("attach reply relation");
        let serialized_event =
            SerializableEventContent::new(&AnyMessageLikeEventContent::RoomMessage(content))
                .expect("serialize reply text event");

        assert_eq!(
            message_presentation_and_body_from_serialized_event(&serialized_event)
                .expect("read local echo body"),
            Some(LocalEchoMessageDetails {
                presentation: RoomMessagePresentationKind::Text,
                body: "hello in reply".to_owned(),
                relation: Some(RoomActivityRelation::Reply {
                    in_reply_to_event_id: "$parent:example.org".to_owned(),
                }),
            })
        );
    }

    #[test]
    fn local_threaded_queue_mapping_preserves_thread_relation() {
        let created_at = MilliSecondsSinceUnixEpoch::now();
        let mut content = RoomMessageEventContent::text_plain("hello from thread");
        attach_thread_relation(
            &mut content,
            SendMessageThreadTarget {
                thread_root_event_id: "$thread-root:example.org".to_owned(),
                latest_event_id: "$thread-1:example.org".to_owned(),
            },
        )
        .expect("attach thread relation");
        let serialized_event =
            SerializableEventContent::new(&AnyMessageLikeEventContent::RoomMessage(content))
                .expect("serialize threaded text event");

        let activity = map_local_message_activity(
            "!room:example.org".to_owned(),
            "txn-1".to_owned(),
            "@alice:example.org",
            created_at,
            &serialized_event,
        )
        .expect("map local threaded text")
        .expect("threaded text local echo");

        assert_eq!(
            activity,
            RoomActivity {
                room_id: "!room:example.org".to_owned(),
                activity_id: "txn:txn-1".to_owned(),
                timestamp_ms: created_at.get().into(),
                relation: Some(RoomActivityRelation::Thread {
                    thread_root_event_id: "$thread-root:example.org".to_owned(),
                    in_reply_to_event_id: Some("$thread-1:example.org".to_owned()),
                    is_falling_back: true,
                }),
                kind: RoomActivityKind::Message(RoomMessageActivity {
                    event_id: None,
                    transaction_id: Some("txn-1".to_owned()),
                    sender: "@alice:example.org".to_owned(),
                    presentation: RoomMessagePresentationKind::Text,
                    body: "hello from thread".to_owned(),
                    send_state: RoomMessageSendState::Pending,
                }),
            }
        );
    }

    #[test]
    fn local_reply_queue_mapping_preserves_reply_relation() {
        let created_at = MilliSecondsSinceUnixEpoch::now();
        let mut content = RoomMessageEventContent::text_plain("hello in reply");
        attach_reply_relation(
            &mut content,
            SendMessageReplyTarget {
                event_id: "$parent:example.org".to_owned(),
            },
        )
        .expect("attach reply relation");
        let serialized_event =
            SerializableEventContent::new(&AnyMessageLikeEventContent::RoomMessage(content))
                .expect("serialize reply text event");

        let activity = map_local_message_activity(
            "!room:example.org".to_owned(),
            "txn-1".to_owned(),
            "@alice:example.org",
            created_at,
            &serialized_event,
        )
        .expect("map local reply")
        .expect("reply local echo");

        assert_eq!(
            activity,
            RoomActivity {
                room_id: "!room:example.org".to_owned(),
                activity_id: "txn:txn-1".to_owned(),
                timestamp_ms: created_at.get().into(),
                relation: Some(RoomActivityRelation::Reply {
                    in_reply_to_event_id: "$parent:example.org".to_owned(),
                }),
                kind: RoomActivityKind::Message(RoomMessageActivity {
                    event_id: None,
                    transaction_id: Some("txn-1".to_owned()),
                    sender: "@alice:example.org".to_owned(),
                    presentation: RoomMessagePresentationKind::Text,
                    body: "hello in reply".to_owned(),
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
        let pending = pending_outgoing_registry();
        insert_pending_outgoing_kind(&pending, "txn-1".to_owned(), PendingOutgoingKind::Message);
        let events = map_send_queue_update(
            SendQueueUpdate {
                room_id: room_id(),
                update: RoomSendQueueUpdate::SentEvent {
                    transaction_id: transaction_id(),
                    event_id: event_id(),
                },
            },
            "@alice:example.org",
            &pending,
        )
        .expect("map sent update");

        assert_eq!(
            events,
            vec![Event::RoomActivityUpdate(RoomActivityUpdate {
                room_id: "!room:example.org".to_owned(),
                target: RoomActivityUpdateTarget::Transaction {
                    transaction_id: "txn-1".to_owned(),
                },
                kind: RoomActivityUpdateKind::MessageSent {
                    event_id: "$event:example.org".to_owned(),
                },
            })]
        );
    }

    #[test]
    fn reaction_send_queue_sent_update_emits_no_message_sent() {
        let pending = pending_outgoing_registry();
        insert_pending_outgoing_kind(
            &pending,
            "txn-1".to_owned(),
            PendingOutgoingKind::Reaction {
                target_event_id: "$target:example.org".to_owned(),
                key: "+1".to_owned(),
            },
        );

        let events = map_send_queue_update(
            SendQueueUpdate {
                room_id: room_id(),
                update: RoomSendQueueUpdate::SentEvent {
                    transaction_id: transaction_id(),
                    event_id: event_id(),
                },
            },
            "@alice:example.org",
            &pending,
        )
        .expect("map reaction sent update");

        assert!(events.is_empty());
    }

    #[test]
    fn edit_send_queue_sent_update_emits_no_message_sent() {
        let pending = pending_outgoing_registry();
        insert_pending_outgoing_kind(
            &pending,
            "txn-1".to_owned(),
            PendingOutgoingKind::MessageEdit {
                target_event_id: "$target:example.org".to_owned(),
            },
        );

        let events = map_send_queue_update(
            SendQueueUpdate {
                room_id: room_id(),
                update: RoomSendQueueUpdate::SentEvent {
                    transaction_id: transaction_id(),
                    event_id: event_id(),
                },
            },
            "@alice:example.org",
            &pending,
        )
        .expect("map edit sent update");

        assert!(events.is_empty());
    }

    #[test]
    fn redaction_send_queue_sent_update_emits_no_message_sent() {
        let pending = pending_outgoing_registry();
        insert_pending_outgoing_kind(
            &pending,
            "txn-1".to_owned(),
            PendingOutgoingKind::Redaction {
                redacts: "$reaction:example.org".to_owned(),
            },
        );

        let events = map_send_queue_update(
            SendQueueUpdate {
                room_id: room_id(),
                update: RoomSendQueueUpdate::SentEvent {
                    transaction_id: transaction_id(),
                    event_id: event_id(),
                },
            },
            "@alice:example.org",
            &pending,
        )
        .expect("map redaction sent update");

        assert!(events.is_empty());
    }

    #[test]
    fn restored_reaction_local_echo_registers_pending_reaction_without_room_activity() {
        let pending = pending_outgoing_registry();
        let serialized_event =
            SerializableEventContent::new(&AnyMessageLikeEventContent::Reaction(
                reaction_event_content("$target:example.org".to_owned(), "+1".to_owned())
                    .expect("build reaction content"),
            ))
            .expect("serialize reaction event");

        let events = outgoing_events_from_local_echo_event(
            "!room:example.org".to_owned(),
            "txn-1".to_owned(),
            "@alice:example.org",
            MilliSecondsSinceUnixEpoch::now(),
            &serialized_event,
            None,
            &pending,
        )
        .expect("map restored reaction local echo");

        assert!(events.is_empty());
        assert_eq!(
            pending.lock().expect("lock pending registry").get("txn-1"),
            Some(&PendingOutgoingKind::Reaction {
                target_event_id: "$target:example.org".to_owned(),
                key: "+1".to_owned(),
            })
        );
    }

    #[test]
    fn restored_edit_local_echo_registers_pending_edit_without_room_activity() {
        let pending = pending_outgoing_registry();
        let serialized_event =
            SerializableEventContent::new(&AnyMessageLikeEventContent::RoomMessage(
                room_message_event_content_without_relation(
                    RoomMessagePresentationKind::Text,
                    "edited body".to_owned(),
                )
                .make_replacement(ReplacementMetadata::new(event_id(), None)),
            ))
            .expect("serialize edit event");

        let events = outgoing_events_from_local_echo_event(
            "!room:example.org".to_owned(),
            "txn-1".to_owned(),
            "@alice:example.org",
            MilliSecondsSinceUnixEpoch::now(),
            &serialized_event,
            None,
            &pending,
        )
        .expect("map restored edit local echo");

        assert!(events.is_empty());
        assert_eq!(
            pending.lock().expect("lock pending registry").get("txn-1"),
            Some(&PendingOutgoingKind::MessageEdit {
                target_event_id: "$event:example.org".to_owned(),
            })
        );
    }

    #[test]
    fn local_echo_react_branch_registers_pending_reaction_without_room_activity() {
        let pending = pending_outgoing_registry();
        let (runtime, _client, room_send_queue) = build_test_room_send_queue_context();

        runtime.block_on(async {
            let (existing_local_echoes, mut updates) = room_send_queue
                .subscribe()
                .await
                .expect("subscribe to room send queue");
            assert!(existing_local_echoes.is_empty());

            let send_handle = room_send_queue
                .send(AnyMessageLikeEventContent::RoomMessage(
                    RoomMessageEventContent::text_plain("hello"),
                ))
                .await
                .expect("queue message");

            let message_transaction_id = loop {
                let update = updates.recv().await.expect("send queue update");
                let RoomSendQueueUpdate::NewLocalEvent(local_echo) = update else {
                    continue;
                };

                if matches!(local_echo.content, LocalEchoContent::Event { .. }) {
                    break local_echo.transaction_id.to_string();
                }
            };
            send_handle
                .react("+1".to_owned())
                .await
                .expect("queue reaction")
                .expect("reaction handle");

            let react_local_echo = loop {
                let update = updates.recv().await.expect("send queue update");
                let RoomSendQueueUpdate::NewLocalEvent(local_echo) = update else {
                    continue;
                };

                if matches!(local_echo.content, LocalEchoContent::React { .. }) {
                    break local_echo;
                }
            };
            let reaction_transaction_id = react_local_echo.transaction_id.to_string();

            let events = map_local_echo(
                "!room:example.org".to_owned(),
                react_local_echo,
                "@alice:example.org",
                &pending,
            )
            .expect("map react local echo");

            assert!(events.is_empty());
            assert_eq!(
                pending
                    .lock()
                    .expect("lock pending registry")
                    .get(&reaction_transaction_id),
                Some(&PendingOutgoingKind::Reaction {
                    target_event_id: message_transaction_id,
                    key: "+1".to_owned(),
                })
            );
        });
    }

    #[test]
    fn local_echo_redaction_branch_registers_pending_redaction_without_room_activity() {
        let pending = pending_outgoing_registry();
        let (runtime, _client, room_send_queue) = build_test_room_send_queue_context();

        runtime.block_on(async {
            let (existing_local_echoes, mut updates) = room_send_queue
                .subscribe()
                .await
                .expect("subscribe to room send queue");
            assert!(existing_local_echoes.is_empty());

            room_send_queue
                .redact(
                    OwnedEventId::try_from("$reaction:example.org")
                        .expect("valid reaction event id"),
                    None,
                )
                .await
                .expect("queue redaction");

            let redaction_local_echo = loop {
                let update = updates.recv().await.expect("send queue update");
                let RoomSendQueueUpdate::NewLocalEvent(local_echo) = update else {
                    continue;
                };

                if matches!(local_echo.content, LocalEchoContent::Redaction { .. }) {
                    break local_echo;
                }
            };
            let redaction_transaction_id = redaction_local_echo.transaction_id.to_string();

            let events = map_local_echo(
                "!room:example.org".to_owned(),
                redaction_local_echo,
                "@alice:example.org",
                &pending,
            )
            .expect("map redaction local echo");

            assert!(events.is_empty());
            assert_eq!(
                pending
                    .lock()
                    .expect("lock pending registry")
                    .get(&redaction_transaction_id),
                Some(&PendingOutgoingKind::Redaction {
                    redacts: "$reaction:example.org".to_owned(),
                })
            );
        });
    }

    #[test]
    fn restored_local_echo_hydrates_to_pending_room_activity() {
        let created_at = MilliSecondsSinceUnixEpoch::now();
        let pending = pending_outgoing_registry();
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
            &pending,
        )
        .expect("map restored local echo");

        assert_eq!(
            events,
            vec![Event::RoomActivity(Box::new(RoomActivity {
                room_id: "!room:example.org".to_owned(),
                activity_id: "txn:txn-1".to_owned(),
                timestamp_ms: created_at.get().into(),
                relation: None,
                kind: RoomActivityKind::Message(RoomMessageActivity {
                    event_id: None,
                    transaction_id: Some("txn-1".to_owned()),
                    sender: "@alice:example.org".to_owned(),
                    presentation: RoomMessagePresentationKind::Text,
                    body: "hello".to_owned(),
                    send_state: RoomMessageSendState::Pending,
                }),
            }))]
        );
    }

    #[test]
    fn restored_threaded_local_echo_hydrates_to_pending_room_activity() {
        let created_at = MilliSecondsSinceUnixEpoch::now();
        let pending = pending_outgoing_registry();
        let mut content = RoomMessageEventContent::text_plain("hello from thread");
        attach_thread_relation(
            &mut content,
            SendMessageThreadTarget {
                thread_root_event_id: "$thread-root:example.org".to_owned(),
                latest_event_id: "$thread-1:example.org".to_owned(),
            },
        )
        .expect("attach thread relation");
        let serialized_event =
            SerializableEventContent::new(&AnyMessageLikeEventContent::RoomMessage(content))
                .expect("serialize threaded text event");

        let events = outgoing_events_from_local_echo_event(
            "!room:example.org".to_owned(),
            "txn-1".to_owned(),
            "@alice:example.org",
            created_at,
            &serialized_event,
            None,
            &pending,
        )
        .expect("map restored local echo");

        assert_eq!(
            events,
            vec![Event::RoomActivity(Box::new(RoomActivity {
                room_id: "!room:example.org".to_owned(),
                activity_id: "txn:txn-1".to_owned(),
                timestamp_ms: created_at.get().into(),
                relation: Some(RoomActivityRelation::Thread {
                    thread_root_event_id: "$thread-root:example.org".to_owned(),
                    in_reply_to_event_id: Some("$thread-1:example.org".to_owned()),
                    is_falling_back: true,
                }),
                kind: RoomActivityKind::Message(RoomMessageActivity {
                    event_id: None,
                    transaction_id: Some("txn-1".to_owned()),
                    sender: "@alice:example.org".to_owned(),
                    presentation: RoomMessagePresentationKind::Text,
                    body: "hello from thread".to_owned(),
                    send_state: RoomMessageSendState::Pending,
                }),
            }))]
        );
    }

    #[test]
    fn restored_reply_local_echo_hydrates_to_pending_room_activity() {
        let created_at = MilliSecondsSinceUnixEpoch::now();
        let pending = pending_outgoing_registry();
        let mut content = RoomMessageEventContent::text_plain("hello in reply");
        attach_reply_relation(
            &mut content,
            SendMessageReplyTarget {
                event_id: "$parent:example.org".to_owned(),
            },
        )
        .expect("attach reply relation");
        let serialized_event =
            SerializableEventContent::new(&AnyMessageLikeEventContent::RoomMessage(content))
                .expect("serialize reply text event");

        let events = outgoing_events_from_local_echo_event(
            "!room:example.org".to_owned(),
            "txn-1".to_owned(),
            "@alice:example.org",
            created_at,
            &serialized_event,
            None,
            &pending,
        )
        .expect("map restored local echo");

        assert_eq!(
            events,
            vec![Event::RoomActivity(Box::new(RoomActivity {
                room_id: "!room:example.org".to_owned(),
                activity_id: "txn:txn-1".to_owned(),
                timestamp_ms: created_at.get().into(),
                relation: Some(RoomActivityRelation::Reply {
                    in_reply_to_event_id: "$parent:example.org".to_owned(),
                }),
                kind: RoomActivityKind::Message(RoomMessageActivity {
                    event_id: None,
                    transaction_id: Some("txn-1".to_owned()),
                    sender: "@alice:example.org".to_owned(),
                    presentation: RoomMessagePresentationKind::Text,
                    body: "hello in reply".to_owned(),
                    send_state: RoomMessageSendState::Pending,
                }),
            }))]
        );
    }

    #[test]
    fn restored_failed_local_echo_re_emits_error_surface() {
        let created_at = MilliSecondsSinceUnixEpoch::now();
        let pending = pending_outgoing_registry();
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
            &pending,
        )
        .expect("map restored failed local echo");

        assert_eq!(
            events,
            vec![
                Event::RoomActivity(Box::new(RoomActivity {
                    room_id: "!room:example.org".to_owned(),
                    activity_id: "txn:txn-1".to_owned(),
                    timestamp_ms: created_at.get().into(),
                    relation: None,
                    kind: RoomActivityKind::Message(RoomMessageActivity {
                        event_id: None,
                        transaction_id: Some("txn-1".to_owned()),
                        sender: "@alice:example.org".to_owned(),
                        presentation: RoomMessagePresentationKind::Text,
                        body: "hello".to_owned(),
                        send_state: RoomMessageSendState::Pending,
                    }),
                })),
                Event::RoomActivityUpdate(RoomActivityUpdate {
                    room_id: "!room:example.org".to_owned(),
                    target: RoomActivityUpdateTarget::Transaction {
                        transaction_id: "txn-1".to_owned(),
                    },
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
    fn restored_failed_edit_local_echo_re_emits_edit_error_surface() {
        let pending = pending_outgoing_registry();
        let serialized_event =
            SerializableEventContent::new(&AnyMessageLikeEventContent::RoomMessage(
                room_message_event_content_without_relation(
                    RoomMessagePresentationKind::Text,
                    "edited body".to_owned(),
                )
                .make_replacement(ReplacementMetadata::new(event_id(), None)),
            ))
            .expect("serialize failed edit event");

        let events = outgoing_events_from_local_echo_event(
            "!room:example.org".to_owned(),
            "txn-1".to_owned(),
            "@alice:example.org",
            MilliSecondsSinceUnixEpoch::now(),
            &serialized_event,
            Some("edit send failed".to_owned()),
            &pending,
        )
        .expect("map restored failed edit local echo");

        assert_eq!(
            events,
            vec![Event::Error(CoreErrorEvent {
                operation: Operation::EditMessage,
                message: "edit send failed".to_owned(),
            })]
        );
        assert_eq!(
            pending.lock().expect("lock pending registry").get("txn-1"),
            Some(&PendingOutgoingKind::MessageEdit {
                target_event_id: "$event:example.org".to_owned(),
            })
        );
    }

    #[test]
    fn send_queue_error_maps_to_failed_activity_update_and_error_surface() {
        let pending = pending_outgoing_registry();
        insert_pending_outgoing_kind(&pending, "txn-1".to_owned(), PendingOutgoingKind::Message);
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
            &pending,
        )
        .expect("map send error");

        assert_eq!(
            events,
            vec![
                Event::RoomActivityUpdate(RoomActivityUpdate {
                    room_id: "!room:example.org".to_owned(),
                    target: RoomActivityUpdateTarget::Transaction {
                        transaction_id: "txn-1".to_owned(),
                    },
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
    fn edit_send_queue_error_emits_typed_error_only() {
        let pending = pending_outgoing_registry();
        insert_pending_outgoing_kind(
            &pending,
            "txn-1".to_owned(),
            PendingOutgoingKind::MessageEdit {
                target_event_id: "$target:example.org".to_owned(),
            },
        );

        let events = map_send_queue_update(
            SendQueueUpdate {
                room_id: room_id(),
                update: RoomSendQueueUpdate::SendError {
                    transaction_id: transaction_id(),
                    error: Arc::new(matrix_sdk::Error::from(std::io::Error::other(
                        "edit send failed",
                    ))),
                    is_recoverable: true,
                },
            },
            "@alice:example.org",
            &pending,
        )
        .expect("map edit send error");

        assert_eq!(
            events,
            vec![Event::Error(CoreErrorEvent {
                operation: Operation::EditMessage,
                message: "edit send failed".to_owned(),
            })]
        );
    }

    #[test]
    fn reaction_send_queue_error_emits_typed_error_only() {
        let pending = pending_outgoing_registry();
        insert_pending_outgoing_kind(
            &pending,
            "txn-1".to_owned(),
            PendingOutgoingKind::Reaction {
                target_event_id: "$target:example.org".to_owned(),
                key: "+1".to_owned(),
            },
        );

        let events = map_send_queue_update(
            SendQueueUpdate {
                room_id: room_id(),
                update: RoomSendQueueUpdate::SendError {
                    transaction_id: transaction_id(),
                    error: Arc::new(matrix_sdk::Error::from(std::io::Error::other(
                        "reaction send failed",
                    ))),
                    is_recoverable: true,
                },
            },
            "@alice:example.org",
            &pending,
        )
        .expect("map reaction send error");

        assert_eq!(
            events,
            vec![Event::Error(CoreErrorEvent {
                operation: Operation::SendReaction,
                message: "reaction send failed".to_owned(),
            })]
        );
    }

    #[test]
    fn redaction_send_queue_error_emits_typed_error_only() {
        let pending = pending_outgoing_registry();
        insert_pending_outgoing_kind(
            &pending,
            "txn-1".to_owned(),
            PendingOutgoingKind::Redaction {
                redacts: "$reaction:example.org".to_owned(),
            },
        );

        let events = map_send_queue_update(
            SendQueueUpdate {
                room_id: room_id(),
                update: RoomSendQueueUpdate::SendError {
                    transaction_id: transaction_id(),
                    error: Arc::new(matrix_sdk::Error::from(std::io::Error::other(
                        "redaction send failed",
                    ))),
                    is_recoverable: true,
                },
            },
            "@alice:example.org",
            &pending,
        )
        .expect("map redaction send error");

        assert_eq!(
            events,
            vec![Event::Error(CoreErrorEvent {
                operation: Operation::RedactEvent,
                message: "redaction send failed".to_owned(),
            })]
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

    #[test]
    fn room_message_event_content_without_relation_serializes_expected_shapes() {
        let text = serde_json::to_value(room_message_event_content_without_relation(
            RoomMessagePresentationKind::Text,
            "hello".to_owned(),
        ))
        .expect("serialize text content without relation");
        let notice = serde_json::to_value(room_message_event_content_without_relation(
            RoomMessagePresentationKind::Notice,
            "heads up".to_owned(),
        ))
        .expect("serialize notice content without relation");
        let emote = serde_json::to_value(room_message_event_content_without_relation(
            RoomMessagePresentationKind::Emote,
            "waves".to_owned(),
        ))
        .expect("serialize emote content without relation");

        assert_eq!(text["msgtype"], "m.text");
        assert_eq!(text["body"], "hello");
        assert!(text.get("m.relates_to").is_none());

        assert_eq!(notice["msgtype"], "m.notice");
        assert_eq!(notice["body"], "heads up");
        assert!(notice.get("m.relates_to").is_none());

        assert_eq!(emote["msgtype"], "m.emote");
        assert_eq!(emote["body"], "waves");
        assert!(emote.get("m.relates_to").is_none());
    }

    #[test]
    fn room_message_edit_content_serializes_replacement_shape() {
        let content = room_message_event_content_without_relation(
            RoomMessagePresentationKind::Text,
            "edited body".to_owned(),
        )
        .make_replacement(ReplacementMetadata::new(event_id(), None));

        assert_eq!(
            serde_json::to_value(content).expect("serialize replacement content"),
            serde_json::json!({
                "msgtype": "m.text",
                "body": "* edited body",
                "format": "org.matrix.custom.html",
                "formatted_body": "* ",
                "m.new_content": {
                    "msgtype": "m.text",
                    "body": "edited body",
                },
                "m.relates_to": {
                    "rel_type": "m.replace",
                    "event_id": "$event:example.org",
                }
            })
        );
    }

    #[test]
    fn reaction_event_content_serializes_annotation_relation() {
        let content = reaction_event_content("$target:example.org".to_owned(), "+1".to_owned())
            .expect("build reaction content");

        assert_eq!(
            serde_json::to_value(content).expect("serialize reaction content"),
            serde_json::json!({
                "m.relates_to": {
                    "rel_type": "m.annotation",
                    "event_id": "$target:example.org",
                    "key": "+1",
                }
            })
        );
    }

    #[test]
    fn reaction_event_content_rejects_invalid_target_event_id() {
        let err = reaction_event_content("not-an-event-id".to_owned(), "+1".to_owned())
            .expect_err("reject invalid target event id");

        assert!(err.to_string().contains("invalid Matrix event id"));
    }

    #[test]
    fn owned_event_id_from_str_rejects_invalid_reaction_event_id() {
        let err =
            owned_event_id_from_str("not-an-event-id").expect_err("reject invalid reaction id");

        assert!(err.to_string().contains("invalid Matrix event id"));
    }

    #[test]
    fn reply_room_message_content_serializes_reply_relation() {
        for (presentation, body, msgtype) in [
            (RoomMessagePresentationKind::Text, "hello", "m.text"),
            (RoomMessagePresentationKind::Notice, "heads up", "m.notice"),
            (RoomMessagePresentationKind::Emote, "waves", "m.emote"),
        ] {
            let mut content = room_message_event_content(presentation, body.to_owned());
            attach_reply_relation(
                &mut content,
                SendMessageReplyTarget {
                    event_id: "$parent:example.org".to_owned(),
                },
            )
            .expect("attach reply relation");

            let serialized = serde_json::to_value(content).expect("serialize reply content");

            assert_eq!(serialized["msgtype"], msgtype);
            assert_eq!(serialized["body"], body);
            assert_eq!(
                serialized["m.relates_to"]["m.in_reply_to"]["event_id"],
                "$parent:example.org"
            );
            assert!(serialized["m.relates_to"].get("rel_type").is_none());
        }
    }

    #[test]
    fn thread_reply_room_message_content_serializes_thread_reply_relation() {
        let mut content =
            room_message_event_content(RoomMessagePresentationKind::Text, "hello".to_owned());
        attach_thread_reply_relation(
            &mut content,
            SendMessageThreadTarget {
                thread_root_event_id: "$thread-root:example.org".to_owned(),
                latest_event_id: "$latest-thread:example.org".to_owned(),
            },
            SendMessageReplyTarget {
                event_id: "$reply-target:example.org".to_owned(),
            },
        )
        .expect("attach thread reply relation");

        let serialized = serde_json::to_value(content).expect("serialize threaded reply content");

        assert_eq!(serialized["m.relates_to"]["rel_type"], "m.thread");
        assert_eq!(
            serialized["m.relates_to"]["event_id"],
            "$thread-root:example.org"
        );
        assert_eq!(
            serialized["m.relates_to"]["m.in_reply_to"]["event_id"],
            "$reply-target:example.org"
        );
        assert!(serialized["m.relates_to"].get("is_falling_back").is_none());
    }

    #[test]
    fn reply_room_message_content_rejects_invalid_target_event_id() {
        let mut content =
            room_message_event_content(RoomMessagePresentationKind::Text, "hello".to_owned());
        let err = attach_reply_relation(
            &mut content,
            SendMessageReplyTarget {
                event_id: "not-an-event-id".to_owned(),
            },
        )
        .expect_err("reject invalid reply event id");

        assert!(err.to_string().contains("invalid Matrix event id"));
    }

    #[test]
    fn thread_reply_room_message_content_rejects_invalid_event_ids() {
        let mut content =
            room_message_event_content(RoomMessagePresentationKind::Text, "hello".to_owned());
        let invalid_thread_root = attach_thread_reply_relation(
            &mut content,
            SendMessageThreadTarget {
                thread_root_event_id: "not-an-event-id".to_owned(),
                latest_event_id: "$latest-thread:example.org".to_owned(),
            },
            SendMessageReplyTarget {
                event_id: "$reply-target:example.org".to_owned(),
            },
        )
        .expect_err("reject invalid thread root");
        assert!(
            invalid_thread_root
                .to_string()
                .contains("invalid Matrix event id")
        );

        let invalid_latest_event = attach_thread_reply_relation(
            &mut content,
            SendMessageThreadTarget {
                thread_root_event_id: "$thread-root:example.org".to_owned(),
                latest_event_id: "not-an-event-id".to_owned(),
            },
            SendMessageReplyTarget {
                event_id: "$reply-target:example.org".to_owned(),
            },
        )
        .expect_err("reject invalid latest event id");
        assert!(
            invalid_latest_event
                .to_string()
                .contains("invalid Matrix event id")
        );

        let invalid_reply_target = attach_thread_reply_relation(
            &mut content,
            SendMessageThreadTarget {
                thread_root_event_id: "$thread-root:example.org".to_owned(),
                latest_event_id: "$latest-thread:example.org".to_owned(),
            },
            SendMessageReplyTarget {
                event_id: "not-an-event-id".to_owned(),
            },
        )
        .expect_err("reject invalid reply target");
        assert!(
            invalid_reply_target
                .to_string()
                .contains("invalid Matrix event id")
        );
    }

    #[test]
    fn threaded_room_message_content_serializes_plain_thread_relation() {
        let mut content =
            room_message_event_content(RoomMessagePresentationKind::Text, "hello".to_owned());
        attach_thread_relation(
            &mut content,
            SendMessageThreadTarget {
                thread_root_event_id: "$thread-root:example.org".to_owned(),
                latest_event_id: "$latest-thread:example.org".to_owned(),
            },
        )
        .expect("attach thread relation");

        let serialized = serde_json::to_value(content).expect("serialize threaded content");

        assert_eq!(serialized["msgtype"], "m.text");
        assert_eq!(serialized["body"], "hello");
        assert_eq!(serialized["m.relates_to"]["rel_type"], "m.thread");
        assert_eq!(
            serialized["m.relates_to"]["event_id"],
            "$thread-root:example.org"
        );
        assert_eq!(
            serialized["m.relates_to"]["m.in_reply_to"]["event_id"],
            "$latest-thread:example.org"
        );
        assert_eq!(serialized["m.relates_to"]["is_falling_back"], true);
    }

    #[test]
    fn unthreaded_room_message_content_omits_relation_for_all_presentations() {
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

        assert!(text.get("m.relates_to").is_none());
        assert!(notice.get("m.relates_to").is_none());
        assert!(emote.get("m.relates_to").is_none());
    }
}
