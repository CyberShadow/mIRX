use std::borrow::Cow;
use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, BTreeSet};
use std::error::Error;
use std::fmt::{self, Display, Formatter};
use std::rc::Rc;
use std::str::FromStr;
use std::sync::{Arc, Mutex};

use futures_util::StreamExt;
use matrix_sdk::{
    Client, LoopCtrl, Room, RoomMemberships, SessionChange, SessionMeta, SessionTokens,
    attachment::{
        AttachmentConfig, AttachmentInfo, BaseAudioInfo, BaseFileInfo, BaseImageInfo, BaseVideoInfo,
    },
    authentication::{
        AuthSession,
        matrix::MatrixSession,
        oauth::{
            ClientId, ClientRegistrationData, OAuthSession, UserSession,
            error::OAuthDiscoveryError,
            registration::{ApplicationType, ClientMetadata, Localized, OAuthGrantType},
        },
    },
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
    notification_settings::{
        IsEncrypted, IsOneToOne, NotificationSettings,
        RoomNotificationMode as SdkRoomNotificationMode,
    },
    room::edit::EditedContent,
    room::{MessagesOptions, Receipts, RoomMemberRole},
    ruma::{
        DeviceId, EventId, Int, MilliSecondsSinceUnixEpoch, MxcUri, OwnedDeviceId, OwnedEventId,
        OwnedRoomAliasId, OwnedRoomId, OwnedUserId, RoomId, RoomOrAliasId, ServerName, UInt,
        UserId,
        api::client::profile::DisplayName as DisplayNameField,
        api::client::push::{delete_pushrule, set_pushrule, set_pushrule_enabled},
        api::client::session::get_login_types::v3::LoginType,
        api::client::{
            directory::get_public_rooms_filtered,
            media::get_content_thumbnail::v3::Method,
            room::{
                Visibility,
                create_room::v3::{Request as CreateRoomHttpRequest, RoomPreset},
            },
        },
        api::error::ErrorKind,
        directory::Filter,
        events::{
            AnyMessageLikeEventContent, AnySyncStateEvent, AnySyncTimelineEvent, InitialStateEvent,
            Mentions, SyncStateEvent, TimelineEventType, ToDeviceEvent,
            direct::{DirectEvent, DirectEventContent},
            ignored_user_list::{IgnoredUserListEvent, IgnoredUserListEventContent},
            key::verification::{
                VerificationMethod, request::ToDeviceKeyVerificationRequestEventContent,
            },
            push_rules::PushRulesEvent,
            reaction::{OriginalSyncReactionEvent, ReactionEventContent},
            relation::{Annotation, Reply, Thread},
            room::{
                EncryptedFile, ImageInfo, MediaSource,
                encryption::RoomEncryptionEventContent,
                member::{MembershipChange, MembershipState, OriginalSyncRoomMemberEvent},
                message::{
                    AudioInfo, FileInfo, FormattedBody, MessageFormat, MessageType,
                    OriginalSyncRoomMessageEvent, Relation as MessageContentRelation,
                    Relation as MessageRelation, RoomMessageEventContent,
                    RoomMessageEventContentWithoutRelation, VideoInfo,
                    sanitize::remove_plain_reply_fallback,
                },
                power_levels::{
                    OriginalSyncRoomPowerLevelsEvent, PossiblyRedactedRoomPowerLevelsEventContent,
                    RoomPowerLevels, RoomPowerLevelsEventContent, UserPowerLevel,
                },
                redaction::OriginalSyncRoomRedactionEvent,
                topic::OriginalSyncRoomTopicEvent,
            },
            space::child::{SpaceChildEventContent, SpaceChildOrd},
            tag::{TagEvent, TagEventContent, Tags},
            typing::SyncTypingEvent,
        },
        push::{
            Action, HighlightTweakValue, NewPatternedPushRule, NewPushRule, RuleKind, Ruleset,
            SoundTweakValue, Tweak,
        },
        serde::Raw,
    },
    send_queue::{LocalEcho, LocalEchoContent, RoomSendQueueUpdate, SendQueueUpdate},
    store::{RoomLoadSettings, SerializableEventContent},
    sync::{JoinedRoomUpdate, State, SyncResponse},
    utils::UrlOrQuery,
};
use matrix_sdk_base::{
    RoomInfoNotableUpdate, RoomInfoNotableUpdateReasons, RoomState,
    deserialized_responses::DisplayName, read_receipts::RoomReadReceipts,
};
use matrix_sdk_crypto::CrossSigningStatus;
use mime::Mime;
use serde::Deserialize;
use tokio::sync::broadcast;
use tokio::task::JoinHandle;
use url::Url;

use crate::api::{
    BackfillRoomHistoryRequest, BanRoomMemberRequest, CoreConfig, CoreErrorEvent,
    CreateRoomRequest, CreateRoomVisibility, E2eeBackupState, E2eeBackupStatus,
    E2eeCrossSigningStatus, E2eeRecoveryState, E2eeStatus, EditMessageRequest, Event,
    FetchRoomEventRequest, FetchRoomMediaRequest, HighlightWord, InviteRoomMemberRequest,
    JoinRoomRequest, KickRoomMemberRequest, ListPublicRoomsRequest, MarkRoomReadRequest,
    MemberPowerRole, Operation, PublicRoom, PublicRoomsPage, PublicRoomsResult,
    ROOM_HISTORY_BACKFILL_LIMIT_MAX, RecoverE2eeRequest, RedactEventRequest,
    RequestVerificationRequest, RoomActivity, RoomActivityKind, RoomActivityRelation,
    RoomActivityUpdate, RoomActivityUpdateKind, RoomActivityUpdateTarget, RoomAudioActivity,
    RoomAudioInfo, RoomEncryptedFileHashes, RoomEncryptedFileKey, RoomEventFetch,
    RoomEventFetchResult, RoomFileActivity, RoomFileInfo, RoomHistoryBackfill,
    RoomHistoryBackfillEvent, RoomImageActivity, RoomImageInfo, RoomMedia, RoomMediaFormat,
    RoomMediaKind, RoomMediaSource, RoomMemberChange, RoomMemberRank, RoomMemberSummary,
    RoomMembers, RoomMembership, RoomMessageActivity, RoomMessagePresentationKind,
    RoomMessageSendState, RoomNotificationMode, RoomPowerLevelUserChange,
    RoomPowerLevelValueChange, RoomReadState, RoomStateNoticeActivity, RoomStateNoticeContent,
    RoomSummary, RoomTagSummary, RoomTyping, RoomUnableToDecryptActivity,
    RoomUnableToDecryptReason, RoomUserProfile, RoomVideoActivity, RoomVideoInfo, SendFileRequest,
    SendMessageReplyTarget, SendMessageRequest, SendMessageThreadTarget, SendReactionRequest,
    SessionAuth, SetDisplayNameRequest, SetRoomMemberPowerLevelRequest,
    SetRoomNotificationModeRequest, SetRoomTopicRequest, SetupE2eeRecoveryRequest,
    SpaceChildSummary, StoredSession, SyncStopReason, UnbanRoomMemberRequest,
    UnsupportedRoomMessageActivity, UserProfile, VerificationDecimals, VerificationDirection,
    VerificationEmoji, VerificationFlowId, VerificationRequest, VerificationState,
    VerificationStateEvent,
};
use crate::api::{TextStyleSpan, UserLinkSpan, check_text_styles, validate_highlight_word};
use crate::mentions::body_mentions;
use crate::rich_text::{StyledText, html_from_styled_text, styled_text_from_html};
use crate::runtime::{EventSender, InitialSnapshot, emit_event};

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

/// Where the client finds its homeserver.
pub(crate) enum HomeserverSpec<'a> {
    /// Login: a server name or URL, resolved through `.well-known` and a
    /// `/versions` check.
    Discover(&'a str),
    /// Restore: the URL a login resolved, used as is.
    Url(&'a str),
}

pub(crate) async fn build_client(
    config: &CoreConfig,
    homeserver: HomeserverSpec<'_>,
) -> Result<Client, MatrixCoreError> {
    let store_path = config.profile_dir.join("matrix-store");
    std::fs::create_dir_all(&store_path)
        .map_err(|err| MatrixCoreError::new(format!("failed to create store directory: {err}")))?;

    let builder = match homeserver {
        HomeserverSpec::Discover(server) => Client::builder().server_name_or_homeserver_url(server),
        HomeserverSpec::Url(url) => Client::builder().homeserver_url(url),
    };
    builder
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
    let meta = SessionMeta {
        user_id: OwnedUserId::try_from(dto.user_id)
            .map_err(|err| MatrixCoreError::new(format!("invalid session user id: {err}")))?,
        device_id: OwnedDeviceId::from(dto.device_id),
    };
    let tokens = SessionTokens {
        access_token: dto.access_token,
        refresh_token: dto.refresh_token,
    };
    let restored = match dto.auth {
        SessionAuth::Matrix => {
            client
                .matrix_auth()
                .restore_session(MatrixSession { meta, tokens }, RoomLoadSettings::default())
                .await
        }
        SessionAuth::OAuth { client_id } => {
            client
                .oauth()
                .restore_session(
                    OAuthSession {
                        client_id: ClientId::new(client_id),
                        user: UserSession { meta, tokens },
                    },
                    RoomLoadSettings::default(),
                )
                .await
        }
    };
    restored.map_err(|err| MatrixCoreError::new(format!("failed to restore Matrix session: {err}")))
}

pub(crate) async fn login(
    client: &Client,
    username: &str,
    password: &str,
    device_display_name: &str,
) -> Result<StoredSession, MatrixCoreError> {
    client
        .matrix_auth()
        .login_username(username, password)
        .initial_device_display_name(device_display_name)
        .request_refresh_token()
        .send()
        .await
        .map_err(|err| MatrixCoreError::new(format!("failed to log in to Matrix: {err}")))?;

    Ok(stored_session(client))
}

/// The project's homepage, which an OAuth 2.0 server shows on its consent
/// page. MAS requires https.
const MIRX_CLIENT_URI: &str = "https://github.com/CyberShadow/mIRX";

/// How the browser signs in, by what the homeserver offers.
#[derive(Debug)]
pub(crate) enum BrowserFlow {
    /// The authorization code flow; the client keeps the PKCE verifier and
    /// checks the redirect's `state`.
    OAuth,
    /// Legacy SSO: the redirect carries a `loginToken` for `m.login.token`.
    Sso { device_display_name: String },
}

pub(crate) struct BrowserSignIn {
    pub(crate) flow: BrowserFlow,
    /// The page to open in the browser.
    pub(crate) url: String,
}

/// Prepares a browser sign-in that ends at `redirect_uri`: registers mirx
/// with the homeserver's OAuth 2.0 server, or falls back to legacy SSO
/// when the homeserver has none. `server` names the homeserver in errors.
pub(crate) async fn start_browser_login(
    client: &Client,
    server: &str,
    redirect_uri: &Url,
    user_id_hint: Option<&str>,
    device_display_name: &str,
) -> Result<BrowserSignIn, MatrixCoreError> {
    let oauth = client.oauth();
    match oauth.server_metadata().await {
        Ok(_) => {
            let registration = ClientRegistrationData::new(
                Raw::new(&oauth_client_metadata(redirect_uri))
                    .expect("OAuth client metadata serializes"),
            );
            let mut login = oauth.login(redirect_uri.clone(), None, Some(registration), None);
            if let Some(user_id) = user_id_hint {
                let user_id = UserId::parse(user_id).map_err(|err| {
                    MatrixCoreError::new(format!("invalid user ID {user_id}: {err}"))
                })?;
                login = login.user_id_hint(&user_id);
            }
            let authorization = login.build().await.map_err(|err| {
                MatrixCoreError::new(format!("failed to start the OAuth 2.0 sign-in: {err}"))
            })?;
            // mirx hands the page to the shell unasked. The SDK checks the
            // endpoint is https only for an https homeserver; for a plain
            // http one, any scheme (file:, ms-msdt:) would reach the shell.
            if !matches!(authorization.url.scheme(), "https" | "http") {
                return Err(MatrixCoreError::new(format!(
                    "the homeserver's sign-in page is not a web address: {}",
                    authorization.url
                )));
            }
            Ok(BrowserSignIn {
                flow: BrowserFlow::OAuth,
                url: authorization.url.into(),
            })
        }
        // Only the homeserver saying it has no such endpoint (404
        // M_UNRECOGNIZED) means legacy SSO; a broken OAuth 2.0 server, or a
        // bare 404, is an error rather than a reason to sign in elsewhere.
        Err(OAuthDiscoveryError::NotSupported) => {
            let login_types = client
                .matrix_auth()
                .get_login_types()
                .await
                .map_err(|err| {
                    MatrixCoreError::new(format!(
                        "failed to read the homeserver's login types: {err}"
                    ))
                })?;
            if !login_types
                .flows
                .iter()
                .any(|flow| matches!(flow, LoginType::Sso(_)))
            {
                return Err(MatrixCoreError::new(format!(
                    "{server} offers no browser sign-in; enter a password"
                )));
            }
            // No identity provider: the homeserver shows its own picker.
            let url = client
                .matrix_auth()
                .get_sso_login_url(redirect_uri.as_str(), None)
                .await
                .map_err(|err| {
                    MatrixCoreError::new(format!("failed to build the SSO sign-in address: {err}"))
                })?;
            Ok(BrowserSignIn {
                flow: BrowserFlow::Sso {
                    device_display_name: device_display_name.to_owned(),
                },
                url,
            })
        }
        Err(err) => Err(MatrixCoreError::new(format!(
            "failed to discover the homeserver's OAuth 2.0 server: {err}"
        ))),
    }
}

/// mirx as an OAuth 2.0 client: a native application that receives the
/// authorization code at the loopback `redirect_uri`, registered anew for
/// each sign-in. No logo, policy or terms: MAS requires those to share the
/// client URI's host.
fn oauth_client_metadata(redirect_uri: &Url) -> ClientMetadata {
    let mut metadata = ClientMetadata::new(
        ApplicationType::Native,
        vec![OAuthGrantType::AuthorizationCode {
            redirect_uris: vec![redirect_uri.clone()],
        }],
        Localized::new(
            Url::parse(MIRX_CLIENT_URI).expect("the client URI parses"),
            [],
        ),
    );
    metadata.client_name = Some(Localized::new(crate::APP_NAME.to_owned(), []));
    metadata
}

/// Completes a browser sign-in with the `query` of the browser's request
/// for the redirect URI, and returns the new session.
pub(crate) async fn finish_browser_login(
    client: &Client,
    flow: BrowserFlow,
    query: &str,
) -> Result<StoredSession, MatrixCoreError> {
    match flow {
        // Checks the state, and reports the server's `error=`.
        BrowserFlow::OAuth => client
            .oauth()
            .finish_login(UrlOrQuery::Query(query.to_owned()))
            .await
            .map_err(|err| MatrixCoreError::new(format!("failed to sign in: {err}")))?,
        BrowserFlow::Sso {
            device_display_name,
        } => {
            let token = url::form_urlencoded::parse(query.as_bytes())
                .find_map(|(key, value)| (key == "loginToken").then_some(value))
                .ok_or_else(|| MatrixCoreError::new("the sign-in page returned no login token"))?;
            client
                .matrix_auth()
                .login_token(&token)
                .initial_device_display_name(&device_display_name)
                .request_refresh_token()
                .send()
                .await
                .map_err(|err| {
                    MatrixCoreError::new(format!("failed to log in to Matrix: {err}"))
                })?;
        }
    }
    Ok(stored_session(client))
}

/// The session's first sync; the runtime then reads the room snapshot.
pub(crate) async fn initial_room_sync(client: &Client) -> Result<(), MatrixCoreError> {
    // No `emit_member_snapshots_after_sync`: the snapshot has every joined
    // room's members.
    client
        .sync_once(SyncSettings::default())
        .await
        .map_err(|err| MatrixCoreError::new(format!("initial Matrix sync failed: {err}")))?;
    Ok(())
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
    // One instance for every room: it reads the stored push rules once and
    // registers an event handler for as long as it lives.
    let notification_settings = client.notification_settings().await;

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
        let notification_mode =
            room_notification_mode(&notification_settings, &room, encrypted.is_encrypted()).await;

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
            notification_mode: Some(notification_mode),
            canonical_alias: room.canonical_alias().map(|alias| alias.to_string()),
            alt_aliases: alias_strings(room.alt_aliases()),
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
            notification_mode: None,
            canonical_alias: room.canonical_alias().map(|alias| alias.to_string()),
            alt_aliases: alias_strings(room.alt_aliases()),
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

fn alias_strings(aliases: Vec<OwnedRoomAliasId>) -> Vec<String> {
    aliases.into_iter().map(|alias| alias.to_string()).collect()
}

/// The room's own push rule if it has one, else the account default for its
/// kind: what `Room::notification_mode` computes, without building a
/// `NotificationSettings` per room. "One-to-one" is the SDK's rule, exactly
/// two members, not `m.direct`.
async fn room_notification_mode(
    notification_settings: &NotificationSettings,
    room: &Room,
    encrypted: bool,
) -> RoomNotificationMode {
    let mode = match notification_settings
        .get_user_defined_room_notification_mode(room.room_id())
        .await
    {
        Some(mode) => mode,
        None => {
            notification_settings
                .get_default_room_notification_mode(
                    IsEncrypted::from(encrypted),
                    IsOneToOne::from(room.active_members_count() == 2),
                )
                .await
        }
    };
    room_notification_mode_from_sdk(mode)
}

fn room_notification_mode_from_sdk(mode: SdkRoomNotificationMode) -> RoomNotificationMode {
    match mode {
        SdkRoomNotificationMode::AllMessages => RoomNotificationMode::AllMessages,
        SdkRoomNotificationMode::MentionsAndKeywordsOnly => {
            RoomNotificationMode::MentionsAndKeywordsOnly
        }
        SdkRoomNotificationMode::Mute => RoomNotificationMode::Mute,
    }
}

/// Joins the room and returns its ID; an alias resolves to the room it
/// names.
pub(crate) async fn join_room(
    client: &Client,
    request: &JoinRoomRequest,
) -> Result<OwnedRoomId, MatrixCoreError> {
    let room_or_alias = <&RoomOrAliasId>::try_from(request.room_or_alias.as_str())
        .map_err(|err| MatrixCoreError::new(format!("invalid Matrix room id or alias: {err}")))?;
    let via = request
        .via
        .iter()
        .map(|server| {
            ServerName::parse(server).map_err(|err| {
                MatrixCoreError::new(format!("invalid Matrix server name {server}: {err}"))
            })
        })
        .collect::<Result<Vec<_>, _>>()?;

    let room = client
        .join_room_by_id_or_alias(room_or_alias, &via)
        .await
        .map_err(|err| MatrixCoreError::new(format!("failed to join Matrix room: {err}")))?;

    Ok(room.room_id().to_owned())
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

/// Creates the direct room and returns its ID.
pub(crate) async fn create_direct_room(
    client: &Client,
    user_id: &str,
) -> Result<OwnedRoomId, MatrixCoreError> {
    let user_id = <&UserId>::try_from(user_id)
        .map_err(|err| MatrixCoreError::new(format!("invalid Matrix user id: {err}")))?;

    let room = client.create_dm(user_id).await.map_err(|err| {
        MatrixCoreError::new(format!("failed to create Matrix direct room: {err}"))
    })?;

    Ok(room.room_id().to_owned())
}

/// Creates the room and returns its ID. A private room is encrypted from
/// the start, as Element creates them.
pub(crate) async fn create_room(
    client: &Client,
    request: &CreateRoomRequest,
) -> Result<OwnedRoomId, MatrixCoreError> {
    let mut create = CreateRoomHttpRequest::new();
    create.name = Some(request.name.clone());
    match &request.visibility {
        CreateRoomVisibility::Private => {
            create.preset = Some(RoomPreset::PrivateChat);
            create.visibility = Visibility::Private;
            create.initial_state = vec![
                InitialStateEvent::with_empty_state_key(
                    RoomEncryptionEventContent::with_recommended_defaults(),
                )
                .to_raw_any(),
            ];
        }
        CreateRoomVisibility::Public { alias_localpart } => {
            create.preset = Some(RoomPreset::PublicChat);
            create.visibility = Visibility::Public;
            create.room_alias_name = Some(alias_localpart.clone());
        }
    }

    let room = client
        .create_room(create)
        .await
        .map_err(|err| MatrixCoreError::new(format!("failed to create Matrix room: {err}")))?;

    Ok(room.room_id().to_owned())
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

    room.kick_user(user_id, request.reason.as_deref())
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

    room.ban_user(user_id, request.reason.as_deref())
        .await
        .map_err(|err| MatrixCoreError::new(format!("failed to ban Matrix room member: {err}")))
}

pub(crate) async fn unban_room_member(
    client: &Client,
    request: &UnbanRoomMemberRequest,
) -> Result<(), MatrixCoreError> {
    let room_id = <&RoomId>::try_from(request.room_id.as_str())
        .map_err(|err| MatrixCoreError::new(format!("invalid Matrix room id: {err}")))?;
    let user_id = <&UserId>::try_from(request.user_id.as_str())
        .map_err(|err| MatrixCoreError::new(format!("invalid Matrix user id: {err}")))?;
    let room = joined_room_by_id(client, room_id.as_str())?;

    room.unban_user(user_id, request.reason.as_deref())
        .await
        .map_err(|err| MatrixCoreError::new(format!("failed to unban Matrix room member: {err}")))
}

pub(crate) async fn invite_room_member(
    client: &Client,
    request: &InviteRoomMemberRequest,
) -> Result<(), MatrixCoreError> {
    let room_id = <&RoomId>::try_from(request.room_id.as_str())
        .map_err(|err| MatrixCoreError::new(format!("invalid Matrix room id: {err}")))?;
    let user_id = <&UserId>::try_from(request.user_id.as_str())
        .map_err(|err| MatrixCoreError::new(format!("invalid Matrix user id: {err}")))?;
    let room = joined_room_by_id(client, room_id.as_str())?;

    room.invite_user_by_id(user_id)
        .await
        .map_err(|err| MatrixCoreError::new(format!("failed to invite Matrix room member: {err}")))
}

/// Moderator in the IRC sense: the conventional Matrix moderator level.
const MODERATOR_POWER_LEVEL: u8 = 50;

/// The power level that gives `role` in a room with `power_levels`.
///
/// Voiced is the lowest level above `users_default` that may send
/// `m.room.message`; a room where that level reaches moderator has no
/// distinct voice level.
fn member_power_role_level(
    power_levels: &RoomPowerLevels,
    role: MemberPowerRole,
) -> Result<Int, MatrixCoreError> {
    let moderator = Int::from(MODERATOR_POWER_LEVEL);
    match role {
        MemberPowerRole::Moderator => Ok(moderator),
        MemberPowerRole::Default => Ok(power_levels.users_default),
        MemberPowerRole::Voiced => {
            let message_level = power_levels
                .events
                .get(&TimelineEventType::RoomMessage)
                .copied()
                .unwrap_or(power_levels.events_default);
            let level = message_level.max(power_levels.users_default + Int::from(1));
            if level >= moderator {
                return Err(MatrixCoreError::new(format!(
                    "room has no voice level below moderator: voice needs power level {level}"
                )));
            }
            Ok(level)
        }
    }
}

pub(crate) async fn set_room_member_power_level(
    client: &Client,
    request: &SetRoomMemberPowerLevelRequest,
) -> Result<(), MatrixCoreError> {
    let room_id = <&RoomId>::try_from(request.room_id.as_str())
        .map_err(|err| MatrixCoreError::new(format!("invalid Matrix room id: {err}")))?;
    let user_id = <&UserId>::try_from(request.user_id.as_str())
        .map_err(|err| MatrixCoreError::new(format!("invalid Matrix user id: {err}")))?;
    let room = joined_room_by_id(client, room_id.as_str())?;

    let power_levels = room.power_levels().await.map_err(|err| {
        MatrixCoreError::new(format!("failed to load Matrix room power levels: {err}"))
    })?;
    let level = member_power_role_level(&power_levels, request.role)?;

    room.update_power_levels(vec![(user_id, level)])
        .await
        .map(|_| ())
        .map_err(|err| {
            MatrixCoreError::new(format!(
                "failed to set Matrix room member power level: {err}"
            ))
        })
}

/// Writes the room's own push rule for the mode, replacing any other rule
/// of the user's for that room.
pub(crate) async fn set_room_notification_mode(
    client: &Client,
    request: SetRoomNotificationModeRequest,
) -> Result<(), MatrixCoreError> {
    let room_id = <&RoomId>::try_from(request.room_id.as_str())
        .map_err(|err| MatrixCoreError::new(format!("invalid Matrix room id: {err}")))?;
    joined_room_by_id(client, room_id.as_str())?;
    let mode = match request.mode {
        RoomNotificationMode::AllMessages => SdkRoomNotificationMode::AllMessages,
        RoomNotificationMode::MentionsAndKeywordsOnly => {
            SdkRoomNotificationMode::MentionsAndKeywordsOnly
        }
        RoomNotificationMode::Mute => SdkRoomNotificationMode::Mute,
    };
    client
        .notification_settings()
        .await
        .set_room_notification_mode(room_id, mode)
        .await
        .map_err(|err| {
            MatrixCoreError::new(format!(
                "failed to set Matrix room notification mode: {err}"
            ))
        })
}

/// The user's keyword rules: every enabled content rule that is not a
/// server default (the SDK's `enabled_keywords`), one per pattern, in
/// ruleset order. A pattern highlights if any of its rules does.
fn highlight_words_from_ruleset(ruleset: &Ruleset) -> Vec<HighlightWord> {
    let mut words: Vec<HighlightWord> = Vec::new();
    for rule in ruleset
        .content
        .iter()
        .filter(|rule| !rule.default && rule.enabled)
    {
        let highlights = rule.actions.iter().any(Action::is_highlight);
        match words.iter_mut().find(|word| word.word == rule.pattern) {
            Some(word) => word.highlights |= highlights,
            None => words.push(HighlightWord {
                word: rule.pattern.clone(),
                highlights,
            }),
        }
    }
    words
}

pub(crate) async fn highlight_words(
    client: &Client,
) -> Result<Vec<HighlightWord>, MatrixCoreError> {
    let ruleset =
        client.account().push_rules().await.map_err(|err| {
            MatrixCoreError::new(format!("failed to read Matrix push rules: {err}"))
        })?;
    Ok(highlight_words_from_ruleset(&ruleset))
}

/// The account's ignore list, sorted; no `m.ignored_user_list` event means
/// an empty list.
pub(crate) async fn ignored_users(client: &Client) -> Result<Vec<String>, MatrixCoreError> {
    let content = client
        .account()
        .account_data::<IgnoredUserListEventContent>()
        .await
        .map_err(|err| MatrixCoreError::new(format!("failed to read the ignore list: {err}")))?
        .map(|raw| raw.deserialize())
        .transpose()
        .map_err(|err| MatrixCoreError::new(format!("failed to decode the ignore list: {err}")))?
        .unwrap_or_default();
    Ok(ignored_user_ids(&content))
}

fn ignored_user_ids(content: &IgnoredUserListEventContent) -> Vec<String> {
    // A map keyed by user ID, so already sorted and unique.
    content
        .ignored_users
        .keys()
        .map(|user_id| user_id.to_string())
        .collect()
}

fn parse_user_id(user_id: &str) -> Result<OwnedUserId, MatrixCoreError> {
    UserId::parse(user_id)
        .map_err(|err| MatrixCoreError::new(format!("invalid user ID \"{user_id}\": {err}")))
}

/// Adds the user to the ignore list. The server's echo of the list, in the
/// next sync, is the confirmation.
pub(crate) async fn ignore_user(client: &Client, user_id: &str) -> Result<(), MatrixCoreError> {
    let user_id = parse_user_id(user_id)?;
    client
        .account()
        .ignore_user(&user_id)
        .await
        .map_err(|err| match err {
            matrix_sdk::Error::CantIgnoreLoggedInUser => {
                MatrixCoreError::new("you cannot ignore yourself")
            }
            err => MatrixCoreError::new(format!("failed to ignore {user_id}: {err}")),
        })
}

/// Removes the user from the ignore list; one who is not on it changes
/// nothing.
pub(crate) async fn unignore_user(client: &Client, user_id: &str) -> Result<(), MatrixCoreError> {
    let user_id = parse_user_id(user_id)?;
    client
        .account()
        .unignore_user(&user_id)
        .await
        .map_err(|err| MatrixCoreError::new(format!("failed to unignore {user_id}: {err}")))
}

/// The user's global profile from their homeserver. An empty display name
/// is none.
pub(crate) async fn fetch_user_profile(
    client: &Client,
    user_id: &str,
) -> Result<UserProfile, MatrixCoreError> {
    let user_id = parse_user_id(user_id)?;
    let response = client
        .account()
        .fetch_user_profile_of(&user_id)
        .await
        .map_err(|err| match err.client_api_error_kind() {
            Some(ErrorKind::NotFound) => MatrixCoreError::new(format!("no such user {user_id}")),
            _ => MatrixCoreError::new(format!("failed to fetch the profile of {user_id}: {err}")),
        })?;
    let display_name = response.get_static::<DisplayNameField>().map_err(|err| {
        MatrixCoreError::new(format!(
            "invalid display name in the profile of {user_id}: {err}"
        ))
    })?;
    Ok(UserProfile {
        user_id: user_id.to_string(),
        display_name: display_name.filter(|name| !name.is_empty()),
    })
}

/// What a highlight word's rule does: notify with a sound and highlight,
/// as the server's own mention rules do.
fn highlight_word_actions() -> Vec<Action> {
    vec![
        Action::Notify,
        Action::SetTweak(Tweak::Sound(SoundTweakValue::Default)),
        Action::SetTweak(Tweak::Highlight(HighlightTweakValue::Yes)),
    ]
}

/// Makes `word` one highlighting content rule named after it. The SDK's
/// `add_keyword` writes notify-only rules, so mirx writes its own: the PUT
/// creates the rule or replaces the actions of a notify-only one, a
/// disabled one is enabled again, and the word's other rules go.
pub(crate) async fn add_highlight_word(client: &Client, word: &str) -> Result<(), MatrixCoreError> {
    let failed = |err: &dyn Display| {
        MatrixCoreError::new(format!("failed to add highlight word \"{word}\": {err}"))
    };
    validate_highlight_word(word).map_err(|err| failed(&err))?;
    let ruleset = client
        .account()
        .push_rules()
        .await
        .map_err(|err| failed(&err))?;

    client
        .send(set_pushrule::v3::Request::new(NewPushRule::Content(
            NewPatternedPushRule::new(word.to_owned(), word.to_owned(), highlight_word_actions()),
        )))
        .await
        .map_err(|err| failed(&err))?;
    if ruleset
        .get(RuleKind::Content, word)
        .is_some_and(|rule| !rule.enabled())
    {
        client
            .send(set_pushrule_enabled::v3::Request::new(
                RuleKind::Content,
                word.to_owned(),
                true,
            ))
            .await
            .map_err(|err| failed(&err))?;
    }
    for rule in ruleset
        .content
        .iter()
        .filter(|rule| !rule.default && rule.pattern == word && rule.rule_id != word)
    {
        client
            .send(delete_pushrule::v3::Request::new(
                RuleKind::Content,
                rule.rule_id.clone(),
            ))
            .await
            .map_err(|err| failed(&err))?;
    }
    Ok(())
}

/// Deletes every content rule of the user's with the word as its pattern.
pub(crate) async fn remove_highlight_word(
    client: &Client,
    word: &str,
) -> Result<(), MatrixCoreError> {
    client
        .notification_settings()
        .await
        .remove_keyword(word)
        .await
        .map_err(|err| {
            MatrixCoreError::new(format!("failed to remove highlight word \"{word}\": {err}"))
        })
}

pub(crate) async fn set_display_name(
    client: &Client,
    request: &SetDisplayNameRequest,
) -> Result<(), MatrixCoreError> {
    client
        .account()
        .set_display_name(Some(&request.display_name))
        .await
        .map_err(|err| MatrixCoreError::new(format!("failed to set Matrix display name: {err}")))
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
                room_id: None,
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
            display_name_ambiguous: false,
            is_own_user: false,
            rank: RoomMemberRank::Member,
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

/// Orders members as the nick list shows them: highest rank first, then by
/// label, with the user ID breaking ties between equal labels.
fn room_members_from_summaries(room_id: &str, mut members: Vec<RoomMemberSummary>) -> RoomMembers {
    members.sort_by(|left, right| {
        left.rank
            .cmp(&right.rank)
            .then_with(|| {
                room_member_display_label(left)
                    .to_lowercase()
                    .cmp(&room_member_display_label(right).to_lowercase())
            })
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

/// Ranks a power level. Creator, administrator and moderator are the SDK's
/// suggested roles (infinite, 100 and 50); a lower level that still exceeds
/// the room's default for users is voiced.
fn room_member_rank(power_level: UserPowerLevel, users_default: Int) -> RoomMemberRank {
    match RoomMemberRole::suggested_role_for_power_level(power_level) {
        RoomMemberRole::Creator => RoomMemberRank::Creator,
        RoomMemberRole::Administrator => RoomMemberRank::Administrator,
        RoomMemberRole::Moderator => RoomMemberRank::Moderator,
        RoomMemberRole::User if power_level > users_default => RoomMemberRank::Voiced,
        RoomMemberRole::User => RoomMemberRank::Member,
    }
}

/// Summarizes `members`. `users_default` must come from the same power
/// levels the SDK used for the members' own levels.
fn room_members_from_matrix_members(
    room_id: &str,
    authenticated_user_id: &UserId,
    users_default: Int,
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
                    display_name_ambiguous: member.name_ambiguous(),
                    is_own_user: member.user_id() == authenticated_user_id,
                    rank: room_member_rank(member.power_level(), users_default),
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
    room_members_from_matrix_members(
        room.room_id().as_str(),
        authenticated_user_id,
        room_users_default_power_level(room).await,
        members,
    )
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
    room_members_from_matrix_members(
        room.room_id().as_str(),
        authenticated_user_id,
        room_users_default_power_level(room).await,
        members,
    )
}

/// The room's default power level for users. The SDK computes every
/// member's level from `power_levels_or_default`, which applies the spec
/// defaults when the room has no readable `m.room.power_levels` state, so
/// ranks compare against the same levels.
async fn room_users_default_power_level(room: &Room) -> Int {
    room.power_levels_or_default().await.users_default
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

/// Resolves `user_id`'s current profile in `room` from the SDK member store.
/// A user the store has never seen as a member has no room profile.
async fn room_user_profile(
    room: &Room,
    user_id: &UserId,
) -> Result<RoomUserProfile, MatrixCoreError> {
    let member = room.get_member_no_sync(user_id).await.map_err(|err| {
        MatrixCoreError::new(format!(
            "failed to read Matrix room member {user_id} in {}: {err}",
            room.room_id()
        ))
    })?;
    Ok(match member {
        Some(member) => RoomUserProfile {
            user_id: user_id.to_string(),
            display_name: member.display_name().map(str::to_owned),
            display_name_ambiguous: member.name_ambiguous(),
        },
        None => RoomUserProfile {
            user_id: user_id.to_string(),
            display_name: None,
            display_name_ambiguous: false,
        },
    })
}

/// Builds the profile of a membership event's target from the display name
/// that event carried. Ambiguity follows the SDK rule for members: the name
/// is ambiguous when it looks like a user ID or another user in the room
/// currently uses it.
async fn member_target_profile(
    room: &Room,
    target: &UserId,
    display_name: Option<String>,
) -> Result<RoomUserProfile, MatrixCoreError> {
    let display_name_ambiguous = match display_name.as_deref() {
        // Without a display name the target is shown by user ID.
        None => false,
        Some(name) => {
            let name = DisplayName::new(name);
            name.is_inherently_ambiguous()
                || room
                    .client()
                    .state_store()
                    .get_users_with_display_name(room.room_id(), &name)
                    .await
                    .map_err(|err| {
                        MatrixCoreError::new(format!(
                            "failed to read Matrix room display names in {}: {err}",
                            room.room_id()
                        ))
                    })?
                    .iter()
                    .any(|user_id| user_id != target)
        }
    };
    Ok(RoomUserProfile {
        user_id: target.to_string(),
        display_name,
        display_name_ambiguous,
    })
}

/// Sender profiles resolved while mapping one batch of events, so each
/// distinct sender is read from the store once.
struct SenderProfileCache<'a> {
    room: &'a Room,
    profiles: BTreeMap<OwnedUserId, RoomUserProfile>,
}

impl<'a> SenderProfileCache<'a> {
    fn new(room: &'a Room) -> Self {
        Self {
            room,
            profiles: BTreeMap::new(),
        }
    }

    async fn get(&mut self, user_id: &UserId) -> Result<RoomUserProfile, MatrixCoreError> {
        if let Some(profile) = self.profiles.get(user_id) {
            return Ok(profile.clone());
        }
        let profile = room_user_profile(self.room, user_id).await?;
        self.profiles.insert(user_id.to_owned(), profile.clone());
        Ok(profile)
    }
}

fn emit_sync_error(events: &EventSender, err: MatrixCoreError) {
    emit_event(
        events,
        Event::Error(CoreErrorEvent {
            operation: Operation::StartSync,
            room_id: None,
            message: err.to_string(),
        }),
    );
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
                        room_id: None,
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
                            room_id: None,
                            message: err.to_string(),
                        }),
                    ),
                },
                Err(_) => emit_event(
                    &events,
                    Event::Error(CoreErrorEvent {
                        operation: Operation::E2eeStatus,
                        room_id: None,
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

    // The base client stores global account data before it calls the event
    // handlers, so the snapshot reads the new rules from the store
    // (`push_rules_change_re_snapshots_room_notification_modes`).
    let push_rules_events = events.clone();
    client.add_event_handler(move |event: PushRulesEvent, client: Client| {
        let events = push_rules_events.clone();
        async move {
            emit_room_snapshot_or_room_list_error(&client, &events).await;
            emit_event(
                &events,
                Event::HighlightWords(highlight_words_from_ruleset(&event.content.global)),
            );
        }
    });

    let ignored_user_list_events = events.clone();
    client.add_event_handler(move |event: IgnoredUserListEvent| {
        let events = ignored_user_list_events.clone();
        async move {
            emit_event(
                &events,
                Event::IgnoredUsers(ignored_user_ids(&event.content)),
            );
        }
    });

    let timeline_events = events.clone();
    client.add_event_handler(
        move |event: OriginalSyncRoomMessageEvent, room: Room, actions: Vec<Action>| {
            let events = timeline_events.clone();
            async move {
                let room_id = room.room_id().as_str();
                if let Some(replacement) = sync_message_replacement(&event) {
                    if let Some(update) = room_activity_update_from_sync_message_replacement(
                        room_id,
                        &event,
                        replacement,
                    ) {
                        emit_event(&events, Event::RoomActivityUpdate(update));
                    }
                    return;
                }
                let highlight = activity_highlight(room.own_user_id(), &event.sender, &actions);
                match room_user_profile(&room, &event.sender).await {
                    Ok(sender) => {
                        if let Some(activity) =
                            room_activity_from_sync_message(room_id, &event, sender, highlight)
                        {
                            emit_event(&events, Event::RoomActivity(Box::new(activity)));
                        }
                    }
                    Err(err) => emit_sync_error(&events, err),
                }
            }
        },
    );

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

    // Timeline-only: the SDK also dispatches `SyncStateEvent` handlers for
    // the sync `state` section, which is the room's existing state, not
    // something that happened. Member snapshots follow each sync response
    // instead, see `emit_member_snapshots_after_sync`.
    let state_notice_events = events.clone();
    client.add_event_handler(
        move |event: AnySyncTimelineEvent, room: Room, actions: Vec<Action>| {
            let events = state_notice_events.clone();
            async move {
                match room_activity_from_timeline_state_event(&room, &event, &actions).await {
                    Ok(Some(activity)) => {
                        emit_event(&events, Event::RoomActivity(Box::new(activity)))
                    }
                    Ok(None) => {}
                    Err(err) => emit_sync_error(&events, err),
                }
            }
        },
    );

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
        move |event: ToDeviceEvent<ToDeviceKeyVerificationRequestEventContent>, client: Client| {
            let events = verification_events.clone();
            async move {
                emit_incoming_verification_request(
                    &client,
                    &events,
                    &event.sender,
                    event.content.transaction_id.as_str(),
                    &event.content.from_device,
                )
                .await;
            }
        },
    );

    // Another user verifying you (Element's "Verify user") asks in your DM,
    // with a room message whose event ID is the flow ID.
    let room_verification_events = events.clone();
    client.add_event_handler(move |event: OriginalSyncRoomMessageEvent, client: Client| {
        let events = room_verification_events.clone();
        async move {
            let MessageType::VerificationRequest(content) = &event.content.msgtype else {
                return;
            };
            let own_user_id = client.user_id().expect("sync runs signed in");
            // Your own requests come back by sync too.
            if content.to != own_user_id || event.sender == own_user_id {
                return;
            }
            emit_incoming_verification_request(
                &client,
                &events,
                &event.sender,
                event.event_id.as_str(),
                &content.from_device,
            )
            .await;
        }
    });
}

/// Announces a request another session sent this one, once the SDK holds
/// it as pending. The SDK drops requests older than ten minutes, which a
/// first sync replays; one done, cancelled or answered by another of your
/// sessions (passive) needs no answer from this one.
async fn emit_incoming_verification_request(
    client: &Client,
    events: &EventSender,
    sender: &UserId,
    flow_id: &str,
    from_device: &DeviceId,
) {
    let Some(request) = client
        .encryption()
        .get_verification_request(sender, flow_id)
        .await
    else {
        return;
    };
    if request.is_done() || request.is_cancelled() || request.is_passive() {
        return;
    }
    emit_event(
        events,
        Event::VerificationRequest(VerificationRequest {
            flow_id: flow_id.to_owned(),
            user_id: sender.to_string(),
            direction: VerificationDirection::Incoming {
                from_device: from_device.to_string(),
            },
        }),
    );
}

/// Whether the user's push rules highlight an event. The spec forbids
/// notifying for the user's own events, which push rule evaluation alone
/// does not exclude.
fn activity_highlight(own_user_id: &UserId, sender: &UserId, actions: &[Action]) -> bool {
    sender != own_user_id && actions.iter().any(Action::is_highlight)
}

type MessageReplacement =
    matrix_sdk::ruma::events::relation::Replacement<RoomMessageEventContentWithoutRelation>;

/// An edit replaces an existing activity instead of adding a timeline line.
fn sync_message_replacement(event: &OriginalSyncRoomMessageEvent) -> Option<&MessageReplacement> {
    match event.content.relates_to.as_ref()? {
        MessageRelation::Replacement(replacement) => Some(replacement),
        _ => None,
    }
}

fn room_activity_from_sync_message(
    room_id: &str,
    event: &OriginalSyncRoomMessageEvent,
    sender: RoomUserProfile,
    highlight: bool,
) -> Option<RoomActivity> {
    let relation = room_activity_relation_from_message(event);

    if let Some((presentation, text)) = room_message_presentation_and_text_from_message_type(
        &event.content.msgtype,
        relation.as_ref(),
    ) {
        return Some(RoomActivity {
            room_id: room_id.to_owned(),
            activity_id: format!("event:{}", event.event_id),
            timestamp_ms: event.origin_server_ts.get().into(),
            highlight,
            relation,
            kind: RoomActivityKind::Message(RoomMessageActivity {
                event_id: Some(event.event_id.to_string()),
                transaction_id: event
                    .unsigned
                    .transaction_id
                    .as_ref()
                    .map(ToString::to_string),
                sender,
                presentation,
                body: text.body,
                styles: text.styles,
                user_links: text.user_links,
                send_state: RoomMessageSendState::Sent,
            }),
        });
    }

    if let MessageType::Image(image) = &event.content.msgtype {
        return Some(RoomActivity {
            room_id: room_id.to_owned(),
            activity_id: format!("event:{}", event.event_id),
            timestamp_ms: event.origin_server_ts.get().into(),
            highlight,
            relation: relation.clone(),
            kind: RoomActivityKind::Image(Box::new(RoomImageActivity {
                event_id: event.event_id.to_string(),
                sender,
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
            highlight,
            relation: relation.clone(),
            kind: RoomActivityKind::File(Box::new(RoomFileActivity {
                event_id: event.event_id.to_string(),
                sender,
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
            highlight,
            relation: relation.clone(),
            kind: RoomActivityKind::Audio(Box::new(RoomAudioActivity {
                event_id: event.event_id.to_string(),
                sender,
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
            highlight,
            relation: relation.clone(),
            kind: RoomActivityKind::Video(Box::new(RoomVideoActivity {
                event_id: event.event_id.to_string(),
                sender,
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
        highlight,
        relation,
        kind: RoomActivityKind::UnsupportedMessage(UnsupportedRoomMessageActivity {
            event_id: event.event_id.to_string(),
            sender,
            msgtype,
            body,
        }),
    })
}

fn room_activity_update_from_sync_message_replacement(
    room_id: &str,
    event: &OriginalSyncRoomMessageEvent,
    replacement: &MessageReplacement,
) -> Option<RoomActivityUpdate> {
    // The replacement's own content is a `* `-prefixed fallback for
    // clients without edits (the SDK even writes a `* ` formatted body
    // there); the edit is `m.new_content`, which has no relation.
    let (presentation, text) = room_message_presentation_and_text_from_message_type(
        &replacement.new_content.msgtype,
        None,
    )?;

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
            body: text.body,
            styles: text.styles,
            user_links: text.user_links,
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

/// A reply's body without the plain-text quote of the message it replies
/// to (`> <@alice:example.org> hi`), which older clients still send: the UI
/// shows what a reply answers from its relation, so the quote would repeat
/// it. Only replies can carry one; any other body is the user's text, even
/// when it starts with `> <`.
fn strip_reply_fallback(relation: Option<&RoomActivityRelation>, body: String) -> String {
    match relation {
        Some(RoomActivityRelation::Reply { .. })
        | Some(RoomActivityRelation::Thread {
            in_reply_to_event_id: Some(_),
            ..
        }) => remove_plain_reply_fallback(&body).to_owned(),
        Some(RoomActivityRelation::Thread {
            in_reply_to_event_id: None,
            ..
        })
        | None => body,
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

/// A text, notice or emote message's presentation and text: its HTML
/// `formatted_body` when it has one with text, else its plain body without
/// a reply fallback for `relation`. HTML loses its rich reply fallback
/// whatever the relation, as the sanitizer drops every `<mx-reply>`.
fn room_message_presentation_and_text_from_message_type(
    message_type: &MessageType,
    relation: Option<&RoomActivityRelation>,
) -> Option<(RoomMessagePresentationKind, StyledText)> {
    let (presentation, body, formatted) = match message_type {
        MessageType::Text(text) => (
            RoomMessagePresentationKind::Text,
            &text.body,
            text.formatted.as_ref(),
        ),
        MessageType::Notice(notice) => (
            RoomMessagePresentationKind::Notice,
            &notice.body,
            notice.formatted.as_ref(),
        ),
        MessageType::Emote(emote) => (
            RoomMessagePresentationKind::Emote,
            &emote.body,
            emote.formatted.as_ref(),
        ),
        MessageType::Image(_)
        | MessageType::Audio(_)
        | MessageType::File(_)
        | MessageType::Video(_)
        | MessageType::Location(_)
        | MessageType::VerificationRequest(_)
        | MessageType::_Custom(_) => return None,
        // ruma marks MessageType as non_exhaustive, so a final fallback is required.
        _ => return None,
    };
    let text = formatted
        .and_then(styled_text_from_formatted_body)
        .unwrap_or_else(|| StyledText::plain(strip_reply_fallback(relation, body.clone())));
    Some((presentation, text))
}

/// An HTML formatted body's text, or `None` for another format or HTML
/// without text (only a reply fallback, say): the plain body is then the
/// sender's own rendering of it.
fn styled_text_from_formatted_body(formatted: &FormattedBody) -> Option<StyledText> {
    if formatted.format != MessageFormat::Html {
        return None;
    }
    let text = styled_text_from_html(&formatted.body);
    (!text.body.is_empty()).then_some(text)
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

/// The timeline line for a state event in a sync timeline: a topic, member
/// or power-level change. Every other timeline event has its own handler or
/// no line.
async fn room_activity_from_timeline_state_event(
    room: &Room,
    event: &AnySyncTimelineEvent,
    actions: &[Action],
) -> Result<Option<RoomActivity>, MatrixCoreError> {
    let AnySyncTimelineEvent::State(event) = event else {
        return Ok(None);
    };
    let mut senders = SenderProfileCache::new(room);
    match event {
        AnySyncStateEvent::RoomTopic(SyncStateEvent::Original(event)) => {
            let highlight = activity_highlight(room.own_user_id(), &event.sender, actions);
            let sender = senders.get(&event.sender).await?;
            Ok(Some(room_activity_from_sync_topic(
                room.room_id().as_str(),
                event,
                sender,
                highlight,
            )))
        }
        AnySyncStateEvent::RoomMember(SyncStateEvent::Original(event)) => {
            let highlight = activity_highlight(room.own_user_id(), &event.sender, actions);
            room_activity_from_member_event(room, event, &mut senders, highlight).await
        }
        AnySyncStateEvent::RoomPowerLevels(SyncStateEvent::Original(event)) => {
            let highlight = activity_highlight(room.own_user_id(), &event.sender, actions);
            room_activity_from_power_levels_event(room, event, &mut senders, highlight).await
        }
        _ => Ok(None),
    }
}

fn room_activity_from_sync_topic(
    room_id: &str,
    event: &OriginalSyncRoomTopicEvent,
    sender: RoomUserProfile,
    highlight: bool,
) -> RoomActivity {
    RoomActivity {
        room_id: room_id.to_owned(),
        activity_id: format!("state:{}", event.event_id),
        timestamp_ms: event.origin_server_ts.get().into(),
        highlight,
        relation: None,
        kind: RoomActivityKind::StateNotice(RoomStateNoticeActivity {
            event_id: event.event_id.to_string(),
            sender,
            content: RoomStateNoticeContent::Topic {
                topic: event.content.topic.to_string(),
            },
        }),
    }
}

/// Classifies a membership event. `None` means the event changed nothing a
/// timeline line could report: no transition, a transition the spec forbids,
/// or one ruma does not classify.
fn room_member_change_from_sync_member(
    event: &OriginalSyncRoomMemberEvent,
) -> Option<RoomMemberChange> {
    Some(match event.membership_change() {
        MembershipChange::None | MembershipChange::Error | MembershipChange::NotImplemented => {
            return None;
        }
        MembershipChange::Joined => RoomMemberChange::Joined,
        MembershipChange::Left => RoomMemberChange::Left,
        MembershipChange::Kicked => RoomMemberChange::Kicked,
        MembershipChange::Banned => RoomMemberChange::Banned,
        MembershipChange::KickedAndBanned => RoomMemberChange::KickedAndBanned,
        MembershipChange::Unbanned => RoomMemberChange::Unbanned,
        MembershipChange::Invited => RoomMemberChange::Invited,
        MembershipChange::InvitationAccepted => RoomMemberChange::InvitationAccepted,
        MembershipChange::InvitationRejected => RoomMemberChange::InvitationRejected,
        MembershipChange::InvitationRevoked => RoomMemberChange::InvitationRevoked,
        MembershipChange::Knocked => RoomMemberChange::Knocked,
        MembershipChange::KnockAccepted => RoomMemberChange::KnockAccepted,
        MembershipChange::KnockRetracted => RoomMemberChange::KnockRetracted,
        MembershipChange::KnockDenied => RoomMemberChange::KnockDenied,
        // A combined rename and avatar change is reported as the rename.
        MembershipChange::ProfileChanged {
            displayname_change: Some(change),
            ..
        } => RoomMemberChange::DisplayNameChanged {
            old: change.old.map(str::to_owned),
            new: change.new.map(str::to_owned),
        },
        MembershipChange::ProfileChanged {
            displayname_change: None,
            avatar_url_change: Some(_),
        } => RoomMemberChange::AvatarChanged,
        MembershipChange::ProfileChanged {
            displayname_change: None,
            avatar_url_change: None,
        } => unreachable!("ruma reports ProfileChanged only when the name or avatar differs"),
        // ruma marks MembershipChange as non_exhaustive.
        _ => return None,
    })
}

/// The display name a membership event's target had: the event's own for
/// memberships that describe the member (join, invite, knock), the previous
/// one for leave and ban events, which carry no profile.
fn member_target_display_name(event: &OriginalSyncRoomMemberEvent) -> Option<String> {
    match event.content.membership {
        MembershipState::Join | MembershipState::Invite | MembershipState::Knock => {
            event.content.displayname.clone()
        }
        _ => event
            .unsigned
            .prev_content
            .as_ref()
            .and_then(|prev| prev.displayname.clone()),
    }
}

fn room_activity_from_sync_member(
    room_id: &str,
    event: &OriginalSyncRoomMemberEvent,
    change: RoomMemberChange,
    sender: RoomUserProfile,
    target: RoomUserProfile,
    highlight: bool,
) -> RoomActivity {
    RoomActivity {
        room_id: room_id.to_owned(),
        activity_id: format!("state:{}", event.event_id),
        timestamp_ms: event.origin_server_ts.get().into(),
        highlight,
        relation: None,
        kind: RoomActivityKind::StateNotice(RoomStateNoticeActivity {
            event_id: event.event_id.to_string(),
            sender,
            content: RoomStateNoticeContent::Member {
                change,
                target,
                reason: event.content.reason.clone(),
            },
        }),
    }
}

async fn room_activity_from_member_event(
    room: &Room,
    event: &OriginalSyncRoomMemberEvent,
    senders: &mut SenderProfileCache<'_>,
    highlight: bool,
) -> Result<Option<RoomActivity>, MatrixCoreError> {
    let Some(change) = room_member_change_from_sync_member(event) else {
        return Ok(None);
    };
    let sender = senders.get(&event.sender).await?;
    let target =
        member_target_profile(room, &event.state_key, member_target_display_name(event)).await?;
    Ok(Some(room_activity_from_sync_member(
        room.room_id().as_str(),
        event,
        change,
        sender,
        target,
        highlight,
    )))
}

/// Whether a joined room's sync update can change its member snapshot: a
/// membership or power-level event in its state section or timeline. Power
/// levels decide every member's rank and our moderation rights. Only the
/// event type is decoded.
fn joined_room_update_changes_members(update: &JoinedRoomUpdate) -> bool {
    #[derive(Deserialize)]
    struct EventType<'a> {
        #[serde(borrow, rename = "type")]
        event_type: Cow<'a, str>,
    }
    fn changes_members(event_type: &str) -> bool {
        matches!(event_type, "m.room.member" | "m.room.power_levels")
    }

    let (State::Before(state) | State::After(state)) = &update.state;
    state
        .iter()
        .map(|raw| raw.deserialize_as_unchecked::<EventType<'_>>())
        .chain(
            update
                .timeline
                .events
                .iter()
                .map(|event| event.raw().deserialize_as_unchecked::<EventType<'_>>()),
        )
        .any(|event_type| {
            changes_members(
                &event_type
                    .expect("the SDK decoded the type of every event of a processed sync response")
                    .event_type,
            )
        })
}

/// Emits one fresh member snapshot for each joined room whose member list
/// this sync response can have changed. Runs after the SDK has processed the
/// response and run its event handlers, so timeline lines such as a join
/// precede the refreshed nicklist.
async fn emit_member_snapshots_after_sync(
    client: &Client,
    response: &SyncResponse,
    events: &EventSender,
) {
    for (room_id, update) in &response.rooms.joined {
        if !joined_room_update_changes_members(update) {
            continue;
        }
        let room = client
            .get_room(room_id)
            .expect("the SDK stores every room of a processed sync response");
        emit_joined_room_members_after_state_change(client, &room, events).await;
    }
}

/// Emits a fresh member snapshot for a room whose state changed. Only a
/// joined room has one: a room in the response's `joined` section can also
/// be left later in the same response.
async fn emit_joined_room_members_after_state_change(
    client: &Client,
    room: &Room,
    events: &EventSender,
) {
    if room.state() != RoomState::Joined {
        return;
    }
    let authenticated_user_id = client
        .user_id()
        .expect("sync event handlers run only for an authenticated client");
    match joined_room_members_no_sync(authenticated_user_id, room).await {
        Ok(snapshot) => emit_event(events, Event::RoomMembers(snapshot)),
        Err(err) => emit_sync_error(events, err),
    }
}

/// What an `m.room.power_levels` event changed, before target profiles are
/// resolved.
#[derive(Debug, PartialEq, Eq)]
struct PowerLevelsChange {
    /// `(user, old, new)` effective levels, ordered by user ID.
    users: Vec<(OwnedUserId, i64, i64)>,
    users_default: Option<RoomPowerLevelValueChange>,
    other_permissions_changed: bool,
}

/// Compares power levels with the ones they replace. A user's effective
/// level is their explicit one or else the default for users, so changing
/// the default also changes the level of every user without an explicit
/// one; those users are not listed, the default change reports them.
/// `None` when nothing changed.
fn power_levels_change(
    old: &PossiblyRedactedRoomPowerLevelsEventContent,
    new: &RoomPowerLevelsEventContent,
) -> Option<PowerLevelsChange> {
    let users = old
        .users
        .keys()
        .chain(new.users.keys())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .filter_map(|user_id| {
            let old_level = old.users.get(user_id).copied().unwrap_or(old.users_default);
            let new_level = new.users.get(user_id).copied().unwrap_or(new.users_default);
            (old_level != new_level).then(|| (user_id.clone(), old_level.into(), new_level.into()))
        })
        .collect::<Vec<_>>();
    let users_default =
        (old.users_default != new.users_default).then(|| RoomPowerLevelValueChange {
            old: old.users_default.into(),
            new: new.users_default.into(),
        });
    let other_permissions_changed = old.ban != new.ban
        || old.kick != new.kick
        || old.redact != new.redact
        || old.invite != new.invite
        || old.events != new.events
        || old.events_default != new.events_default
        || old.state_default != new.state_default
        || old.notifications.room != new.notifications.room;

    (!users.is_empty() || users_default.is_some() || other_permissions_changed).then_some(
        PowerLevelsChange {
            users,
            users_default,
            other_permissions_changed,
        },
    )
}

/// The timeline notice for a power-level change. Without `prev_content`
/// (the room's first levels, or state replayed without history) there is
/// nothing to compare against, so there is no notice.
async fn room_activity_from_power_levels_event(
    room: &Room,
    event: &OriginalSyncRoomPowerLevelsEvent,
    profiles: &mut SenderProfileCache<'_>,
    highlight: bool,
) -> Result<Option<RoomActivity>, MatrixCoreError> {
    let Some(old) = event.unsigned.prev_content.as_ref() else {
        return Ok(None);
    };
    let Some(change) = power_levels_change(old, &event.content) else {
        return Ok(None);
    };
    let sender = profiles.get(&event.sender).await?;
    let mut changes = Vec::with_capacity(change.users.len());
    for (user_id, old, new) in change.users {
        changes.push(RoomPowerLevelUserChange {
            target: profiles.get(&user_id).await?,
            old,
            new,
        });
    }
    Ok(Some(RoomActivity {
        room_id: room.room_id().to_string(),
        activity_id: format!("state:{}", event.event_id),
        timestamp_ms: event.origin_server_ts.get().into(),
        highlight,
        relation: None,
        kind: RoomActivityKind::StateNotice(RoomStateNoticeActivity {
            event_id: event.event_id.to_string(),
            sender,
            content: RoomStateNoticeContent::PowerLevels {
                changes,
                users_default: change.users_default,
                other_permissions_changed: change.other_permissions_changed,
            },
        }),
    }))
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

/// Starts the verification `request` names (see
/// `RequestVerificationRequest`), offering SAS only: mirx cannot show a QR
/// code. Returns the announcement and the SDK's flow, which the caller
/// watches for the other side's answer.
pub(crate) async fn request_verification(
    client: &Client,
    request: &RequestVerificationRequest,
) -> Result<(VerificationRequest, MatrixVerificationRequest), MatrixCoreError> {
    let user_id = parse_verification_user_id(&request.user_id)?;
    let own_user_id = client.user_id().expect("verification runs signed in");
    let encryption = client.encryption();
    let methods = vec![VerificationMethod::SasV1];
    let failed =
        |err: &dyn Display| MatrixCoreError::new(format!("failed to request verification: {err}"));

    let flow = match request.device_id.as_deref() {
        Some(device_id) => {
            let device_id = <&DeviceId>::from(device_id);
            if user_id == own_user_id && Some(device_id) == client.device_id() {
                return Err(MatrixCoreError::new(format!("{device_id} is this session")));
            }
            // `/keys/query` first: the store may not know the user's
            // devices, or know them as they were.
            encryption
                .request_user_identity(&user_id)
                .await
                .map_err(|err| failed(&err))?;
            let device = encryption
                .get_device(&user_id, device_id)
                .await
                .map_err(|err| failed(&err))?
                .ok_or_else(|| {
                    MatrixCoreError::new(format!("{user_id} has no device {device_id}"))
                })?;
            device
                .request_verification_with_methods(methods)
                .await
                .map_err(|err| failed(&err))?
        }
        None if user_id == own_user_id => {
            let identity = encryption
                .get_user_identity(&user_id)
                .await
                .map_err(|err| failed(&err))?
                .ok_or_else(|| {
                    MatrixCoreError::new(
                        "cross-signing is not set up for your account; use /e2ee setup",
                    )
                })?;
            // The SDK asks only the sessions your identity signed.
            let this_device = client.device_id().expect("verification runs signed in");
            let devices = encryption
                .get_user_devices(&user_id)
                .await
                .map_err(|err| failed(&err))?;
            if !devices.devices().any(|device| {
                device.device_id() != this_device && device.is_cross_signed_by_owner()
            }) {
                return Err(MatrixCoreError::new(
                    "none of your other sessions is verified; use /e2ee recover with your recovery key",
                ));
            }
            identity
                .request_verification_with_methods(methods)
                .await
                .map_err(|err| failed(&err))?
        }
        None => {
            let identity = encryption
                .request_user_identity(&user_id)
                .await
                .map_err(|err| failed(&err))?
                .ok_or_else(|| {
                    MatrixCoreError::new(format!(
                        "{user_id} has not set up cross-signing; verify one of their devices: /verify {user_id} <device>"
                    ))
                })?;
            identity
                .request_verification_with_methods(methods)
                .await
                .map_err(|err| failed(&err))?
        }
    };

    let announcement = VerificationRequest {
        flow_id: flow.flow_id().to_owned(),
        user_id: flow.other_user_id().to_string(),
        direction: VerificationDirection::Outgoing {
            to_device: request.device_id.clone(),
        },
    };
    Ok((announcement, flow))
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

/// Follows a request's flow until it ends. Either side may start SAS once
/// both are ready: this session starts it on Ready, and accepts one the
/// other side started; the SDK settles a start race, replacing one SAS by
/// the other.
pub(crate) async fn watch_verification_request(
    request: MatrixVerificationRequest,
    flow: VerificationFlowId,
    events: EventSender,
) {
    let mut state = request.state();
    let mut changes = request.changes();
    // `Event::VerificationRequest` announced the flow, as requested or sent.
    if let Some(event) = map_request_state(&flow, state.clone())
        && !matches!(
            event.state,
            VerificationState::Requested | VerificationState::Sent
        )
    {
        emit_event(&events, Event::VerificationState(event));
    }

    loop {
        match state {
            VerificationRequestState::Created { .. }
            | VerificationRequestState::Requested { .. } => {}
            // The request moves on to Transitioned, which follows.
            VerificationRequestState::Ready { .. } => {
                if let Err(err) = request.start_sas().await {
                    emit_verification_error(
                        &events,
                        format!("failed to start SAS verification: {err}"),
                    );
                    return;
                }
            }
            VerificationRequestState::Transitioned { verification } => {
                // Only SAS is offered, so the other side cannot start another
                // method.
                let Some(sas) = verification.sas() else {
                    emit_verification_error(
                        &events,
                        "the other side started a verification method other than SAS".to_owned(),
                    );
                    return;
                };
                let Some(replaced) = follow_sas(&sas, &flow, &events, &mut changes).await else {
                    return;
                };
                state = replaced;
                continue;
            }
            VerificationRequestState::Done | VerificationRequestState::Cancelled(_) => return,
        }

        let Some(next) = changes.next().await else {
            return;
        };
        if let Some(event) = map_request_state(&flow, next.clone()) {
            emit_event(&events, Event::VerificationState(event));
        }
        state = next;
    }
}

/// Follows a SAS until it ends (`None`), or until the request transitions
/// to another one: the other side's, which won a start race.
async fn follow_sas(
    sas: &MatrixSasVerification,
    flow: &VerificationFlowId,
    events: &EventSender,
    request_changes: &mut (impl futures_util::Stream<Item = VerificationRequestState> + Unpin),
) -> Option<VerificationRequestState> {
    // The other side started it. Accepting goes on to the key exchange; the
    // user still compares the emoji before anything is verified.
    if matches!(sas.state(), SasState::Started { .. })
        && let Err(err) = sas.accept().await
    {
        emit_verification_error(events, format!("failed to accept SAS verification: {err}"));
        return None;
    }
    if let Some(event) = map_sas_verification(flow, sas)
        && emit_verification_state(events, event)
    {
        return None;
    }

    let mut sas_changes = sas.changes();
    loop {
        tokio::select! {
            state = sas_changes.next() => {
                let state = state?;
                if let Some(event) = map_sas_state(flow, state)
                    && emit_verification_state(events, event)
                {
                    return None;
                }
            }
            // The SAS reports how the flow ends; a new one replaces it.
            state = request_changes.next() => {
                let state = state?;
                if matches!(state, VerificationRequestState::Transitioned { .. }) {
                    return Some(state);
                }
            }
        }
    }
}

pub(crate) async fn watch_sas_verification(
    sas: MatrixSasVerification,
    flow: VerificationFlowId,
    events: EventSender,
) {
    if let Some(event) = map_sas_verification(&flow, &sas)
        && emit_verification_state(&events, event)
    {
        return;
    }

    let mut changes = sas.changes();
    while let Some(state) = changes.next().await {
        if let Some(event) = map_sas_state(&flow, state)
            && emit_verification_state(&events, event)
        {
            return;
        }
    }
}

/// Emits the state; whether it ends the flow.
fn emit_verification_state(events: &EventSender, event: VerificationStateEvent) -> bool {
    let terminal = matches!(
        event.state,
        VerificationState::Done | VerificationState::Cancelled { .. }
    );
    emit_event(events, Event::VerificationState(event));
    terminal
}

fn emit_verification_error(events: &EventSender, message: String) {
    emit_event(
        events,
        Event::Error(CoreErrorEvent {
            operation: Operation::Verification,
            room_id: None,
            message,
        }),
    );
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
            Some(verification_state_event(flow, VerificationState::Requested))
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
        VerificationRequestState::Created { .. } => {
            Some(verification_state_event(flow, VerificationState::Sent))
        }
        VerificationRequestState::Transitioned { .. } => None,
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

/// Runs continuous sync until it fails: the initial snapshot if it is still
/// to come and `SyncConnected` at the first response, then `SyncStopped`
/// with why it ended.
pub(crate) fn start_sync_task(
    client: Client,
    events: EventSender,
    initial_snapshot: InitialSnapshot,
    paused_sends: PausedSendRooms,
) -> JoinHandle<()> {
    tokio::task::spawn_local(async move {
        let connected = Cell::new(false);
        let result = client
            .sync_with_result_callback(SyncSettings::default(), |result| {
                let client = client.clone();
                let events = events.clone();
                let connected = &connected;
                let initial_snapshot = &initial_snapshot;
                let paused_sends = &paused_sends;
                async move {
                    let response = result?;
                    if !connected.replace(true) {
                        // Before SyncConnected: the UI's connected session
                        // has its rooms.
                        initial_snapshot.emit_if_pending(&client, &events).await;
                        emit_event(&events, Event::SyncConnected);
                    }
                    // The server answers again.
                    paused_sends.resume(&client);
                    emit_member_snapshots_after_sync(&client, &response, &events).await;
                    Ok(LoopCtrl::Continue)
                }
            })
            .await;
        let err = result.expect_err("the sync callback never breaks the loop");
        emit_event(&events, Event::SyncStopped(sync_stop_reason(&err)));
    })
}

/// Rooms whose send queue a recoverable error stopped (the network, the
/// server): their requests stay queued, and the next sync response, which
/// shows the server answers again, starts the queues again. Shared by the
/// send queue's update task and the session's sync tasks.
#[derive(Clone, Default)]
pub(crate) struct PausedSendRooms(Rc<RefCell<BTreeSet<OwnedRoomId>>>);

impl PausedSendRooms {
    fn pause(&self, room_id: OwnedRoomId) {
        self.0.borrow_mut().insert(room_id);
    }

    fn resume(&self, client: &Client) {
        for room_id in std::mem::take(&mut *self.0.borrow_mut()) {
            // A room left and forgotten meanwhile has no queue left.
            if let Some(room) = client.get_room(&room_id) {
                // Per room: the client-wide switch would also scan the store
                // for every room with unsent requests.
                room.send_queue().set_enabled(true);
            }
        }
    }
}

/// Only a token the server no longer accepts is final; mIRC retries any
/// other disconnection.
fn sync_stop_reason(err: &matrix_sdk::Error) -> SyncStopReason {
    let message = err.to_string();
    match err.client_api_error_kind() {
        Some(ErrorKind::UnknownToken { .. }) => SyncStopReason::SessionEnded(message),
        _ => SyncStopReason::Lost(message),
    }
}

/// Emits a joined room's read state whenever it changes. `updates` is taken
/// before the session's first sync: the event cache counts that sync's unread
/// messages in its own task, possibly after the initial snapshot was read,
/// and the change must still reach the UI.
pub(crate) fn start_room_read_state_update_task(
    client: Client,
    events: EventSender,
    mut updates: broadcast::Receiver<RoomInfoNotableUpdate>,
) -> JoinHandle<()> {
    tokio::task::spawn_local(async move {
        loop {
            let read_states = match updates.recv().await {
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
                    vec![room_read_state_from_room(&room)]
                }
                Err(broadcast::error::RecvError::Closed) => break,
                // A read state is a snapshot: resending every room's makes
                // up for the missed updates.
                Err(broadcast::error::RecvError::Lagged(_)) => {
                    summarize_joined_room_read_states(&client)
                }
            };
            for read_state in read_states {
                if events.send(Event::RoomReadState(read_state)).is_err() {
                    return;
                }
            }
        }
    })
}

pub(crate) fn start_room_list_update_task(client: Client, events: EventSender) -> JoinHandle<()> {
    let mut updates = client.room_info_notable_update_receiver();

    tokio::task::spawn_local(async move {
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
                        room_id: None,
                        message: "missed Matrix room-list update".to_owned(),
                    }),
                ),
            }
        }
    })
}

pub(crate) fn start_session_update_task(client: Client, events: EventSender) -> JoinHandle<()> {
    let mut session_changes = client.subscribe_to_session_changes();

    tokio::task::spawn_local(async move {
        loop {
            match session_changes.recv().await {
                Ok(SessionChange::TokensRefreshed) => emit_event(
                    &events,
                    Event::SessionUpdated {
                        session: stored_session(&client),
                    },
                ),
                Ok(SessionChange::UnknownToken { .. }) => {}
                Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => emit_event(
                    &events,
                    Event::Error(CoreErrorEvent {
                        operation: Operation::SessionUpdate,
                        room_id: None,
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
    paused_sends: PausedSendRooms,
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
                    operation: Operation::SendQueue,
                    room_id: None,
                    message: format!("failed to hydrate Matrix send queue local echoes: {err}"),
                }),
            ),
        }

        loop {
            match updates.recv().await {
                Ok(update) => {
                    if let RoomSendQueueUpdate::SendError { is_recoverable, .. } = &update.update {
                        // The SDK stops the room's queue at any send error.
                        if *is_recoverable {
                            paused_sends.pause(update.room_id.clone());
                        } else {
                            // The request is wedged and the queue skips it:
                            // the room's later sends go now.
                            client
                                .get_room(&update.room_id)
                                .expect("the SDK reports send errors of known rooms")
                                .send_queue()
                                .set_enabled(true);
                        }
                    }
                    let room_id = update.room_id.to_string();
                    match own_room_user_profile(&client, &update.room_id, &current_user_id)
                        .await
                        .and_then(|own_profile| {
                            map_send_queue_update(update, &own_profile, &pending)
                        }) {
                        Ok(mapped_events) => {
                            for event in mapped_events {
                                emit_event(&events, event);
                            }
                        }
                        Err(err) => emit_event(
                            &events,
                            Event::Error(CoreErrorEvent {
                                operation: Operation::SendMessage,
                                room_id: Some(room_id),
                                message: format!("failed to map Matrix send queue update: {err}"),
                            }),
                        ),
                    }
                }
                Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => emit_event(
                    &events,
                    Event::Error(CoreErrorEvent {
                        operation: Operation::SendQueue,
                        room_id: None,
                        message: "missed Matrix send queue update".to_owned(),
                    }),
                ),
            }
        }
    })
}

/// The authenticated user's profile in a room with queued sends; local echoes
/// are attributed to it.
async fn own_room_user_profile(
    client: &Client,
    room_id: &RoomId,
    current_user_id: &str,
) -> Result<RoomUserProfile, MatrixCoreError> {
    let room = client.get_room(room_id).ok_or_else(|| {
        MatrixCoreError::new(format!(
            "Matrix send queue reported unknown room: {room_id}"
        ))
    })?;
    let user_id = UserId::parse(current_user_id).map_err(|err| {
        MatrixCoreError::new(format!(
            "invalid authenticated user id {current_user_id}: {err}"
        ))
    })?;
    room_user_profile(&room, &user_id).await
}

fn map_send_queue_update(
    update: SendQueueUpdate,
    own_profile: &RoomUserProfile,
    pending: &PendingOutgoingRegistry,
) -> Result<Vec<Event>, MatrixCoreError> {
    let room_id = update.room_id.to_string();

    match update.update {
        RoomSendQueueUpdate::NewLocalEvent(local_echo) => {
            map_local_echo(room_id, local_echo, own_profile, pending)
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
        // The request stays queued and goes once the connection is back
        // (`PausedSendRooms`): nothing failed yet, so no error, and its
        // `SentEvent` still finds its kind. Edits, reactions and redactions
        // show nothing until then.
        RoomSendQueueUpdate::SendError {
            transaction_id,
            error,
            is_recoverable: true,
        } => {
            let transaction_id = transaction_id.to_string();
            Ok(match pending_outgoing_kind(pending, &transaction_id) {
                Some(PendingOutgoingKind::Message) => {
                    vec![Event::RoomActivityUpdate(RoomActivityUpdate {
                        room_id,
                        target: RoomActivityUpdateTarget::Transaction { transaction_id },
                        kind: RoomActivityUpdateKind::MessageFailed {
                            message: error.to_string(),
                            recoverable: true,
                        },
                    })]
                }
                Some(
                    PendingOutgoingKind::MessageEdit { .. }
                    | PendingOutgoingKind::Reaction { .. }
                    | PendingOutgoingKind::Redaction { .. },
                )
                | None => Vec::new(),
            })
        }
        // The SDK wedged the request: it will not be sent.
        RoomSendQueueUpdate::SendError {
            transaction_id,
            error,
            is_recoverable: false,
        } => {
            let transaction_id = transaction_id.to_string();
            let message = error.to_string();
            Ok(match take_pending_outgoing_kind(pending, &transaction_id) {
                Some(PendingOutgoingKind::Message) => vec![
                    Event::RoomActivityUpdate(RoomActivityUpdate {
                        room_id: room_id.clone(),
                        target: RoomActivityUpdateTarget::Transaction { transaction_id },
                        kind: RoomActivityUpdateKind::MessageFailed {
                            message: message.clone(),
                            recoverable: false,
                        },
                    }),
                    Event::Error(CoreErrorEvent {
                        operation: Operation::SendMessage,
                        room_id: Some(room_id),
                        message,
                    }),
                ],
                Some(PendingOutgoingKind::MessageEdit { .. }) => {
                    vec![Event::Error(CoreErrorEvent {
                        operation: Operation::EditMessage,
                        room_id: Some(room_id),
                        message,
                    })]
                }
                Some(PendingOutgoingKind::Reaction { .. }) => vec![Event::Error(CoreErrorEvent {
                    operation: Operation::SendReaction,
                    room_id: Some(room_id),
                    message,
                })],
                Some(PendingOutgoingKind::Redaction { .. }) => vec![Event::Error(CoreErrorEvent {
                    operation: Operation::RedactEvent,
                    room_id: Some(room_id),
                    message,
                })],
                None => vec![Event::Error(CoreErrorEvent {
                    operation: Operation::SendMessage,
                    room_id: Some(room_id),
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
        let own_profile = own_room_user_profile(client, &room_id, current_user_id).await?;
        for local_echo in room_local_echoes {
            events.extend(map_local_echo(
                room_id.to_string(),
                local_echo,
                &own_profile,
                pending,
            )?);
        }
    }

    Ok(events)
}

fn map_local_echo(
    room_id: String,
    local_echo: LocalEcho,
    own_profile: &RoomUserProfile,
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
            own_profile,
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
                    room_id: Some(room_id),
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
    own_profile: &RoomUserProfile,
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
                room_id: Some(room_id),
                message: send_error,
            }));
        }

        return Ok(events);
    }

    if let Some(activity) = map_local_message_activity(
        room_id.clone(),
        transaction_id.clone(),
        own_profile,
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
                room_id: room_id.clone(),
                target: RoomActivityUpdateTarget::Transaction { transaction_id },
                kind: RoomActivityUpdateKind::MessageFailed {
                    message: send_error.clone(),
                    recoverable: false,
                },
            }));
            events.push(Event::Error(CoreErrorEvent {
                operation: Operation::SendMessage,
                room_id: Some(room_id),
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
            room_id: Some(room_id),
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

fn pending_outgoing_kind(
    pending: &PendingOutgoingRegistry,
    transaction_id: &str,
) -> Option<PendingOutgoingKind> {
    pending
        .lock()
        .expect("lock pending outgoing registry")
        .get(transaction_id)
        .cloned()
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
    text: StyledText,
    relation: Option<RoomActivityRelation>,
}

fn map_local_message_activity(
    room_id: String,
    transaction_id: String,
    own_profile: &RoomUserProfile,
    created_at: MilliSecondsSinceUnixEpoch,
    serialized_event: &SerializableEventContent,
) -> Result<Option<RoomActivity>, MatrixCoreError> {
    let Some(LocalEchoMessageDetails {
        presentation,
        text,
        relation,
    }) = message_presentation_and_text_from_serialized_event(serialized_event)?
    else {
        return Ok(None);
    };

    Ok(Some(RoomActivity {
        room_id,
        activity_id: format!("txn:{transaction_id}"),
        timestamp_ms: created_at.get().into(),
        // Our own pending message: never a highlight.
        highlight: false,
        relation,
        kind: RoomActivityKind::Message(RoomMessageActivity {
            event_id: None,
            transaction_id: Some(transaction_id),
            sender: own_profile.clone(),
            presentation,
            body: text.body,
            styles: text.styles,
            user_links: text.user_links,
            send_state: RoomMessageSendState::Pending,
        }),
    }))
}

fn message_presentation_and_text_from_serialized_event(
    serialized_event: &SerializableEventContent,
) -> Result<Option<LocalEchoMessageDetails>, MatrixCoreError> {
    let AnyMessageLikeEventContent::RoomMessage(content) = serialized_event
        .deserialize()
        .map_err(|err| MatrixCoreError::new(format!("failed to deserialize local echo: {err}")))?
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

    let Some((presentation, text)) =
        room_message_presentation_and_text_from_message_type(&content.msgtype, relation.as_ref())
    else {
        return Ok(None);
    };

    Ok(Some(LocalEchoMessageDetails {
        presentation,
        text,
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

/// The message type for a text, notice or emote: plain `body`, plus an
/// HTML `formatted_body` when `styles` format it or `pills` link names in
/// it. Text with neither has no `formatted_body` at all.
fn room_message_type(
    presentation: RoomMessagePresentationKind,
    body: String,
    styles: &[TextStyleSpan],
    pills: &[UserLinkSpan],
) -> MessageType {
    if styles.is_empty() && pills.is_empty() {
        return match presentation {
            RoomMessagePresentationKind::Text => MessageType::text_plain(body),
            RoomMessagePresentationKind::Notice => MessageType::notice_plain(body),
            RoomMessagePresentationKind::Emote => MessageType::emote_plain(body),
        };
    }
    let html = html_from_styled_text(&body, styles, pills);
    match presentation {
        RoomMessagePresentationKind::Text => MessageType::text_html(body, html),
        RoomMessagePresentationKind::Notice => MessageType::notice_html(body, html),
        RoomMessagePresentationKind::Emote => MessageType::emote_html(body, html),
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

/// The `m.mentions` (MSC3952) and pills of a message with `body`, replying
/// to `reply` if it does: the members it names (see `body_mentions`), the
/// room if it says `@room`, and the replied-to sender unless that is the
/// user. Every message carries one, an empty one when it names nobody:
/// since MSC4210, servers notify only the users and room it lists.
///
/// The members are the cached joined ones (`members_no_sync`), complete for
/// a room whose window was opened, as opening loads them. A send to a room
/// never opened (`/msg #room`) may miss a name; fetching the members on
/// every send would cost a request each time.
async fn message_mentions(
    client: &Client,
    room: &Room,
    body: &str,
    reply: Option<&SendMessageReplyTarget>,
) -> Result<(Mentions, Vec<UserLinkSpan>), MatrixCoreError> {
    let own_user_id = client.user_id().expect("a client that sends is logged in");
    let members = room
        .members_no_sync(RoomMemberships::JOIN)
        .await
        .map_err(|err| {
            MatrixCoreError::new(format!(
                "failed to read cached joined Matrix room members: {err}"
            ))
        })?;
    let named = body_mentions(
        body,
        members
            .iter()
            .map(|member| (member.user_id(), member.display_name())),
        own_user_id,
    );
    let mut user_ids = named.user_ids;
    if let Some(reply) = reply {
        let replied_to_sender = UserId::parse(&reply.sender_user_id)
            .map_err(|err| MatrixCoreError::new(format!("invalid Matrix user id: {err}")))?;
        if replied_to_sender != own_user_id {
            user_ids.insert(replied_to_sender);
        }
    }
    let mut mentions = Mentions::with_user_ids(user_ids);
    mentions.room = named.room;
    Ok((mentions, named.pills))
}

pub(crate) async fn send_message(
    client: &Client,
    request: SendMessageRequest,
) -> Result<(), MatrixCoreError> {
    // The UI builds the styles from what was typed: invalid ones are its bug.
    check_text_styles(&request.body, &request.styles).expect("a sent message's styles are valid");
    let room = joined_room_by_id(client, &request.room_id)?;

    let (mentions, pills) =
        message_mentions(client, &room, &request.body, request.reply.as_ref()).await?;
    let mut content = RoomMessageEventContent::new(room_message_type(
        request.presentation,
        request.body,
        &request.styles,
        &pills,
    ));
    content.mentions = Some(mentions);
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
        None => content_type_for_file_name(&filename),
    };
    let size = UInt::new(size)
        .ok_or_else(|| MatrixCoreError::new(format!("file size cannot fit Matrix UInt: {size}")))?;
    let config = AttachmentConfig::new().info(attachment_info(&mime, size));

    room.send_attachment(filename, &mime, bytes, config)
        .await
        .map_err(|err| MatrixCoreError::new(format!("failed to send Matrix file: {err}")))?;

    Ok(())
}

/// The content type a file is sent with when the caller names none, from the
/// file name's extension (case-insensitive). The SDK picks the message type
/// (`m.image`, `m.video`, `m.audio`, `m.file`) from it. `svg` stays
/// `application/octet-stream` on purpose: an SVG shown as an image is a
/// script vector in some clients.
fn content_type_for_file_name(file_name: &str) -> Mime {
    let extension = std::path::Path::new(file_name)
        .extension()
        .and_then(|extension| extension.to_str())
        .map(str::to_ascii_lowercase);
    let content_type = match extension.as_deref() {
        Some("png") => "image/png",
        Some("jpg" | "jpeg") => "image/jpeg",
        Some("gif") => "image/gif",
        Some("webp") => "image/webp",
        Some("bmp") => "image/bmp",
        Some("mp4" | "m4v") => "video/mp4",
        Some("webm") => "video/webm",
        Some("mov") => "video/quicktime",
        Some("mkv") => "video/x-matroska",
        Some("mp3") => "audio/mpeg",
        Some("ogg" | "oga" | "opus") => "audio/ogg",
        Some("m4a") => "audio/mp4",
        Some("wav") => "audio/wav",
        Some("flac") => "audio/flac",
        Some("pdf") => "application/pdf",
        Some("txt" | "log") => "text/plain",
        Some("zip") => "application/zip",
        _ => "application/octet-stream",
    };
    Mime::from_str(content_type).expect("the extension table holds valid MIME types")
}

/// The attachment metadata variant that matches the message type the SDK
/// derives from `mime`, so the event keeps the file size for every kind.
fn attachment_info(mime: &Mime, size: UInt) -> AttachmentInfo {
    let size = Some(size);
    match mime.type_() {
        mime::IMAGE => AttachmentInfo::Image(BaseImageInfo {
            size,
            ..Default::default()
        }),
        mime::VIDEO => AttachmentInfo::Video(BaseVideoInfo {
            size,
            ..Default::default()
        }),
        mime::AUDIO => AttachmentInfo::Audio(BaseAudioInfo {
            size,
            ..Default::default()
        }),
        _ => AttachmentInfo::File(BaseFileInfo { size }),
    }
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
    check_text_styles(&request.body, &request.styles).expect("an edit's styles are valid");
    let room = joined_room_by_id(client, &request.room_id)?;
    let target_event_id = <&EventId>::try_from(request.target_event_id.as_str())
        .map_err(|err| MatrixCoreError::new(format!("invalid Matrix event id: {err}")))?;
    // The new text's mentions. The SDK's replacement lists them all in
    // `m.new_content` and, at the top level, only those the original did
    // not, so an edit notifies only the newly named. An edited reply's
    // replied-to sender is not added back: the original notified them, and
    // an edit is compared with the original, not with `m.new_content`.
    let (mentions, pills) = message_mentions(client, &room, &request.body, None).await?;
    let mut new_content = RoomMessageEventContentWithoutRelation::new(room_message_type(
        request.presentation,
        request.body,
        &request.styles,
        &pills,
    ));
    new_content.mentions = Some(mentions);
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
        &room,
        response.chunk,
        room.clone_info().room_version_rules_or_default().redaction,
    )
    .await?;

    Ok(RoomHistoryBackfill {
        room_id: request.room_id,
        start: response.start,
        end: response.end,
        events,
    })
}

/// Maps a backward `/messages` page to oldest-first backfill events. Sender
/// profiles are the members' current ones: the SDK store does not keep the
/// profile a sender had when an older event was sent.
async fn room_history_backfill_events_from_timeline(
    room: &Room,
    chunk: Vec<TimelineEvent>,
    redaction_rules: matrix_sdk::ruma::room_version_rules::RedactionRules,
) -> Result<Vec<RoomHistoryBackfillEvent>, MatrixCoreError> {
    let mut reaction_index = BTreeMap::new();
    let mut senders = SenderProfileCache::new(room);
    let mut events = Vec::with_capacity(chunk.len());

    for event in chunk.into_iter().rev() {
        events.extend(
            room_history_backfill_event_from_timeline_event(
                room,
                &event,
                &redaction_rules,
                &mut reaction_index,
                &mut senders,
            )
            .await?,
        );
    }

    Ok(events)
}

fn room_activity_from_unable_to_decrypt_timeline_event(
    room_id: &str,
    event: &TimelineEvent,
    utd_info: &UnableToDecryptInfo,
    sender: RoomUserProfile,
    highlight: bool,
) -> Option<RoomActivity> {
    let event_id = event.event_id()?.to_string();

    Some(RoomActivity {
        room_id: room_id.to_owned(),
        activity_id: event_id.clone(),
        timestamp_ms: event.timestamp()?.get().into(),
        highlight,
        relation: None,
        kind: RoomActivityKind::UnableToDecrypt(RoomUnableToDecryptActivity {
            event_id,
            sender,
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

async fn room_history_backfill_event_from_timeline_event(
    room: &Room,
    event: &TimelineEvent,
    redaction_rules: &matrix_sdk::ruma::room_version_rules::RedactionRules,
    reaction_index: &mut BTreeMap<String, ReactionEventIndexEntry>,
    senders: &mut SenderProfileCache<'_>,
) -> Result<Option<RoomHistoryBackfillEvent>, MatrixCoreError> {
    let room_id = room.room_id().as_str();
    // The SDK has no push actions for an event it could not evaluate the
    // rules for: an undecryptable one, or any when the room lacks the state
    // the rules need. Such an event is no highlight.
    let push_actions = event.push_actions().unwrap_or_default();
    let highlight = |sender: &UserId| activity_highlight(room.own_user_id(), sender, push_actions);

    if let TimelineEventKind::UnableToDecrypt { utd_info, .. } = &event.kind {
        let Some(sender) = event.sender() else {
            return Ok(None);
        };
        let highlight = highlight(&sender);
        let sender = senders.get(&sender).await?;
        return Ok(room_activity_from_unable_to_decrypt_timeline_event(
            room_id, event, utd_info, sender, highlight,
        )
        .map(|activity| RoomHistoryBackfillEvent::Activity(Box::new(activity))));
    }

    let raw = event.raw();

    if let Ok(event) = raw.deserialize_as_unchecked::<OriginalSyncRoomMessageEvent>() {
        if let Some(replacement) = sync_message_replacement(&event) {
            return Ok(room_activity_update_from_sync_message_replacement(
                room_id,
                &event,
                replacement,
            )
            .map(RoomHistoryBackfillEvent::ActivityUpdate));
        }
        let sender = senders.get(&event.sender).await?;
        return Ok(room_activity_from_sync_message(
            room_id,
            &event,
            sender,
            highlight(&event.sender),
        )
        .map(|activity| RoomHistoryBackfillEvent::Activity(Box::new(activity))));
    }

    if let Ok(event) = raw.deserialize_as_unchecked::<OriginalSyncReactionEvent>() {
        reaction_index.insert(
            event.event_id.to_string(),
            ReactionEventIndexEntry {
                target_event_id: event.content.relates_to.event_id.to_string(),
                key: event.content.relates_to.key.clone(),
            },
        );
        return Ok(Some(RoomHistoryBackfillEvent::ActivityUpdate(
            room_activity_update_from_sync_reaction(room_id, &event),
        )));
    }

    if let Ok(event) = raw.deserialize_as_unchecked::<OriginalSyncRoomRedactionEvent>() {
        let redacted_event_id = event.redacts(redaction_rules).to_string();
        if let Some(entry) = reaction_index.remove(redacted_event_id.as_str()) {
            return Ok(Some(RoomHistoryBackfillEvent::ActivityUpdate(
                room_activity_update_from_indexed_reaction_redaction(
                    room_id,
                    redacted_event_id,
                    &event,
                    entry,
                ),
            )));
        }

        return Ok(Some(RoomHistoryBackfillEvent::ActivityUpdate(
            room_activity_update_from_sync_redaction(room_id, &event, redacted_event_id),
        )));
    }

    if let Ok(event) = raw.deserialize_as_unchecked::<OriginalSyncRoomTopicEvent>() {
        let sender = senders.get(&event.sender).await?;
        return Ok(Some(RoomHistoryBackfillEvent::Activity(Box::new(
            room_activity_from_sync_topic(room_id, &event, sender, highlight(&event.sender)),
        ))));
    }

    if let Ok(event) = raw.deserialize_as_unchecked::<OriginalSyncRoomMemberEvent>() {
        return Ok(room_activity_from_member_event(
            room,
            &event,
            senders,
            highlight(&event.sender),
        )
        .await?
        .map(|activity| RoomHistoryBackfillEvent::Activity(Box::new(activity))));
    }

    // Every power-level field has a default, so any state event would
    // deserialize as one: match the type first.
    if event.kind.event_type().as_deref() == Some("m.room.power_levels") {
        let event = raw
            .deserialize_as_unchecked::<OriginalSyncRoomPowerLevelsEvent>()
            .map_err(|err| {
                MatrixCoreError::new(format!(
                    "failed to decode Matrix power levels event in {room_id}: {err}"
                ))
            })?;
        return Ok(room_activity_from_power_levels_event(
            room,
            &event,
            senders,
            highlight(&event.sender),
        )
        .await?
        .map(|activity| RoomHistoryBackfillEvent::Activity(Box::new(activity))));
    }

    Ok(None)
}

pub(crate) async fn fetch_room_event(
    client: &Client,
    request: FetchRoomEventRequest,
) -> RoomEventFetch {
    let result = match fetch_room_event_activity(client, &request).await {
        Ok(activity) => RoomEventFetchResult::Loaded {
            activity: Box::new(activity),
        },
        Err(err) => RoomEventFetchResult::Failed {
            message: err.to_string(),
        },
    };
    RoomEventFetch {
        room_id: request.room_id,
        event_id: request.event_id,
        result,
    }
}

pub(crate) async fn list_public_rooms(
    client: &Client,
    request: ListPublicRoomsRequest,
) -> PublicRoomsPage {
    let result = match fetch_public_rooms(client, &request).await {
        Ok(result) => result,
        Err(err) => PublicRoomsResult::Failed {
            message: err.to_string(),
        },
    };
    PublicRoomsPage { request, result }
}

async fn fetch_public_rooms(
    client: &Client,
    request: &ListPublicRoomsRequest,
) -> Result<PublicRoomsResult, MatrixCoreError> {
    let server = request
        .server
        .as_deref()
        .map(|server| {
            ServerName::parse(server).map_err(|err| {
                MatrixCoreError::new(format!("invalid Matrix server name {server}: {err}"))
            })
        })
        .transpose()?;
    let mut filter = Filter::new();
    filter.generic_search_term = request.filter.clone();
    let mut sdk_request = get_public_rooms_filtered::v3::Request::new();
    sdk_request.server = server;
    sdk_request.limit = Some(UInt::from(request.limit));
    sdk_request.since = request.since.clone();
    sdk_request.filter = filter;

    let response = client
        .public_rooms_filtered(sdk_request)
        .await
        .map_err(|err| MatrixCoreError::new(format!("failed to list public rooms: {err}")))?;

    Ok(PublicRoomsResult::Loaded {
        rooms: response
            .chunk
            .into_iter()
            .map(|room| PublicRoom {
                room_id: room.room_id.to_string(),
                canonical_alias: room.canonical_alias.map(|alias| alias.to_string()),
                name: room.name,
                topic: room.topic,
                joined_members: room.num_joined_members.into(),
            })
            .collect(),
        next_batch: response.next_batch,
        total_estimate: response.total_room_count_estimate.map(u64::from),
    })
}

/// The event as the timeline line backfill would make of it, from the event
/// cache or else the server. An event that is no line of its own (an edit,
/// a reaction, a redaction) is unsupported.
async fn fetch_room_event_activity(
    client: &Client,
    request: &FetchRoomEventRequest,
) -> Result<RoomActivity, MatrixCoreError> {
    let room = joined_room_by_id(client, &request.room_id)?;
    let event_id = owned_event_id_from_str(&request.event_id)?;
    let event = room
        .load_or_fetch_event(&event_id, None)
        .await
        .map_err(|err| MatrixCoreError::new(format!("failed to fetch Matrix event: {err}")))?;
    let redaction_rules = room.clone_info().room_version_rules_or_default().redaction;
    match room_history_backfill_event_from_timeline_event(
        &room,
        &event,
        &redaction_rules,
        &mut BTreeMap::new(),
        &mut SenderProfileCache::new(&room),
    )
    .await?
    {
        Some(RoomHistoryBackfillEvent::Activity(activity)) => Ok(*activity),
        Some(RoomHistoryBackfillEvent::ActivityUpdate(_)) | None => {
            Err(MatrixCoreError::new("unsupported event"))
        }
    }
}

pub(crate) async fn logout(client: Client) -> Result<(), MatrixCoreError> {
    client
        .logout()
        .await
        .map_err(|err| MatrixCoreError::new(format!("failed to log out of Matrix: {err}")))
}

/// The signed-in client's session, as the UI saves it.
pub(crate) fn stored_session(client: &Client) -> StoredSession {
    let (meta, tokens, auth) = match client.session().expect("the client is signed in") {
        AuthSession::Matrix(session) => (session.meta, session.tokens, SessionAuth::Matrix),
        AuthSession::OAuth(session) => (
            session.user.meta,
            session.user.tokens,
            SessionAuth::OAuth {
                client_id: session.client_id.as_str().to_owned(),
            },
        ),
        // mirx signs in only through the two APIs above.
        _ => unreachable!("unknown authentication API"),
    };
    StoredSession {
        user_id: meta.user_id.to_string(),
        device_id: meta.device_id.to_string(),
        access_token: tokens.access_token,
        refresh_token: tokens.refresh_token,
        auth,
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::sync::mpsc;
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    use super::edit_message;
    use super::{
        LocalEchoMessageDetails, PendingOutgoingKind, PendingOutgoingRegistry, PowerLevelsChange,
        ReactionEventIndex, ReactionEventIndexEntry, SenderProfileCache, activity_highlight,
        add_highlight_word, attach_reply_relation, attach_thread_relation,
        attach_thread_reply_relation, backfill_room_history, ban_room_member, cancelled_state,
        content_type_for_file_name, create_room, direct_user_ids_by_room_from_content,
        emit_member_snapshots_after_sync, fetch_room_event, fetch_room_media, fetch_user_profile,
        highlight_word_actions, highlight_words_from_ruleset, ignore_user, ignored_users,
        insert_pending_outgoing_kind, invite_room_member, joined_room_members,
        joined_room_members_no_sync, joined_room_update_changes_members, kick_room_member,
        list_public_rooms, map_cross_signing_status, map_decimals, map_e2ee_backup_state,
        map_e2ee_recovery_state, map_emoji_array, map_emoji_short_auth_string, map_local_echo,
        map_local_message_activity, map_send_queue_update, member_target_display_name,
        message_presentation_and_text_from_serialized_event, outgoing_events_from_local_echo_event,
        own_room_user_profile, owned_event_id_from_str, power_levels_change,
        reaction_event_content, read_file_bytes, register_event_handlers, remove_highlight_word,
        restore_session, room_activity_from_sync_member, room_activity_from_sync_message,
        room_activity_from_sync_topic, room_activity_update_from_indexed_reaction_redaction,
        room_activity_update_from_sync_message_replacement,
        room_activity_update_from_sync_reaction, room_activity_update_from_sync_redaction,
        room_history_backfill_event_from_timeline_event,
        room_history_backfill_events_from_timeline, room_list_update_is_relevant,
        room_media_request_parameters, room_member_change_from_sync_member, room_member_rank,
        room_members_from_parts, room_message_type, room_read_state_from_parts,
        room_read_update_is_relevant, room_tag_summaries_from_tags, room_typing_from_sync_typing,
        room_unable_to_decrypt_reason_from_sdk, send_file, send_message, set_display_name,
        set_room_member_power_level, set_room_notification_mode,
        sorted_room_member_summaries_from_parts, summarize_rooms, sync_message_replacement,
        unban_room_member, unignore_user,
    };
    use crate::api::{
        CreateRoomRequest, CreateRoomVisibility, HighlightWord, ListPublicRoomsRequest, PublicRoom,
        PublicRoomsPage, PublicRoomsResult, RoomNotificationMode, RoomSummary, SendFileRequest,
        SetRoomNotificationModeRequest, UserProfile,
    };
    use crate::api::{
        EditMessageRequest, SessionAuth, TextColor, TextStyle, TextStyleSpan, UserLinkSpan,
    };
    use crate::rich_text::StyledText;

    #[test]
    fn joined_member_snapshots_use_sdk_members_and_default_power_levels() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("build tokio runtime");

        runtime.block_on(async {
            let mock_server = MatrixMockServer::new().await;
            let client = mock_server.client_builder().build().await;
            let state_events = serde_json::json!([
                {
                    "type": "m.room.create",
                    "state_key": "",
                    "sender": "@example:localhost",
                    "event_id": "$create:localhost",
                    "origin_server_ts": 1u64,
                    "content": { "creator": "@example:localhost" }
                },
                {
                    "type": "m.room.power_levels",
                    "state_key": "",
                    "sender": "@example:localhost",
                    "event_id": "$power:localhost",
                    "origin_server_ts": 2u64,
                    "content": {
                        "ban": 75,
                        "events": {},
                        "events_default": 0,
                        "invite": 50,
                        "kick": 50,
                        "redact": 50,
                        "state_default": 50,
                        "users": { "@example:localhost": 51 },
                        "users_default": 10
                    }
                },
                {
                    "type": "m.room.member",
                    "state_key": "@example:localhost",
                    "sender": "@example:localhost",
                    "event_id": "$actor:localhost",
                    "origin_server_ts": 3u64,
                    "content": { "membership": "join", "displayname": "Actor" }
                },
                {
                    "type": "m.room.member",
                    "state_key": "@bob:localhost",
                    "sender": "@bob:localhost",
                    "event_id": "$bob:localhost",
                    "origin_server_ts": 4u64,
                    "content": { "membership": "join", "displayname": "Bob" }
                }
            ]);
            mock_server
                .mock_get_members()
                .ok(raw_room_member_events(&state_events))
                .mock_once()
                .mount()
                .await;
            let room = mock_server
                .sync_room(
                    &client,
                    JoinedRoomBuilder::new(room_id().as_ref())
                        .add_state_bulk(raw_sync_state_events(&state_events)),
                )
                .await;
            let actor = client.user_id().expect("authenticated user");
            let sdk_actor = room
                .members_no_sync(RoomMemberships::JOIN)
                .await
                .expect("cached SDK members")
                .into_iter()
                .find(|member| member.user_id() == actor)
                .expect("actor SDK member");

            assert!(sdk_actor.can_kick());
            assert!(!sdk_actor.can_ban());
            let expected = RoomMembers {
                room_id: "!room:example.org".to_owned(),
                members: vec![
                    RoomMemberSummary {
                        user_id: "@example:localhost".to_owned(),
                        display_name: Some("Actor".to_owned()),
                        display_name_ambiguous: false,
                        is_own_user: true,
                        rank: RoomMemberRank::Moderator,
                        can_kick: false,
                        can_ban: false,
                    },
                    RoomMemberSummary {
                        user_id: "@bob:localhost".to_owned(),
                        display_name: Some("Bob".to_owned()),
                        display_name_ambiguous: false,
                        is_own_user: false,
                        rank: RoomMemberRank::Member,
                        can_kick: true,
                        can_ban: false,
                    },
                ],
            };

            assert_eq!(
                joined_room_members_no_sync(actor, &room)
                    .await
                    .expect("cached members"),
                expected
            );
            assert_eq!(
                joined_room_members(actor, &room)
                    .await
                    .expect("synced members"),
                expected
            );
        });
    }

    #[test]
    fn member_snapshot_after_restore_projects_using_restored_user() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("build tokio runtime");

        runtime.block_on(async {
            let mock_server = MatrixMockServer::new().await;
            let client = mock_server
                .client_builder()
                .unlogged()
                .build()
                .await;
            let (events, event_rx) = mpsc::channel();
            register_event_handlers(&client, events.clone());
            restore_session(
                &client,
                StoredSession {
                    user_id: "@example:localhost".to_owned(),
                    device_id: "DEVICE".to_owned(),
                    access_token: "access-token".to_owned(),
                    refresh_token: None,
                    auth: SessionAuth::Matrix,
                },
            )
            .await
            .expect("restore authenticated session");

            let state_events = serde_json::json!([
                { "type": "m.room.create", "state_key": "", "sender": "@example:localhost", "event_id": "$create:localhost", "origin_server_ts": 1u64, "content": { "creator": "@example:localhost" } },
                { "type": "m.room.power_levels", "state_key": "", "sender": "@example:localhost", "event_id": "$power:localhost", "origin_server_ts": 2u64, "content": { "kick": 50, "ban": 50, "users_default": 0, "users": { "@example:localhost": 100 }, "events": {} } },
                { "type": "m.room.member", "state_key": "@example:localhost", "sender": "@example:localhost", "event_id": "$actor:localhost", "origin_server_ts": 3u64, "content": { "membership": "join", "displayname": "Actor" } },
                { "type": "m.room.member", "state_key": "@target:localhost", "sender": "@target:localhost", "event_id": "$target:localhost", "origin_server_ts": 4u64, "content": { "membership": "join", "displayname": "Target" } }
            ]);
            sync_and_refresh_members(
&mock_server,
&client,
                    JoinedRoomBuilder::new(room_id().as_ref())
                        .add_state_bulk(raw_sync_state_events(&state_events)),
&events,
)
.await;

            let got = drain_events(&event_rx);

            assert!(
                !got.iter().any(|event| matches!(event, Event::RoomActivity(_))),
                "the state section is no activity: {got:?}"
            );
            let snapshots = member_snapshots(&got);
            assert_eq!(snapshots.len(), 1, "{got:?}");
            assert!(
                snapshots[0]
                    .members
                    .iter()
                    .find(|member| member.user_id == "@target:localhost")
                    .expect("target member")
                    .can_kick
            );
        });
    }

    #[test]
    fn kick_room_member_submits_exact_sdk_request_and_rejects_invalid_or_non_joined_rooms() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("build tokio runtime");

        runtime.block_on(async {
            let mock_server = MatrixMockServer::new().await;
            let client = mock_server.client_builder().build().await;
            mock_server
                .sync_joined_room(&client, room_id().as_ref())
                .await;
            mock_server.mock_kick_user().ok().expect(1).mount().await;

            kick_room_member(
                &client,
                &KickRoomMemberRequest {
                    room_id: "!room:example.org".to_owned(),
                    user_id: "@target:example.org".to_owned(),
                    reason: None,
                },
            )
            .await
            .expect("kick joined member");

            let requests = mock_server
                .received_requests()
                .await
                .expect("request recording enabled");
            let kick_requests = requests
                .iter()
                .filter(|request| request.url.path().ends_with("/kick"))
                .collect::<Vec<_>>();
            assert_eq!(kick_requests.len(), 1);
            assert_eq!(
                kick_requests[0].url.path(),
                "/_matrix/client/v3/rooms/!room:example.org/kick"
            );
            assert_eq!(
                kick_requests[0]
                    .body_json::<serde_json::Value>()
                    .expect("kick JSON body"),
                serde_json::json!({ "user_id": "@target:example.org" })
            );

            let invalid_room = kick_room_member(
                &client,
                &KickRoomMemberRequest {
                    room_id: "invalid-room-id".to_owned(),
                    user_id: "@target:example.org".to_owned(),
                    reason: None,
                },
            )
            .await
            .expect_err("invalid room id");
            assert_eq!(
                invalid_room.to_string(),
                "invalid Matrix room id: leading sigil is incorrect or missing"
            );

            let invalid_user = kick_room_member(
                &client,
                &KickRoomMemberRequest {
                    room_id: "!room:example.org".to_owned(),
                    user_id: "invalid-user-id".to_owned(),
                    reason: None,
                },
            )
            .await
            .expect_err("invalid user id");
            assert_eq!(
                invalid_user.to_string(),
                "invalid Matrix user id: leading sigil is incorrect or missing"
            );

            let non_joined_room = kick_room_member(
                &client,
                &KickRoomMemberRequest {
                    room_id: "!other:example.org".to_owned(),
                    user_id: "@target:example.org".to_owned(),
                    reason: None,
                },
            )
            .await
            .expect_err("non-joined room");
            assert_eq!(
                non_joined_room.to_string(),
                "joined room not found: !other:example.org"
            );

            assert_eq!(
                mock_server
                    .received_requests()
                    .await
                    .expect("request recording enabled")
                    .iter()
                    .filter(|request| request.url.path().ends_with("/kick"))
                    .count(),
                1
            );
        });
    }

    #[test]
    fn ban_room_member_submits_exact_sdk_request_and_rejects_invalid_or_non_joined_rooms() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("build tokio runtime");

        runtime.block_on(async {
            let mock_server = MatrixMockServer::new().await;
            let client = mock_server.client_builder().build().await;
            mock_server
                .sync_joined_room(&client, room_id().as_ref())
                .await;
            mock_server.mock_ban_user().ok().expect(1).mount().await;

            ban_room_member(
                &client,
                &BanRoomMemberRequest {
                    room_id: "!room:example.org".to_owned(),
                    user_id: "@target:example.org".to_owned(),
                    reason: None,
                },
            )
            .await
            .expect("ban joined member");

            let requests = mock_server
                .received_requests()
                .await
                .expect("request recording enabled");
            let ban_requests = requests
                .iter()
                .filter(|request| request.url.path().ends_with("/ban"))
                .collect::<Vec<_>>();
            assert_eq!(ban_requests.len(), 1);
            assert_eq!(
                ban_requests[0].url.path(),
                "/_matrix/client/v3/rooms/!room:example.org/ban"
            );
            assert_eq!(
                ban_requests[0]
                    .body_json::<serde_json::Value>()
                    .expect("ban JSON body"),
                serde_json::json!({ "user_id": "@target:example.org" })
            );

            let invalid_room = ban_room_member(
                &client,
                &BanRoomMemberRequest {
                    room_id: "invalid-room-id".to_owned(),
                    user_id: "@target:example.org".to_owned(),
                    reason: None,
                },
            )
            .await
            .expect_err("invalid room id");
            assert_eq!(
                invalid_room.to_string(),
                "invalid Matrix room id: leading sigil is incorrect or missing"
            );

            let invalid_user = ban_room_member(
                &client,
                &BanRoomMemberRequest {
                    room_id: "!room:example.org".to_owned(),
                    user_id: "invalid-user-id".to_owned(),
                    reason: None,
                },
            )
            .await
            .expect_err("invalid user id");
            assert_eq!(
                invalid_user.to_string(),
                "invalid Matrix user id: leading sigil is incorrect or missing"
            );

            let non_joined_room = ban_room_member(
                &client,
                &BanRoomMemberRequest {
                    room_id: "!other:example.org".to_owned(),
                    user_id: "@target:example.org".to_owned(),
                    reason: None,
                },
            )
            .await
            .expect_err("non-joined room");
            assert_eq!(
                non_joined_room.to_string(),
                "joined room not found: !other:example.org"
            );

            assert_eq!(
                mock_server
                    .received_requests()
                    .await
                    .expect("request recording enabled")
                    .iter()
                    .filter(|request| request.url.path().ends_with("/ban"))
                    .count(),
                1
            );
        });
    }

    fn moderation_test_runtime() -> tokio::runtime::Runtime {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("build tokio runtime")
    }

    async fn request_bodies_ending_with(
        mock_server: &MatrixMockServer,
        suffix: &str,
    ) -> Vec<serde_json::Value> {
        request_bodies_where(mock_server, |path| path.ends_with(suffix)).await
    }

    async fn request_bodies_containing(
        mock_server: &MatrixMockServer,
        part: &str,
    ) -> Vec<serde_json::Value> {
        request_bodies_where(mock_server, |path| path.contains(part)).await
    }

    async fn request_bodies_where(
        mock_server: &MatrixMockServer,
        path_matches: impl Fn(&str) -> bool,
    ) -> Vec<serde_json::Value> {
        mock_server
            .received_requests()
            .await
            .expect("request recording enabled")
            .iter()
            .filter(|request| path_matches(request.url.path()))
            .map(|request| {
                request
                    .body_json::<serde_json::Value>()
                    .expect("request JSON body")
            })
            .collect()
    }

    #[test]
    fn kick_and_ban_room_member_send_the_reason_in_the_request_body() {
        moderation_test_runtime().block_on(async {
            let mock_server = MatrixMockServer::new().await;
            let client = mock_server.client_builder().build().await;
            mock_server
                .sync_joined_room(&client, room_id().as_ref())
                .await;
            mock_server.mock_kick_user().ok().expect(1).mount().await;
            mock_server.mock_ban_user().ok().expect(1).mount().await;

            kick_room_member(
                &client,
                &KickRoomMemberRequest {
                    room_id: "!room:example.org".to_owned(),
                    user_id: "@target:example.org".to_owned(),
                    reason: Some("spamming".to_owned()),
                },
            )
            .await
            .expect("kick joined member with reason");
            ban_room_member(
                &client,
                &BanRoomMemberRequest {
                    room_id: "!room:example.org".to_owned(),
                    user_id: "@target:example.org".to_owned(),
                    reason: Some("spamming".to_owned()),
                },
            )
            .await
            .expect("ban joined member with reason");

            let expected =
                serde_json::json!({ "user_id": "@target:example.org", "reason": "spamming" });
            assert_eq!(
                request_bodies_ending_with(&mock_server, "/kick").await,
                vec![expected.clone()]
            );
            assert_eq!(
                request_bodies_ending_with(&mock_server, "/ban").await,
                vec![expected]
            );
        });
    }

    #[test]
    fn unban_room_member_submits_exact_request_and_rejects_invalid_or_non_joined_rooms() {
        moderation_test_runtime().block_on(async {
            let mock_server = MatrixMockServer::new().await;
            let client = mock_server.client_builder().build().await;
            mock_server
                .sync_joined_room(&client, room_id().as_ref())
                .await;
            wiremock::Mock::given(wiremock::matchers::method("POST"))
                .and(wiremock::matchers::path_regex(
                    r"^/_matrix/client/v3/rooms/.*/unban$",
                ))
                .respond_with(
                    wiremock::ResponseTemplate::new(200).set_body_json(serde_json::json!({})),
                )
                .expect(2)
                .mount(mock_server.server())
                .await;

            for reason in [None, Some("appealed".to_owned())] {
                unban_room_member(
                    &client,
                    &UnbanRoomMemberRequest {
                        room_id: "!room:example.org".to_owned(),
                        user_id: "@target:example.org".to_owned(),
                        reason,
                    },
                )
                .await
                .expect("unban member of joined room");
            }

            let unban_requests = mock_server
                .received_requests()
                .await
                .expect("request recording enabled")
                .into_iter()
                .filter(|request| request.url.path().ends_with("/unban"))
                .collect::<Vec<_>>();
            assert_eq!(
                unban_requests
                    .iter()
                    .map(|request| request.url.path().to_owned())
                    .collect::<Vec<_>>(),
                vec!["/_matrix/client/v3/rooms/!room:example.org/unban"; 2]
            );
            assert_eq!(
                request_bodies_ending_with(&mock_server, "/unban").await,
                vec![
                    serde_json::json!({ "user_id": "@target:example.org" }),
                    serde_json::json!({ "user_id": "@target:example.org", "reason": "appealed" }),
                ]
            );

            for (room, user, expected) in [
                (
                    "invalid-room-id",
                    "@target:example.org",
                    "invalid Matrix room id: leading sigil is incorrect or missing",
                ),
                (
                    "!room:example.org",
                    "invalid-user-id",
                    "invalid Matrix user id: leading sigil is incorrect or missing",
                ),
                (
                    "!other:example.org",
                    "@target:example.org",
                    "joined room not found: !other:example.org",
                ),
            ] {
                let err = unban_room_member(
                    &client,
                    &UnbanRoomMemberRequest {
                        room_id: room.to_owned(),
                        user_id: user.to_owned(),
                        reason: None,
                    },
                )
                .await
                .expect_err("rejected unban");
                assert_eq!(err.to_string(), expected);
            }
            assert_eq!(
                request_bodies_ending_with(&mock_server, "/unban")
                    .await
                    .len(),
                2
            );
        });
    }

    #[test]
    fn invite_room_member_submits_exact_request_and_rejects_invalid_or_non_joined_rooms() {
        moderation_test_runtime().block_on(async {
            let mock_server = MatrixMockServer::new().await;
            let client = mock_server.client_builder().build().await;
            mock_server
                .sync_joined_room(&client, room_id().as_ref())
                .await;
            mock_server
                .mock_invite_user_by_id()
                .ok()
                .expect(1)
                .mount()
                .await;

            invite_room_member(
                &client,
                &InviteRoomMemberRequest {
                    room_id: "!room:example.org".to_owned(),
                    user_id: "@guest:example.org".to_owned(),
                },
            )
            .await
            .expect("invite into joined room");

            let invite_paths = mock_server
                .received_requests()
                .await
                .expect("request recording enabled")
                .into_iter()
                .filter(|request| request.url.path().ends_with("/invite"))
                .map(|request| request.url.path().to_owned())
                .collect::<Vec<_>>();
            assert_eq!(
                invite_paths,
                vec!["/_matrix/client/v3/rooms/!room:example.org/invite"]
            );
            assert_eq!(
                request_bodies_ending_with(&mock_server, "/invite").await,
                vec![serde_json::json!({ "user_id": "@guest:example.org" })]
            );

            for (room, user, expected) in [
                (
                    "invalid-room-id",
                    "@guest:example.org",
                    "invalid Matrix room id: leading sigil is incorrect or missing",
                ),
                (
                    "!room:example.org",
                    "invalid-user-id",
                    "invalid Matrix user id: leading sigil is incorrect or missing",
                ),
                (
                    "!other:example.org",
                    "@guest:example.org",
                    "joined room not found: !other:example.org",
                ),
            ] {
                let err = invite_room_member(
                    &client,
                    &InviteRoomMemberRequest {
                        room_id: room.to_owned(),
                        user_id: user.to_owned(),
                    },
                )
                .await
                .expect_err("rejected invite");
                assert_eq!(err.to_string(), expected);
            }
            assert_eq!(
                request_bodies_ending_with(&mock_server, "/invite")
                    .await
                    .len(),
                1
            );
        });
    }

    /// Syncs a joined room with the given power-levels content, sets
    /// `@target:example.org` to `role` and returns the result plus the
    /// `users` map of every power-levels PUT body.
    async fn set_target_power_role(
        power_levels: serde_json::Value,
        role: MemberPowerRole,
    ) -> (Result<(), super::MatrixCoreError>, Vec<serde_json::Value>) {
        let mock_server = MatrixMockServer::new().await;
        let client = mock_server.client_builder().build().await;
        let state_events = serde_json::json!([
            { "type": "m.room.create", "state_key": "", "sender": "@example:localhost", "event_id": "$create:localhost", "origin_server_ts": 1u64, "content": { "creator": "@example:localhost" } },
            { "type": "m.room.power_levels", "state_key": "", "sender": "@example:localhost", "event_id": "$power:localhost", "origin_server_ts": 2u64, "content": power_levels },
            { "type": "m.room.member", "state_key": "@example:localhost", "sender": "@example:localhost", "event_id": "$actor:localhost", "origin_server_ts": 3u64, "content": { "membership": "join" } }
        ]);
        mock_server
            .sync_room(
                &client,
                JoinedRoomBuilder::new(room_id().as_ref())
                    .add_state_bulk(raw_sync_state_events(&state_events)),
            )
            .await;
        mock_server
            .mock_room_send_state()
            .ok(OwnedEventId::try_from("$power-update:localhost").expect("valid event id"))
            .mount()
            .await;

        let result = set_room_member_power_level(
            &client,
            &SetRoomMemberPowerLevelRequest {
                room_id: "!room:example.org".to_owned(),
                user_id: "@target:example.org".to_owned(),
                role,
            },
        )
        .await;
        let users = request_bodies_ending_with(&mock_server, "/state/m.room.power_levels/")
            .await
            .into_iter()
            .map(|body| body["users"].clone())
            .collect();
        (result, users)
    }

    #[test]
    fn set_room_member_power_level_maps_irc_roles_to_room_levels() {
        moderation_test_runtime().block_on(async {
            let base = serde_json::json!({
                "users_default": 0,
                "events_default": 0,
                "users": { "@example:localhost": 100, "@target:example.org": 25 },
            });
            let with = |extra: serde_json::Value| {
                let mut content = base.clone();
                content
                    .as_object_mut()
                    .expect("power levels object")
                    .extend(extra.as_object().expect("extra object").clone());
                content
            };

            for (content, role, target_level) in [
                (base.clone(), MemberPowerRole::Moderator, Some(50)),
                (base.clone(), MemberPowerRole::Voiced, Some(1)),
                (
                    with(serde_json::json!({ "events": { "m.room.message": 10 } })),
                    MemberPowerRole::Voiced,
                    Some(10),
                ),
                (
                    with(serde_json::json!({ "events_default": 20 })),
                    MemberPowerRole::Voiced,
                    Some(20),
                ),
                (
                    with(serde_json::json!({ "users_default": 5 })),
                    MemberPowerRole::Voiced,
                    Some(6),
                ),
                (base.clone(), MemberPowerRole::Default, None),
            ] {
                let (result, users) = set_target_power_role(content, role).await;
                result.expect("set power level");
                let mut expected = serde_json::json!({ "@example:localhost": 100 });
                if let Some(level) = target_level {
                    expected["@target:example.org"] = serde_json::json!(level);
                }
                assert_eq!(users, vec![expected], "{role:?}");
            }
        });
    }

    #[test]
    fn set_room_member_power_level_refuses_voice_when_messages_need_moderator() {
        moderation_test_runtime().block_on(async {
            for content in [
                serde_json::json!({
                    "users_default": 0,
                    "events_default": 0,
                    "events": { "m.room.message": 50 },
                    "users": { "@example:localhost": 100 },
                }),
                serde_json::json!({
                    "users_default": 49,
                    "events_default": 0,
                    "users": { "@example:localhost": 100 },
                }),
            ] {
                let (result, users) = set_target_power_role(content, MemberPowerRole::Voiced).await;
                let err = result.expect_err("no voice level below moderator");
                assert!(
                    err.to_string()
                        .contains("room has no voice level below moderator"),
                    "{err}"
                );
                assert!(users.is_empty(), "no power-levels request: {users:?}");
            }
        });
    }

    #[test]
    fn set_room_member_power_level_rejects_invalid_or_non_joined_rooms() {
        moderation_test_runtime().block_on(async {
            let mock_server = MatrixMockServer::new().await;
            let client = mock_server.client_builder().build().await;
            mock_server
                .sync_joined_room(&client, room_id().as_ref())
                .await;
            for (room, user, expected) in [
                (
                    "invalid-room-id",
                    "@target:example.org",
                    "invalid Matrix room id: leading sigil is incorrect or missing",
                ),
                (
                    "!room:example.org",
                    "invalid-user-id",
                    "invalid Matrix user id: leading sigil is incorrect or missing",
                ),
                (
                    "!other:example.org",
                    "@target:example.org",
                    "joined room not found: !other:example.org",
                ),
            ] {
                let err = set_room_member_power_level(
                    &client,
                    &SetRoomMemberPowerLevelRequest {
                        room_id: room.to_owned(),
                        user_id: user.to_owned(),
                        role: MemberPowerRole::Moderator,
                    },
                )
                .await
                .expect_err("rejected power level change");
                assert_eq!(err.to_string(), expected);
            }
        });
    }

    #[test]
    fn set_display_name_puts_the_new_display_name_profile_field() {
        moderation_test_runtime().block_on(async {
            let mock_server = MatrixMockServer::new().await;
            let client = mock_server.client_builder().build().await;
            let user_id = client.user_id().expect("authenticated user").to_owned();
            mock_server
                .mock_set_profile_field(
                    &user_id,
                    matrix_sdk::ruma::profile::ProfileFieldName::DisplayName,
                )
                .ok()
                .expect(1)
                .mount()
                .await;

            set_display_name(
                &client,
                &SetDisplayNameRequest {
                    display_name: "New Nick".to_owned(),
                },
            )
            .await
            .expect("set display name");

            assert_eq!(
                request_bodies_ending_with(&mock_server, "/profile/@example:localhost/displayname")
                    .await,
                vec![serde_json::json!({ "displayname": "New Nick" })]
            );
        });
    }

    #[test]
    fn joined_member_snapshots_project_sdk_kick_and_ban_permissions_for_each_target_power() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("build tokio runtime");

        runtime.block_on(async {
            let client = client_with_joined_members(serde_json::json!([
                {
                    "type": "m.room.create", "state_key": "",
                    "sender": "@alice:example.org", "event_id": "$create:example.org",
                    "origin_server_ts": 1u64,
                    "content": { "creator": "@alice:example.org" }
                },
                {
                    "type": "m.room.power_levels", "state_key": "",
                    "sender": "@alice:example.org", "event_id": "$power:example.org",
                    "origin_server_ts": 2u64,
                    "content": {
                        "ban": 75, "events": {}, "events_default": 0, "invite": 50,
                        "kick": 100, "redact": 50, "state_default": 50, "users_default": 10,
                        "users": {
                            "@alice:example.org": 76,
                            "@equal:example.org": 76,
                            "@higher:example.org": 100
                        }
                    }
                },
                {
                    "type": "m.room.member", "state_key": "@alice:example.org",
                    "sender": "@alice:example.org", "event_id": "$alice:example.org",
                    "origin_server_ts": 3u64,
                    "content": { "membership": "join", "displayname": "Actor" }
                },
                {
                    "type": "m.room.member", "state_key": "@default:example.org",
                    "sender": "@default:example.org", "event_id": "$default:example.org",
                    "origin_server_ts": 4u64,
                    "content": { "membership": "join", "displayname": "Default" }
                },
                {
                    "type": "m.room.member", "state_key": "@equal:example.org",
                    "sender": "@equal:example.org", "event_id": "$equal:example.org",
                    "origin_server_ts": 5u64,
                    "content": { "membership": "join", "displayname": "Equal" }
                },
                {
                    "type": "m.room.member", "state_key": "@higher:example.org",
                    "sender": "@higher:example.org", "event_id": "$higher:example.org",
                    "origin_server_ts": 6u64,
                    "content": { "membership": "join", "displayname": "Higher" }
                }
            ]))
            .await;
            let room = client
                .get_room(room_id().as_ref())
                .expect("joined room from sync response");
            let members = room
                .members_no_sync(RoomMemberships::JOIN)
                .await
                .expect("cached SDK members");
            let actor = members
                .iter()
                .find(|member| member.user_id().as_str() == "@alice:example.org")
                .expect("actor SDK member");

            assert!(!actor.can_kick());
            assert!(actor.can_ban());
            assert_eq!(
                joined_room_members_no_sync(client.user_id().expect("authenticated user"), &room)
                    .await
                    .expect("projected SDK members"),
                RoomMembers {
                    room_id: "!room:example.org".to_owned(),
                    members: vec![
                        RoomMemberSummary {
                            user_id: "@higher:example.org".to_owned(),
                            display_name: Some("Higher".to_owned()),
                            display_name_ambiguous: false,
                            is_own_user: false,
                            rank: RoomMemberRank::Administrator,
                            can_kick: false,
                            can_ban: false
                        },
                        RoomMemberSummary {
                            user_id: "@alice:example.org".to_owned(),
                            display_name: Some("Actor".to_owned()),
                            display_name_ambiguous: false,
                            is_own_user: true,
                            rank: RoomMemberRank::Moderator,
                            can_kick: false,
                            can_ban: false
                        },
                        RoomMemberSummary {
                            user_id: "@equal:example.org".to_owned(),
                            display_name: Some("Equal".to_owned()),
                            display_name_ambiguous: false,
                            is_own_user: false,
                            rank: RoomMemberRank::Moderator,
                            can_kick: false,
                            can_ban: false
                        },
                        RoomMemberSummary {
                            user_id: "@default:example.org".to_owned(),
                            display_name: Some("Default".to_owned()),
                            display_name_ambiguous: false,
                            is_own_user: false,
                            rank: RoomMemberRank::Member,
                            can_kick: false,
                            can_ban: true
                        },
                    ],
                }
            );
        });
    }

    fn joined_member_state_events(
        room_version: &str,
        power_levels: serde_json::Value,
        members: &[(&str, &str)],
    ) -> serde_json::Value {
        let mut events = vec![
            serde_json::json!({
                "type": "m.room.create", "state_key": "",
                "sender": "@alice:example.org", "event_id": "$create:example.org",
                "origin_server_ts": 1u64,
                "content": { "creator": "@alice:example.org", "room_version": room_version }
            }),
            serde_json::json!({
                "type": "m.room.power_levels", "state_key": "",
                "sender": "@alice:example.org", "event_id": "$power:example.org",
                "origin_server_ts": 2u64,
                "content": power_levels
            }),
        ];
        events.extend(members.iter().enumerate().map(|(index, (user_id, name))| {
            serde_json::json!({
                "type": "m.room.member", "state_key": user_id,
                "sender": user_id, "event_id": format!("$member{index}:example.org"),
                "origin_server_ts": 3u64 + index as u64,
                "content": { "membership": "join", "displayname": name }
            })
        }));
        serde_json::Value::Array(events)
    }

    async fn joined_member_ranks(
        member_events: serde_json::Value,
    ) -> Vec<(String, RoomMemberRank)> {
        let client = client_with_joined_members(member_events).await;
        let room = client
            .get_room(room_id().as_ref())
            .expect("joined room from sync response");
        joined_room_members_no_sync(client.user_id().expect("authenticated user"), &room)
            .await
            .expect("ranked members")
            .members
            .into_iter()
            .map(|member| (member.user_id, member.rank))
            .collect()
    }

    #[test]
    fn joined_member_snapshots_rank_members_by_power_level_then_sort_by_rank_and_name() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("build tokio runtime");

        runtime.block_on(async {
            let ranks = joined_member_ranks(joined_member_state_events(
                "10",
                serde_json::json!({
                    "users_default": 0,
                    "users": {
                        "@alice:example.org": 100,
                        "@mod:example.org": 50,
                        "@voice:example.org": 10,
                        "@bmod:example.org": 99
                    }
                }),
                &[
                    ("@alice:example.org", "Zed"),
                    ("@aaron:example.org", "Aaron"),
                    ("@voice:example.org", "Voice"),
                    ("@mod:example.org", "Mod"),
                    ("@bmod:example.org", "Bmod"),
                ],
            ))
            .await;

            assert_eq!(
                ranks,
                vec![
                    (
                        "@alice:example.org".to_owned(),
                        RoomMemberRank::Administrator
                    ),
                    ("@bmod:example.org".to_owned(), RoomMemberRank::Moderator),
                    ("@mod:example.org".to_owned(), RoomMemberRank::Moderator),
                    ("@voice:example.org".to_owned(), RoomMemberRank::Voiced),
                    ("@aaron:example.org".to_owned(), RoomMemberRank::Member),
                ]
            );
        });
    }

    #[test]
    fn joined_member_snapshots_voice_only_levels_above_the_users_default() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("build tokio runtime");

        runtime.block_on(async {
            let ranks = joined_member_ranks(joined_member_state_events(
                "10",
                serde_json::json!({
                    "users_default": 10,
                    "users": {
                        "@alice:example.org": 10,
                        "@voice:example.org": 20,
                        "@muted:example.org": 0
                    }
                }),
                &[
                    ("@alice:example.org", "Alice"),
                    ("@default:example.org", "Default"),
                    ("@muted:example.org", "Muted"),
                    ("@voice:example.org", "Voice"),
                ],
            ))
            .await;

            assert_eq!(
                ranks,
                vec![
                    ("@voice:example.org".to_owned(), RoomMemberRank::Voiced),
                    ("@alice:example.org".to_owned(), RoomMemberRank::Member),
                    ("@default:example.org".to_owned(), RoomMemberRank::Member),
                    ("@muted:example.org".to_owned(), RoomMemberRank::Member),
                ]
            );
        });
    }

    #[test]
    fn joined_member_snapshots_rank_room_version_12_creators_above_administrators() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("build tokio runtime");

        runtime.block_on(async {
            let ranks = joined_member_ranks(joined_member_state_events(
                "12",
                serde_json::json!({ "users": { "@admin:example.org": 100 } }),
                &[
                    ("@admin:example.org", "Admin"),
                    ("@alice:example.org", "Zed"),
                ],
            ))
            .await;

            assert_eq!(
                ranks,
                vec![
                    ("@alice:example.org".to_owned(), RoomMemberRank::Creator),
                    (
                        "@admin:example.org".to_owned(),
                        RoomMemberRank::Administrator
                    ),
                ]
            );
        });
    }

    #[test]
    fn room_member_rank_maps_sdk_roles_and_voice() {
        let rank = |level: i64, users_default: i64| {
            room_member_rank(
                UserPowerLevel::Int(Int::new(level).expect("power level")),
                Int::new(users_default).expect("users default"),
            )
        };
        assert_eq!(
            room_member_rank(
                UserPowerLevel::Infinite,
                Int::new(0).expect("users default")
            ),
            RoomMemberRank::Creator
        );
        assert_eq!(rank(150, 0), RoomMemberRank::Administrator);
        assert_eq!(rank(100, 0), RoomMemberRank::Administrator);
        assert_eq!(rank(99, 0), RoomMemberRank::Moderator);
        assert_eq!(rank(50, 50), RoomMemberRank::Moderator);
        assert_eq!(rank(49, 0), RoomMemberRank::Voiced);
        assert_eq!(rank(1, 0), RoomMemberRank::Voiced);
        assert_eq!(rank(0, 0), RoomMemberRank::Member);
        assert_eq!(rank(10, 10), RoomMemberRank::Member);
        assert_eq!(rank(-5, 0), RoomMemberRank::Member);
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
            backfill_event(&event, &RedactionRules::V1, &mut BTreeMap::new(),),
            Some(RoomHistoryBackfillEvent::Activity(Box::new(RoomActivity {
                room_id: "!room:example.org".to_owned(),
                activity_id: "$encrypted1:example.org".to_owned(),
                timestamp_ms: 1_700_000_000_000,
                highlight: false,
                relation: None,
                kind: RoomActivityKind::UnableToDecrypt(RoomUnableToDecryptActivity {
                    event_id: "$encrypted1:example.org".to_owned(),
                    sender: profile("@alice:example.org"),
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
            backfill_events(vec![event], RedactionRules::V1,),
            vec![RoomHistoryBackfillEvent::Activity(Box::new(RoomActivity {
                room_id: "!room:example.org".to_owned(),
                activity_id: "$encrypted-vector:example.org".to_owned(),
                timestamp_ms: 1_700_000_000_000,
                highlight: false,
                relation: None,
                kind: RoomActivityKind::UnableToDecrypt(RoomUnableToDecryptActivity {
                    event_id: "$encrypted-vector:example.org".to_owned(),
                    sender: profile("@alice:example.org"),
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
        BackfillRoomHistoryRequest, BanRoomMemberRequest, CoreErrorEvent, E2eeBackupState,
        E2eeCrossSigningStatus, E2eeRecoveryState, Event, FetchRoomEventRequest,
        FetchRoomMediaRequest, InviteRoomMemberRequest, KickRoomMemberRequest, MemberPowerRole,
        Operation, ROOM_HISTORY_BACKFILL_LIMIT_MAX, RoomActivity, RoomActivityKind,
        RoomActivityRelation, RoomActivityUpdate, RoomActivityUpdateKind, RoomActivityUpdateTarget,
        RoomAudioActivity, RoomAudioInfo, RoomEncryptedFileHashes, RoomEncryptedFileKey,
        RoomEventFetch, RoomEventFetchResult, RoomFileActivity, RoomFileInfo,
        RoomHistoryBackfillEvent, RoomImageActivity, RoomImageInfo, RoomMediaFormat, RoomMediaKind,
        RoomMediaSource, RoomMemberChange, RoomMemberRank, RoomMemberSummary, RoomMembers,
        RoomMessageActivity, RoomMessagePresentationKind, RoomMessageSendState,
        RoomPowerLevelUserChange, RoomPowerLevelValueChange, RoomReadState,
        RoomStateNoticeActivity, RoomStateNoticeContent, RoomTagSummary, RoomTyping,
        RoomUnableToDecryptActivity, RoomUnableToDecryptReason, RoomUserProfile, RoomVideoActivity,
        RoomVideoInfo, SendMessageReplyTarget, SendMessageRequest, SendMessageThreadTarget,
        SetDisplayNameRequest, SetRoomMemberPowerLevelRequest, StoredSession,
        UnbanRoomMemberRequest, UnsupportedRoomMessageActivity, VerificationDecimals,
        VerificationDirection, VerificationEmoji, VerificationFlowId, VerificationRequest,
        VerificationState,
    };
    use matrix_sdk::ruma::push::{
        Action, EventMatchConditionData, NewConditionalPushRule, NewPatternedPushRule, NewPushRule,
        NewSimplePushRule, PatternedPushRule, PushCondition, RuleKind, Ruleset,
    };
    use matrix_sdk::{
        Client, Room, RoomMemberships, SessionMeta,
        deserialized_responses::{TimelineEvent, UnableToDecryptInfo, UnableToDecryptReason},
        encryption::{
            backups::BackupState,
            recovery::RecoveryState,
            verification::{Emoji as MatrixEmoji, EmojiShortAuthString},
        },
        media::{MediaFormat, MediaThumbnailSettings},
        ruma::{
            Int, MilliSecondsSinceUnixEpoch, OwnedEventId, OwnedMxcUri, OwnedRoomId,
            OwnedTransactionId, OwnedUserId, RoomId, UInt, UserId,
            api::client::media::get_content_thumbnail::v3::Method,
            events::{
                AnyMessageLikeEventContent, AnySyncStateEvent, AnySyncTimelineEvent,
                AnyTimelineEvent,
                direct::DirectEventContent,
                reaction::OriginalSyncReactionEvent,
                room::MediaSource,
                room::power_levels::{
                    PossiblyRedactedRoomPowerLevelsEventContent, RoomPowerLevelsEventContent,
                    UserPowerLevel,
                },
                room::{
                    member::{OriginalSyncRoomMemberEvent, RoomMemberEvent},
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
        test_utils::mocks::{MatrixMockServer, PushRuleIdSpec, RoomMessagesResponseTemplate},
    };
    use matrix_sdk_base::cross_process_lock::CrossProcessLockConfig;
    use matrix_sdk_base::{
        BaseClient, DmRoomDefinition, RoomInfoNotableUpdateReasons, RoomState, ThreadingSupport,
        read_receipts::{LatestReadReceipt, RoomReadReceipts},
        store::StoreConfig,
    };
    use matrix_sdk_crypto::CrossSigningStatus;
    use matrix_sdk_test::{InvitedRoomBuilder, JoinedRoomBuilder, ruma_response_from_json};

    use crate::runtime::EventSender;
    use matrix_sdk::sync::{JoinedRoomUpdate, State, Timeline};

    fn room_id() -> OwnedRoomId {
        OwnedRoomId::try_from("!room:example.org").expect("valid room id")
    }

    fn raw_room_member_events(events: &serde_json::Value) -> Vec<Raw<RoomMemberEvent>> {
        events
            .as_array()
            .expect("member event array")
            .iter()
            .filter(|event| event["type"] == "m.room.member")
            .map(|event| {
                Raw::<RoomMemberEvent>::from_json_string(event.to_string())
                    .expect("serialize raw member event")
            })
            .collect()
    }

    fn raw_sync_state_events(events: &serde_json::Value) -> Vec<Raw<AnySyncStateEvent>> {
        let events = events.as_array().expect("state event array");
        events
            .iter()
            .filter(|event| event["type"] == "m.room.create")
            .chain(
                events
                    .iter()
                    .filter(|event| event["type"] == "m.room.power_levels"),
            )
            .chain(events.iter().filter(|event| {
                event["type"] != "m.room.create" && event["type"] != "m.room.power_levels"
            }))
            .map(|event| {
                Raw::<AnySyncStateEvent>::from_json_string(event.to_string())
                    .expect("serialize raw sync state event")
            })
            .collect()
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

    fn profile(user_id: &str) -> RoomUserProfile {
        RoomUserProfile {
            user_id: user_id.to_owned(),
            display_name: None,
            display_name_ambiguous: false,
        }
    }

    fn sync_message_replacement_update(
        event: &OriginalSyncRoomMessageEvent,
    ) -> Option<RoomActivityUpdate> {
        room_activity_update_from_sync_message_replacement(
            "!room:example.org",
            event,
            sync_message_replacement(event).expect("replacement relation"),
        )
    }

    /// Runs `map` against `!room:example.org` in an offline client whose
    /// store knows no members, so every sender resolves to a bare user ID.
    fn with_memberless_room<T>(map: impl AsyncFnOnce(&Room) -> T) -> T {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("build tokio runtime")
            .block_on(async {
                let client = client_with_joined_members(serde_json::json!([])).await;
                let room = client.get_room(room_id().as_ref()).expect("joined room");
                map(&room).await
            })
    }

    fn backfill_event(
        event: &TimelineEvent,
        redaction_rules: &RedactionRules,
        reaction_index: &mut BTreeMap<String, ReactionEventIndexEntry>,
    ) -> Option<RoomHistoryBackfillEvent> {
        with_memberless_room(async |room| {
            room_history_backfill_event_from_timeline_event(
                room,
                event,
                redaction_rules,
                reaction_index,
                &mut SenderProfileCache::new(room),
            )
            .await
            .expect("map backfill event")
        })
    }

    fn backfill_events(
        chunk: Vec<TimelineEvent>,
        redaction_rules: RedactionRules,
    ) -> Vec<RoomHistoryBackfillEvent> {
        with_memberless_room(async |room| {
            room_history_backfill_events_from_timeline(room, chunk, redaction_rules)
                .await
                .expect("map backfill events")
        })
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
                    display_name_ambiguous: false,
                    is_own_user: false,
                    rank: RoomMemberRank::Member,
                    can_kick: false,
                    can_ban: false,
                },
                RoomMemberSummary {
                    user_id: "@charlie:example.org".to_owned(),
                    display_name: None,
                    display_name_ambiguous: false,
                    is_own_user: false,
                    rank: RoomMemberRank::Member,
                    can_kick: false,
                    can_ban: false,
                },
                RoomMemberSummary {
                    user_id: "@alpha:example.org".to_owned(),
                    display_name: Some("ALICE".to_owned()),
                    display_name_ambiguous: false,
                    is_own_user: false,
                    rank: RoomMemberRank::Member,
                    can_kick: false,
                    can_ban: false,
                },
                RoomMemberSummary {
                    user_id: "@echo:example.org".to_owned(),
                    display_name: Some("alice".to_owned()),
                    display_name_ambiguous: false,
                    is_own_user: false,
                    rank: RoomMemberRank::Member,
                    can_kick: false,
                    can_ban: false,
                },
                RoomMemberSummary {
                    user_id: "@delta:example.org".to_owned(),
                    display_name: Some("beta".to_owned()),
                    display_name_ambiguous: false,
                    is_own_user: false,
                    rank: RoomMemberRank::Member,
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
                    display_name_ambiguous: false,
                    is_own_user: false,
                    rank: RoomMemberRank::Member,
                    can_kick: false,
                    can_ban: false,
                },
                RoomMemberSummary {
                    user_id: "@mike:example.org".to_owned(),
                    display_name: Some("same".to_owned()),
                    display_name_ambiguous: false,
                    is_own_user: false,
                    rank: RoomMemberRank::Member,
                    can_kick: false,
                    can_ban: false,
                },
                RoomMemberSummary {
                    user_id: "@zulu:example.org".to_owned(),
                    display_name: Some("Same".to_owned()),
                    display_name_ambiguous: false,
                    is_own_user: false,
                    rank: RoomMemberRank::Member,
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
            room_activity_from_sync_message(
                "!room:example.org",
                &event,
                profile("@alice:example.org"),
                false,
            ),
            Some(RoomActivity {
                room_id: "!room:example.org".to_owned(),
                activity_id: "event:$event:example.org".to_owned(),
                timestamp_ms: 1_700_000_000_000,
                highlight: false,
                relation: None,
                kind: RoomActivityKind::Message(RoomMessageActivity {
                    event_id: Some("$event:example.org".to_owned()),
                    transaction_id: Some("txn-1".to_owned()),
                    sender: profile("@alice:example.org"),
                    presentation: RoomMessagePresentationKind::Text,
                    body: "hello".to_owned(),
                    styles: Vec::new(),
                    user_links: Vec::new(),
                    send_state: RoomMessageSendState::Sent,
                }),
            })
        );
    }

    fn room_message_event_content(
        presentation: RoomMessagePresentationKind,
        body: String,
    ) -> RoomMessageEventContent {
        RoomMessageEventContent::new(room_message_type(presentation, body, &[], &[]))
    }

    fn room_message_event_content_without_relation(
        presentation: RoomMessagePresentationKind,
        body: String,
    ) -> super::RoomMessageEventContentWithoutRelation {
        super::RoomMessageEventContentWithoutRelation::new(room_message_type(
            presentation,
            body,
            &[],
            &[],
        ))
    }

    #[test]
    fn plain_message_types_have_no_formatted_body() {
        for (presentation, msgtype) in [
            (RoomMessagePresentationKind::Text, "m.text"),
            (RoomMessagePresentationKind::Notice, "m.notice"),
            (RoomMessagePresentationKind::Emote, "m.emote"),
        ] {
            assert_eq!(
                serde_json::to_value(room_message_type(
                    presentation,
                    "hi there".to_owned(),
                    &[],
                    &[]
                ))
                .unwrap(),
                serde_json::json!({ "msgtype": msgtype, "body": "hi there" })
            );
        }
    }

    #[test]
    fn styled_message_types_carry_an_html_formatted_body() {
        for (presentation, msgtype) in [
            (RoomMessagePresentationKind::Text, "m.text"),
            (RoomMessagePresentationKind::Notice, "m.notice"),
            (RoomMessagePresentationKind::Emote, "m.emote"),
        ] {
            assert_eq!(
                serde_json::to_value(room_message_type(
                    presentation,
                    "hi <there>".to_owned(),
                    &[style_span(3, 10, bold())],
                    &[]
                ))
                .unwrap(),
                serde_json::json!({
                    "msgtype": msgtype,
                    "body": "hi <there>",
                    "format": "org.matrix.custom.html",
                    "formatted_body": "hi <b>&lt;there&gt;</b>"
                })
            );
        }
    }

    fn sync_message_body(content: serde_json::Value) -> String {
        let (presentation_and_body, styles) = sync_message_text(content);
        assert_eq!(styles, Vec::new(), "a plain message");
        presentation_and_body.1
    }

    fn bold() -> TextStyle {
        TextStyle {
            bold: true,
            ..TextStyle::default()
        }
    }

    fn style_span(start: usize, end: usize, style: TextStyle) -> TextStyleSpan {
        TextStyleSpan { start, end, style }
    }

    #[test]
    fn sync_messages_show_their_html_formatted_body() {
        assert_eq!(
            sync_message_text(serde_json::json!({
                "msgtype": "m.text",
                "body": "**deploy** at [noon](https://example.org/plan)",
                "format": "org.matrix.custom.html",
                "formatted_body": "<strong>deploy</strong> at <a href=\"https://example.org/plan\">noon</a>"
            })),
            (
                (
                    RoomMessagePresentationKind::Text,
                    "deploy at noon (https://example.org/plan)".to_owned()
                ),
                vec![style_span(0, 6, bold())]
            )
        );
        let red = TextStyle {
            foreground: Some(TextColor {
                red: 0xff,
                green: 0,
                blue: 0,
            }),
            ..TextStyle::default()
        };
        assert_eq!(
            sync_message_text(serde_json::json!({
                "msgtype": "m.notice",
                "body": "build failed",
                "format": "org.matrix.custom.html",
                "formatted_body": "build <font color=\"#ff0000\">failed</font>"
            })),
            (
                (
                    RoomMessagePresentationKind::Notice,
                    "build failed".to_owned()
                ),
                vec![style_span(6, 12, red)]
            )
        );
        assert_eq!(
            sync_message_text(serde_json::json!({
                "msgtype": "m.emote",
                "body": "waves _hard_",
                "format": "org.matrix.custom.html",
                "formatted_body": "waves <em>hard</em>"
            })),
            (
                (RoomMessagePresentationKind::Emote, "waves hard".to_owned()),
                vec![style_span(
                    6,
                    10,
                    TextStyle {
                        italic: true,
                        ..TextStyle::default()
                    }
                )]
            )
        );
    }

    #[test]
    fn sync_messages_in_another_format_or_without_html_text_show_the_plain_body() {
        assert_eq!(
            sync_message_body(serde_json::json!({
                "msgtype": "m.text",
                "body": "**plain**",
                "format": "org.example.custom",
                "formatted_body": "<b>not html</b>"
            })),
            "**plain**"
        );
        assert_eq!(
            sync_message_body(serde_json::json!({
                "msgtype": "m.text",
                "body": "> <@alice:example.org> hi\n\nalt text only",
                "format": "org.matrix.custom.html",
                "formatted_body": "<mx-reply><blockquote>hi</blockquote></mx-reply><img src=\"https://example.org/x.png\">",
                "m.relates_to": { "m.in_reply_to": { "event_id": "$hi:example.org" } }
            })),
            "alt text only"
        );
    }

    #[test]
    fn sync_html_replies_drop_mx_reply_but_keep_a_quote_like_start() {
        assert_eq!(
            sync_message_text(serde_json::json!({
                "msgtype": "m.text",
                "body": "> <@alice:example.org> hi\n\n**hello**",
                "format": "org.matrix.custom.html",
                "formatted_body": "<mx-reply><blockquote><a href=\"https://matrix.to/#/!r:example.org/$hi:example.org\">In reply to</a> <a href=\"https://matrix.to/#/@alice:example.org\">@alice:example.org</a><br>hi</blockquote></mx-reply><b>hello</b>",
                "m.relates_to": { "m.in_reply_to": { "event_id": "$hi:example.org" } }
            })),
            (
                (RoomMessagePresentationKind::Text, "hello".to_owned()),
                vec![style_span(0, 5, bold())]
            )
        );
        // The plain fallback is stripped from plain bodies only: HTML text
        // that reads like one is the sender's text.
        assert_eq!(
            sync_message_text(serde_json::json!({
                "msgtype": "m.text",
                "body": "quoted",
                "format": "org.matrix.custom.html",
                "formatted_body": "&gt; &lt;@carol:example.org&gt; said<br><br>this",
                "m.relates_to": { "m.in_reply_to": { "event_id": "$hi:example.org" } }
            })),
            (
                (
                    RoomMessagePresentationKind::Text,
                    "> <@carol:example.org> said\n\nthis".to_owned()
                ),
                Vec::new()
            )
        );
    }

    #[test]
    fn sync_html_edits_take_the_new_content_not_the_fallback() {
        let event: OriginalSyncRoomMessageEvent = serde_json::from_value(serde_json::json!({
            "type": "m.room.message",
            "content": {
                "msgtype": "m.text",
                "body": "* edited **body**",
                "format": "org.matrix.custom.html",
                "formatted_body": "* ",
                "m.new_content": {
                    "msgtype": "m.text",
                    "body": "edited **body**",
                    "format": "org.matrix.custom.html",
                    "formatted_body": "edited <b>body</b>"
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
        .expect("deserialize sync html replacement event");

        assert_eq!(
            sync_message_replacement_update(&event).map(|update| update.kind),
            Some(RoomActivityUpdateKind::MessageReplaced {
                replacement_event_id: "$edit:example.org".to_owned(),
                sender: "@alice:example.org".to_owned(),
                timestamp_ms: 1_700_000_001_000,
                presentation: RoomMessagePresentationKind::Text,
                body: "edited body".to_owned(),
                styles: vec![style_span(7, 11, bold())],
                user_links: Vec::new(),
            })
        );
    }

    #[test]
    fn local_echoes_of_html_messages_carry_their_styles() {
        let content = RoomMessageEventContent::text_html("hi **there**", "hi <b>there</b>");
        let serialized_event =
            SerializableEventContent::new(&AnyMessageLikeEventContent::RoomMessage(content))
                .expect("serialize html text event");

        assert_eq!(
            message_presentation_and_text_from_serialized_event(&serialized_event)
                .expect("read local echo text"),
            Some(LocalEchoMessageDetails {
                presentation: RoomMessagePresentationKind::Text,
                text: StyledText {
                    body: "hi there".to_owned(),
                    styles: vec![style_span(3, 8, bold())],
                    user_links: Vec::new(),
                },
                relation: None,
            })
        );
    }

    fn sync_message_text(
        content: serde_json::Value,
    ) -> ((RoomMessagePresentationKind, String), Vec<TextStyleSpan>) {
        let event: OriginalSyncRoomMessageEvent = serde_json::from_value(serde_json::json!({
            "type": "m.room.message",
            "content": content,
            "event_id": "$event:example.org",
            "sender": "@bob:example.org",
            "origin_server_ts": 1700000000000u64
        }))
        .expect("deserialize sync message event");
        let activity = room_activity_from_sync_message(
            "!room:example.org",
            &event,
            profile("@bob:example.org"),
            false,
        )
        .expect("a message activity");
        let RoomActivityKind::Message(message) = activity.kind else {
            panic!("expected a message activity");
        };
        ((message.presentation, message.body), message.styles)
    }

    #[test]
    fn sync_reply_body_drops_the_plain_reply_fallback() {
        assert_eq!(
            sync_message_body(serde_json::json!({
                "msgtype": "m.text",
                "body": "> <@alice:example.org> hi\n\nhello",
                "m.relates_to": { "m.in_reply_to": { "event_id": "$hi:example.org" } }
            })),
            "hello"
        );
        assert_eq!(
            sync_message_body(serde_json::json!({
                "msgtype": "m.emote",
                "body": "> * <@alice:example.org> waves\n\nwaves back",
                "m.relates_to": { "m.in_reply_to": { "event_id": "$wave:example.org" } }
            })),
            "waves back"
        );
        assert_eq!(
            sync_message_body(serde_json::json!({
                "msgtype": "m.text",
                "body": "> <@alice:example.org> in thread\n\nthread answer",
                "m.relates_to": {
                    "rel_type": "m.thread",
                    "event_id": "$root:example.org",
                    "is_falling_back": true,
                    "m.in_reply_to": { "event_id": "$latest:example.org" }
                }
            })),
            "thread answer"
        );
    }

    #[test]
    fn sync_body_without_a_reply_keeps_a_quote_like_start() {
        let body = "> <@alice:example.org> hi\n\nhello";
        assert_eq!(
            sync_message_body(serde_json::json!({ "msgtype": "m.text", "body": body })),
            body
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
            room_activity_from_sync_message(
                "!room:example.org",
                &event,
                profile("@alice:example.org"),
                false,
            ),
            Some(RoomActivity {
                room_id: "!room:example.org".to_owned(),
                activity_id: "event:$event:example.org".to_owned(),
                timestamp_ms: 1_700_000_000_000,
                highlight: false,
                relation: None,
                kind: RoomActivityKind::Message(RoomMessageActivity {
                    event_id: Some("$event:example.org".to_owned()),
                    transaction_id: None,
                    sender: profile("@alice:example.org"),
                    presentation: RoomMessagePresentationKind::Notice,
                    body: "heads up".to_owned(),
                    styles: Vec::new(),
                    user_links: Vec::new(),
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
            room_activity_from_sync_message(
                "!room:example.org",
                &event,
                profile("@alice:example.org"),
                false,
            ),
            Some(RoomActivity {
                room_id: "!room:example.org".to_owned(),
                activity_id: "event:$event:example.org".to_owned(),
                timestamp_ms: 1_700_000_000_000,
                highlight: false,
                relation: None,
                kind: RoomActivityKind::Message(RoomMessageActivity {
                    event_id: Some("$event:example.org".to_owned()),
                    transaction_id: None,
                    sender: profile("@alice:example.org"),
                    presentation: RoomMessagePresentationKind::Emote,
                    body: "waves".to_owned(),
                    styles: Vec::new(),
                    user_links: Vec::new(),
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
            sync_message_replacement_update(&event),
            Some(RoomActivityUpdate {
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
                    styles: Vec::new(),
                    user_links: Vec::new(),
                },
            })
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
            sync_message_replacement_update(&notice_event),
            Some(RoomActivityUpdate {
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
                    styles: Vec::new(),
                    user_links: Vec::new(),
                },
            })
        );
        assert_eq!(
            sync_message_replacement_update(&emote_event),
            Some(RoomActivityUpdate {
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
                    styles: Vec::new(),
                    user_links: Vec::new(),
                },
            })
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
            sync_message_replacement_update(&event),
            Some(RoomActivityUpdate {
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
                    styles: Vec::new(),
                    user_links: Vec::new(),
                },
            })
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

        let mapped = backfill_event(&event, &RedactionRules::V1, &mut BTreeMap::new());

        assert_eq!(
            mapped,
            Some(RoomHistoryBackfillEvent::Activity(Box::new(RoomActivity {
                room_id: "!room:example.org".to_owned(),
                activity_id: "event:$history:example.org".to_owned(),
                timestamp_ms: 1_700_000_000_100,
                highlight: false,
                relation: None,
                kind: RoomActivityKind::Message(RoomMessageActivity {
                    event_id: Some("$history:example.org".to_owned()),
                    transaction_id: None,
                    sender: profile("@alice:example.org"),
                    presentation: RoomMessagePresentationKind::Text,
                    body: "older message".to_owned(),
                    styles: Vec::new(),
                    user_links: Vec::new(),
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

        let mapped = backfill_event(&event, &RedactionRules::V1, &mut BTreeMap::new());

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
                        styles: Vec::new(),
                        user_links: Vec::new(),
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

        let mapped = backfill_event(&event, &RedactionRules::V1, &mut BTreeMap::new());

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
        let events = backfill_events(
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
                    highlight: false,
                    relation: None,
                    kind: RoomActivityKind::Message(RoomMessageActivity {
                        event_id: Some("$older:example.org".to_owned()),
                        transaction_id: None,
                        sender: profile("@alice:example.org"),
                        presentation: RoomMessagePresentationKind::Text,
                        body: "older".to_owned(),
                        styles: Vec::new(),
                        user_links: Vec::new(),
                        send_state: RoomMessageSendState::Sent,
                    }),
                })),
                RoomHistoryBackfillEvent::Activity(Box::new(RoomActivity {
                    room_id: "!room:example.org".to_owned(),
                    activity_id: "event:$newer:example.org".to_owned(),
                    timestamp_ms: 1_700_000_000_200,
                    highlight: false,
                    relation: None,
                    kind: RoomActivityKind::Message(RoomMessageActivity {
                        event_id: Some("$newer:example.org".to_owned()),
                        transaction_id: None,
                        sender: profile("@alice:example.org"),
                        presentation: RoomMessagePresentationKind::Text,
                        body: "newer".to_owned(),
                        styles: Vec::new(),
                        user_links: Vec::new(),
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
            sync_message_replacement_update(&replacement_event),
            Some(RoomActivityUpdate {
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
                    styles: Vec::new(),
                    user_links: Vec::new(),
                },
            })
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

        assert_eq!(sync_message_replacement_update(&image_event), None);
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
            room_activity_from_sync_message(
                "!room:example.org",
                &event,
                profile("@alice:example.org"),
                false,
            ),
            Some(RoomActivity {
                room_id: "!room:example.org".to_owned(),
                activity_id: "event:$threaded:example.org".to_owned(),
                timestamp_ms: 1_700_000_000_005,
                highlight: false,
                relation: Some(RoomActivityRelation::Thread {
                    thread_root_event_id: "$thread-root:example.org".to_owned(),
                    in_reply_to_event_id: Some("$reply-target:example.org".to_owned()),
                    is_falling_back: true,
                }),
                kind: RoomActivityKind::Message(RoomMessageActivity {
                    event_id: Some("$threaded:example.org".to_owned()),
                    transaction_id: None,
                    sender: profile("@alice:example.org"),
                    presentation: RoomMessagePresentationKind::Text,
                    body: "hello from thread".to_owned(),
                    styles: Vec::new(),
                    user_links: Vec::new(),
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
            room_activity_from_sync_message(
                "!room:example.org",
                &event,
                profile("@alice:example.org"),
                false,
            ),
            Some(RoomActivity {
                room_id: "!room:example.org".to_owned(),
                activity_id: "event:$reply:example.org".to_owned(),
                timestamp_ms: 1_700_000_000_010,
                highlight: false,
                relation: Some(RoomActivityRelation::Reply {
                    in_reply_to_event_id: "$parent:example.org".to_owned(),
                }),
                kind: RoomActivityKind::Message(RoomMessageActivity {
                    event_id: Some("$reply:example.org".to_owned()),
                    transaction_id: None,
                    sender: profile("@alice:example.org"),
                    presentation: RoomMessagePresentationKind::Text,
                    body: "reply only".to_owned(),
                    styles: Vec::new(),
                    user_links: Vec::new(),
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
            room_activity_from_sync_message(
                "!room:example.org",
                &event,
                profile("@alice:example.org"),
                false,
            ),
            Some(RoomActivity {
                room_id: "!room:example.org".to_owned(),
                activity_id: "event:$image-thread:example.org".to_owned(),
                timestamp_ms: 1_700_000_000_015,
                highlight: false,
                relation: Some(RoomActivityRelation::Thread {
                    thread_root_event_id: "$thread-root:example.org".to_owned(),
                    in_reply_to_event_id: None,
                    is_falling_back: false,
                }),
                kind: RoomActivityKind::Image(Box::new(RoomImageActivity {
                    event_id: "$image-thread:example.org".to_owned(),
                    sender: profile("@alice:example.org"),
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
            room_activity_from_sync_message(
                "!room:example.org",
                &event,
                profile("@alice:example.org"),
                false,
            ),
            Some(RoomActivity {
                room_id: "!room:example.org".to_owned(),
                activity_id: "event:$event:example.org".to_owned(),
                timestamp_ms: 1_700_000_000_000,
                highlight: false,
                relation: None,
                kind: RoomActivityKind::Image(Box::new(RoomImageActivity {
                    event_id: "$event:example.org".to_owned(),
                    sender: profile("@alice:example.org"),
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
            room_activity_from_sync_message(
                "!room:example.org",
                &event,
                profile("@alice:example.org"),
                false,
            ),
            Some(RoomActivity {
                room_id: "!room:example.org".to_owned(),
                activity_id: "event:$image:example.org".to_owned(),
                timestamp_ms: 1_700_000_000_050,
                highlight: false,
                relation: None,
                kind: RoomActivityKind::Image(Box::new(RoomImageActivity {
                    event_id: "$image:example.org".to_owned(),
                    sender: profile("@alice:example.org"),
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
            room_activity_from_sync_message(
                "!room:example.org",
                &event,
                profile("@alice:example.org"),
                false,
            ),
            Some(RoomActivity {
                room_id: "!room:example.org".to_owned(),
                activity_id: "event:$encrypted:example.org".to_owned(),
                timestamp_ms: 1_700_000_000_060,
                highlight: false,
                relation: None,
                kind: RoomActivityKind::Image(Box::new(RoomImageActivity {
                    event_id: "$encrypted:example.org".to_owned(),
                    sender: profile("@alice:example.org"),
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
            room_activity_from_sync_message(
                "!room:example.org",
                &event,
                profile("@alice:example.org"),
                false,
            ),
            Some(RoomActivity {
                room_id: "!room:example.org".to_owned(),
                activity_id: "event:$image-thumb:example.org".to_owned(),
                timestamp_ms: 1_700_000_000_070,
                highlight: false,
                relation: None,
                kind: RoomActivityKind::Image(Box::new(RoomImageActivity {
                    event_id: "$image-thumb:example.org".to_owned(),
                    sender: profile("@alice:example.org"),
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
            room_activity_from_sync_message(
                "!room:example.org",
                &event,
                profile("@alice:example.org"),
                false,
            ),
            Some(RoomActivity {
                room_id: "!room:example.org".to_owned(),
                activity_id: "event:$audio:example.org".to_owned(),
                timestamp_ms: 1_700_000_000_100,
                highlight: false,
                relation: None,
                kind: RoomActivityKind::Audio(Box::new(RoomAudioActivity {
                    event_id: "$audio:example.org".to_owned(),
                    sender: profile("@alice:example.org"),
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
            room_activity_from_sync_message(
                "!room:example.org",
                &event,
                profile("@alice:example.org"),
                false,
            ),
            Some(RoomActivity {
                room_id: "!room:example.org".to_owned(),
                activity_id: "event:$file:example.org".to_owned(),
                timestamp_ms: 1_700_000_000_200,
                highlight: false,
                relation: None,
                kind: RoomActivityKind::File(Box::new(RoomFileActivity {
                    event_id: "$file:example.org".to_owned(),
                    sender: profile("@alice:example.org"),
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
            room_activity_from_sync_message(
                "!room:example.org",
                &event,
                profile("@alice:example.org"),
                false,
            ),
            Some(RoomActivity {
                room_id: "!room:example.org".to_owned(),
                activity_id: "event:$video:example.org".to_owned(),
                timestamp_ms: 1_700_000_000_300,
                highlight: false,
                relation: None,
                kind: RoomActivityKind::Video(Box::new(RoomVideoActivity {
                    event_id: "$video:example.org".to_owned(),
                    sender: profile("@alice:example.org"),
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
            room_activity_from_sync_message(
                "!room:example.org",
                &event,
                profile("@alice:example.org"),
                false,
            ),
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
            room_activity_from_sync_message(
                "!room:example.org",
                &event,
                profile("@alice:example.org"),
                false,
            ),
            Some(RoomActivity {
                room_id: "!room:example.org".to_owned(),
                activity_id: "event:$custom:example.org".to_owned(),
                timestamp_ms: 1_700_000_000_450,
                highlight: false,
                relation: None,
                kind: RoomActivityKind::UnsupportedMessage(UnsupportedRoomMessageActivity {
                    event_id: "$custom:example.org".to_owned(),
                    sender: profile("@alice:example.org"),
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
            room_activity_from_sync_topic(
                "!room:example.org",
                &event,
                profile("@alice:example.org"),
                false,
            ),
            RoomActivity {
                room_id: "!room:example.org".to_owned(),
                activity_id: "state:$topic:example.org".to_owned(),
                timestamp_ms: 1_700_000_000_001,
                highlight: false,
                relation: None,
                kind: RoomActivityKind::StateNotice(RoomStateNoticeActivity {
                    event_id: "$topic:example.org".to_owned(),
                    sender: profile("@alice:example.org"),
                    content: RoomStateNoticeContent::Topic {
                        topic: "incident review".to_owned(),
                    },
                }),
            }
        );
    }

    fn member_event(
        sender: &str,
        target: &str,
        content: serde_json::Value,
        prev_content: Option<serde_json::Value>,
    ) -> OriginalSyncRoomMemberEvent {
        let mut event = serde_json::json!({
            "type": "m.room.member",
            "content": content,
            "event_id": "$member:example.org",
            "sender": sender,
            "origin_server_ts": 1700000000002u64,
            "state_key": target
        });
        if let Some(prev_content) = prev_content {
            event["unsigned"] = serde_json::json!({ "prev_content": prev_content });
        }
        serde_json::from_value(event).expect("deserialize member event")
    }

    #[test]
    fn member_change_classification_covers_spec_transitions() {
        let carol = "@carol:example.org";
        let op = "@alice:example.org";
        let joined = serde_json::json!({ "membership": "join", "displayname": "Carol" });
        let invited = serde_json::json!({ "membership": "invite", "displayname": "Carol" });
        let knocking = serde_json::json!({ "membership": "knock", "displayname": "Carol" });
        let left = serde_json::json!({ "membership": "leave" });
        let banned = serde_json::json!({ "membership": "ban" });
        let cases = [
            (carol, joined.clone(), None, Some(RoomMemberChange::Joined)),
            (
                carol,
                joined.clone(),
                Some(knocking.clone()),
                Some(RoomMemberChange::Joined),
            ),
            (
                carol,
                joined.clone(),
                Some(invited.clone()),
                Some(RoomMemberChange::InvitationAccepted),
            ),
            (
                carol,
                left.clone(),
                Some(joined.clone()),
                Some(RoomMemberChange::Left),
            ),
            (
                op,
                left.clone(),
                Some(joined.clone()),
                Some(RoomMemberChange::Kicked),
            ),
            (
                op,
                banned.clone(),
                Some(joined.clone()),
                Some(RoomMemberChange::KickedAndBanned),
            ),
            (
                op,
                banned.clone(),
                Some(left.clone()),
                Some(RoomMemberChange::Banned),
            ),
            (
                op,
                left.clone(),
                Some(banned.clone()),
                Some(RoomMemberChange::Unbanned),
            ),
            (
                op,
                invited.clone(),
                Some(left.clone()),
                Some(RoomMemberChange::Invited),
            ),
            (
                carol,
                left.clone(),
                Some(invited.clone()),
                Some(RoomMemberChange::InvitationRejected),
            ),
            (
                op,
                left.clone(),
                Some(invited.clone()),
                Some(RoomMemberChange::InvitationRevoked),
            ),
            (
                carol,
                knocking.clone(),
                Some(left.clone()),
                Some(RoomMemberChange::Knocked),
            ),
            (
                op,
                invited.clone(),
                Some(knocking.clone()),
                Some(RoomMemberChange::KnockAccepted),
            ),
            (
                carol,
                left.clone(),
                Some(knocking.clone()),
                Some(RoomMemberChange::KnockRetracted),
            ),
            (
                op,
                left.clone(),
                Some(knocking.clone()),
                Some(RoomMemberChange::KnockDenied),
            ),
            (
                carol,
                serde_json::json!({ "membership": "join", "displayname": "Caz" }),
                Some(joined.clone()),
                Some(RoomMemberChange::DisplayNameChanged {
                    old: Some("Carol".to_owned()),
                    new: Some("Caz".to_owned()),
                }),
            ),
            (
                carol,
                serde_json::json!({ "membership": "join" }),
                Some(joined.clone()),
                Some(RoomMemberChange::DisplayNameChanged {
                    old: Some("Carol".to_owned()),
                    new: None,
                }),
            ),
            (
                carol,
                serde_json::json!({
                    "membership": "join",
                    "displayname": "Carol",
                    "avatar_url": "mxc://example.org/avatar"
                }),
                Some(joined.clone()),
                Some(RoomMemberChange::AvatarChanged),
            ),
            (
                carol,
                serde_json::json!({
                    "membership": "join",
                    "displayname": "Caz",
                    "avatar_url": "mxc://example.org/avatar"
                }),
                Some(joined.clone()),
                Some(RoomMemberChange::DisplayNameChanged {
                    old: Some("Carol".to_owned()),
                    new: Some("Caz".to_owned()),
                }),
            ),
            // A member event repeating the current profile changes nothing.
            (carol, joined.clone(), Some(joined.clone()), None),
            // The spec forbids join -> invite; ruma classifies it as an error.
            (op, invited.clone(), Some(joined.clone()), None),
        ];

        for (sender, content, prev_content, expected) in cases {
            let event = member_event(sender, carol, content.clone(), prev_content.clone());
            assert_eq!(
                room_member_change_from_sync_member(&event),
                expected,
                "{sender} {prev_content:?} -> {content}"
            );
        }
    }

    #[test]
    fn member_target_display_name_comes_from_the_event_describing_the_member() {
        let carol = "@carol:example.org";
        let joined = serde_json::json!({ "membership": "join", "displayname": "Carol" });

        for (content, prev_content, expected) in [
            (joined.clone(), None, Some("Carol")),
            (
                serde_json::json!({ "membership": "invite", "displayname": "Invitee" }),
                None,
                Some("Invitee"),
            ),
            (
                serde_json::json!({ "membership": "knock", "displayname": "Knocker" }),
                None,
                Some("Knocker"),
            ),
            (
                serde_json::json!({ "membership": "leave" }),
                Some(joined.clone()),
                Some("Carol"),
            ),
            (
                serde_json::json!({ "membership": "ban", "reason": "spam" }),
                Some(joined.clone()),
                Some("Carol"),
            ),
            (serde_json::json!({ "membership": "leave" }), None, None),
        ] {
            let event = member_event(carol, carol, content.clone(), prev_content);
            assert_eq!(
                member_target_display_name(&event).as_deref(),
                expected,
                "{content}"
            );
        }
    }

    #[test]
    fn kick_member_state_mapping_carries_change_target_and_reason() {
        let event = member_event(
            "@alice:example.org",
            "@carol:example.org",
            serde_json::json!({ "membership": "leave", "reason": "flooding" }),
            Some(serde_json::json!({ "membership": "join", "displayname": "Carol" })),
        );
        let target = RoomUserProfile {
            user_id: "@carol:example.org".to_owned(),
            display_name: member_target_display_name(&event),
            display_name_ambiguous: false,
        };

        assert_eq!(
            room_activity_from_sync_member(
                "!room:example.org",
                &event,
                room_member_change_from_sync_member(&event).expect("kick changes membership"),
                profile("@alice:example.org"),
                target,
                false,
            ),
            RoomActivity {
                room_id: "!room:example.org".to_owned(),
                activity_id: "state:$member:example.org".to_owned(),
                timestamp_ms: 1_700_000_000_002,
                highlight: false,
                relation: None,
                kind: RoomActivityKind::StateNotice(RoomStateNoticeActivity {
                    event_id: "$member:example.org".to_owned(),
                    sender: profile("@alice:example.org"),
                    content: RoomStateNoticeContent::Member {
                        change: RoomMemberChange::Kicked,
                        target: RoomUserProfile {
                            user_id: "@carol:example.org".to_owned(),
                            display_name: Some("Carol".to_owned()),
                            display_name_ambiguous: false,
                        },
                        reason: Some("flooding".to_owned()),
                    },
                }),
            }
        );
    }

    fn profile_test_state_events() -> serde_json::Value {
        serde_json::json!([
            { "type": "m.room.create", "state_key": "", "sender": "@example:localhost", "event_id": "$create:localhost", "origin_server_ts": 1u64, "content": { "creator": "@example:localhost" } },
            { "type": "m.room.member", "state_key": "@example:localhost", "sender": "@example:localhost", "event_id": "$me:localhost", "origin_server_ts": 2u64, "content": { "membership": "join", "displayname": "Me" } },
            { "type": "m.room.member", "state_key": "@bob:localhost", "sender": "@bob:localhost", "event_id": "$bob:localhost", "origin_server_ts": 3u64, "content": { "membership": "join", "displayname": "Bob" } },
            { "type": "m.room.member", "state_key": "@sam1:localhost", "sender": "@sam1:localhost", "event_id": "$sam1:localhost", "origin_server_ts": 4u64, "content": { "membership": "join", "displayname": "Sam" } },
            { "type": "m.room.member", "state_key": "@sam2:localhost", "sender": "@sam2:localhost", "event_id": "$sam2:localhost", "origin_server_ts": 5u64, "content": { "membership": "join", "displayname": "Sam" } }
        ])
    }

    fn raw_sync_timeline_event(event: serde_json::Value) -> Raw<AnySyncTimelineEvent> {
        Raw::from_json_string(event.to_string()).expect("serialize raw sync timeline event")
    }

    fn text_message_json(event_id: &str, sender: &str, body: &str) -> serde_json::Value {
        serde_json::json!({
            "type": "m.room.message",
            "content": { "msgtype": "m.text", "body": body },
            "event_id": event_id,
            "sender": sender,
            "origin_server_ts": 1700000000000u64
        })
    }

    fn member_change_json(
        event_id: &str,
        user_id: &str,
        content: serde_json::Value,
        prev_content: serde_json::Value,
    ) -> serde_json::Value {
        serde_json::json!({
            "type": "m.room.member",
            "content": content,
            "unsigned": { "prev_content": prev_content },
            "event_id": event_id,
            "sender": user_id,
            "state_key": user_id,
            "origin_server_ts": 1700000000001u64
        })
    }

    fn drain_events(event_rx: &mpsc::Receiver<Event>) -> Vec<Event> {
        std::iter::from_fn(|| event_rx.recv_timeout(Duration::from_millis(500)).ok()).collect()
    }

    /// Syncs one joined room and then does what the continuous sync does
    /// after each response.
    async fn sync_and_refresh_members(
        server: &MatrixMockServer,
        client: &Client,
        room: JoinedRoomBuilder,
        events: &EventSender,
    ) {
        let _scope = server
            .mock_sync()
            .ok(|builder| {
                builder.add_joined_room(room);
            })
            .mount_as_scoped()
            .await;
        let response = client.sync_once(Default::default()).await.expect("sync");
        emit_member_snapshots_after_sync(client, &response, events).await;
    }

    fn state_activity_ids(events: &[Event]) -> Vec<&str> {
        events
            .iter()
            .filter_map(|event| match event {
                Event::RoomActivity(activity) if activity.activity_id.starts_with("state:") => {
                    Some(activity.activity_id.as_str())
                }
                _ => None,
            })
            .collect()
    }

    fn member_snapshots(events: &[Event]) -> Vec<&RoomMembers> {
        events
            .iter()
            .filter_map(|event| match event {
                Event::RoomMembers(snapshot) => Some(snapshot),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn state_section_events_produce_no_room_activity() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("build tokio runtime");

        runtime.block_on(async {
            let mock_server = MatrixMockServer::new().await;
            let client = mock_server.client_builder().build().await;
            let (events, event_rx) = mpsc::channel();
            register_event_handlers(&client, events.clone());
            let state_events = serde_json::json!([
                { "type": "m.room.create", "state_key": "", "sender": "@example:localhost", "event_id": "$create:localhost", "origin_server_ts": 1u64, "content": { "creator": "@example:localhost" } },
                { "type": "m.room.power_levels", "state_key": "", "sender": "@example:localhost", "event_id": "$power:localhost", "origin_server_ts": 2u64, "content": { "users": { "@example:localhost": 100 } }, "unsigned": { "prev_content": { "users": {} } } },
                { "type": "m.room.member", "state_key": "@example:localhost", "sender": "@example:localhost", "event_id": "$me:localhost", "origin_server_ts": 3u64, "content": { "membership": "join", "displayname": "Me" } },
                { "type": "m.room.member", "state_key": "@bob:localhost", "sender": "@bob:localhost", "event_id": "$bob:localhost", "origin_server_ts": 4u64, "content": { "membership": "join", "displayname": "Bob" } },
                { "type": "m.room.member", "state_key": "@carol:localhost", "sender": "@carol:localhost", "event_id": "$carol:localhost", "origin_server_ts": 5u64, "content": { "membership": "join", "displayname": "Carol" } },
                { "type": "m.room.topic", "state_key": "", "sender": "@example:localhost", "event_id": "$topic:localhost", "origin_server_ts": 6u64, "content": { "topic": "welcome" } }
            ]);

            sync_and_refresh_members(
                &mock_server,
                &client,
                JoinedRoomBuilder::new(room_id().as_ref())
                    .add_state_bulk(raw_sync_state_events(&state_events))
                    .add_timeline_event(raw_sync_timeline_event(text_message_json(
                        "$bob-msg:localhost",
                        "@bob:localhost",
                        "hi",
                    ))),
                &events,
            )
            .await;
            let got = drain_events(&event_rx);

            assert_eq!(state_activity_ids(&got), Vec::<&str>::new(), "{got:?}");
            let activity_ids = got
                .iter()
                .filter_map(|event| match event {
                    Event::RoomActivity(activity) => Some(activity.activity_id.as_str()),
                    _ => None,
                })
                .collect::<Vec<_>>();
            assert_eq!(activity_ids, vec!["event:$bob-msg:localhost"]);
            let snapshots = member_snapshots(&got);
            assert_eq!(snapshots.len(), 1, "{got:?}");
            assert_eq!(snapshots[0].room_id, room_id().as_str());
            assert_eq!(snapshots[0].members.len(), 3);
        });
    }

    #[test]
    fn timeline_state_events_still_produce_notices() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("build tokio runtime");

        runtime.block_on(async {
            let mock_server = MatrixMockServer::new().await;
            let client = mock_server.client_builder().build().await;
            let (events, event_rx) = mpsc::channel();
            register_event_handlers(&client, events.clone());
            sync_and_refresh_members(
                &mock_server,
                &client,
                JoinedRoomBuilder::new(room_id().as_ref())
                    .add_state_bulk(raw_sync_state_events(&profile_test_state_events())),
                &events,
            )
            .await;
            drain_events(&event_rx);

            let carol_join = serde_json::json!({
                "type": "m.room.member",
                "content": { "membership": "join", "displayname": "Carol" },
                "event_id": "$carol-join:localhost",
                "sender": "@carol:localhost",
                "state_key": "@carol:localhost",
                "origin_server_ts": 1700000000003u64
            });
            let topic = serde_json::json!({
                "type": "m.room.topic",
                "content": { "topic": "new topic" },
                "event_id": "$topic:localhost",
                "sender": "@example:localhost",
                "state_key": "",
                "origin_server_ts": 1700000000004u64
            });
            sync_and_refresh_members(
                &mock_server,
                &client,
                JoinedRoomBuilder::new(room_id().as_ref())
                    .add_timeline_event(raw_sync_timeline_event(carol_join))
                    .add_timeline_event(raw_sync_timeline_event(topic))
                    .add_timeline_event(raw_sync_timeline_event(power_levels_event_json(
                        "$power:localhost",
                        serde_json::json!({ "users": { "@bob:localhost": 50 } }),
                        Some(serde_json::json!({ "users": {} })),
                    ))),
                &events,
            )
            .await;
            let got = drain_events(&event_rx);

            assert_eq!(
                state_activity_ids(&got),
                vec![
                    "state:$carol-join:localhost",
                    "state:$topic:localhost",
                    "state:$power:localhost",
                ],
                "{got:?}"
            );
            assert!(
                got.iter().all(|event| match event {
                    Event::RoomActivity(activity) => !activity.highlight,
                    _ => true,
                }),
                "{got:?}"
            );
            assert_eq!(member_snapshots(&got).len(), 1, "{got:?}");
            let Some(Event::RoomMembers(snapshot)) = got.last() else {
                panic!("member snapshot last in {got:?}");
            };
            assert!(
                snapshot
                    .members
                    .iter()
                    .any(|member| member.user_id == "@carol:localhost"),
                "{snapshot:?}"
            );
        });
    }

    #[test]
    fn message_only_sync_emits_no_member_snapshot() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("build tokio runtime");

        runtime.block_on(async {
            let mock_server = MatrixMockServer::new().await;
            let client = mock_server.client_builder().build().await;
            let (events, event_rx) = mpsc::channel();
            register_event_handlers(&client, events.clone());
            sync_and_refresh_members(
                &mock_server,
                &client,
                JoinedRoomBuilder::new(room_id().as_ref())
                    .add_state_bulk(raw_sync_state_events(&profile_test_state_events())),
                &events,
            )
            .await;
            drain_events(&event_rx);

            sync_and_refresh_members(
                &mock_server,
                &client,
                JoinedRoomBuilder::new(room_id().as_ref()).add_timeline_event(
                    raw_sync_timeline_event(text_message_json(
                        "$bob-msg:localhost",
                        "@bob:localhost",
                        "hi",
                    )),
                ),
                &events,
            )
            .await;
            let got = drain_events(&event_rx);

            assert_eq!(member_snapshots(&got), Vec::<&RoomMembers>::new());
            assert_eq!(
                room_activity_by_id(&got, "event:$bob-msg:localhost").len(),
                1,
                "{got:?}"
            );
        });
    }

    fn joined_room_update(
        state: &serde_json::Value,
        timeline: &serde_json::Value,
    ) -> JoinedRoomUpdate {
        JoinedRoomUpdate {
            state: State::Before(raw_sync_state_events(state)),
            timeline: Timeline {
                events: timeline
                    .as_array()
                    .expect("timeline event array")
                    .iter()
                    .map(|event| {
                        TimelineEvent::from_plaintext(raw_sync_timeline_event(event.clone()))
                    })
                    .collect(),
                ..Default::default()
            },
            ..Default::default()
        }
    }

    #[test]
    fn joined_room_update_changes_members_for_a_state_section_member() {
        let state = serde_json::json!([
            { "type": "m.room.member", "state_key": "@bob:localhost", "sender": "@bob:localhost", "event_id": "$bob:localhost", "origin_server_ts": 1u64, "content": { "membership": "join" } }
        ]);
        assert!(joined_room_update_changes_members(&joined_room_update(
            &state,
            &serde_json::json!([])
        )));
    }

    #[test]
    fn joined_room_update_changes_members_for_timeline_power_levels() {
        let timeline = serde_json::json!([power_levels_event_json(
            "$power:localhost",
            serde_json::json!({ "users": { "@bob:localhost": 50 } }),
            None,
        )]);
        assert!(joined_room_update_changes_members(&joined_room_update(
            &serde_json::json!([]),
            &timeline
        )));
    }

    #[test]
    fn joined_room_update_changes_members_not_for_topic_and_messages() {
        let state = serde_json::json!([
            { "type": "m.room.topic", "state_key": "", "sender": "@bob:localhost", "event_id": "$topic:localhost", "origin_server_ts": 1u64, "content": { "topic": "t" } }
        ]);
        let timeline =
            serde_json::json!([text_message_json("$msg:localhost", "@bob:localhost", "hi")]);
        assert!(!joined_room_update_changes_members(&joined_room_update(
            &state, &timeline
        )));
    }

    #[test]
    fn joined_room_update_changes_members_not_for_an_empty_update() {
        assert!(!joined_room_update_changes_members(
            &JoinedRoomUpdate::default()
        ));
    }

    fn room_activity_by_id<'a>(events: &'a [Event], activity_id: &str) -> Vec<&'a RoomActivity> {
        events
            .iter()
            .filter_map(|event| match event {
                Event::RoomActivity(activity) if activity.activity_id == activity_id => {
                    Some(activity.as_ref())
                }
                _ => None,
            })
            .collect()
    }

    fn sender_of(activity: &RoomActivity) -> &RoomUserProfile {
        match &activity.kind {
            RoomActivityKind::Message(message) => &message.sender,
            RoomActivityKind::StateNotice(notice) => &notice.sender,
            kind => panic!("unexpected activity kind {kind:?}"),
        }
    }

    fn member_profile(user_id: &str, display_name: &str, ambiguous: bool) -> RoomUserProfile {
        RoomUserProfile {
            user_id: user_id.to_owned(),
            display_name: Some(display_name.to_owned()),
            display_name_ambiguous: ambiguous,
        }
    }

    #[test]
    fn live_activity_carries_sender_profiles_and_member_changes() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("build tokio runtime");

        runtime.block_on(async {
            let mock_server = MatrixMockServer::new().await;
            let client = mock_server.client_builder().build().await;
            let (events, event_rx) = mpsc::channel();
            register_event_handlers(&client, events.clone());
            let state_events = profile_test_state_events();

            sync_and_refresh_members(
                &mock_server,
                &client,
                JoinedRoomBuilder::new(room_id().as_ref())
                    .add_state_bulk(raw_sync_state_events(&state_events))
                    .add_timeline_event(raw_sync_timeline_event(text_message_json(
                        "$bob-msg:localhost",
                        "@bob:localhost",
                        "hi",
                    )))
                    .add_timeline_event(raw_sync_timeline_event(text_message_json(
                        "$sam-msg:localhost",
                        "@sam1:localhost",
                        "hello",
                    ))),
                &events,
            )
            .await;
            let first = drain_events(&event_rx);
            assert_eq!(
                state_activity_ids(&first),
                Vec::<&str>::new(),
                "the state section is no activity: {first:?}"
            );

            let bob_message = room_activity_by_id(&first, "event:$bob-msg:localhost");
            assert_eq!(bob_message.len(), 1);
            assert_eq!(
                sender_of(bob_message[0]),
                &member_profile("@bob:localhost", "Bob", false)
            );
            let sam_message = room_activity_by_id(&first, "event:$sam-msg:localhost");
            assert_eq!(sam_message.len(), 1);
            assert_eq!(
                sender_of(sam_message[0]),
                &member_profile("@sam1:localhost", "Sam", true)
            );
            let Some(Event::RoomMembers(snapshot)) = first
                .iter()
                .rev()
                .find(|event| matches!(event, Event::RoomMembers(_)))
            else {
                panic!("member snapshot in {first:?}");
            };
            assert_eq!(
                snapshot
                    .members
                    .iter()
                    .map(|member| (member.user_id.as_str(), member.display_name_ambiguous))
                    .collect::<Vec<_>>(),
                vec![
                    // Without power levels the creator has 100, so ranks first.
                    ("@example:localhost", false),
                    ("@bob:localhost", false),
                    ("@sam1:localhost", true),
                    ("@sam2:localhost", true),
                ]
            );

            let bob_joined = serde_json::json!({ "membership": "join", "displayname": "Bob" });
            let sam_joined = serde_json::json!({ "membership": "join", "displayname": "Sam" });
            sync_and_refresh_members(
                &mock_server,
                &client,
                JoinedRoomBuilder::new(room_id().as_ref())
                    .add_timeline_event(raw_sync_timeline_event(member_change_json(
                        "$bob-rename:localhost",
                        "@bob:localhost",
                        serde_json::json!({ "membership": "join", "displayname": "Robert" }),
                        bob_joined,
                    )))
                    .add_timeline_event(raw_sync_timeline_event(member_change_json(
                        "$sam-avatar:localhost",
                        "@sam2:localhost",
                        serde_json::json!({
                            "membership": "join",
                            "displayname": "Sam",
                            "avatar_url": "mxc://localhost/avatar"
                        }),
                        sam_joined.clone(),
                    )))
                    .add_timeline_event(raw_sync_timeline_event(member_change_json(
                        "$sam-noop:localhost",
                        "@sam1:localhost",
                        sam_joined.clone(),
                        sam_joined,
                    ))),
                &events,
            )
            .await;
            let second = drain_events(&event_rx);

            let rename = room_activity_by_id(&second, "state:$bob-rename:localhost");
            assert_eq!(rename.len(), 1);
            assert_eq!(
                rename[0].kind,
                RoomActivityKind::StateNotice(RoomStateNoticeActivity {
                    event_id: "$bob-rename:localhost".to_owned(),
                    sender: member_profile("@bob:localhost", "Robert", false),
                    content: RoomStateNoticeContent::Member {
                        change: RoomMemberChange::DisplayNameChanged {
                            old: Some("Bob".to_owned()),
                            new: Some("Robert".to_owned()),
                        },
                        target: member_profile("@bob:localhost", "Robert", false),
                        reason: None,
                    },
                })
            );
            let avatar = room_activity_by_id(&second, "state:$sam-avatar:localhost");
            assert_eq!(avatar.len(), 1);
            assert_eq!(
                avatar[0].kind,
                RoomActivityKind::StateNotice(RoomStateNoticeActivity {
                    event_id: "$sam-avatar:localhost".to_owned(),
                    sender: member_profile("@sam2:localhost", "Sam", true),
                    content: RoomStateNoticeContent::Member {
                        change: RoomMemberChange::AvatarChanged,
                        target: member_profile("@sam2:localhost", "Sam", true),
                        reason: None,
                    },
                })
            );
            assert!(room_activity_by_id(&second, "state:$sam-noop:localhost").is_empty());
            assert!(
                !second.iter().any(|event| matches!(event, Event::Error(_))),
                "{second:?}"
            );
        });
    }

    #[test]
    fn backfill_resolves_current_sender_profiles_and_member_targets() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("build tokio runtime");

        runtime.block_on(async {
            let mock_server = MatrixMockServer::new().await;
            let client = mock_server.client_builder().build().await;
            let state_events = profile_test_state_events();
            mock_server
                .sync_room(
                    &client,
                    JoinedRoomBuilder::new(room_id().as_ref())
                        .add_state_bulk(raw_sync_state_events(&state_events)),
                )
                .await;
            let mut history_message = text_message_json("$old:localhost", "@bob:localhost", "old");
            history_message["room_id"] = serde_json::json!(room_id().as_str());
            let history_kick = serde_json::json!({
                "type": "m.room.member",
                "room_id": room_id().as_str(),
                "content": { "membership": "leave", "reason": "flooding" },
                "unsigned": { "prev_content": { "membership": "join", "displayname": "Sam" } },
                "event_id": "$kick:localhost",
                "sender": "@example:localhost",
                "state_key": "@sam2:localhost",
                "origin_server_ts": 1700000000003u64
            });
            mock_server
                .mock_room_messages()
                .ok(RoomMessagesResponseTemplate::default().events(vec![
                    Raw::<AnyTimelineEvent>::from_json_string(history_kick.to_string())
                        .expect("raw kick"),
                    Raw::<AnyTimelineEvent>::from_json_string(history_message.to_string())
                        .expect("raw message"),
                ]))
                .mock_once()
                .mount()
                .await;

            let backfill = backfill_room_history(
                &client,
                BackfillRoomHistoryRequest {
                    room_id: room_id().to_string(),
                    from: None,
                    limit: 10,
                },
            )
            .await
            .expect("backfill room history");

            let activities = backfill
                .events
                .iter()
                .map(|event| match event {
                    RoomHistoryBackfillEvent::Activity(activity) => activity.as_ref(),
                    update => panic!("unexpected backfill update {update:?}"),
                })
                .collect::<Vec<_>>();
            assert_eq!(activities.len(), 2);
            assert_eq!(
                sender_of(activities[0]),
                &member_profile("@bob:localhost", "Bob", false)
            );
            assert_eq!(
                activities[1].kind,
                RoomActivityKind::StateNotice(RoomStateNoticeActivity {
                    event_id: "$kick:localhost".to_owned(),
                    sender: member_profile("@example:localhost", "Me", false),
                    content: RoomStateNoticeContent::Member {
                        change: RoomMemberChange::Kicked,
                        target: member_profile("@sam2:localhost", "Sam", true),
                        reason: Some("flooding".to_owned()),
                    },
                })
            );
        });
    }

    fn list_request(
        server: Option<&str>,
        filter: Option<&str>,
        since: Option<&str>,
    ) -> ListPublicRoomsRequest {
        ListPublicRoomsRequest {
            server: server.map(str::to_owned),
            filter: filter.map(str::to_owned),
            since: since.map(str::to_owned),
            limit: 100,
        }
    }

    #[test]
    fn list_public_rooms_converts_the_page_in_server_order() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("build tokio runtime");

        runtime.block_on(async {
            let mock_server = MatrixMockServer::new().await;
            let client = mock_server.client_builder().build().await;
            wiremock::Mock::given(wiremock::matchers::method("POST"))
                .and(wiremock::matchers::path("/_matrix/client/v3/publicRooms"))
                .and(wiremock::matchers::query_param("server", "other.example"))
                .and(wiremock::matchers::body_partial_json(serde_json::json!({
                    "limit": 100,
                    "since": "batch-1",
                    "filter": { "generic_search_term": "rust" }
                })))
                .respond_with(wiremock::ResponseTemplate::new(200).set_body_json(
                    serde_json::json!({
                        "chunk": [
                            {
                                "room_id": "!big:other.example",
                                "canonical_alias": "#rust:other.example",
                                "name": "Rust",
                                "topic": "The Rust language\nBe nice",
                                "num_joined_members": 4200,
                                "world_readable": true,
                                "guest_can_join": false
                            },
                            {
                                "room_id": "!small:other.example",
                                "num_joined_members": 3,
                                "world_readable": false,
                                "guest_can_join": false
                            }
                        ],
                        "next_batch": "batch-2",
                        "total_room_count_estimate": 250
                    }),
                ))
                .expect(1)
                .mount(mock_server.server())
                .await;

            let request = list_request(Some("other.example"), Some("rust"), Some("batch-1"));
            assert_eq!(
                list_public_rooms(&client, request.clone()).await,
                PublicRoomsPage {
                    request,
                    result: PublicRoomsResult::Loaded {
                        rooms: vec![
                            PublicRoom {
                                room_id: "!big:other.example".to_owned(),
                                canonical_alias: Some("#rust:other.example".to_owned()),
                                name: Some("Rust".to_owned()),
                                topic: Some("The Rust language\nBe nice".to_owned()),
                                joined_members: 4200,
                            },
                            PublicRoom {
                                room_id: "!small:other.example".to_owned(),
                                canonical_alias: None,
                                name: None,
                                topic: None,
                                joined_members: 3,
                            },
                        ],
                        next_batch: Some("batch-2".to_owned()),
                        total_estimate: Some(250),
                    },
                }
            );
        });
    }

    #[test]
    fn list_public_rooms_of_the_home_server_sends_no_server_filter_or_since() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("build tokio runtime");

        runtime.block_on(async {
            let mock_server = MatrixMockServer::new().await;
            let client = mock_server.client_builder().build().await;
            mock_server
                .mock_public_rooms()
                .ok(Vec::new(), None, None, None)
                .expect(1)
                .mount()
                .await;

            let page = list_public_rooms(&client, list_request(None, None, None)).await;
            assert_eq!(
                page.result,
                PublicRoomsResult::Loaded {
                    rooms: Vec::new(),
                    next_batch: None,
                    total_estimate: None,
                }
            );
            let requests = mock_server
                .received_requests()
                .await
                .expect("request recording enabled");
            let request = requests
                .iter()
                .find(|request| request.url.path() == "/_matrix/client/v3/publicRooms")
                .expect("the publicRooms request");
            assert_eq!(request.url.query(), None);
            assert_eq!(
                serde_json::from_slice::<serde_json::Value>(&request.body).expect("JSON body"),
                serde_json::json!({ "limit": 100 })
            );
        });
    }

    #[test]
    fn list_public_rooms_reports_a_server_error_as_failed() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("build tokio runtime");

        runtime.block_on(async {
            let mock_server = MatrixMockServer::new().await;
            let client = mock_server.client_builder().build().await;
            mock_server
                .mock_public_rooms()
                .respond_with(wiremock::ResponseTemplate::new(500).set_body_json(
                    serde_json::json!({ "errcode": "M_UNKNOWN", "error": "directory down" }),
                ))
                .mount()
                .await;

            let page = list_public_rooms(&client, list_request(None, None, None)).await;
            let PublicRoomsResult::Failed { message } = &page.result else {
                panic!("expected a failure, got {page:?}");
            };
            assert!(
                message.starts_with("failed to list public rooms:")
                    && message.contains("directory down"),
                "{message}"
            );
        });
    }

    #[test]
    fn list_public_rooms_rejects_an_invalid_server_name_without_a_request() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("build tokio runtime");

        runtime.block_on(async {
            let mock_server = MatrixMockServer::new().await;
            let client = mock_server.client_builder().build().await;
            mock_server
                .mock_public_rooms()
                .ok(Vec::new(), None, None, None)
                .expect(0)
                .mount()
                .await;

            let page =
                list_public_rooms(&client, list_request(Some("bad server"), None, None)).await;
            let PublicRoomsResult::Failed { message } = &page.result else {
                panic!("expected a failure, got {page:?}");
            };
            assert!(
                message.starts_with("invalid Matrix server name bad server:"),
                "{message}"
            );
        });
    }

    /// Creates a room through a mocked `/createRoom` and returns the request
    /// body the client sent.
    fn created_room_body(request: CreateRoomRequest) -> serde_json::Value {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("build tokio runtime");

        runtime.block_on(async {
            let mock_server = MatrixMockServer::new().await;
            let client = mock_server.client_builder().build().await;
            mock_server.mock_create_room().ok().expect(1).mount().await;

            assert_eq!(
                create_room(&client, &request).await.expect("create room"),
                "!room:example.org"
            );

            let requests = mock_server
                .received_requests()
                .await
                .expect("request recording enabled");
            let create_requests = requests
                .iter()
                .filter(|request| request.url.path() == "/_matrix/client/v3/createRoom")
                .collect::<Vec<_>>();
            assert_eq!(create_requests.len(), 1);
            create_requests[0]
                .body_json::<serde_json::Value>()
                .expect("createRoom JSON body")
        })
    }

    #[test]
    fn create_room_private_is_an_encrypted_unlisted_private_chat() {
        let body = created_room_body(CreateRoomRequest {
            name: "Team room".to_owned(),
            visibility: CreateRoomVisibility::Private,
        });

        assert_eq!(body["name"], "Team room");
        assert_eq!(body["preset"], "private_chat");
        assert_eq!(body.get("visibility"), None, "private is the default");
        assert_eq!(body.get("room_alias_name"), None);
        assert_eq!(
            body["initial_state"],
            serde_json::json!([{
                "type": "m.room.encryption",
                "state_key": "",
                "content": {
                    "algorithm": "m.megolm.v1.aes-sha2",
                    "rotation_period_ms": 604800000,
                    "rotation_period_msgs": 100
                }
            }])
        );
    }

    #[test]
    fn create_room_public_is_a_listed_public_chat_with_an_alias() {
        let body = created_room_body(CreateRoomRequest {
            name: "Foo Bar".to_owned(),
            visibility: CreateRoomVisibility::Public {
                alias_localpart: "foo".to_owned(),
            },
        });

        assert_eq!(body["name"], "Foo Bar");
        assert_eq!(body["preset"], "public_chat");
        assert_eq!(body["visibility"], "public");
        assert_eq!(body["room_alias_name"], "foo");
        assert_eq!(
            body.get("initial_state"),
            None,
            "a public room is not encrypted"
        );
    }

    #[test]
    fn fetch_room_event_maps_the_event_or_reports_why_not() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("build tokio runtime");

        runtime.block_on(async {
            let mock_server = MatrixMockServer::new().await;
            let client = mock_server.client_builder().build().await;
            mock_server
                .sync_room(
                    &client,
                    JoinedRoomBuilder::new(room_id().as_ref())
                        .add_state_bulk(raw_sync_state_events(&profile_test_state_events())),
                )
                .await;
            let in_room = |mut event: serde_json::Value| {
                event["room_id"] = serde_json::json!(room_id().as_str());
                timeline_event(event)
            };
            mock_server
                .mock_room_event()
                .room(room_id())
                .match_event_id()
                .ok(in_room(text_message_json(
                    "$old:localhost",
                    "@bob:localhost",
                    "are we deploying today?",
                )))
                .mount()
                .await;
            mock_server
                .mock_room_event()
                .room(room_id())
                .match_event_id()
                .ok(in_room(serde_json::json!({
                    "type": "m.room.message",
                    "content": {
                        "msgtype": "m.text",
                        "body": "* fixed",
                        "m.new_content": { "msgtype": "m.text", "body": "fixed" },
                        "m.relates_to": { "rel_type": "m.replace", "event_id": "$old:localhost" }
                    },
                    "event_id": "$edit:localhost",
                    "sender": "@bob:localhost",
                    "origin_server_ts": 1700000000001u64
                })))
                .mount()
                .await;
            mock_server
                .mock_room_event()
                .room(room_id())
                .ok_with_template(wiremock::ResponseTemplate::new(404).set_body_json(
                    serde_json::json!({ "errcode": "M_NOT_FOUND", "error": "Event not found." }),
                ))
                .mount()
                .await;
            let fetch = |room_id: &str, event_id: &str| {
                fetch_room_event(
                    &client,
                    FetchRoomEventRequest {
                        room_id: room_id.to_owned(),
                        event_id: event_id.to_owned(),
                    },
                )
            };

            let loaded = fetch("!room:example.org", "$old:localhost").await;
            let RoomEventFetchResult::Loaded { activity } = loaded.result else {
                panic!("expected a loaded event, got {loaded:?}");
            };
            assert_eq!(
                (loaded.room_id.as_str(), loaded.event_id.as_str()),
                ("!room:example.org", "$old:localhost")
            );
            let RoomActivityKind::Message(message) = &activity.kind else {
                panic!("expected a message, got {activity:?}");
            };
            assert_eq!(message.body, "are we deploying today?");
            assert_eq!(message.event_id.as_deref(), Some("$old:localhost"));
            assert_eq!(
                sender_of(&activity),
                &member_profile("@bob:localhost", "Bob", false)
            );

            assert_eq!(
                fetch("!room:example.org", "$edit:localhost").await,
                RoomEventFetch {
                    room_id: "!room:example.org".to_owned(),
                    event_id: "$edit:localhost".to_owned(),
                    result: RoomEventFetchResult::Failed {
                        message: "unsupported event".to_owned(),
                    },
                }
            );

            let missing = fetch("!room:example.org", "$missing:localhost").await;
            let RoomEventFetchResult::Failed { message } = &missing.result else {
                panic!("expected a failure, got {missing:?}");
            };
            assert!(
                message.starts_with("failed to fetch Matrix event"),
                "{message}"
            );

            assert_eq!(
                fetch("!other:example.org", "$old:localhost").await.result,
                RoomEventFetchResult::Failed {
                    message: "joined room not found: !other:example.org".to_owned(),
                }
            );
        });
    }

    #[test]
    fn activity_highlight_requires_highlight_tweak_from_another_sender() {
        use matrix_sdk::ruma::push::{Action, HighlightTweakValue, Tweak};

        let own = OwnedUserId::try_from("@me:example.org").expect("own user id");
        let other = OwnedUserId::try_from("@bob:example.org").expect("other user id");
        let highlight = [
            Action::Notify,
            Action::SetTweak(Tweak::Highlight(HighlightTweakValue::Yes)),
        ];

        assert!(activity_highlight(&own, &other, &highlight));
        assert!(!activity_highlight(&own, &other, &[Action::Notify]));
        assert!(!activity_highlight(&own, &other, &[]));
        assert!(!activity_highlight(&own, &own, &highlight));
    }

    fn mention_message_json(event_id: &str, sender: &str, mentioned: &str) -> serde_json::Value {
        serde_json::json!({
            "type": "m.room.message",
            "content": {
                "msgtype": "m.text",
                "body": "ping",
                "m.mentions": { "user_ids": [mentioned] }
            },
            "event_id": event_id,
            "sender": sender,
            "origin_server_ts": 1700000000000u64
        })
    }

    #[test]
    fn live_activity_highlight_follows_push_rules_except_for_own_events() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("build tokio runtime");

        runtime.block_on(async {
            let mock_server = MatrixMockServer::new().await;
            let client = mock_server.client_builder().build().await;
            let (events, event_rx) = mpsc::channel();
            register_event_handlers(&client, events);
            let state_events = profile_test_state_events();

            // No `m.push_rules` account data: the SDK applies the server
            // default rules, which highlight a user mention.
            mock_server
                .sync_room(
                    &client,
                    JoinedRoomBuilder::new(room_id().as_ref())
                        .add_state_bulk(raw_sync_state_events(&state_events))
                        .add_timeline_event(raw_sync_timeline_event(mention_message_json(
                            "$mention:localhost",
                            "@bob:localhost",
                            "@example:localhost",
                        )))
                        .add_timeline_event(raw_sync_timeline_event(text_message_json(
                            "$plain:localhost",
                            "@bob:localhost",
                            "hi",
                        )))
                        .add_timeline_event(raw_sync_timeline_event(mention_message_json(
                            "$own-mention:localhost",
                            "@example:localhost",
                            "@example:localhost",
                        ))),
                )
                .await;
            let events = drain_events(&event_rx);

            let highlight_of = |activity_id| {
                let activities = room_activity_by_id(&events, activity_id);
                assert_eq!(activities.len(), 1, "{activity_id} in {events:?}");
                activities[0].highlight
            };
            assert!(highlight_of("event:$mention:localhost"));
            assert!(!highlight_of("event:$plain:localhost"));
            assert!(!highlight_of("event:$own-mention:localhost"));
        });
    }

    #[test]
    fn backfill_activity_highlight_follows_push_rules() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("build tokio runtime");

        runtime.block_on(async {
            let mock_server = MatrixMockServer::new().await;
            let client = mock_server.client_builder().build().await;
            let state_events = profile_test_state_events();
            mock_server
                .sync_room(
                    &client,
                    JoinedRoomBuilder::new(room_id().as_ref())
                        .add_state_bulk(raw_sync_state_events(&state_events)),
                )
                .await;
            let mut mention = mention_message_json(
                "$old-mention:localhost",
                "@bob:localhost",
                "@example:localhost",
            );
            mention["room_id"] = serde_json::json!(room_id().as_str());
            let mut plain = text_message_json("$old-plain:localhost", "@bob:localhost", "old");
            plain["room_id"] = serde_json::json!(room_id().as_str());
            mock_server
                .mock_room_messages()
                .ok(RoomMessagesResponseTemplate::default().events(vec![
                    Raw::<AnyTimelineEvent>::from_json_string(plain.to_string())
                        .expect("raw plain message"),
                    Raw::<AnyTimelineEvent>::from_json_string(mention.to_string())
                        .expect("raw mention"),
                ]))
                .mock_once()
                .mount()
                .await;

            let backfill = backfill_room_history(
                &client,
                BackfillRoomHistoryRequest {
                    room_id: room_id().to_string(),
                    from: None,
                    limit: 10,
                },
            )
            .await
            .expect("backfill room history");

            let highlights = backfill
                .events
                .iter()
                .map(|event| match event {
                    RoomHistoryBackfillEvent::Activity(activity) => {
                        (activity.activity_id.as_str(), activity.highlight)
                    }
                    update => panic!("unexpected backfill update {update:?}"),
                })
                .collect::<Vec<_>>();
            assert_eq!(
                highlights,
                vec![
                    ("event:$old-mention:localhost", true),
                    ("event:$old-plain:localhost", false),
                ]
            );
        });
    }

    fn power_levels_change_of(
        old: serde_json::Value,
        new: serde_json::Value,
    ) -> Option<PowerLevelsChange> {
        power_levels_change(
            &serde_json::from_value::<PossiblyRedactedRoomPowerLevelsEventContent>(old)
                .expect("old power levels"),
            &serde_json::from_value::<RoomPowerLevelsEventContent>(new).expect("new power levels"),
        )
    }

    fn user_level_change(user_id: &str, old: i64, new: i64) -> (OwnedUserId, i64, i64) {
        (
            OwnedUserId::try_from(user_id).expect("valid user id"),
            old,
            new,
        )
    }

    #[test]
    fn power_levels_change_reports_effective_user_levels_in_user_id_order() {
        assert_eq!(
            power_levels_change_of(
                serde_json::json!({
                    "users_default": 0,
                    "users": {
                        "@zed:example.org": 0,
                        "@demoted:example.org": 50,
                        "@removed:example.org": 100,
                        "@same:example.org": 50
                    }
                }),
                serde_json::json!({
                    "users_default": 0,
                    "users": {
                        "@zed:example.org": 50,
                        "@demoted:example.org": 0,
                        "@added:example.org": 10,
                        "@same:example.org": 50
                    }
                }),
            ),
            Some(PowerLevelsChange {
                users: vec![
                    user_level_change("@added:example.org", 0, 10),
                    user_level_change("@demoted:example.org", 50, 0),
                    user_level_change("@removed:example.org", 100, 0),
                    user_level_change("@zed:example.org", 0, 50),
                ],
                users_default: None,
                other_permissions_changed: false,
            })
        );
    }

    #[test]
    fn power_levels_change_reports_default_and_permission_changes_separately() {
        assert_eq!(
            power_levels_change_of(
                serde_json::json!({ "users_default": 0, "users": { "@op:example.org": 100 } }),
                serde_json::json!({ "users_default": 10, "users": { "@op:example.org": 100 } }),
            ),
            Some(PowerLevelsChange {
                users: Vec::new(),
                users_default: Some(RoomPowerLevelValueChange { old: 0, new: 10 }),
                other_permissions_changed: false,
            })
        );
        // A user whose explicit level is dropped falls back to the new
        // default.
        assert_eq!(
            power_levels_change_of(
                serde_json::json!({ "users_default": 0, "users": { "@op:example.org": 0 } }),
                serde_json::json!({ "users_default": 10 }),
            ),
            Some(PowerLevelsChange {
                users: vec![user_level_change("@op:example.org", 0, 10)],
                users_default: Some(RoomPowerLevelValueChange { old: 0, new: 10 }),
                other_permissions_changed: false,
            })
        );
        for (old, new) in [
            (serde_json::json!({}), serde_json::json!({ "ban": 100 })),
            (serde_json::json!({}), serde_json::json!({ "kick": 0 })),
            (serde_json::json!({}), serde_json::json!({ "redact": 0 })),
            (serde_json::json!({}), serde_json::json!({ "invite": 50 })),
            (
                serde_json::json!({}),
                serde_json::json!({ "events": { "m.room.name": 100 } }),
            ),
            (
                serde_json::json!({}),
                serde_json::json!({ "events_default": 10 }),
            ),
            (
                serde_json::json!({}),
                serde_json::json!({ "state_default": 100 }),
            ),
            (
                serde_json::json!({}),
                serde_json::json!({ "notifications": { "room": 0 } }),
            ),
        ] {
            assert_eq!(
                power_levels_change_of(old, new.clone()),
                Some(PowerLevelsChange {
                    users: Vec::new(),
                    users_default: None,
                    other_permissions_changed: true,
                }),
                "{new}"
            );
        }
    }

    #[test]
    fn power_levels_change_is_none_for_identical_levels() {
        let levels = serde_json::json!({
            "ban": 50, "kick": 50, "users_default": 0,
            "users": { "@op:example.org": 100 }
        });
        assert_eq!(power_levels_change_of(levels.clone(), levels), None);
        // Spelling a default out explicitly is not a change.
        assert_eq!(
            power_levels_change_of(
                serde_json::json!({ "users": { "@op:example.org": 100 } }),
                serde_json::json!({
                    "ban": 50, "users_default": 0,
                    "users": { "@op:example.org": 100, "@zero:example.org": 0 }
                }),
            ),
            None
        );
    }

    fn power_levels_event_json(
        event_id: &str,
        content: serde_json::Value,
        prev_content: Option<serde_json::Value>,
    ) -> serde_json::Value {
        let mut event = serde_json::json!({
            "type": "m.room.power_levels",
            "content": content,
            "event_id": event_id,
            "sender": "@example:localhost",
            "state_key": "",
            "origin_server_ts": 1700000000002u64
        });
        if let Some(prev_content) = prev_content {
            event["unsigned"] = serde_json::json!({ "prev_content": prev_content });
        }
        event
    }

    fn member_can_kick(events: &[Event], user_id: &str) -> Vec<bool> {
        events
            .iter()
            .filter_map(|event| match event {
                Event::RoomMembers(snapshot) => snapshot
                    .members
                    .iter()
                    .find(|member| member.user_id == user_id)
                    .map(|member| member.can_kick),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn live_power_level_change_emits_notice_and_refreshes_member_capabilities() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("build tokio runtime");

        runtime.block_on(async {
            let mock_server = MatrixMockServer::new().await;
            let client = mock_server.client_builder().build().await;
            let (events, event_rx) = mpsc::channel();
            register_event_handlers(&client, events.clone());
            let levels = |actor: i64| {
                serde_json::json!({
                    "kick": 50, "ban": 50, "users_default": 0,
                    "users": { "@example:localhost": actor, "@bob:localhost": 0 }
                })
            };
            let mut state_events = profile_test_state_events();
            state_events
                .as_array_mut()
                .expect("state events")
                .push(power_levels_event_json(
                    "$power0:localhost",
                    levels(0),
                    None,
                ));

            sync_and_refresh_members(
                &mock_server,
                &client,
                JoinedRoomBuilder::new(room_id().as_ref())
                    .add_state_bulk(raw_sync_state_events(&state_events)),
                &events,
            )
            .await;
            let first = drain_events(&event_rx);
            assert_eq!(
                state_activity_ids(&first),
                Vec::<&str>::new(),
                "the state section is no activity: {first:?}"
            );
            assert!(
                room_activity_by_id(&first, "state:$power0:localhost").is_empty(),
                "levels without previous content are not a change: {first:?}"
            );
            assert_eq!(
                member_can_kick(&first, "@bob:localhost").last(),
                Some(&false)
            );

            sync_and_refresh_members(
                &mock_server,
                &client,
                JoinedRoomBuilder::new(room_id().as_ref()).add_timeline_event(
                    raw_sync_timeline_event(power_levels_event_json(
                        "$power1:localhost",
                        levels(50),
                        Some(levels(0)),
                    )),
                ),
                &events,
            )
            .await;
            let second = drain_events(&event_rx);

            let notice = room_activity_by_id(&second, "state:$power1:localhost");
            assert_eq!(notice.len(), 1, "{second:?}");
            assert_eq!(
                notice[0].kind,
                RoomActivityKind::StateNotice(RoomStateNoticeActivity {
                    event_id: "$power1:localhost".to_owned(),
                    sender: member_profile("@example:localhost", "Me", false),
                    content: RoomStateNoticeContent::PowerLevels {
                        changes: vec![RoomPowerLevelUserChange {
                            target: member_profile("@example:localhost", "Me", false),
                            old: 0,
                            new: 50,
                        }],
                        users_default: None,
                        other_permissions_changed: false,
                    },
                })
            );
            // The regression: kick rights follow the new levels without any
            // member event.
            assert_eq!(member_can_kick(&second, "@bob:localhost"), vec![true]);
            let Some(Event::RoomMembers(snapshot)) = second
                .iter()
                .find(|event| matches!(event, Event::RoomMembers(_)))
            else {
                panic!("member snapshot in {second:?}");
            };
            assert_eq!(snapshot.members[0].user_id, "@example:localhost");
            assert_eq!(snapshot.members[0].rank, RoomMemberRank::Moderator);
        });
    }

    #[test]
    fn backfill_decodes_power_level_changes_and_ignores_other_state() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("build tokio runtime");

        runtime.block_on(async {
            let mock_server = MatrixMockServer::new().await;
            let client = mock_server.client_builder().build().await;
            mock_server
                .sync_room(
                    &client,
                    JoinedRoomBuilder::new(room_id().as_ref())
                        .add_state_bulk(raw_sync_state_events(&profile_test_state_events())),
                )
                .await;
            let mut history_levels = power_levels_event_json(
                "$power:localhost",
                serde_json::json!({
                    "users_default": 10,
                    "users": { "@bob:localhost": 50 }
                }),
                Some(serde_json::json!({ "users": { "@bob:localhost": 0 } })),
            );
            history_levels["room_id"] = serde_json::json!(room_id().as_str());
            // Every power-level field has a default, so this unrelated
            // state event would decode as a change of the default level.
            let history_other_state = serde_json::json!({
                "type": "org.example.settings",
                "room_id": room_id().as_str(),
                "content": { "users_default": 5 },
                "unsigned": { "prev_content": { "users_default": 0 } },
                "event_id": "$settings:localhost",
                "sender": "@example:localhost",
                "state_key": "",
                "origin_server_ts": 1700000000004u64
            });
            mock_server
                .mock_room_messages()
                .ok(RoomMessagesResponseTemplate::default().events(vec![
                    Raw::<AnyTimelineEvent>::from_json_string(history_other_state.to_string())
                        .expect("raw settings"),
                    Raw::<AnyTimelineEvent>::from_json_string(history_levels.to_string())
                        .expect("raw power levels"),
                ]))
                .mock_once()
                .mount()
                .await;

            let backfill = backfill_room_history(
                &client,
                BackfillRoomHistoryRequest {
                    room_id: room_id().to_string(),
                    from: None,
                    limit: 10,
                },
            )
            .await
            .expect("backfill room history");

            assert_eq!(
                backfill.events,
                vec![RoomHistoryBackfillEvent::Activity(Box::new(RoomActivity {
                    room_id: room_id().to_string(),
                    activity_id: "state:$power:localhost".to_owned(),
                    timestamp_ms: 1700000000002,
                    highlight: false,
                    relation: None,
                    kind: RoomActivityKind::StateNotice(RoomStateNoticeActivity {
                        event_id: "$power:localhost".to_owned(),
                        sender: member_profile("@example:localhost", "Me", false),
                        content: RoomStateNoticeContent::PowerLevels {
                            changes: vec![RoomPowerLevelUserChange {
                                target: member_profile("@bob:localhost", "Bob", false),
                                old: 0,
                                new: 50,
                            },],
                            users_default: Some(RoomPowerLevelValueChange { old: 0, new: 10 }),
                            other_permissions_changed: false,
                        },
                    }),
                }))]
            );
        });
    }

    #[test]
    fn local_echo_sender_is_the_authenticated_users_room_profile() {
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
                }
            ]))
            .await;

            assert_eq!(
                own_room_user_profile(&client, room_id().as_ref(), "@alice:example.org")
                    .await
                    .expect("own room profile"),
                RoomUserProfile {
                    user_id: "@alice:example.org".to_owned(),
                    display_name: Some("Alice".to_owned()),
                    display_name_ambiguous: false,
                }
            );
        });
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

    fn push_rules_account_data(ruleset: &Ruleset) -> serde_json::Value {
        serde_json::json!({
            "type": "m.push_rules",
            "content": { "global": ruleset },
        })
    }

    /// The server defaults plus a mute (override) rule for `muted` and a
    /// mentions-only (room, no actions) rule for `mentions`.
    fn notification_mode_ruleset(client: &Client, muted: &RoomId, mentions: &RoomId) -> Ruleset {
        let mut ruleset = Ruleset::server_default(client.user_id().expect("logged-in test client"));
        ruleset
            .insert(
                NewPushRule::Override(NewConditionalPushRule::new(
                    muted.to_string(),
                    vec![PushCondition::EventMatch(EventMatchConditionData::new(
                        "room_id".to_owned(),
                        muted.to_string(),
                    ))],
                    Vec::new(),
                )),
                None,
                None,
            )
            .expect("insert mute rule");
        ruleset
            .insert(
                NewPushRule::Room(NewSimplePushRule::new(mentions.to_owned(), Vec::new())),
                None,
                None,
            )
            .expect("insert mentions rule");
        ruleset
    }

    fn summary_notification_modes(
        rooms: &[RoomSummary],
    ) -> Vec<(String, Option<RoomNotificationMode>)> {
        rooms
            .iter()
            .map(|room| (room.room_id.clone(), room.notification_mode))
            .collect()
    }

    #[test]
    fn summarize_rooms_reports_each_joined_rooms_notification_mode() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("build tokio runtime");

        runtime.block_on(async {
            let mock_server = MatrixMockServer::new().await;
            let client = mock_server.client_builder().build().await;
            mock_server
                .mock_room_state_encryption()
                .plain()
                .mount()
                .await;
            let muted = OwnedRoomId::try_from("!a-muted:example.org").expect("room id");
            let mentions = OwnedRoomId::try_from("!b-mentions:example.org").expect("room id");
            let default = OwnedRoomId::try_from("!c-default:example.org").expect("room id");
            let invite = OwnedRoomId::try_from("!d-invite:example.org").expect("room id");
            let rules =
                push_rules_account_data(&notification_mode_ruleset(&client, &muted, &mentions));

            mock_server
                .mock_sync()
                .ok_and_run(&client, |builder| {
                    builder
                        .add_joined_room(JoinedRoomBuilder::new(&muted))
                        .add_joined_room(JoinedRoomBuilder::new(&mentions))
                        .add_joined_room(JoinedRoomBuilder::new(&default))
                        .add_invited_room(InvitedRoomBuilder::new(&invite))
                        .add_custom_global_account_data(rules);
                })
                .await;

            let mut rooms = summarize_rooms(&client).await.expect("summarize rooms");
            rooms.sort_by(|left, right| left.room_id.cmp(&right.room_id));
            assert_eq!(
                summary_notification_modes(&rooms),
                vec![
                    (muted.to_string(), Some(RoomNotificationMode::Mute)),
                    (
                        mentions.to_string(),
                        Some(RoomNotificationMode::MentionsAndKeywordsOnly)
                    ),
                    // A group room under the server defaults.
                    (default.to_string(), Some(RoomNotificationMode::AllMessages)),
                    (invite.to_string(), None),
                ]
            );
        });
    }

    /// Settles when the handler runs: after the base client stored the new
    /// push rules, so the snapshot it emits already uses them.
    #[test]
    fn push_rules_change_re_snapshots_room_notification_modes() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("build tokio runtime");

        runtime.block_on(async {
            let mock_server = MatrixMockServer::new().await;
            let client = mock_server.client_builder().build().await;
            mock_server
                .mock_room_state_encryption()
                .plain()
                .mount()
                .await;
            let muted = OwnedRoomId::try_from("!a-muted:example.org").expect("room id");
            let mentions = OwnedRoomId::try_from("!b-mentions:example.org").expect("room id");
            mock_server
                .mock_sync()
                .ok_and_run(&client, |builder| {
                    builder
                        .add_joined_room(JoinedRoomBuilder::new(&muted))
                        .add_joined_room(JoinedRoomBuilder::new(&mentions));
                })
                .await;
            let (events, event_rx) = mpsc::channel();
            register_event_handlers(&client, events);

            let mut ruleset = notification_mode_ruleset(&client, &muted, &mentions);
            ruleset
                .insert(
                    NewPushRule::Content(NewPatternedPushRule::new(
                        "deploy".to_owned(),
                        "deploy".to_owned(),
                        highlight_word_actions(),
                    )),
                    None,
                    None,
                )
                .expect("insert keyword rule");
            sync_push_rules(&mock_server, &client, &ruleset).await;

            let events = drain_events(&event_rx);
            let [Event::SyncedRooms(rooms), Event::HighlightWords(words)] = events.as_slice()
            else {
                panic!("expected SyncedRooms then HighlightWords, got {events:?}");
            };
            let mut rooms = rooms.clone();
            rooms.sort_by(|left, right| left.room_id.cmp(&right.room_id));
            assert_eq!(
                summary_notification_modes(&rooms),
                vec![
                    (muted.to_string(), Some(RoomNotificationMode::Mute)),
                    (
                        mentions.to_string(),
                        Some(RoomNotificationMode::MentionsAndKeywordsOnly)
                    ),
                ]
            );
            assert_eq!(
                words,
                &vec![HighlightWord {
                    word: "deploy".to_owned(),
                    highlights: true,
                }]
            );
        });
    }

    #[test]
    fn set_room_notification_mode_writes_the_rooms_push_rule() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("build tokio runtime");

        runtime.block_on(async {
            let mock_server = MatrixMockServer::new().await;
            let client = mock_server.client_builder().build().await;
            mock_server
                .sync_room(&client, JoinedRoomBuilder::new(room_id().as_ref()))
                .await;
            // Muting is an override rule named after the room.
            mock_server
                .mock_set_push_rules(
                    RuleKind::Override,
                    PushRuleIdSpec::Some("!room:example.org"),
                )
                .ok()
                .expect(1)
                .mount()
                .await;

            set_room_notification_mode(
                &client,
                SetRoomNotificationModeRequest {
                    room_id: room_id().to_string(),
                    mode: RoomNotificationMode::Mute,
                },
            )
            .await
            .expect("mute the room");

            let error = set_room_notification_mode(
                &client,
                SetRoomNotificationModeRequest {
                    room_id: "!other:example.org".to_owned(),
                    mode: RoomNotificationMode::Mute,
                },
            )
            .await
            .expect_err("only a joined room has a mode");
            assert_eq!(
                error.to_string(),
                "joined room not found: !other:example.org"
            );
        });
    }

    fn content_rule(
        rule_id: &str,
        pattern: &str,
        actions: Vec<Action>,
        enabled: bool,
    ) -> PatternedPushRule {
        serde_json::from_value(serde_json::json!({
            "rule_id": rule_id,
            "pattern": pattern,
            "actions": actions,
            "default": false,
            "enabled": enabled,
        }))
        .expect("content rule")
    }

    /// The server defaults with these user content rules appended.
    fn ruleset_with_content_rules(user_id: &UserId, rules: Vec<PatternedPushRule>) -> Ruleset {
        let mut ruleset = Ruleset::server_default(user_id);
        ruleset.content.extend(rules);
        ruleset
    }

    fn notify_only() -> Vec<Action> {
        vec![Action::Notify]
    }

    #[test]
    fn highlight_words_from_ruleset_lists_enabled_user_keywords_once() {
        let user_id = UserId::parse("@example:localhost").expect("user id");
        let server_default: PatternedPushRule = serde_json::from_value(serde_json::json!({
            "rule_id": ".m.rule.contains_user_name",
            "pattern": "example",
            "actions": highlight_word_actions(),
            "default": true,
            "enabled": true,
        }))
        .expect("server default content rule");
        let ruleset = ruleset_with_content_rules(
            &user_id,
            vec![
                server_default,
                content_rule("deploy", "deploy", highlight_word_actions(), true),
                content_rule("build", "build", notify_only(), true),
                content_rule("off", "off", highlight_word_actions(), false),
                // A second rule for a word merges into the first; either
                // highlighting makes the word highlight.
                content_rule("build-highlight", "build", highlight_word_actions(), true),
                content_rule("deploy-again", "deploy", notify_only(), true),
            ],
        );

        // The server default rule is not the user's word.
        assert_eq!(
            highlight_words_from_ruleset(&ruleset),
            vec![
                HighlightWord {
                    word: "deploy".to_owned(),
                    highlights: true,
                },
                HighlightWord {
                    word: "build".to_owned(),
                    highlights: true,
                },
            ]
        );
        assert_eq!(
            highlight_words_from_ruleset(&Ruleset::server_default(&user_id)),
            Vec::new()
        );
    }

    fn canonical_alias_state(alias: Option<&str>, alt_aliases: &[&str]) -> Raw<AnySyncStateEvent> {
        Raw::from_json_string(
            serde_json::json!({
                "type": "m.room.canonical_alias",
                "state_key": "",
                "sender": "@example:localhost",
                "event_id": format!("$alias-{}:localhost", alt_aliases.len()),
                "origin_server_ts": 1u64,
                "content": { "alias": alias, "alt_aliases": alt_aliases },
            })
            .to_string(),
        )
        .expect("canonical alias state event")
    }

    fn summary_aliases(rooms: &[RoomSummary]) -> Vec<(String, Option<String>, Vec<String>)> {
        rooms
            .iter()
            .map(|room| {
                (
                    room.room_id.clone(),
                    room.canonical_alias.clone(),
                    room.alt_aliases.clone(),
                )
            })
            .collect()
    }

    #[test]
    fn summarize_rooms_reports_room_aliases() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("build tokio runtime");

        runtime.block_on(async {
            let mock_server = MatrixMockServer::new().await;
            let client = mock_server.client_builder().build().await;
            mock_server
                .mock_room_state_encryption()
                .plain()
                .mount()
                .await;
            let ops = OwnedRoomId::try_from("!a-ops:example.org").expect("room id");
            let invite = OwnedRoomId::try_from("!b-invite:example.org").expect("room id");
            mock_server
                .mock_sync()
                .ok_and_run(&client, |builder| {
                    builder
                        .add_joined_room(JoinedRoomBuilder::new(&ops).add_state_event(
                            canonical_alias_state(
                                Some("#ops:example.org"),
                                &["#hall:example.org", "#ops:other.org"],
                            ),
                        ))
                        .add_invited_room(InvitedRoomBuilder::new(&invite));
                })
                .await;

            let mut rooms = summarize_rooms(&client).await.expect("summarize rooms");
            rooms.sort_by(|left, right| left.room_id.cmp(&right.room_id));
            assert_eq!(
                summary_aliases(&rooms),
                vec![
                    (
                        ops.to_string(),
                        Some("#ops:example.org".to_owned()),
                        vec!["#hall:example.org".to_owned(), "#ops:other.org".to_owned()],
                    ),
                    (invite.to_string(), None, Vec::new()),
                ]
            );
        });
    }

    /// The room-list task re-snapshots the rooms when a room's
    /// `m.room.canonical_alias` changes.
    #[test]
    fn canonical_alias_change_re_snapshots_rooms() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("build tokio runtime");

        tokio::task::LocalSet::new().block_on(&runtime, async {
            let mock_server = MatrixMockServer::new().await;
            let client = mock_server.client_builder().build().await;
            mock_server
                .mock_room_state_encryption()
                .plain()
                .mount()
                .await;
            let ops = OwnedRoomId::try_from("!ops:example.org").expect("room id");
            mock_server
                .mock_sync()
                .ok_and_run(&client, |builder| {
                    builder.add_joined_room(JoinedRoomBuilder::new(&ops));
                })
                .await;
            let (events, event_rx) = mpsc::channel();
            let task = super::start_room_list_update_task(client.clone(), events);

            mock_server
                .mock_sync()
                .ok_and_run(&client, |builder| {
                    builder.add_joined_room(
                        JoinedRoomBuilder::new(&ops)
                            .add_state_event(canonical_alias_state(Some("#ops:example.org"), &[])),
                    );
                })
                .await;

            let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
            let rooms = loop {
                match event_rx.try_recv() {
                    Ok(Event::SyncedRooms(rooms)) => break rooms,
                    Ok(event) => panic!("expected SyncedRooms, got {event:?}"),
                    Err(mpsc::TryRecvError::Empty) => {
                        assert!(
                            tokio::time::Instant::now() < deadline,
                            "no SyncedRooms after the alias change"
                        );
                        tokio::time::sleep(Duration::from_millis(10)).await;
                    }
                    Err(mpsc::TryRecvError::Disconnected) => panic!("room-list task stopped"),
                }
            };
            assert_eq!(
                summary_aliases(&rooms),
                vec![(
                    ops.to_string(),
                    Some("#ops:example.org".to_owned()),
                    Vec::new()
                )]
            );
            task.abort();
        });
    }

    fn ignored_user_list_json(user_ids: &[&str]) -> serde_json::Value {
        let ignored_users = user_ids
            .iter()
            .map(|user_id| ((*user_id).to_owned(), serde_json::json!({})))
            .collect::<serde_json::Map<_, _>>();
        serde_json::json!({
            "type": "m.ignored_user_list",
            "content": { "ignored_users": ignored_users },
        })
    }

    async fn sync_ignored_users(
        mock_server: &MatrixMockServer,
        client: &Client,
        user_ids: &[&str],
    ) {
        let list = ignored_user_list_json(user_ids);
        mock_server
            .mock_sync()
            .ok_and_run(client, |builder| {
                builder.add_custom_global_account_data(list);
            })
            .await;
    }

    /// The bodies of the ignore-list PUTs the server received.
    async fn ignored_user_list_puts(mock_server: &MatrixMockServer) -> Vec<serde_json::Value> {
        mock_server
            .server()
            .received_requests()
            .await
            .expect("recorded requests")
            .into_iter()
            .filter(|request| {
                request.method == wiremock::http::Method::PUT
                    && request
                        .url
                        .path()
                        .ends_with("/account_data/m.ignored_user_list")
            })
            .map(|request| serde_json::from_slice(&request.body).expect("JSON body"))
            .collect()
    }

    #[test]
    fn ignore_user_puts_the_ignored_user_list() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("build tokio runtime");

        runtime.block_on(async {
            let mock_server = MatrixMockServer::new().await;
            let client = mock_server.client_builder().build().await;
            wiremock::Mock::given(wiremock::matchers::method("PUT"))
                .and(wiremock::matchers::path_regex(
                    r"^/_matrix/client/v3/user/.*/account_data/m\.ignored_user_list$",
                ))
                .respond_with(
                    wiremock::ResponseTemplate::new(200).set_body_json(serde_json::json!({})),
                )
                .mount(mock_server.server())
                .await;

            ignore_user(&client, "@bob:localhost")
                .await
                .expect("ignore bob");
            assert_eq!(
                ignored_user_list_puts(&mock_server).await,
                vec![serde_json::json!({ "ignored_users": { "@bob:localhost": {} } })]
            );

            // The SDK edits the stored list: the server's echo seeds it.
            sync_ignored_users(
                &mock_server,
                &client,
                &["@bob:localhost", "@spam:localhost"],
            )
            .await;
            assert_eq!(
                ignored_users(&client).await.expect("stored list"),
                vec!["@bob:localhost".to_owned(), "@spam:localhost".to_owned()]
            );
            unignore_user(&client, "@bob:localhost")
                .await
                .expect("unignore bob");
            assert_eq!(
                ignored_user_list_puts(&mock_server).await.last(),
                Some(&serde_json::json!({ "ignored_users": { "@spam:localhost": {} } }))
            );

            let puts = ignored_user_list_puts(&mock_server).await.len();
            assert_eq!(
                ignore_user(&client, "@example:localhost")
                    .await
                    .expect_err("ignoring yourself")
                    .to_string(),
                "you cannot ignore yourself"
            );
            assert_eq!(
                ignore_user(&client, "bob")
                    .await
                    .expect_err("an invalid user ID")
                    .to_string(),
                r#"invalid user ID "bob": leading sigil is incorrect or missing"#
            );
            assert_eq!(
                unignore_user(&client, "@bob")
                    .await
                    .expect_err("an invalid user ID")
                    .to_string(),
                r#"invalid user ID "@bob": required colon is missing"#
            );
            assert_eq!(ignored_user_list_puts(&mock_server).await.len(), puts);
        });
    }

    #[test]
    fn ignored_user_list_change_emits_ignored_users() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("build tokio runtime");

        runtime.block_on(async {
            let mock_server = MatrixMockServer::new().await;
            let client = mock_server.client_builder().build().await;
            assert_eq!(
                ignored_users(&client).await.expect("no list yet"),
                Vec::<String>::new()
            );
            let (events, event_rx) = mpsc::channel();
            register_event_handlers(&client, events);

            sync_ignored_users(
                &mock_server,
                &client,
                &["@spam:localhost", "@bob:localhost"],
            )
            .await;

            assert_eq!(
                drain_events(&event_rx),
                vec![Event::IgnoredUsers(vec![
                    "@bob:localhost".to_owned(),
                    "@spam:localhost".to_owned(),
                ])]
            );
        });
    }

    /// The SDK's event cache clears every room when the ignore list changes;
    /// the room-list and read-state tasks, and the timeline handlers, keep
    /// working after it.
    #[test]
    fn ignored_user_list_change_keeps_core_tasks_alive() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("build tokio runtime");

        tokio::task::LocalSet::new().block_on(&runtime, async {
            let mock_server = MatrixMockServer::new().await;
            let client = mock_server.client_builder().build().await;
            mock_server
                .mock_room_state_encryption()
                .plain()
                .mount()
                .await;
            mock_server.sync_joined_room(&client, &room_id()).await;
            let (events, event_rx) = mpsc::channel();
            register_event_handlers(&client, events.clone());
            client.event_cache().subscribe().expect("subscribe the event cache");
            let read_state_task = super::start_room_read_state_update_task(
                client.clone(),
                events.clone(),
                client.room_info_notable_update_receiver(),
            );
            let room_list_task = super::start_room_list_update_task(client.clone(), events);

            sync_ignored_users(&mock_server, &client, &["@spam:localhost"]).await;
            mock_server
                .mock_sync()
                .ok_and_run(&client, |builder| {
                    builder.add_joined_room(
                        JoinedRoomBuilder::new(&room_id())
                            .add_timeline_event(raw_sync_timeline_event(text_message_json(
                                "$after:localhost",
                                "@bob:localhost",
                                "still here",
                            )))
                            .add_account_data(
                                Raw::<matrix_sdk::ruma::events::AnyRoomAccountDataEvent>::from_json_string(
                                    serde_json::json!({
                                        "type": "m.fully_read",
                                        "content": { "event_id": "$after:localhost" },
                                    })
                                    .to_string(),
                                )
                                .expect("fully read marker"),
                            ),
                    );
                })
                .await;

            let mut got = Vec::new();
            let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
            let seen = |got: &[Event]| {
                got.iter().any(|event| matches!(event, Event::RoomActivity(_)))
                    && got.iter().any(|event| {
                        matches!(event, Event::RoomReadState(state)
                            if state.fully_read_event_id.as_deref() == Some("$after:localhost"))
                    })
            };
            while !seen(&got) {
                assert!(
                    tokio::time::Instant::now() < deadline,
                    "activity and read state did not arrive: {got:?}"
                );
                got.extend(std::iter::from_fn(|| event_rx.try_recv().ok()));
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
            assert!(
                got.contains(&Event::IgnoredUsers(vec!["@spam:localhost".to_owned()])),
                "{got:?}"
            );
            assert!(
                !got.iter().any(|event| matches!(event, Event::Error(_))),
                "{got:?}"
            );
            read_state_task.abort();
            room_list_task.abort();
        });
    }

    #[test]
    fn fetch_user_profile_returns_display_name() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("build tokio runtime");

        runtime.block_on(async {
            let mock_server = MatrixMockServer::new().await;
            let client = mock_server.client_builder().build().await;
            for (user_id, body) in [
                (
                    "@alice:remote.org",
                    serde_json::json!({ "displayname": "Alice Liddell" }),
                ),
                ("@bare:remote.org", serde_json::json!({})),
                (
                    "@empty:remote.org",
                    serde_json::json!({ "displayname": "" }),
                ),
            ] {
                wiremock::Mock::given(wiremock::matchers::method("GET"))
                    .and(wiremock::matchers::path(format!(
                        "/_matrix/client/v3/profile/{user_id}"
                    )))
                    .respond_with(wiremock::ResponseTemplate::new(200).set_body_json(body))
                    .mount(mock_server.server())
                    .await;
            }
            wiremock::Mock::given(wiremock::matchers::method("GET"))
                .and(wiremock::matchers::path(
                    "/_matrix/client/v3/profile/@ghost:remote.org",
                ))
                .respond_with(wiremock::ResponseTemplate::new(404).set_body_json(
                    serde_json::json!({ "errcode": "M_NOT_FOUND", "error": "Profile not found" }),
                ))
                .mount(mock_server.server())
                .await;

            assert_eq!(
                fetch_user_profile(&client, "@alice:remote.org")
                    .await
                    .expect("alice"),
                UserProfile {
                    user_id: "@alice:remote.org".to_owned(),
                    display_name: Some("Alice Liddell".to_owned()),
                }
            );
            for user_id in ["@bare:remote.org", "@empty:remote.org"] {
                assert_eq!(
                    fetch_user_profile(&client, user_id).await.expect(user_id),
                    UserProfile {
                        user_id: user_id.to_owned(),
                        display_name: None,
                    }
                );
            }
            assert_eq!(
                fetch_user_profile(&client, "@ghost:remote.org")
                    .await
                    .expect_err("no such user")
                    .to_string(),
                "no such user @ghost:remote.org"
            );
            assert_eq!(
                fetch_user_profile(&client, "ghost")
                    .await
                    .expect_err("an invalid user ID")
                    .to_string(),
                r#"invalid user ID "ghost": leading sigil is incorrect or missing"#
            );
        });
    }

    async fn sync_push_rules(mock_server: &MatrixMockServer, client: &Client, ruleset: &Ruleset) {
        let rules = push_rules_account_data(ruleset);
        mock_server
            .mock_sync()
            .ok_and_run(client, |builder| {
                builder.add_custom_global_account_data(rules);
            })
            .await;
    }

    async fn mount_push_rule_writes(mock_server: &MatrixMockServer) {
        mock_server
            .mock_set_push_rules(RuleKind::Content, PushRuleIdSpec::Any)
            .ok()
            .mount()
            .await;
        mock_server
            .mock_enable_push_rule(RuleKind::Content, "[^/]*")
            .ok()
            .mount()
            .await;
        mock_server
            .mock_delete_push_rules(RuleKind::Content, PushRuleIdSpec::Any)
            .ok()
            .mount()
            .await;
    }

    /// Every push-rule request the server saw: method, path and JSON body.
    async fn push_rule_requests(
        mock_server: &MatrixMockServer,
    ) -> Vec<(String, String, serde_json::Value)> {
        mock_server
            .received_requests()
            .await
            .expect("request recording enabled")
            .iter()
            .filter(|request| request.url.path().contains("/pushrules/"))
            .map(|request| {
                (
                    request.method.to_string(),
                    request.url.path().to_owned(),
                    request
                        .body_json::<serde_json::Value>()
                        .expect("request JSON body"),
                )
            })
            .collect()
    }

    fn highlight_rule_body(pattern: &str) -> serde_json::Value {
        serde_json::json!({
            "pattern": pattern,
            "actions": ["notify", {"set_tweak": "sound", "value": "default"}, {"set_tweak": "highlight"}],
        })
    }

    #[test]
    fn add_highlight_word_writes_one_enabled_highlighting_rule_for_the_word() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("build tokio runtime");

        runtime.block_on(async {
            let mock_server = MatrixMockServer::new().await;
            let client = mock_server.client_builder().build().await;
            // Element's disabled notify-only rule for the word, and an older
            // rule for it under another ID.
            let ruleset = ruleset_with_content_rules(
                client.user_id().expect("logged-in test client"),
                vec![
                    content_rule("deploy", "deploy", notify_only(), false),
                    content_rule("deploy-old", "deploy", notify_only(), true),
                ],
            );
            sync_push_rules(&mock_server, &client, &ruleset).await;
            mount_push_rule_writes(&mock_server).await;

            add_highlight_word(&client, "deploy")
                .await
                .expect("add highlight word");
            add_highlight_word(&client, "release day")
                .await
                .expect("add a phrase");

            assert_eq!(
                push_rule_requests(&mock_server).await,
                vec![
                    (
                        "PUT".to_owned(),
                        "/_matrix/client/v3/pushrules/global/content/deploy".to_owned(),
                        highlight_rule_body("deploy"),
                    ),
                    (
                        "PUT".to_owned(),
                        "/_matrix/client/v3/pushrules/global/content/deploy/enabled".to_owned(),
                        serde_json::json!({ "enabled": true }),
                    ),
                    (
                        "DELETE".to_owned(),
                        "/_matrix/client/v3/pushrules/global/content/deploy-old".to_owned(),
                        serde_json::json!({}),
                    ),
                    (
                        "PUT".to_owned(),
                        "/_matrix/client/v3/pushrules/global/content/release%20day".to_owned(),
                        highlight_rule_body("release day"),
                    ),
                ]
            );
        });
    }

    #[test]
    fn add_highlight_word_rejects_an_invalid_word_without_a_request() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("build tokio runtime");

        runtime.block_on(async {
            let mock_server = MatrixMockServer::new().await;
            let client = mock_server.client_builder().build().await;
            mount_push_rule_writes(&mock_server).await;

            let error = add_highlight_word(&client, "a/b")
                .await
                .expect_err("a slash cannot be in a rule ID");
            assert_eq!(
                error.to_string(),
                r#"failed to add highlight word "a/b": a highlight word cannot contain / or \"#
            );
            assert_eq!(push_rule_requests(&mock_server).await, Vec::new());
        });
    }

    #[test]
    fn remove_highlight_word_deletes_every_rule_for_the_word() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("build tokio runtime");

        runtime.block_on(async {
            let mock_server = MatrixMockServer::new().await;
            let client = mock_server.client_builder().build().await;
            let ruleset = ruleset_with_content_rules(
                client.user_id().expect("logged-in test client"),
                vec![
                    content_rule("deploy", "deploy", highlight_word_actions(), true),
                    content_rule("deploy-old", "deploy", notify_only(), false),
                    content_rule("build", "build", notify_only(), true),
                ],
            );
            sync_push_rules(&mock_server, &client, &ruleset).await;
            mount_push_rule_writes(&mock_server).await;

            remove_highlight_word(&client, "deploy")
                .await
                .expect("remove highlight word");

            assert_eq!(
                push_rule_requests(&mock_server).await,
                vec![
                    (
                        "DELETE".to_owned(),
                        "/_matrix/client/v3/pushrules/global/content/deploy".to_owned(),
                        serde_json::json!({}),
                    ),
                    (
                        "DELETE".to_owned(),
                        "/_matrix/client/v3/pushrules/global/content/deploy-old".to_owned(),
                        serde_json::json!({}),
                    ),
                ]
            );
        });
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
            &profile("@alice:example.org"),
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
                highlight: false,
                relation: None,
                kind: RoomActivityKind::Message(RoomMessageActivity {
                    event_id: None,
                    transaction_id: Some("txn-1".to_owned()),
                    sender: profile("@alice:example.org"),
                    presentation: RoomMessagePresentationKind::Text,
                    body: "hello".to_owned(),
                    styles: Vec::new(),
                    user_links: Vec::new(),
                    send_state: RoomMessageSendState::Pending,
                }),
            }
        );
    }

    #[test]
    fn local_echoes_of_our_pills_are_user_links() {
        let pill = UserLinkSpan {
            start: 0,
            end: 5,
            user_id: "@alice:example.org".to_owned(),
        };
        let content = RoomMessageEventContent::new(room_message_type(
            RoomMessagePresentationKind::Text,
            "Alice: hi".to_owned(),
            &[],
            std::slice::from_ref(&pill),
        ));
        let serialized_event =
            SerializableEventContent::new(&AnyMessageLikeEventContent::RoomMessage(content))
                .expect("serialize pill event");

        let activity = map_local_message_activity(
            "!room:example.org".to_owned(),
            "txn-1".to_owned(),
            &profile("@me:example.org"),
            MilliSecondsSinceUnixEpoch::now(),
            &serialized_event,
        )
        .expect("map local text")
        .expect("text local echo");

        let RoomActivityKind::Message(message) = activity.kind else {
            panic!("expected a message activity");
        };
        assert_eq!(message.body, "Alice: hi");
        assert_eq!(message.user_links, vec![pill]);
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
            &profile("@alice:example.org"),
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
                highlight: false,
                relation: None,
                kind: RoomActivityKind::Message(RoomMessageActivity {
                    event_id: None,
                    transaction_id: Some("txn-1".to_owned()),
                    sender: profile("@alice:example.org"),
                    presentation: RoomMessagePresentationKind::Notice,
                    body: "heads up".to_owned(),
                    styles: Vec::new(),
                    user_links: Vec::new(),
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
            &profile("@alice:example.org"),
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
                highlight: false,
                relation: None,
                kind: RoomActivityKind::Message(RoomMessageActivity {
                    event_id: None,
                    transaction_id: Some("txn-1".to_owned()),
                    sender: profile("@alice:example.org"),
                    presentation: RoomMessagePresentationKind::Emote,
                    body: "waves".to_owned(),
                    styles: Vec::new(),
                    user_links: Vec::new(),
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
            message_presentation_and_text_from_serialized_event(&serialized_event)
                .expect("read local echo body"),
            Some(LocalEchoMessageDetails {
                presentation: RoomMessagePresentationKind::Text,
                text: StyledText::plain("hello from thread".to_owned()),
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
                sender_user_id: "@alice:example.org".to_owned(),
            },
        )
        .expect("attach reply relation");
        let serialized_event =
            SerializableEventContent::new(&AnyMessageLikeEventContent::RoomMessage(content))
                .expect("serialize reply text event");

        assert_eq!(
            message_presentation_and_text_from_serialized_event(&serialized_event)
                .expect("read local echo body"),
            Some(LocalEchoMessageDetails {
                presentation: RoomMessagePresentationKind::Text,
                text: StyledText::plain("hello in reply".to_owned()),
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
            &profile("@alice:example.org"),
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
                highlight: false,
                relation: Some(RoomActivityRelation::Thread {
                    thread_root_event_id: "$thread-root:example.org".to_owned(),
                    in_reply_to_event_id: Some("$thread-1:example.org".to_owned()),
                    is_falling_back: true,
                }),
                kind: RoomActivityKind::Message(RoomMessageActivity {
                    event_id: None,
                    transaction_id: Some("txn-1".to_owned()),
                    sender: profile("@alice:example.org"),
                    presentation: RoomMessagePresentationKind::Text,
                    body: "hello from thread".to_owned(),
                    styles: Vec::new(),
                    user_links: Vec::new(),
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
                sender_user_id: "@alice:example.org".to_owned(),
            },
        )
        .expect("attach reply relation");
        let serialized_event =
            SerializableEventContent::new(&AnyMessageLikeEventContent::RoomMessage(content))
                .expect("serialize reply text event");

        let activity = map_local_message_activity(
            "!room:example.org".to_owned(),
            "txn-1".to_owned(),
            &profile("@alice:example.org"),
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
                highlight: false,
                relation: Some(RoomActivityRelation::Reply {
                    in_reply_to_event_id: "$parent:example.org".to_owned(),
                }),
                kind: RoomActivityKind::Message(RoomMessageActivity {
                    event_id: None,
                    transaction_id: Some("txn-1".to_owned()),
                    sender: profile("@alice:example.org"),
                    presentation: RoomMessagePresentationKind::Text,
                    body: "hello in reply".to_owned(),
                    styles: Vec::new(),
                    user_links: Vec::new(),
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
            message_presentation_and_text_from_serialized_event(&serialized_event)
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
            &profile("@alice:example.org"),
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
            &profile("@alice:example.org"),
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
            &profile("@alice:example.org"),
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
            &profile("@alice:example.org"),
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
            &profile("@alice:example.org"),
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
            &profile("@alice:example.org"),
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
                &profile("@alice:example.org"),
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
                &profile("@alice:example.org"),
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
            &profile("@alice:example.org"),
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
                highlight: false,
                relation: None,
                kind: RoomActivityKind::Message(RoomMessageActivity {
                    event_id: None,
                    transaction_id: Some("txn-1".to_owned()),
                    sender: profile("@alice:example.org"),
                    presentation: RoomMessagePresentationKind::Text,
                    body: "hello".to_owned(),
                    styles: Vec::new(),
                    user_links: Vec::new(),
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
            &profile("@alice:example.org"),
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
                highlight: false,
                relation: Some(RoomActivityRelation::Thread {
                    thread_root_event_id: "$thread-root:example.org".to_owned(),
                    in_reply_to_event_id: Some("$thread-1:example.org".to_owned()),
                    is_falling_back: true,
                }),
                kind: RoomActivityKind::Message(RoomMessageActivity {
                    event_id: None,
                    transaction_id: Some("txn-1".to_owned()),
                    sender: profile("@alice:example.org"),
                    presentation: RoomMessagePresentationKind::Text,
                    body: "hello from thread".to_owned(),
                    styles: Vec::new(),
                    user_links: Vec::new(),
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
                sender_user_id: "@alice:example.org".to_owned(),
            },
        )
        .expect("attach reply relation");
        let serialized_event =
            SerializableEventContent::new(&AnyMessageLikeEventContent::RoomMessage(content))
                .expect("serialize reply text event");

        let events = outgoing_events_from_local_echo_event(
            "!room:example.org".to_owned(),
            "txn-1".to_owned(),
            &profile("@alice:example.org"),
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
                highlight: false,
                relation: Some(RoomActivityRelation::Reply {
                    in_reply_to_event_id: "$parent:example.org".to_owned(),
                }),
                kind: RoomActivityKind::Message(RoomMessageActivity {
                    event_id: None,
                    transaction_id: Some("txn-1".to_owned()),
                    sender: profile("@alice:example.org"),
                    presentation: RoomMessagePresentationKind::Text,
                    body: "hello in reply".to_owned(),
                    styles: Vec::new(),
                    user_links: Vec::new(),
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
            &profile("@alice:example.org"),
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
                    highlight: false,
                    relation: None,
                    kind: RoomActivityKind::Message(RoomMessageActivity {
                        event_id: None,
                        transaction_id: Some("txn-1".to_owned()),
                        sender: profile("@alice:example.org"),
                        presentation: RoomMessagePresentationKind::Text,
                        body: "hello".to_owned(),
                        styles: Vec::new(),
                        user_links: Vec::new(),
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
                    room_id: Some("!room:example.org".to_owned()),
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
            &profile("@alice:example.org"),
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
                room_id: Some("!room:example.org".to_owned()),
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

    /// A send the network or the server stopped stays queued: a message
    /// shows it is waiting, and nothing is an error yet. The kind stays for
    /// the `SentEvent` once the queue goes again.
    #[test]
    fn recoverable_send_errors_keep_the_request_queued() {
        for (kind, expected) in [
            (
                PendingOutgoingKind::Message,
                vec![Event::RoomActivityUpdate(RoomActivityUpdate {
                    room_id: "!room:example.org".to_owned(),
                    target: RoomActivityUpdateTarget::Transaction {
                        transaction_id: "txn-1".to_owned(),
                    },
                    kind: RoomActivityUpdateKind::MessageFailed {
                        message: "offline".to_owned(),
                        recoverable: true,
                    },
                })],
            ),
            (
                PendingOutgoingKind::MessageEdit {
                    target_event_id: "$target:example.org".to_owned(),
                },
                Vec::new(),
            ),
            (
                PendingOutgoingKind::Reaction {
                    target_event_id: "$target:example.org".to_owned(),
                    key: "+1".to_owned(),
                },
                Vec::new(),
            ),
            (
                PendingOutgoingKind::Redaction {
                    redacts: "$reaction:example.org".to_owned(),
                },
                Vec::new(),
            ),
        ] {
            let pending = pending_outgoing_registry();
            insert_pending_outgoing_kind(&pending, "txn-1".to_owned(), kind.clone());

            let events = map_send_queue_update(
                SendQueueUpdate {
                    room_id: room_id(),
                    update: RoomSendQueueUpdate::SendError {
                        transaction_id: transaction_id(),
                        error: Arc::new(matrix_sdk::Error::from(std::io::Error::other("offline"))),
                        is_recoverable: true,
                    },
                },
                &profile("@alice:example.org"),
                &pending,
            )
            .expect("map recoverable send error");

            assert_eq!(events, expected, "{kind:?}");
            assert_eq!(
                pending.lock().expect("lock pending registry").get("txn-1"),
                Some(&kind)
            );
        }
    }

    #[test]
    fn refused_send_maps_to_failed_activity_update_and_error_surface() {
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
                    is_recoverable: false,
                },
            },
            &profile("@alice:example.org"),
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
                        recoverable: false,
                    },
                }),
                Event::Error(CoreErrorEvent {
                    operation: Operation::SendMessage,
                    room_id: Some("!room:example.org".to_owned()),
                    message: "send failed".to_owned(),
                }),
            ]
        );
    }

    #[test]
    fn refused_edit_emits_typed_error_only() {
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
                    is_recoverable: false,
                },
            },
            &profile("@alice:example.org"),
            &pending,
        )
        .expect("map edit send error");

        assert_eq!(
            events,
            vec![Event::Error(CoreErrorEvent {
                operation: Operation::EditMessage,
                room_id: Some("!room:example.org".to_owned()),
                message: "edit send failed".to_owned(),
            })]
        );
    }

    #[test]
    fn refused_reaction_emits_typed_error_only() {
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
                    is_recoverable: false,
                },
            },
            &profile("@alice:example.org"),
            &pending,
        )
        .expect("map reaction send error");

        assert_eq!(
            events,
            vec![Event::Error(CoreErrorEvent {
                operation: Operation::SendReaction,
                room_id: Some("!room:example.org".to_owned()),
                message: "reaction send failed".to_owned(),
            })]
        );
    }

    #[test]
    fn refused_redaction_emits_typed_error_only() {
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
                    is_recoverable: false,
                },
            },
            &profile("@alice:example.org"),
            &pending,
        )
        .expect("map redaction send error");

        assert_eq!(
            events,
            vec![Event::Error(CoreErrorEvent {
                operation: Operation::RedactEvent,
                room_id: Some("!room:example.org".to_owned()),
                message: "redaction send failed".to_owned(),
            })]
        );
    }

    #[test]
    fn content_type_comes_from_the_file_name_extension() {
        for (name, expected) in [
            ("shot.png", "image/png"),
            ("a.jpg", "image/jpeg"),
            ("a.jpeg", "image/jpeg"),
            ("a.gif", "image/gif"),
            ("a.webp", "image/webp"),
            ("a.bmp", "image/bmp"),
            ("a.mp4", "video/mp4"),
            ("a.m4v", "video/mp4"),
            ("a.webm", "video/webm"),
            ("a.mov", "video/quicktime"),
            ("a.mkv", "video/x-matroska"),
            ("a.mp3", "audio/mpeg"),
            ("a.ogg", "audio/ogg"),
            ("a.oga", "audio/ogg"),
            ("a.opus", "audio/ogg"),
            ("a.m4a", "audio/mp4"),
            ("a.wav", "audio/wav"),
            ("a.flac", "audio/flac"),
            ("a.pdf", "application/pdf"),
            ("a.txt", "text/plain"),
            ("a.log", "text/plain"),
            ("a.zip", "application/zip"),
            ("PHOTO.PNG", "image/png"),
            ("README", "application/octet-stream"),
            ("archive.tar.gz", "application/octet-stream"),
            ("drawing.svg", "application/octet-stream"),
            ("notes.bin", "application/octet-stream"),
        ] {
            assert_eq!(
                content_type_for_file_name(name).as_ref(),
                expected,
                "content type of {name}"
            );
        }
    }

    #[test]
    fn send_file_infers_image_and_falls_back_to_octet_stream() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("build tokio runtime");

        runtime.block_on(async {
            let mock_server = MatrixMockServer::new().await;
            let client = mock_server.client_builder().build().await;
            mock_server
                .sync_room(&client, JoinedRoomBuilder::new(room_id().as_ref()))
                .await;
            mock_server
                .mock_room_state_encryption()
                .plain()
                .mount()
                .await;
            mock_server
                .mock_authenticated_media_config()
                .ok_default()
                .mount()
                .await;
            mock_server
                .mock_upload()
                .ok(&OwnedMxcUri::from("mxc://localhost/uploaded"))
                .mount()
                .await;
            mock_server
                .mock_room_send()
                .ok(OwnedEventId::try_from("$sent:localhost").expect("valid event id"))
                .mount()
                .await;
            let dir = tempfile::tempdir().expect("temporary directory");
            let send = |name: &str, content_type: Option<&str>| {
                let path = dir.path().join(name);
                std::fs::write(&path, b"0123456789").expect("write the file to send");
                send_file(
                    &client,
                    SendFileRequest {
                        room_id: room_id().to_string(),
                        path,
                        filename: None,
                        content_type: content_type.map(str::to_owned),
                    },
                )
            };

            send("shot.png", None).await.expect("send the picture");
            send("notes.bin", None).await.expect("send the file");
            send("forced.png", Some("text/plain"))
                .await
                .expect("send the typed file");

            let bodies = request_bodies_containing(&mock_server, "/send/m.room.message/").await;
            assert_eq!(bodies.len(), 3, "{bodies:?}");
            assert_eq!(bodies[0]["msgtype"], "m.image");
            assert_eq!(bodies[0]["body"], "shot.png");
            assert_eq!(
                bodies[0]["info"],
                serde_json::json!({ "mimetype": "image/png", "size": 10 })
            );
            assert_eq!(bodies[1]["msgtype"], "m.file");
            assert_eq!(
                bodies[1]["info"],
                serde_json::json!({ "mimetype": "application/octet-stream", "size": 10 })
            );
            assert_eq!(bodies[2]["msgtype"], "m.file");
            assert_eq!(bodies[2]["info"]["mimetype"], "text/plain");

            let upload_types = mock_server
                .received_requests()
                .await
                .expect("request recording enabled")
                .iter()
                .filter(|request| request.url.path() == "/_matrix/media/v3/upload")
                .map(|request| {
                    request
                        .headers
                        .get("content-type")
                        .expect("upload has a content type")
                        .to_str()
                        .expect("ASCII content type")
                        .to_owned()
                })
                .collect::<Vec<_>>();
            assert_eq!(
                upload_types,
                ["image/png", "application/octet-stream", "text/plain"]
            );
        });
    }

    #[test]
    fn styled_sends_and_edits_carry_an_html_formatted_body() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("build tokio runtime");

        runtime.block_on(async {
            let mock_server = MatrixMockServer::new().await;
            let client = mock_server.client_builder().build().await;
            mock_server
                .sync_room(
                    &client,
                    JoinedRoomBuilder::new(room_id().as_ref())
                        .add_state_bulk(raw_sync_state_events(&profile_test_state_events())),
                )
                .await;
            mock_server
                .mock_room_state_encryption()
                .plain()
                .mount()
                .await;
            mock_server
                .mock_room_send()
                .ok(OwnedEventId::try_from("$sent:localhost").expect("valid event id"))
                .mount()
                .await;
            let mut mine = text_message_json("$mine:localhost", "@example:localhost", "typo");
            mine["room_id"] = serde_json::json!(room_id().as_str());
            mock_server
                .mock_room_event()
                .room(room_id())
                .match_event_id()
                .ok(timeline_event(mine))
                .mount()
                .await;
            let red = TextStyle {
                foreground: Some(TextColor {
                    red: 0xff,
                    green: 0,
                    blue: 0,
                }),
                ..TextStyle::default()
            };

            send_message(
                &client,
                SendMessageRequest {
                    room_id: room_id().to_string(),
                    presentation: RoomMessagePresentationKind::Emote,
                    body: "waves at <you>".to_owned(),
                    styles: vec![style_span(0, 5, bold()), style_span(9, 14, red)],
                    thread: None,
                    reply: None,
                },
            )
            .await
            .expect("send the styled emote");
            send_message(
                &client,
                SendMessageRequest {
                    room_id: room_id().to_string(),
                    presentation: RoomMessagePresentationKind::Text,
                    body: "plain line".to_owned(),
                    styles: Vec::new(),
                    thread: None,
                    reply: None,
                },
            )
            .await
            .expect("send the plain line");
            edit_message(
                &client,
                EditMessageRequest {
                    room_id: room_id().to_string(),
                    target_event_id: "$mine:localhost".to_owned(),
                    presentation: RoomMessagePresentationKind::Text,
                    body: "fixed it".to_owned(),
                    styles: vec![style_span(6, 8, bold())],
                },
            )
            .await
            .expect("send the styled edit");

            let bodies = tokio::time::timeout(Duration::from_secs(10), async {
                loop {
                    let bodies =
                        request_bodies_containing(&mock_server, "/send/m.room.message/").await;
                    if bodies.len() == 3 {
                        return bodies;
                    }
                    tokio::time::sleep(Duration::from_millis(20)).await;
                }
            })
            .await
            .expect("all three messages are sent");
            assert_eq!(
                bodies[0],
                serde_json::json!({
                    "msgtype": "m.emote",
                    "body": "waves at <you>",
                    "format": "org.matrix.custom.html",
                    "formatted_body":
                        "<b>waves</b> at <span data-mx-color=\"#ff0000\">&lt;you&gt;</span>",
                    "m.mentions": {},
                })
            );
            assert_eq!(
                bodies[1],
                serde_json::json!({ "msgtype": "m.text", "body": "plain line", "m.mentions": {} })
            );
            assert_eq!(
                bodies[2]["m.new_content"],
                serde_json::json!({
                    "msgtype": "m.text",
                    "body": "fixed it",
                    "format": "org.matrix.custom.html",
                    "formatted_body": "fixed <b>it</b>",
                    "m.mentions": {},
                })
            );
            // The fallback for clients without edits is the SDK's, from the
            // new content.
            assert_eq!(bodies[2]["body"], "* fixed it");
            assert_eq!(bodies[2]["format"], "org.matrix.custom.html");
        });
    }

    #[test]
    fn an_edit_notifies_only_the_members_it_newly_names() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("build tokio runtime");

        runtime.block_on(async {
            let mock_server = MatrixMockServer::new().await;
            let client = mock_server.client_builder().build().await;
            mock_server
                .sync_room(
                    &client,
                    JoinedRoomBuilder::new(room_id().as_ref())
                        .add_state_bulk(raw_sync_state_events(&profile_test_state_events())),
                )
                .await;
            mock_server
                .mock_room_state_encryption()
                .plain()
                .mount()
                .await;
            mock_server
                .mock_room_send()
                .ok(OwnedEventId::try_from("$sent:localhost").expect("valid event id"))
                .mount()
                .await;
            let mut mine = text_message_json("$mine:localhost", "@example:localhost", "bob: typo");
            mine["room_id"] = serde_json::json!(room_id().as_str());
            mine["content"]["m.mentions"] = serde_json::json!({ "user_ids": ["@bob:localhost"] });
            mock_server
                .mock_room_event()
                .room(room_id())
                .match_event_id()
                .ok(timeline_event(mine))
                .mount()
                .await;

            edit_message(
                &client,
                EditMessageRequest {
                    room_id: room_id().to_string(),
                    target_event_id: "$mine:localhost".to_owned(),
                    presentation: RoomMessagePresentationKind::Text,
                    body: "bob, @sam1:localhost: fixed".to_owned(),
                    styles: Vec::new(),
                },
            )
            .await
            .expect("send the edit");

            let bodies = tokio::time::timeout(Duration::from_secs(10), async {
                loop {
                    let bodies =
                        request_bodies_containing(&mock_server, "/send/m.room.message/").await;
                    if !bodies.is_empty() {
                        return bodies;
                    }
                    tokio::time::sleep(Duration::from_millis(20)).await;
                }
            })
            .await
            .expect("the edit is sent");
            assert_eq!(
                bodies[0]["m.new_content"],
                serde_json::json!({
                    "msgtype": "m.text",
                    "body": "bob, @sam1:localhost: fixed",
                    "format": "org.matrix.custom.html",
                    "formatted_body":
                        "<a href=\"https://matrix.to/#/@bob:localhost\">bob</a>, \
                         <a href=\"https://matrix.to/#/@sam1:localhost\">@sam1:localhost</a>: fixed",
                    "m.mentions": { "user_ids": ["@bob:localhost", "@sam1:localhost"] },
                })
            );
            assert_eq!(
                bodies[0]["m.mentions"],
                serde_json::json!({ "user_ids": ["@sam1:localhost"] }),
                "the original already notified Bob"
            );
        });
    }

    #[test]
    fn every_send_carries_m_mentions() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("build tokio runtime");

        runtime.block_on(async {
            let mock_server = MatrixMockServer::new().await;
            let client = mock_server.client_builder().build().await;
            mock_server
                .sync_room(
                    &client,
                    JoinedRoomBuilder::new(room_id().as_ref())
                        .add_state_bulk(raw_sync_state_events(&profile_test_state_events())),
                )
                .await;
            mock_server
                .mock_room_state_encryption()
                .plain()
                .mount()
                .await;
            mock_server
                .mock_room_send()
                .ok(OwnedEventId::try_from("$sent:localhost").expect("valid event id"))
                .mount()
                .await;
            let send = |presentation, body: &str, reply: Option<SendMessageReplyTarget>| {
                send_message(
                    &client,
                    SendMessageRequest {
                        room_id: room_id().to_string(),
                        presentation,
                        body: body.to_owned(),
                        styles: Vec::new(),
                        thread: None,
                        reply,
                    },
                )
            };
            let text = RoomMessagePresentationKind::Text;

            send(
                text,
                "Sam: are you on it?",
                Some(SendMessageReplyTarget {
                    event_id: "$question:localhost".to_owned(),
                    sender_user_id: "@bob:localhost".to_owned(),
                }),
            )
            .await
            .expect("send the reply");
            send(text, "plain line", None).await.expect("send the line");
            send(text, "bob: deploy  done", None)
                .await
                .expect("send the named line");
            send(RoomMessagePresentationKind::Emote, "waves at Bob", None)
                .await
                .expect("send the emote");
            send(RoomMessagePresentationKind::Notice, "@room: Me here", None)
                .await
                .expect("send the notice");

            // The send queue sends in the background, in order.
            let bodies = tokio::time::timeout(Duration::from_secs(10), async {
                loop {
                    let bodies =
                        request_bodies_containing(&mock_server, "/send/m.room.message/").await;
                    if bodies.len() == 5 {
                        return bodies;
                    }
                    tokio::time::sleep(Duration::from_millis(20)).await;
                }
            })
            .await
            .expect("all five messages are sent");
            // Two members are named Sam: both are named, and neither pilled.
            assert_eq!(
                bodies[0],
                serde_json::json!({
                    "msgtype": "m.text",
                    "body": "Sam: are you on it?",
                    "m.mentions": {
                        "user_ids": ["@bob:localhost", "@sam1:localhost", "@sam2:localhost"]
                    },
                    "m.relates_to": { "m.in_reply_to": { "event_id": "$question:localhost" } },
                })
            );
            assert_eq!(
                bodies[1],
                serde_json::json!({ "msgtype": "m.text", "body": "plain line", "m.mentions": {} }),
                "a line naming nobody says so"
            );
            assert_eq!(
                bodies[2],
                serde_json::json!({
                    "msgtype": "m.text",
                    "body": "bob: deploy  done",
                    "format": "org.matrix.custom.html",
                    "formatted_body":
                        "<a href=\"https://matrix.to/#/@bob:localhost\">bob</a>: deploy &nbsp;done",
                    "m.mentions": { "user_ids": ["@bob:localhost"] },
                })
            );
            assert_eq!(
                bodies[3],
                serde_json::json!({
                    "msgtype": "m.emote",
                    "body": "waves at Bob",
                    "format": "org.matrix.custom.html",
                    "formatted_body":
                        "waves at <a href=\"https://matrix.to/#/@bob:localhost\">Bob</a>",
                    "m.mentions": { "user_ids": ["@bob:localhost"] },
                })
            );
            // The user's own name ("Me") is no mention.
            assert_eq!(
                bodies[4],
                serde_json::json!({
                    "msgtype": "m.notice",
                    "body": "@room: Me here",
                    "m.mentions": { "room": true },
                })
            );
        });
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
                    sender_user_id: "@alice:example.org".to_owned(),
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
                sender_user_id: "@alice:example.org".to_owned(),
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
                sender_user_id: "@alice:example.org".to_owned(),
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
                sender_user_id: "@alice:example.org".to_owned(),
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
                sender_user_id: "@alice:example.org".to_owned(),
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
                sender_user_id: "@alice:example.org".to_owned(),
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

    /// Verification against the SDK's mock crypto server: real
    /// `OlmMachine`s that know each other's keys, offline.
    mod verification {
        use matrix_sdk::ruma::{DeviceId, owned_device_id, owned_user_id};

        use super::super::{
            accept_verification, confirm_verification_sas, lookup_verification_request,
            request_verification, watch_verification_request,
        };
        use super::*;
        use crate::api::{RequestVerificationRequest, VerificationStateEvent};

        fn block_on<F: std::future::Future>(future: F) -> F::Output {
            tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("build tokio runtime")
                .block_on(future)
        }

        fn now_ms() -> u64 {
            MilliSecondsSinceUnixEpoch::now().get().into()
        }

        fn verification_requests(events: &[Event]) -> Vec<&VerificationRequest> {
            events
                .iter()
                .filter_map(|event| match event {
                    Event::VerificationRequest(request) => Some(request),
                    _ => None,
                })
                .collect()
        }

        fn room_request_json(
            event_id: &str,
            sender: &str,
            to: &str,
            origin_server_ts: u64,
        ) -> serde_json::Value {
            serde_json::json!({
                "type": "m.room.message",
                "event_id": event_id,
                "sender": sender,
                "origin_server_ts": origin_server_ts,
                "content": {
                    "msgtype": "m.key.verification.request",
                    "body": "Bob is requesting to verify your key",
                    "from_device": "B0B0B0B0B",
                    "methods": ["m.sas.v1"],
                    "to": to,
                }
            })
        }

        #[test]
        fn in_room_verification_request_emits_incoming_request() {
            block_on(async {
                let server = MatrixMockServer::new().await;
                server.mock_crypto_endpoints_preset().await;
                let (alice, _bob) = server.set_up_alice_and_bob_for_encryption().await;
                let (events, event_rx) = mpsc::channel();
                register_event_handlers(&alice, events);

                server
                    .sync_room(
                        &alice,
                        JoinedRoomBuilder::new(room_id().as_ref()).add_timeline_event(
                            raw_sync_timeline_event(room_request_json(
                                "$fresh:example.org",
                                "@bob:example.org",
                                "@alice:example.org",
                                now_ms(),
                            )),
                        ),
                    )
                    .await;

                let got = drain_events(&event_rx);
                assert_eq!(
                    verification_requests(&got),
                    vec![&VerificationRequest {
                        flow_id: "$fresh:example.org".to_owned(),
                        user_id: "@bob:example.org".to_owned(),
                        direction: VerificationDirection::Incoming {
                            from_device: "B0B0B0B0B".to_owned(),
                        },
                    }],
                    "{got:?}"
                );
                assert!(
                    !got.iter()
                        .any(|event| matches!(event, Event::VerificationState(_))),
                    "the request alone announces the flow: {got:?}"
                );
                // The SDK holds the flow by the request's event ID, which
                // Accept looks up.
                let flow = VerificationFlowId {
                    flow_id: "$fresh:example.org".to_owned(),
                    user_id: "@bob:example.org".to_owned(),
                };
                assert!(lookup_verification_request(&alice, &flow).await.is_ok());
            });
        }

        #[test]
        fn in_room_verification_requests_to_others_or_stale_emit_nothing() {
            block_on(async {
                let server = MatrixMockServer::new().await;
                server.mock_crypto_endpoints_preset().await;
                let (alice, _bob) = server.set_up_alice_and_bob_for_encryption().await;
                let (events, event_rx) = mpsc::channel();
                register_event_handlers(&alice, events);

                // The SDK drops a request older than ten minutes.
                let eleven_minutes_ago = now_ms() - 11 * 60 * 1000;
                server
                    .sync_room(
                        &alice,
                        JoinedRoomBuilder::new(room_id().as_ref())
                            .add_timeline_event(raw_sync_timeline_event(room_request_json(
                                "$for-carol:example.org",
                                "@bob:example.org",
                                "@carol:example.org",
                                now_ms(),
                            )))
                            .add_timeline_event(raw_sync_timeline_event(room_request_json(
                                "$stale:example.org",
                                "@bob:example.org",
                                "@alice:example.org",
                                eleven_minutes_ago,
                            ))),
                    )
                    .await;

                let got = drain_events(&event_rx);
                assert_eq!(
                    verification_requests(&got),
                    Vec::<&VerificationRequest>::new()
                );
            });
        }

        #[test]
        fn to_device_verification_request_emits_incoming_request_once_pending() {
            block_on(async {
                let server = MatrixMockServer::new().await;
                server.mock_crypto_endpoints_preset().await;
                let (alice, _bob) = server.set_up_alice_and_bob_for_encryption().await;
                let (events, event_rx) = mpsc::channel();
                register_event_handlers(&alice, events);

                let request = |transaction_id: &str, timestamp: u64| {
                    serde_json::from_value(serde_json::json!({
                        "type": "m.key.verification.request",
                        "sender": "@bob:example.org",
                        "content": {
                            "from_device": "B0B0B0B0B",
                            "transaction_id": transaction_id,
                            "methods": ["m.sas.v1"],
                            "timestamp": timestamp,
                        }
                    }))
                    .expect("to-device verification request")
                };
                server
                    .mock_sync()
                    .ok_and_run(&alice, |builder| {
                        builder
                            .add_to_device_event(request("txn-fresh", now_ms()))
                            .add_to_device_event(request("txn-stale", 1_700_000_000_000));
                    })
                    .await;

                let got = drain_events(&event_rx);
                assert_eq!(
                    verification_requests(&got),
                    vec![&VerificationRequest {
                        flow_id: "txn-fresh".to_owned(),
                        user_id: "@bob:example.org".to_owned(),
                        direction: VerificationDirection::Incoming {
                            from_device: "B0B0B0B0B".to_owned(),
                        },
                    }],
                    "{got:?}"
                );
            });
        }

        fn verify(user_id: &str, device_id: Option<&str>) -> RequestVerificationRequest {
            RequestVerificationRequest {
                user_id: user_id.to_owned(),
                device_id: device_id.map(str::to_owned),
            }
        }

        /// The bodies of the `m.key.verification.request` to-device
        /// messages the server received.
        async fn sent_to_device_requests(server: &MatrixMockServer) -> Vec<serde_json::Value> {
            server
                .server()
                .received_requests()
                .await
                .expect("the mock server records requests")
                .into_iter()
                .filter(|request| {
                    request
                        .url
                        .path()
                        .contains("/sendToDevice/m.key.verification.request/")
                })
                .map(|request| request.body_json().expect("to-device body"))
                .collect()
        }

        async fn bootstrap_cross_signing(client: &Client) {
            client
                .encryption()
                .bootstrap_cross_signing(None)
                .await
                .expect("bootstrap cross-signing");
        }

        #[test]
        fn request_verification_own_identity_sends_to_device_request() {
            block_on(async {
                let server = MatrixMockServer::new().await;
                server.mock_crypto_endpoints_preset().await;
                server
                    .mock_send_to_device()
                    .expect_any_access_token()
                    .ok()
                    .mount()
                    .await;
                let alice_id = owned_user_id!("@alice:example.org");
                let laptop = server
                    .client_builder_for_crypto_end_to_end(&alice_id, &owned_device_id!("L4PT0P"))
                    .build()
                    .await;
                bootstrap_cross_signing(&laptop).await;
                // A new session, which the laptop's identity has not signed,
                // asks the laptop.
                let phone = server
                    .set_up_new_device_for_encryption(
                        &laptop,
                        &owned_device_id!("PH0NE"),
                        Vec::new(),
                    )
                    .await;

                let (announcement, flow) =
                    request_verification(&phone, &verify("@alice:example.org", None))
                        .await
                        .expect("request own verification");

                assert_eq!(announcement.user_id, "@alice:example.org");
                assert_eq!(
                    announcement.direction,
                    VerificationDirection::Outgoing { to_device: None }
                );
                assert_eq!(announcement.flow_id, flow.flow_id());
                let sent = sent_to_device_requests(&server).await;
                assert_eq!(sent.len(), 1, "{sent:?}");
                let to_alice = sent[0]["messages"]["@alice:example.org"]
                    .as_object()
                    .expect("messages to alice");
                assert_eq!(to_alice.keys().collect::<Vec<_>>(), vec!["L4PT0P"]);
                let content = &to_alice["L4PT0P"];
                assert_eq!(content["from_device"], "PH0NE");
                assert_eq!(content["methods"], serde_json::json!(["m.sas.v1"]));
                assert_eq!(content["transaction_id"], announcement.flow_id);
            });
        }

        #[test]
        fn request_verification_device_targets_that_device() {
            block_on(async {
                let server = MatrixMockServer::new().await;
                server.mock_crypto_endpoints_preset().await;
                server
                    .mock_send_to_device()
                    .expect_any_access_token()
                    .ok()
                    .mount()
                    .await;
                let (alice, _bob) = server.set_up_alice_and_bob_for_encryption().await;

                let (announcement, _flow) =
                    request_verification(&alice, &verify("@bob:example.org", Some("B0B0B0B0B")))
                        .await
                        .expect("request device verification");

                assert_eq!(announcement.user_id, "@bob:example.org");
                assert_eq!(
                    announcement.direction,
                    VerificationDirection::Outgoing {
                        to_device: Some("B0B0B0B0B".to_owned())
                    }
                );
                let sent = sent_to_device_requests(&server).await;
                assert_eq!(sent.len(), 1, "{sent:?}");
                let messages = sent[0]["messages"].as_object().expect("messages");
                assert_eq!(
                    messages.keys().collect::<Vec<_>>(),
                    vec!["@bob:example.org"]
                );
                let to_bob = messages["@bob:example.org"].as_object().expect("to bob");
                assert_eq!(to_bob.keys().collect::<Vec<_>>(), vec!["B0B0B0B0B"]);
                assert_eq!(
                    to_bob["B0B0B0B0B"]["methods"],
                    serde_json::json!(["m.sas.v1"])
                );
            });
        }

        #[test]
        fn request_verification_unknown_device_and_no_identity_errors() {
            block_on(async {
                let server = MatrixMockServer::new().await;
                server.mock_crypto_endpoints_preset().await;
                let (alice, _bob) = server.set_up_alice_and_bob_for_encryption().await;
                let error = |request| {
                    let alice = alice.clone();
                    async move {
                        request_verification(&alice, &request)
                            .await
                            .expect_err("the request must fail")
                            .to_string()
                    }
                };

                assert_eq!(
                    error(verify("@bob:example.org", Some("NOPE"))).await,
                    "@bob:example.org has no device NOPE"
                );
                assert_eq!(
                    error(verify("@bob:example.org", None)).await,
                    "@bob:example.org has not set up cross-signing; verify one of their devices: /verify @bob:example.org <device>"
                );
                assert_eq!(
                    error(verify("@alice:example.org", None)).await,
                    "cross-signing is not set up for your account; use /e2ee setup"
                );
                assert_eq!(
                    error(verify("@alice:example.org", Some("4L1C3"))).await,
                    "4L1C3 is this session"
                );
                assert_eq!(
                    error(verify("not a user", None)).await,
                    error(verify("not a user", Some("DEVICE"))).await,
                );

                // Your identity signed only this session: nobody to ask.
                bootstrap_cross_signing(&alice).await;
                assert_eq!(
                    error(verify("@alice:example.org", None)).await,
                    "none of your other sessions is verified; use /e2ee recover with your recovery key"
                );
                assert!(sent_to_device_requests(&server).await.is_empty());
            });
        }

        #[test]
        fn request_verification_other_user_identity_goes_in_room() {
            block_on(async {
                let server = MatrixMockServer::new().await;
                server.mock_crypto_endpoints_preset().await;
                let (alice, bob) = server.set_up_alice_and_bob_for_encryption().await;
                bootstrap_cross_signing(&bob).await;

                let dm = OwnedRoomId::try_from("!dm:example.org").expect("room id");
                let bob_member = serde_json::json!({
                    "type": "m.room.member",
                    "state_key": "@bob:example.org",
                    "sender": "@bob:example.org",
                    "event_id": "$bob-join:example.org",
                    "origin_server_ts": 1u64,
                    "content": { "membership": "join" }
                });
                server
                    .mock_get_members()
                    .expect_any_access_token()
                    .ok(raw_room_member_events(&serde_json::json!([bob_member])))
                    .mount()
                    .await;
                server
                    .mock_room_state_encryption()
                    .expect_any_access_token()
                    .plain()
                    .mount()
                    .await;
                server
                    .mock_room_send()
                    .expect_any_access_token()
                    .ok(OwnedEventId::try_from("$request:example.org").expect("event id"))
                    .mock_once()
                    .mount()
                    .await;
                server
                    .mock_sync()
                    .ok_and_run(&alice, |builder| {
                        builder
                            .add_joined_room(JoinedRoomBuilder::new(&dm).add_state_bulk(
                                raw_sync_state_events(&serde_json::json!([bob_member])),
                            ))
                            .add_custom_global_account_data(serde_json::json!({
                                "type": "m.direct",
                                "content": { "@bob:example.org": ["!dm:example.org"] }
                            }));
                    })
                    .await;

                let (announcement, _flow) =
                    request_verification(&alice, &verify("@bob:example.org", None))
                        .await
                        .expect("request user verification");

                // In-room, the flow ID is the request's event ID.
                assert_eq!(
                    announcement,
                    VerificationRequest {
                        flow_id: "$request:example.org".to_owned(),
                        user_id: "@bob:example.org".to_owned(),
                        direction: VerificationDirection::Outgoing { to_device: None },
                    }
                );
                let sends = server
                    .server()
                    .received_requests()
                    .await
                    .expect("recorded requests")
                    .into_iter()
                    .filter(|request| {
                        request
                            .url
                            .path()
                            .contains("/rooms/!dm:example.org/send/m.room.message/")
                    })
                    .map(|request| request.body_json::<serde_json::Value>().expect("body"))
                    .collect::<Vec<_>>();
                assert_eq!(sends.len(), 1, "{sends:?}");
                assert_eq!(sends[0]["msgtype"], "m.key.verification.request");
                assert_eq!(sends[0]["to"], "@bob:example.org");
                assert_eq!(sends[0]["from_device"], "4L1C3");
                assert_eq!(sends[0]["methods"], serde_json::json!(["m.sas.v1"]));
            });
        }

        /// The to-device messages each `(user, device)` has waiting, as a
        /// homeserver queues them for the recipient's next sync.
        type ToDeviceInbox = Arc<Mutex<BTreeMap<(String, String), Vec<serde_json::Value>>>>;

        /// Queues every to-device message `sender` sends.
        async fn relay_to_device_from(
            server: &MatrixMockServer,
            sender: &Client,
            inbox: ToDeviceInbox,
        ) {
            let sender_id = sender.user_id().expect("signed in").to_string();
            let token = sender.access_token().expect("an access token");
            wiremock::Mock::given(wiremock::matchers::method("PUT"))
                .and(wiremock::matchers::path_regex(
                    r"^/_matrix/client/v3/sendToDevice/[^/]+/[^/]+$",
                ))
                .and(wiremock::matchers::header(
                    "authorization",
                    format!("Bearer {token}").as_str(),
                ))
                .respond_with(move |request: &wiremock::Request| {
                    let event_type = request
                        .url
                        .path_segments()
                        .and_then(|mut segments| segments.nth_back(1))
                        .expect("the event type in the path")
                        .to_owned();
                    let body: serde_json::Value = request.body_json().expect("to-device body");
                    let mut inbox = inbox.lock().expect("inbox lock");
                    for (user_id, devices) in body["messages"].as_object().expect("messages") {
                        for (device_id, content) in devices.as_object().expect("devices") {
                            inbox
                                .entry((user_id.clone(), device_id.clone()))
                                .or_default()
                                .push(serde_json::json!({
                                    "type": event_type,
                                    "sender": sender_id,
                                    "content": content,
                                }));
                        }
                    }
                    wiremock::ResponseTemplate::new(200).set_body_json(serde_json::json!({}))
                })
                .mount(server.server())
                .await;
        }

        /// Syncs the recipient its waiting messages; whether there were any.
        async fn deliver_to_device(
            server: &MatrixMockServer,
            recipient: &Client,
            inbox: &ToDeviceInbox,
        ) -> bool {
            let key = (
                recipient.user_id().expect("signed in").to_string(),
                recipient.device_id().expect("a device").to_string(),
            );
            let messages = inbox.lock().expect("inbox lock").remove(&key);
            let Some(messages) = messages else {
                return false;
            };
            server
                .mock_sync()
                .ok_and_run(recipient, |builder| {
                    for message in messages {
                        builder.add_to_device_event(message);
                    }
                })
                .await;
            true
        }

        fn sas_ready_decimals(events: &[Event]) -> Option<VerificationDecimals> {
            events.iter().find_map(|event| match event {
                Event::VerificationState(VerificationStateEvent {
                    state: VerificationState::SasReady { decimals, .. },
                    ..
                }) => Some(decimals.clone()),
                _ => None,
            })
        }

        /// Alice asks Bob's device; Bob accepts as mirx does; the watcher
        /// Alice's request starts sees Bob's ready, starts SAS, and both
        /// sides reach the same short authentication string.
        #[test]
        fn outgoing_request_round_trip_verifies_the_device() {
            block_on(tokio::task::LocalSet::new().run_until(async {
                let server = MatrixMockServer::new().await;
                server.mock_crypto_endpoints_preset().await;
                let (alice, bob) = server.set_up_alice_and_bob_for_encryption().await;
                // Bob knows Alice's device, as a sync would tell him.
                bob.encryption()
                    .request_user_identity(alice.user_id().expect("alice"))
                    .await
                    .expect("query alice's keys");
                let inbox = ToDeviceInbox::default();
                relay_to_device_from(&server, &alice, inbox.clone()).await;
                relay_to_device_from(&server, &bob, inbox.clone()).await;
                let (alice_events, alice_rx) = mpsc::channel();
                let (bob_events, bob_rx) = mpsc::channel();
                register_event_handlers(&bob, bob_events.clone());

                let (announcement, request) =
                    request_verification(&alice, &verify("@bob:example.org", Some("B0B0B0B0B")))
                        .await
                        .expect("request bob's device");
                let alice_flow = VerificationFlowId {
                    flow_id: announcement.flow_id.clone(),
                    user_id: announcement.user_id.clone(),
                };
                tokio::task::spawn_local(watch_verification_request(
                    request,
                    alice_flow,
                    alice_events,
                ));

                assert!(deliver_to_device(&server, &bob, &inbox).await);
                let bob_flow = VerificationFlowId {
                    flow_id: announcement.flow_id.clone(),
                    user_id: "@alice:example.org".to_owned(),
                };
                assert_eq!(
                    verification_requests(&drain_events(&bob_rx)),
                    vec![&VerificationRequest {
                        flow_id: announcement.flow_id.clone(),
                        user_id: "@alice:example.org".to_owned(),
                        direction: VerificationDirection::Incoming {
                            from_device: "4L1C3".to_owned(),
                        },
                    }]
                );
                accept_verification(&bob, &bob_flow)
                    .await
                    .expect("bob accepts");
                let bob_request = lookup_verification_request(&bob, &bob_flow)
                    .await
                    .expect("bob's flow");
                tokio::task::spawn_local(watch_verification_request(
                    bob_request,
                    bob_flow,
                    bob_events,
                ));

                let (mut alice_got, mut bob_got) = (Vec::new(), Vec::new());
                relay_until_quiet(&server, [&alice, &bob], &inbox).await;
                alice_got.extend(alice_rx.try_iter());
                bob_got.extend(bob_rx.try_iter());

                let alice_decimals = sas_ready_decimals(&alice_got)
                    .unwrap_or_else(|| panic!("alice reaches SAS: {alice_got:?}"));
                let bob_decimals = sas_ready_decimals(&bob_got)
                    .unwrap_or_else(|| panic!("bob reaches SAS: {bob_got:?}"));
                assert_eq!(alice_decimals, bob_decimals);
                // Alice's watcher reported Bob's ready before SAS, and never
                // re-announced the sent request.
                assert!(
                    !alice_got.iter().any(|event| matches!(
                        event,
                        Event::VerificationState(VerificationStateEvent {
                            state: VerificationState::Sent | VerificationState::Requested,
                            ..
                        })
                    )),
                    "{alice_got:?}"
                );

                // Both compare and confirm: each side now trusts the other's
                // device, as the SDK says.
                let alice_flow = VerificationFlowId {
                    flow_id: announcement.flow_id.clone(),
                    user_id: "@bob:example.org".to_owned(),
                };
                let bob_flow = VerificationFlowId {
                    flow_id: announcement.flow_id.clone(),
                    user_id: "@alice:example.org".to_owned(),
                };
                confirm_verification_sas(&alice, &alice_flow)
                    .await
                    .expect("alice confirms");
                confirm_verification_sas(&bob, &bob_flow)
                    .await
                    .expect("bob confirms");
                relay_until_quiet(&server, [&alice, &bob], &inbox).await;
                let done = |events: &[Event]| {
                    events.iter().any(|event| {
                        matches!(
                            event,
                            Event::VerificationState(VerificationStateEvent {
                                state: VerificationState::Done,
                                ..
                            })
                        )
                    })
                };
                let alice_got = alice_rx.try_iter().collect::<Vec<_>>();
                let bob_got = bob_rx.try_iter().collect::<Vec<_>>();
                assert!(done(&alice_got), "{alice_got:?}");
                assert!(done(&bob_got), "{bob_got:?}");
                let bobs_device = alice
                    .encryption()
                    .get_device(bob.user_id().expect("bob"), <&DeviceId>::from("B0B0B0B0B"))
                    .await
                    .expect("store")
                    .expect("bob's device");
                assert!(bobs_device.is_verified());
            }));
        }

        /// Delivers waiting to-device messages, and lets the watchers
        /// answer, until no client has any.
        async fn relay_until_quiet<const N: usize>(
            server: &MatrixMockServer,
            clients: [&Client; N],
            inbox: &ToDeviceInbox,
        ) {
            for _ in 0..10 {
                let mut delivered = false;
                for client in clients {
                    delivered |= deliver_to_device(server, client, inbox).await;
                }
                tokio::time::sleep(Duration::from_millis(50)).await;
                if !delivered && inbox.lock().expect("inbox lock").is_empty() {
                    return;
                }
            }
            panic!("the clients never stopped talking");
        }
    }
}
