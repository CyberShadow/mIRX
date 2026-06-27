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
pub struct RoomSummary {
    pub room_id: String,
    pub display_name: String,
    pub encrypted: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TimelineText {
    pub room_id: String,
    pub sender: String,
    pub timestamp_ms: u64,
    pub body: String,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct RoomStateEntry {
    pub room_id: String,
    pub sender: String,
    pub timestamp_ms: u64,
    pub event_kind: RoomStateEventKind,
    pub state_key: String,
    pub content: RoomStateContent,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum RoomStateEventKind {
    Topic,
    Member,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum RoomStateContent {
    Topic(RoomTopicState),
    Member(RoomMemberState),
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct RoomStateEntrySerde {
    room_id: String,
    sender: String,
    timestamp_ms: u64,
    event_kind: RoomStateEventKind,
    state_key: String,
    content: RoomStateContent,
}

impl<'de> serde::Deserialize<'de> for RoomStateEntry {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let dto = RoomStateEntrySerde::deserialize(deserializer)?;
        let content_matches_kind = matches!(
            (&dto.event_kind, &dto.content),
            (RoomStateEventKind::Topic, RoomStateContent::Topic(_))
                | (RoomStateEventKind::Member, RoomStateContent::Member(_))
        );

        if !content_matches_kind {
            return Err(serde::de::Error::custom(
                "room state event_kind must match content variant",
            ));
        }

        Ok(Self {
            room_id: dto.room_id,
            sender: dto.sender,
            timestamp_ms: dto.timestamp_ms,
            event_kind: dto.event_kind,
            state_key: dto.state_key,
            content: dto.content,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RoomTopicState {
    pub topic: String,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RoomMemberState {
    pub membership: String,
    pub displayname: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VerificationRequest {
    pub flow_id: String,
    pub user_id: String,
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
    TimelineText(TimelineText),
    RoomState(RoomStateEntry),
    VerificationRequest(VerificationRequest),
    LoggedOut,
    Stopped,
    Error(CoreErrorEvent),
}
