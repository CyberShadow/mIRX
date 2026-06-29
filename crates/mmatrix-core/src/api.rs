use std::path::PathBuf;

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
    SendText(SendTextRequest),
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
pub struct SendTextRequest {
    pub room_id: String,
    pub body: String,
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
pub struct RoomSummary {
    pub room_id: String,
    pub display_name: String,
    pub encrypted: bool,
    pub is_space: bool,
    pub space_children: Vec<SpaceChildSummary>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RoomActivity {
    pub room_id: String,
    pub activity_id: String,
    pub timestamp_ms: u64,
    pub kind: RoomActivityKind,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum RoomActivityKind {
    Message(RoomMessageActivity),
    StateNotice(RoomStateNoticeActivity),
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RoomMessageActivity {
    pub event_id: Option<String>,
    pub transaction_id: Option<String>,
    pub sender: String,
    pub body: String,
    pub send_state: RoomMessageSendState,
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
    SendText,
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
    RoomActivity(RoomActivity),
    RoomActivityUpdate(RoomActivityUpdate),
    RoomTyping(RoomTyping),
    VerificationRequest(VerificationRequest),
    VerificationState(VerificationStateEvent),
    LoggedOut,
    Stopped,
    Error(CoreErrorEvent),
}
