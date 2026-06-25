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

#[derive(Debug, Clone, PartialEq, Eq)]
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RoomSummary {
    pub room_id: String,
    pub display_name: String,
    pub encrypted: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TimelineText {
    pub room_id: String,
    pub body: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerificationRequest {
    pub flow_id: String,
    pub user_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoreErrorEvent {
    pub operation: Operation,
    pub message: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Operation {
    Login,
    RestoreSession,
    StartSync,
    SendText,
    Logout,
    Shutdown,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    Started,
    LoggedIn,
    RestoredSession,
    SyncedRooms(Vec<RoomSummary>),
    TimelineText(TimelineText),
    VerificationRequest(VerificationRequest),
    LoggedOut,
    Stopped,
    Error(CoreErrorEvent),
}
