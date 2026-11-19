use std::path::PathBuf;
use std::{fmt, fmt::Formatter};

pub const ROOM_HISTORY_BACKFILL_LIMIT_MAX: u32 = 50;
pub const ROOM_HISTORY_BACKFILL_DEFAULT_LIMIT: u32 = 50;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoreConfig {
    pub profile_dir: PathBuf,
    pub store_passphrase: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    Login(LoginRequest),
    RestoreSession(RestoreSessionRequest),
    StartSync,
    StopSync,
    JoinRoom(JoinRoomRequest),
    AcceptRoomInvite(AcceptRoomInviteRequest),
    CreateDirectRoom(CreateDirectRoomRequest),
    LeaveRoom(LeaveRoomRequest),
    KickRoomMember(KickRoomMemberRequest),
    BanRoomMember(BanRoomMemberRequest),
    UnbanRoomMember(UnbanRoomMemberRequest),
    InviteRoomMember(InviteRoomMemberRequest),
    SetRoomMemberPowerLevel(SetRoomMemberPowerLevelRequest),
    SetDisplayName(SetDisplayNameRequest),
    RejectRoomInvite(RejectRoomInviteRequest),
    SetRoomTopic(SetRoomTopicRequest),
    SendMessage(SendMessageRequest),
    SendFile(SendFileRequest),
    EditMessage(EditMessageRequest),
    SendReaction(SendReactionRequest),
    RedactEvent(RedactEventRequest),
    MarkRoomRead(MarkRoomReadRequest),
    BackfillRoomHistory(BackfillRoomHistoryRequest),
    FetchRoomMedia(FetchRoomMediaRequest),
    RefreshE2eeStatus,
    SetupE2eeRecovery(SetupE2eeRecoveryRequest),
    RecoverE2ee(RecoverE2eeRequest),
    AcceptVerification(VerificationFlowId),
    CancelVerification(VerificationFlowId),
    ConfirmVerificationSas(VerificationFlowId),
    RejectVerificationSas(VerificationFlowId),
    Logout,
    Shutdown,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoginRequest {
    /// A server name (`matrix.org`, `example.org:8448`) or a homeserver URL;
    /// the client URL is discovered from it. See `login_server`.
    pub server: String,
    pub username: String,
    pub password: String,
    pub device_display_name: String,
    /// Start continuous sync once the session is ready, as `StartSync` would.
    pub start_sync: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RestoreSessionRequest {
    /// The exact homeserver URL `Event::LoggedIn` reported; never
    /// rediscovered.
    pub homeserver_url: String,
    pub session: StoredSession,
    /// Start continuous sync once the session is ready, as `StartSync` would.
    pub start_sync: bool,
}

/// The server to log in to: the one the user typed, else the server part of
/// a full user ID such as `@alice:matrix.org`, as Element and other clients
/// accept.
pub fn login_server(server: &str, username: &str) -> Result<String, String> {
    let server = server.trim();
    if !server.is_empty() {
        return Ok(server.to_owned());
    }
    matrix_sdk::ruma::UserId::parse(username.trim())
        .map(|user_id| user_id.server_name().to_string())
        .map_err(|_| "enter a server, or a full user ID like @alice:matrix.org".to_owned())
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StoredSession {
    pub user_id: String,
    pub device_id: String,
    pub access_token: String,
    pub refresh_token: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SendMessageThreadTarget {
    pub thread_root_event_id: String,
    pub latest_event_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SendMessageReplyTarget {
    pub event_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SendMessageRequest {
    pub room_id: String,
    pub presentation: RoomMessagePresentationKind,
    pub body: String,
    pub thread: Option<SendMessageThreadTarget>,
    pub reply: Option<SendMessageReplyTarget>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SendFileRequest {
    pub room_id: String,
    pub path: PathBuf,
    pub filename: Option<String>,
    pub content_type: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EditMessageRequest {
    pub room_id: String,
    pub target_event_id: String,
    pub presentation: RoomMessagePresentationKind,
    pub body: String,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SendReactionRequest {
    pub room_id: String,
    pub target_event_id: String,
    pub key: String,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RedactEventRequest {
    pub room_id: String,
    pub event_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct JoinRoomRequest {
    pub room_or_alias: String,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CreateDirectRoomRequest {
    pub user_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LeaveRoomRequest {
    pub room_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KickRoomMemberRequest {
    pub room_id: String,
    pub user_id: String,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BanRoomMemberRequest {
    pub room_id: String,
    pub user_id: String,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UnbanRoomMemberRequest {
    pub room_id: String,
    pub user_id: String,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InviteRoomMemberRequest {
    pub room_id: String,
    pub user_id: String,
}

/// The IRC-style role a member is given by setting their room power level.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MemberPowerRole {
    /// Power level 50.
    Moderator,
    /// The lowest level above `users_default` that may send messages.
    Voiced,
    /// The room's `users_default`.
    Default,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SetRoomMemberPowerLevelRequest {
    pub room_id: String,
    pub user_id: String,
    pub role: MemberPowerRole,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SetDisplayNameRequest {
    pub display_name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AcceptRoomInviteRequest {
    pub room_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RejectRoomInviteRequest {
    pub room_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SetRoomTopicRequest {
    pub room_id: String,
    pub topic: String,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MarkRoomReadRequest {
    pub room_id: String,
    pub event_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BackfillRoomHistoryRequest {
    pub room_id: String,
    pub from: Option<String>,
    pub limit: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FetchRoomMediaRequest {
    pub room_id: String,
    pub activity_id: String,
    pub event_id: String,
    pub source: RoomMediaSource,
    pub format: RoomMediaFormat,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum RoomMediaFormat {
    Thumbnail { width: u32, height: u32 },
    File,
}

#[derive(serde::Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
enum RoomMediaFormatDto {
    Thumbnail { width: u32, height: u32 },
    File {},
}

impl<'de> serde::Deserialize<'de> for RoomMediaFormat {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        match RoomMediaFormatDto::deserialize(deserializer)? {
            RoomMediaFormatDto::Thumbnail { width, height } => {
                Ok(Self::Thumbnail { width, height })
            }
            RoomMediaFormatDto::File {} => Ok(Self::File),
        }
    }
}

#[derive(Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RecoverE2eeRequest {
    pub recovery_secret: String,
}

impl fmt::Debug for RecoverE2eeRequest {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.debug_struct("RecoverE2eeRequest")
            .field("recovery_secret", &"<redacted>")
            .finish()
    }
}

#[derive(Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SetupE2eeRecoveryRequest {
    pub passphrase: Option<String>,
    pub wait_for_backups_to_upload: bool,
}

impl fmt::Debug for SetupE2eeRecoveryRequest {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.debug_struct("SetupE2eeRecoveryRequest")
            .field(
                "passphrase",
                &self.passphrase.as_ref().map(|_| "<redacted>"),
            )
            .field(
                "wait_for_backups_to_upload",
                &self.wait_for_backups_to_upload,
            )
            .finish()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SpaceChildSummary {
    pub room_id: String,
    pub suggested: bool,
    pub order: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RoomTagSummary {
    pub tag: String,
    pub order: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RoomMembership {
    Join,
    Invite,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(deny_unknown_fields)]
pub struct RoomSummary {
    pub room_id: String,
    pub display_name: String,
    pub topic: Option<String>,
    pub membership: RoomMembership,
    pub encrypted: bool,
    pub is_space: bool,
    pub is_direct: bool,
    pub direct_user_ids: Vec<String>,
    pub tags: Vec<RoomTagSummary>,
    pub space_children: Vec<SpaceChildSummary>,
}

impl<'de> serde::Deserialize<'de> for RoomSummary {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(serde::Deserialize)]
        #[serde(deny_unknown_fields)]
        struct WireRoomSummary {
            room_id: String,
            display_name: String,
            topic: serde_json::Value,
            membership: RoomMembership,
            encrypted: bool,
            is_space: bool,
            is_direct: bool,
            direct_user_ids: Vec<String>,
            tags: Vec<RoomTagSummary>,
            space_children: Vec<SpaceChildSummary>,
        }

        let room = WireRoomSummary::deserialize(deserializer)?;
        let topic = match room.topic {
            serde_json::Value::Null => None,
            serde_json::Value::String(topic) => Some(topic),
            topic => {
                return Err(serde::de::Error::custom(format!(
                    "invalid topic value: expected string or null, got {topic}"
                )));
            }
        };

        Ok(Self {
            room_id: room.room_id,
            display_name: room.display_name,
            topic,
            membership: room.membership,
            encrypted: room.encrypted,
            is_space: room.is_space,
            is_direct: room.is_direct,
            direct_user_ids: room.direct_user_ids,
            tags: room.tags,
            space_children: room.space_children,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RoomActivity {
    pub room_id: String,
    pub activity_id: String,
    pub timestamp_ms: u64,
    /// The user's push rules mark the event as a highlight
    /// (`set_tweak: highlight`), e.g. it mentions them. Never set for the
    /// user's own events or local echoes. Fixed when the event arrives: an
    /// edit does not change it.
    pub highlight: bool,
    pub relation: Option<RoomActivityRelation>,
    pub kind: RoomActivityKind,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum RoomActivityRelation {
    Reply {
        in_reply_to_event_id: String,
    },
    Thread {
        thread_root_event_id: String,
        in_reply_to_event_id: Option<String>,
        is_falling_back: bool,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum RoomActivityKind {
    Message(RoomMessageActivity),
    Image(Box<RoomImageActivity>),
    File(Box<RoomFileActivity>),
    Audio(Box<RoomAudioActivity>),
    Video(Box<RoomVideoActivity>),
    UnsupportedMessage(UnsupportedRoomMessageActivity),
    UnableToDecrypt(RoomUnableToDecryptActivity),
    StateNotice(RoomStateNoticeActivity),
    Redacted(RoomRedactedActivity),
}

/// A room member's identity together with the room-scoped profile used to
/// display them.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RoomUserProfile {
    pub user_id: String,
    pub display_name: Option<String>,
    /// Whether another room member shares `display_name`, so it alone does
    /// not identify this user.
    pub display_name_ambiguous: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RoomUnableToDecryptActivity {
    pub event_id: String,
    pub sender: RoomUserProfile,
    pub session_id: Option<String>,
    pub reason: RoomUnableToDecryptReason,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RoomUnableToDecryptReason {
    Unknown,
    MalformedEncryptedEvent,
    MissingMegolmSession,
    UnknownMegolmMessageIndex,
    MegolmDecryptionFailure,
    PayloadDeserializationFailure,
    MismatchedIdentityKeys,
    SenderIdentityNotTrusted,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RoomMessagePresentationKind {
    Text,
    Notice,
    Emote,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RoomMessageActivity {
    pub event_id: Option<String>,
    pub transaction_id: Option<String>,
    pub sender: RoomUserProfile,
    pub presentation: RoomMessagePresentationKind,
    pub body: String,
    pub send_state: RoomMessageSendState,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RoomImageActivity {
    pub event_id: String,
    pub sender: RoomUserProfile,
    pub body: String,
    pub filename: Option<String>,
    pub source: RoomMediaSource,
    pub info: Option<RoomImageInfo>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RoomFileActivity {
    pub event_id: String,
    pub sender: RoomUserProfile,
    pub body: String,
    pub filename: Option<String>,
    pub source: RoomMediaSource,
    pub info: Option<RoomFileInfo>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RoomAudioActivity {
    pub event_id: String,
    pub sender: RoomUserProfile,
    pub body: String,
    pub filename: Option<String>,
    pub source: RoomMediaSource,
    pub info: Option<RoomAudioInfo>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RoomVideoActivity {
    pub event_id: String,
    pub sender: RoomUserProfile,
    pub body: String,
    pub filename: Option<String>,
    pub source: RoomMediaSource,
    pub info: Option<RoomVideoInfo>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RoomEncryptedFileKey {
    pub kty: String,
    pub key_ops: Vec<String>,
    pub alg: String,
    pub k: String,
    pub ext: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RoomEncryptedFileHashes {
    pub sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum RoomMediaSource {
    Plain {
        url: String,
    },
    Encrypted {
        url: String,
        key: RoomEncryptedFileKey,
        iv: String,
        hashes: RoomEncryptedFileHashes,
        v: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RoomImageInfo {
    pub mimetype: Option<String>,
    pub size: Option<u64>,
    pub width: Option<u64>,
    pub height: Option<u64>,
    pub is_animated: Option<bool>,
    pub thumbnail_source: Option<RoomMediaSource>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RoomFileInfo {
    pub mimetype: Option<String>,
    pub size: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RoomAudioInfo {
    pub mimetype: Option<String>,
    pub size: Option<u64>,
    pub duration_ms: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RoomVideoInfo {
    pub mimetype: Option<String>,
    pub size: Option<u64>,
    pub duration_ms: Option<u64>,
    pub width: Option<u64>,
    pub height: Option<u64>,
    pub thumbnail_source: Option<RoomMediaSource>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UnsupportedRoomMessageActivity {
    pub event_id: String,
    pub sender: RoomUserProfile,
    pub msgtype: String,
    pub body: String,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "state", rename_all = "snake_case", deny_unknown_fields)]
pub enum RoomMessageSendState {
    Pending,
    Sent,
    Failed { message: String, recoverable: bool },
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RoomStateNoticeActivity {
    pub event_id: String,
    pub sender: RoomUserProfile,
    pub content: RoomStateNoticeContent,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RoomRedactedActivity {
    pub event_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum RoomStateNoticeContent {
    Topic {
        topic: String,
    },
    Member {
        change: RoomMemberChange,
        target: RoomUserProfile,
        reason: Option<String>,
    },
    /// An `m.room.power_levels` change against the room's previous levels.
    /// At least one field reports a change.
    PowerLevels {
        /// Users whose effective level changed, ordered by user ID.
        changes: Vec<RoomPowerLevelUserChange>,
        users_default: Option<RoomPowerLevelValueChange>,
        /// Whether any level required for an action changed: ban, kick,
        /// redact, invite, sending events or state, or notifications.
        other_permissions_changed: bool,
    },
}

/// A user's effective power level before and after a power-level change.
/// A user without an explicit level has the room's default for users.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RoomPowerLevelUserChange {
    pub target: RoomUserProfile,
    pub old: i64,
    pub new: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RoomPowerLevelValueChange {
    pub old: i64,
    pub new: i64,
}

/// A room membership transition, as classified by the Matrix spec's
/// `m.room.member` transition table.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum RoomMemberChange {
    Joined,
    Left,
    Kicked,
    Banned,
    KickedAndBanned,
    Unbanned,
    Invited,
    InvitationAccepted,
    InvitationRejected,
    InvitationRevoked,
    Knocked,
    KnockAccepted,
    KnockRetracted,
    KnockDenied,
    DisplayNameChanged {
        old: Option<String>,
        new: Option<String>,
    },
    AvatarChanged,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RoomActivityUpdate {
    pub room_id: String,
    pub target: RoomActivityUpdateTarget,
    pub kind: RoomActivityUpdateKind,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum RoomHistoryBackfillEvent {
    Activity(Box<RoomActivity>),
    ActivityUpdate(RoomActivityUpdate),
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RoomHistoryBackfill {
    pub room_id: String,
    pub start: String,
    pub end: Option<String>,
    pub events: Vec<RoomHistoryBackfillEvent>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum RoomActivityUpdateTarget {
    Transaction { transaction_id: String },
    Event { event_id: String },
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RoomMedia {
    pub room_id: String,
    pub activity_id: String,
    pub event_id: String,
    pub kind: RoomMediaKind,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RoomTyping {
    pub room_id: String,
    pub user_ids: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RoomMemberSummary {
    pub user_id: String,
    pub display_name: Option<String>,
    pub display_name_ambiguous: bool,
    /// Whether this member is the authenticated user.
    pub is_own_user: bool,
    pub rank: RoomMemberRank,
    pub can_kick: bool,
    pub can_ban: bool,
}

/// A member's standing in a room, derived from their power level. Variants
/// are declared highest first, so the derived order sorts higher ranks
/// before lower ones.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, serde::Serialize, serde::Deserialize,
)]
#[serde(rename_all = "snake_case")]
pub enum RoomMemberRank {
    /// A room creator, whose power level is infinite (room version 12+).
    Creator,
    /// Power level 100 or more.
    Administrator,
    /// Power level 50 or more.
    Moderator,
    /// A power level above the room's default for users, but below 50.
    Voiced,
    /// The room's default power level for users, or less.
    Member,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RoomMembers {
    pub room_id: String,
    pub members: Vec<RoomMemberSummary>,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RoomReadState {
    pub room_id: String,
    pub latest_active_event_id: Option<String>,
    pub fully_read_event_id: Option<String>,
    pub marked_unread: bool,
    pub num_unread: u64,
    pub num_notifications: u64,
    pub num_mentions: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct E2eeStatus {
    pub recovery: E2eeRecoveryState,
    pub backup: E2eeBackupStatus,
    pub cross_signing: Option<E2eeCrossSigningStatus>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum E2eeRecoveryState {
    Unknown,
    Enabled,
    Disabled,
    Incomplete,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct E2eeBackupStatus {
    pub state: E2eeBackupState,
    pub enabled_locally: bool,
    pub exists_on_server: Option<bool>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum E2eeBackupState {
    Unknown,
    Creating,
    Enabling,
    Resuming,
    Enabled,
    Downloading,
    Disabling,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct E2eeCrossSigningStatus {
    pub has_master: bool,
    pub has_self_signing: bool,
    pub has_user_signing: bool,
    pub complete: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct E2eeRecoverySucceeded {
    pub operation: E2eeRecoveryOperation,
}

#[derive(Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct E2eeRecoverySetupSucceeded {
    pub recovery_key: String,
}

impl fmt::Debug for E2eeRecoverySetupSucceeded {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.debug_struct("E2eeRecoverySetupSucceeded")
            .field("recovery_key", &"<redacted>")
            .finish()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum E2eeRecoveryOperation {
    Recover,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum RoomActivityUpdateKind {
    MessageSent {
        event_id: String,
    },
    MessageFailed {
        message: String,
        recoverable: bool,
    },
    MessageReplaced {
        replacement_event_id: String,
        sender: String,
        timestamp_ms: u64,
        presentation: RoomMessagePresentationKind,
        body: String,
    },
    ReactionAdded {
        reaction_event_id: String,
        sender: String,
        timestamp_ms: u64,
        key: String,
    },
    ReactionRemoved {
        reaction_event_id: String,
        redaction_event_id: String,
        key: String,
    },
    ActivityRedacted {
        redaction_event_id: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum RoomMediaKind {
    Loaded {
        format: RoomMediaFormat,
        bytes: Vec<u8>,
    },
    Failed {
        format: RoomMediaFormat,
        message: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VerificationRequest {
    pub flow_id: String,
    pub user_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VerificationFlowId {
    pub flow_id: String,
    pub user_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VerificationStateEvent {
    pub flow_id: String,
    pub user_id: String,
    pub state: VerificationState,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum VerificationState {
    Requested,
    Accepted,
    Ready,
    SasReady {
        emojis: Vec<VerificationEmoji>,
        decimals: VerificationDecimals,
    },
    Confirmed,
    Done,
    Cancelled {
        reason: String,
        code: String,
        cancelled_by_us: bool,
    },
}

#[derive(serde::Deserialize)]
#[serde(tag = "state", rename_all = "snake_case", deny_unknown_fields)]
enum VerificationStateSerde {
    Requested {},
    Accepted {},
    Ready {},
    SasReady {
        emojis: Vec<VerificationEmoji>,
        decimals: VerificationDecimals,
    },
    Confirmed {},
    Done {},
    Cancelled {
        reason: String,
        code: String,
        cancelled_by_us: bool,
    },
}

impl<'de> serde::Deserialize<'de> for VerificationState {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let state = VerificationStateSerde::deserialize(deserializer)?;
        Ok(match state {
            VerificationStateSerde::Requested {} => Self::Requested,
            VerificationStateSerde::Accepted {} => Self::Accepted,
            VerificationStateSerde::Ready {} => Self::Ready,
            VerificationStateSerde::SasReady { emojis, decimals } => {
                Self::SasReady { emojis, decimals }
            }
            VerificationStateSerde::Confirmed {} => Self::Confirmed,
            VerificationStateSerde::Done {} => Self::Done,
            VerificationStateSerde::Cancelled {
                reason,
                code,
                cancelled_by_us,
            } => Self::Cancelled {
                reason,
                code,
                cancelled_by_us,
            },
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VerificationEmoji {
    pub symbol: String,
    pub description: String,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VerificationDecimals {
    pub first: u16,
    pub second: u16,
    pub third: u16,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CoreErrorEvent {
    pub operation: Operation,
    pub message: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Operation {
    Login,
    RestoreSession,
    SessionUpdate,
    StartSync,
    StopSync,
    JoinRoom,
    AcceptRoomInvite,
    CreateDirectRoom,
    LeaveRoom,
    KickRoomMember,
    BanRoomMember,
    UnbanRoomMember,
    InviteRoomMember,
    SetRoomMemberPowerLevel,
    SetDisplayName,
    RejectRoomInvite,
    SetRoomTopic,
    SendMessage,
    SendFile,
    EditMessage,
    SendReaction,
    RedactEvent,
    MarkRoomRead,
    BackfillRoomHistory,
    FetchRoomMedia,
    RoomList,
    RoomReadState,
    E2eeStatus,
    SetupE2eeRecovery,
    RecoverE2ee,
    Verification,
    Logout,
    Shutdown,
}

/// Why continuous sync ended. The session stays signed in either way.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum SyncStopReason {
    /// `Command::StopSync`.
    Requested,
    /// The sync loop ended with this error.
    Failed(String),
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Event {
    Started,
    /// `homeserver_url` is the client URL discovered from
    /// `LoginRequest.server`, which a later `RestoreSessionRequest` uses.
    LoggedIn {
        session: StoredSession,
        homeserver_url: String,
    },
    SessionUpdated {
        session: StoredSession,
    },
    RestoredSession,
    SyncStarted,
    SyncStopped(SyncStopReason),
    SyncedRooms(Vec<RoomSummary>),
    RoomActivity(Box<RoomActivity>),
    RoomActivityUpdate(RoomActivityUpdate),
    RoomHistoryBackfill(RoomHistoryBackfill),
    RoomMedia(RoomMedia),
    RoomTyping(RoomTyping),
    RoomMembers(RoomMembers),
    RoomReadState(RoomReadState),
    E2eeStatus(E2eeStatus),
    E2eeRecoverySetupSucceeded(E2eeRecoverySetupSucceeded),
    E2eeRecoverySucceeded(E2eeRecoverySucceeded),
    VerificationRequest(VerificationRequest),
    VerificationState(VerificationStateEvent),
    LoggedOut,
    Stopped,
    Error(CoreErrorEvent),
}
