use std::path::PathBuf;
use std::{fmt, fmt::Formatter};

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
    JoinRoom(JoinRoomRequest),
    LeaveRoom(LeaveRoomRequest),
    SendMessage(SendMessageRequest),
    MarkRoomRead(MarkRoomReadRequest),
    RefreshE2eeStatus,
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
    pub homeserver_url: String,
    pub username: String,
    pub password: String,
    pub device_display_name: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RestoreSessionRequest {
    pub homeserver_url: String,
    pub session: StoredSession,
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
pub struct SendMessageRequest {
    pub room_id: String,
    pub presentation: RoomMessagePresentationKind,
    pub body: String,
    pub thread: Option<SendMessageThreadTarget>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct JoinRoomRequest {
    pub room_or_alias: String,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LeaveRoomRequest {
    pub room_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MarkRoomReadRequest {
    pub room_id: String,
    pub event_id: String,
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

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RoomSummary {
    pub room_id: String,
    pub display_name: String,
    pub encrypted: bool,
    pub is_space: bool,
    pub is_direct: bool,
    pub direct_user_ids: Vec<String>,
    pub tags: Vec<RoomTagSummary>,
    pub space_children: Vec<SpaceChildSummary>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RoomActivity {
    pub room_id: String,
    pub activity_id: String,
    pub timestamp_ms: u64,
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
    Image(RoomImageActivity),
    UnsupportedMessage(UnsupportedRoomMessageActivity),
    StateNotice(RoomStateNoticeActivity),
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
    pub sender: String,
    pub presentation: RoomMessagePresentationKind,
    pub body: String,
    pub send_state: RoomMessageSendState,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RoomImageActivity {
    pub event_id: String,
    pub sender: String,
    pub body: String,
    pub filename: Option<String>,
    pub source: RoomMediaSource,
    pub info: Option<RoomImageInfo>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum RoomMediaSource {
    Plain { url: String },
    Encrypted { url: String },
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RoomImageInfo {
    pub mimetype: Option<String>,
    pub size: Option<u64>,
    pub width: Option<u64>,
    pub height: Option<u64>,
    pub is_animated: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UnsupportedRoomMessageActivity {
    pub event_id: String,
    pub sender: String,
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
    pub sender: String,
    pub state_key: String,
    pub content: RoomStateNoticeContent,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum RoomStateNoticeContent {
    Topic {
        topic: String,
    },
    Member {
        membership: String,
        displayname: Option<String>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RoomActivityUpdate {
    pub room_id: String,
    pub transaction_id: String,
    pub kind: RoomActivityUpdateKind,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum E2eeRecoveryOperation {
    Recover,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum RoomActivityUpdateKind {
    MessageSent { event_id: String },
    MessageFailed { message: String, recoverable: bool },
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
    JoinRoom,
    LeaveRoom,
    SendMessage,
    MarkRoomRead,
    RoomList,
    RoomReadState,
    E2eeStatus,
    RecoverE2ee,
    Verification,
    Logout,
    Shutdown,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Event {
    Started,
    LoggedIn { session: StoredSession },
    SessionUpdated { session: StoredSession },
    RestoredSession,
    SyncStarted,
    SyncedRooms(Vec<RoomSummary>),
    RoomActivity(Box<RoomActivity>),
    RoomActivityUpdate(RoomActivityUpdate),
    RoomTyping(RoomTyping),
    RoomMembers(RoomMembers),
    RoomReadState(RoomReadState),
    E2eeStatus(E2eeStatus),
    E2eeRecoverySucceeded(E2eeRecoverySucceeded),
    VerificationRequest(VerificationRequest),
    VerificationState(VerificationStateEvent),
    LoggedOut,
    Stopped,
    Error(CoreErrorEvent),
}
