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
    /// Stops a sign-in or restore in progress, or the wait for the browser
    /// after `Event::SignInUrl`; answered by `Event::LoginCancelled`. The
    /// cancelled `Login` or `RestoreSession` is answered after it too.
    CancelLogin,
    RestoreSession(RestoreSessionRequest),
    StartSync,
    /// Stops continuous sync and keeps the session. During a logout, it
    /// stops the logout instead (`LogoutCancelled`).
    StopSync,
    JoinRoom(JoinRoomRequest),
    AcceptRoomInvite(AcceptRoomInviteRequest),
    CreateDirectRoom(CreateDirectRoomRequest),
    CreateRoom(CreateRoomRequest),
    LeaveRoom(LeaveRoomRequest),
    KickRoomMember(KickRoomMemberRequest),
    BanRoomMember(BanRoomMemberRequest),
    UnbanRoomMember(UnbanRoomMemberRequest),
    InviteRoomMember(InviteRoomMemberRequest),
    SetRoomMemberPowerLevel(SetRoomMemberPowerLevelRequest),
    SetDisplayName(SetDisplayNameRequest),
    RejectRoomInvite(RejectRoomInviteRequest),
    SetRoomTopic(SetRoomTopicRequest),
    /// Reads a joined room's settings from the server (not the store, which
    /// lags a write until its echo). Answered by `RoomSettings`, or a
    /// `FetchRoomSettings` error.
    FetchRoomSettings(String),
    /// Changes a joined room's settings, part by part in the request's
    /// field order; it stops at the first refused part (the parts before
    /// it stay), and the error names it. The server's echo brings the
    /// lines; nothing else answers.
    UpdateRoomSettings(UpdateRoomSettingsRequest),
    /// Writes the room's push rule; the next sync's `SyncedRooms` reports
    /// the new mode.
    SetRoomNotificationMode(SetRoomNotificationModeRequest),
    /// Writes a content push rule that highlights messages containing the
    /// word (see `validate_highlight_word`); the next sync's
    /// `HighlightWords` lists it.
    AddHighlightWord(String),
    /// Deletes the user's content push rules for the word.
    RemoveHighlightWord(String),
    /// Adds the user ID to the account's ignore list
    /// (`m.ignored_user_list`); the next sync's `IgnoredUsers` lists it.
    IgnoreUser(String),
    /// Removes the user ID from the ignore list; the next sync's
    /// `IgnoredUsers` no longer lists it.
    UnignoreUser(String),
    /// Asks the user's homeserver for their global profile; answered by
    /// `UserProfile`.
    FetchUserProfile(String),
    /// `Some(message)` marks you away (`unavailable`, with the message as
    /// your status), `None` present again (`online`, no status). Answered
    /// by `AwayChanged`, or a `SetPresence` error. While you are away
    /// every sync request of the session says `unavailable`, since a sync
    /// that says `online` would make you present again; a new session
    /// starts present.
    SetAway(Option<String>),
    /// Fetches the user's cross-signing identity and devices fresh from
    /// their server (`/keys/query`; for the own user also `/devices`);
    /// answered by `UserDevices`.
    ListUserDevices(String),
    /// Accepts a user's changed identity: withdraws the verification of a
    /// verified identity that changed (which also pins the new one), else
    /// pins the current one. Answered by `UserIdentityAccepted`; the room
    /// identity streams then report the user `Pinned`.
    AcceptUserIdentity(String),
    /// Signs out another of your sessions. Answered by `DeviceSignedOut`,
    /// by `DeviceSignOutApproval` when the server signs sessions out on
    /// its account page, or by an error. See `SignOutDeviceRequest`.
    SignOutDevice(SignOutDeviceRequest),
    /// Renames one of your sessions, this one too. Answered by
    /// `DeviceRenamed`, or by an error.
    RenameDevice(RenameDeviceRequest),
    SendMessage(SendMessageRequest),
    SendFile(SendFileRequest),
    EditMessage(EditMessageRequest),
    SendReaction(SendReactionRequest),
    RedactEvent(RedactEventRequest),
    /// Sends the room's public read receipt (`m.read`), which clears its
    /// unread counts. It leaves the read marker alone. Answered only by an
    /// error.
    MarkRoomRead(MarkRoomReadRequest),
    /// Moves the room's read marker (`m.fully_read`), which the user's
    /// other clients show as their "new messages" line. Its new place comes
    /// back as `RoomReadState` with the next sync; a failure is an error.
    SetReadMarker(SetReadMarkerRequest),
    /// Tells the room you are typing (`m.typing`), or have stopped.
    /// Answered only by an error. The SDK sends `typing` at most every 3 s
    /// (each lasting 4 s), and a stop only while a notice is live, so this
    /// may come with every key typed.
    SetTyping(SetTypingRequest),
    BackfillRoomHistory(BackfillRoomHistoryRequest),
    FetchRoomMedia(FetchRoomMediaRequest),
    FetchRoomEvent(FetchRoomEventRequest),
    ListPublicRooms(ListPublicRoomsRequest),
    RefreshE2eeStatus,
    SetupE2eeRecovery(SetupE2eeRecoveryRequest),
    RecoverE2ee(RecoverE2eeRequest),
    /// Resets your cross-signing identity, and with it key backup and
    /// recovery. See `ResetE2eeIdentityRequest`.
    ResetE2eeIdentity(ResetE2eeIdentityRequest),
    /// Writes the session's room keys to a file, encrypted with the
    /// passphrase, as Element's "Export E2E room keys" does. Answered by
    /// `RoomKeysExported`, or an `ExportRoomKeys` error.
    ExportRoomKeys(ExportRoomKeysRequest),
    /// Reads room keys from such a file (Element's, or another session's).
    /// Answered by `RoomKeysImported`, or an `ImportRoomKeys` error; the
    /// lines the keys decrypt follow as `RoomEventsDecrypted`.
    ImportRoomKeys(ImportRoomKeysRequest),
    /// Stops a reset that waits for approval in the browser; answered by
    /// `E2eeIdentityReset(Cancelled)`, or `Done` if the approval won.
    CancelE2eeIdentityReset,
    /// Starts a verification, answered by `Event::VerificationRequest` with
    /// `VerificationDirection::Outgoing`, then `VerificationState` events.
    /// See `RequestVerificationRequest`.
    RequestVerification(RequestVerificationRequest),
    AcceptVerification(VerificationFlowId),
    CancelVerification(VerificationFlowId),
    ConfirmVerificationSas(VerificationFlowId),
    RejectVerificationSas(VerificationFlowId),
    /// Reads again the events the UI shows that could not be decrypted:
    /// the answer to `DecryptionRetryWanted`, and to a change of this
    /// session's verification or key backup, which their causes depend on.
    /// Answered with one `RoomEventsDecrypted` of each still in the event
    /// cache, as it is now: decrypted, or still undecryptable with its
    /// current cause. The SDK also tries again the room's other events of
    /// their sessions; what that decrypts arrives later, as
    /// `RoomEventsDecrypted` too.
    RetryDecryption(RetryDecryptionRequest),
    /// Ends the session on the server and removes its data from this
    /// computer. Answered by `LogoutWouldLoseRoomKeys` at once (the
    /// session stays, as it was), or by `LoggingOut` and then `LoggedOut`,
    /// `LogoutWouldLoseRoomKeys` or a `Logout` error (the session stays),
    /// or `LogoutCancelled` when a `StopSync` stops it.
    Logout(LogoutRequest),
    Shutdown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LogoutRequest {
    /// Log out even when room keys would be lost: when no key backup a
    /// new session can open holds them all. Without it the core first
    /// waits for the key backup to hold every key, and refuses when it
    /// cannot.
    pub discard_room_keys: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoginRequest {
    /// A server name (`matrix.org`, `example.org:8448`) or a homeserver URL;
    /// the client URL is discovered from it. See `login_server`.
    pub server: String,
    pub method: LoginMethod,
    /// Names the new device for password and SSO sign-in; OAuth 2.0 servers
    /// name the session after the client mirx registers ("mirx").
    pub device_display_name: String,
    /// Start continuous sync once the session is ready, as `StartSync` would.
    pub start_sync: bool,
}

/// See `login_method`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LoginMethod {
    Password {
        username: String,
        password: String,
    },
    /// Sign in on the homeserver's web page, in the user's browser: through
    /// OAuth 2.0 if the server publishes authorization server metadata,
    /// else through legacy SSO. `Event::SignInUrl` names the page.
    Browser {
        /// A full user ID, which pre-fills an OAuth 2.0 server's page
        /// (`login_hint`); legacy SSO has no use for it.
        user_id_hint: Option<String>,
    },
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

/// How to sign in with Select Server's User and Password: a password
/// signs in with it, and needs a user; no password signs in with the
/// browser, where User is empty or a full user ID. A bare name cannot
/// pre-fill the page: the server field may be a URL rather than the
/// user's server name.
pub fn login_method(username: &str, password: &str) -> Result<LoginMethod, String> {
    let username = username.trim();
    if !password.is_empty() {
        if username.is_empty() {
            return Err(
                "enter a user name, or leave Password empty to sign in with your browser"
                    .to_owned(),
            );
        }
        return Ok(LoginMethod::Password {
            username: username.to_owned(),
            password: password.to_owned(),
        });
    }
    if username.is_empty() {
        return Ok(LoginMethod::Browser { user_id_hint: None });
    }
    matrix_sdk::ruma::UserId::parse(username)
        .map(|user_id| LoginMethod::Browser {
            user_id_hint: Some(user_id.to_string()),
        })
        .map_err(|_| {
            "to sign in with your browser, leave User empty or enter a full user ID like @alice:matrix.org"
                .to_owned()
        })
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StoredSession {
    pub user_id: String,
    pub device_id: String,
    pub access_token: String,
    pub refresh_token: Option<String>,
    pub auth: SessionAuth,
}

/// How a session signed in, which decides where its tokens are refreshed
/// and revoked.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum SessionAuth {
    /// The Matrix login API: a password or an SSO login token.
    Matrix,
    /// OAuth 2.0: the homeserver's authorization server refreshes and
    /// revokes the tokens of the client it registered as `client_id`.
    OAuth { client_id: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SendMessageThreadTarget {
    pub thread_root_event_id: String,
    pub latest_event_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SendMessageReplyTarget {
    pub event_id: String,
    /// Who sent the message replied to, whom the reply mentions.
    pub sender_user_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SendMessageRequest {
    pub room_id: String,
    pub presentation: RoomMessagePresentationKind,
    pub body: String,
    /// Formats `body`; must pass [`check_text_styles`].
    pub styles: Vec<TextStyleSpan>,
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
    /// Formats `body`; must pass [`check_text_styles`].
    pub styles: Vec<TextStyleSpan>,
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
    /// Servers to join a room ID through, when the user's homeserver is
    /// not in the room; an alias resolves its own servers.
    pub via: Vec<String>,
}

/// One page of a server's public room directory.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ListPublicRoomsRequest {
    /// The server whose directory to list; `None` is the user's homeserver.
    pub server: Option<String>,
    /// Text the server matches against room names, topics and aliases;
    /// `None` lists every room.
    pub filter: Option<String>,
    /// The previous page's `next_batch`; `None` asks for the first page.
    pub since: Option<String>,
    pub limit: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CreateDirectRoomRequest {
    pub user_id: String,
}

/// A new room the user creates and joins.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CreateRoomRequest {
    pub name: String,
    pub visibility: CreateRoomVisibility,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum CreateRoomVisibility {
    /// Invite-only, encrypted and unlisted.
    Private,
    /// Anyone may join; listed in the room directory under
    /// `#<alias_localpart>:<the user's server>`.
    Public { alias_localpart: String },
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
    /// Applied together, in one write of the room's power levels.
    pub changes: Vec<MemberPowerLevelChange>,
}

/// One member's new role in a `SetRoomMemberPowerLevelRequest`.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MemberPowerLevelChange {
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

/// A room's settings as Channel Central shows them, read from the server.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RoomSettings {
    pub room_id: String,
    /// `m.room.name`; `None` when absent or empty.
    pub name: Option<String>,
    /// `m.room.topic`; `None` when absent.
    pub topic: Option<String>,
    /// `m.room.join_rules`; the spec's `invite` when absent.
    pub join_rule: RoomJoinRule,
    /// Changing the topic needs more than the default user level.
    pub topic_by_operators: bool,
    /// Sending messages (`m.room.message` and `m.room.encrypted`) needs
    /// more than the default user level: only voiced members speak.
    pub moderated: bool,
    /// Published in the server's room directory (the Rooms List).
    pub listed_in_directory: bool,
    /// Newest first.
    pub bans: Vec<RoomBan>,
    /// What the signed-in user may change.
    pub allowed: RoomSettingsPermissions,
}

/// A banned user: their `m.room.member` ban event.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RoomBan {
    pub user_id: String,
    /// The ban event's `displayname`, if it kept one.
    pub display_name: Option<String>,
    /// Who banned them: the event's sender.
    pub banned_by: String,
    pub reason: Option<String>,
    pub timestamp_ms: u64,
}

/// Which of a `RoomSettings`' parts the signed-in user's power level lets
/// them change.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RoomSettingsPermissions {
    pub name: bool,
    pub topic: bool,
    pub join_rule: bool,
    /// `topic_by_operators` and `moderated`: the power levels.
    pub power_levels: bool,
    /// `listed_in_directory`.
    pub directory: bool,
    pub unban: bool,
}

/// The settings to change; `None` (and no unbans) leaves a part as it is.
/// The parts are written in field order.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UpdateRoomSettingsRequest {
    pub room_id: String,
    /// `topic_by_operators` and `moderated` go in one power-levels write.
    pub topic_by_operators: Option<bool>,
    pub moderated: Option<bool>,
    /// `true`: join rule `invite`; `false`: `public`.
    pub invite_only: Option<bool>,
    /// `Some("")` removes the name.
    pub name: Option<String>,
    /// `Some("")` clears the topic.
    pub topic: Option<String>,
    pub listed_in_directory: Option<bool>,
    /// User IDs to unban, in order.
    pub unban: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SetRoomNotificationModeRequest {
    pub room_id: String,
    pub mode: RoomNotificationMode,
}

/// One of the user's keyword (content) push rules, as mIRC's highlight
/// list shows it.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HighlightWord {
    /// The rule's pattern: a word or phrase, `*` and `?` are wildcards.
    pub word: String,
    /// The rule highlights (`set_tweak: highlight`); otherwise it only
    /// notifies, as the keywords other clients add do.
    pub highlights: bool,
}

/// Whether `word` can name a highlight word's push rule: the rule ID is the
/// word itself, as Element names keyword rules.
pub fn validate_highlight_word(word: &str) -> Result<(), String> {
    if word.trim().is_empty() {
        return Err("a highlight word cannot be empty".to_owned());
    }
    if word.trim() != word {
        return Err("a highlight word cannot start or end with a space".to_owned());
    }
    if word.contains(['/', '\\']) {
        return Err("a highlight word cannot contain / or \\".to_owned());
    }
    if word.starts_with('.') {
        return Err("a highlight word cannot start with .".to_owned());
    }
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MarkRoomReadRequest {
    pub room_id: String,
    pub event_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SetReadMarkerRequest {
    pub room_id: String,
    pub event_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SetTypingRequest {
    pub room_id: String,
    pub typing: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BackfillRoomHistoryRequest {
    pub room_id: String,
    pub from: HistoryFrom,
    pub limit: u32,
}

/// Where a history page starts, going back.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum HistoryFrom {
    /// The live end of the room: the first page a window asks for.
    Latest,
    /// A `/messages` token: the previous page's `end`.
    Token { token: String },
    /// Just before this event: what a window that dropped its older lines
    /// asks for.
    BeforeEvent { event_id: String },
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

/// One event of a joined room, such as the message a reply answers when it
/// is not loaded.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FetchRoomEventRequest {
    pub room_id: String,
    pub event_id: String,
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

/// `path` is the file's full path. The passphrase must not be empty.
#[derive(Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExportRoomKeysRequest {
    pub path: String,
    pub passphrase: String,
}

impl fmt::Debug for ExportRoomKeysRequest {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.debug_struct("ExportRoomKeysRequest")
            .field("path", &self.path)
            .field("passphrase", &"<redacted>")
            .finish()
    }
}

/// `path` is the file's full path.
#[derive(Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ImportRoomKeysRequest {
    pub path: String,
    pub passphrase: String,
}

impl fmt::Debug for ImportRoomKeysRequest {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.debug_struct("ImportRoomKeysRequest")
            .field("path", &self.path)
            .field("passphrase", &"<redacted>")
            .finish()
    }
}

/// Answered by `E2eeIdentityReset(Done)` and an `E2eeStatus`, or first by
/// `E2eeIdentityResetApproval` when the server wants the reset approved in
/// a browser. The reset deletes the key backup and turns recovery off
/// before it asks the server, so a refused or cancelled reset leaves them
/// off; an `E2eeStatus` follows either way.
#[derive(Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResetE2eeIdentityRequest {
    /// The account password, for a server that confirms the reset with it
    /// (user-interactive auth). `None` for an OAuth 2.0 session.
    pub password: Option<String>,
}

impl fmt::Debug for ResetE2eeIdentityRequest {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.debug_struct("ResetE2eeIdentityRequest")
            .field("password", &self.password.as_ref().map(|_| "<redacted>"))
            .finish()
    }
}

/// The core refuses this session (File > Log Out signs it out) and a
/// session the account does not have, before it asks the server to sign
/// anything out.
#[derive(Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SignOutDeviceRequest {
    pub device_id: String,
    /// The account password, for a server that confirms the sign-out with
    /// it (user-interactive auth). `None` for an OAuth 2.0 session, whose
    /// account page confirms it instead.
    pub password: Option<String>,
}

impl fmt::Debug for SignOutDeviceRequest {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.debug_struct("SignOutDeviceRequest")
            .field("device_id", &self.device_id)
            .field("password", &self.password.as_ref().map(|_| "<redacted>"))
            .finish()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RenameDeviceRequest {
    pub device_id: String,
    pub display_name: String,
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
    /// How the user's push rules treat the room: `Some` exactly for joined
    /// rooms, `None` for invites.
    pub notification_mode: Option<RoomNotificationMode>,
    /// `m.room.canonical_alias`'s `alias`, e.g. `#ops:example.org`.
    pub canonical_alias: Option<String>,
    /// `m.room.canonical_alias`'s `alt_aliases`, in event order.
    pub alt_aliases: Vec<String>,
}

/// A joined room's effective notification mode under the user's push
/// rules: its own rule if it has one, else the account default for its kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RoomNotificationMode {
    AllMessages,
    MentionsAndKeywordsOnly,
    Mute,
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
            notification_mode: serde_json::Value,
            canonical_alias: serde_json::Value,
            alt_aliases: Vec<String>,
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
        let canonical_alias = match room.canonical_alias {
            serde_json::Value::Null => None,
            serde_json::Value::String(alias) => Some(alias),
            alias => {
                return Err(serde::de::Error::custom(format!(
                    "invalid canonical_alias for room {}: expected string or null, got {alias}",
                    room.room_id
                )));
            }
        };
        let notification_mode = match (room.membership, room.notification_mode) {
            (RoomMembership::Invite, serde_json::Value::Null) => None,
            (RoomMembership::Join, mode @ serde_json::Value::String(_)) => {
                Some(serde_json::from_value(mode).map_err(|err| {
                    serde::de::Error::custom(format!(
                        "invalid notification_mode for room {}: {err}",
                        room.room_id
                    ))
                })?)
            }
            (membership, mode) => {
                return Err(serde::de::Error::custom(format!(
                    "invalid notification_mode for {membership:?} room {}: expected {}, got {mode}",
                    room.room_id,
                    match membership {
                        RoomMembership::Join => "a mode",
                        RoomMembership::Invite => "null",
                    }
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
            notification_mode,
            canonical_alias,
            alt_aliases: room.alt_aliases,
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
    /// `m.sticker`: an image sent as a sticker. Its body is the sticker's
    /// description; `filename` is always `None`.
    Sticker(Box<RoomImageActivity>),
    /// A poll started or ended (`m.poll.*` and MSC3381's unstable types).
    Poll(Box<RoomPollActivity>),
    /// An in-room verification request (`m.key.verification.request`).
    VerificationRequest(RoomVerificationRequestActivity),
    /// A message of a type without a line of its own: its body, named by
    /// its msgtype.
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
    /// The name the room knows the user by: their display name, or their
    /// user ID's localpart when they set none (or an empty one). `None`
    /// when the client has no member event for them, as for a user who
    /// never joined.
    pub nick: Option<String>,
    /// Whether another member's nick is the same (as the SDK normalises
    /// names), so the nick alone does not identify them. Always false
    /// without a nick.
    pub nick_ambiguous: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RoomUnableToDecryptActivity {
    pub event_id: String,
    pub sender: RoomUserProfile,
    pub session_id: Option<String>,
    /// Why, as far as this session can tell now: it depends on this
    /// session's verification and key backup, so a retry can tell it anew.
    pub cause: RoomUnableToDecryptCause,
}

/// Why an event could not be decrypted: the SDK's `UtdCause`, which reads
/// the reason with this session's state and the event's membership info.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RoomUnableToDecryptCause {
    /// No explanation: the key may yet come (from the sender, another of
    /// your sessions, the key backup), or it is a bug.
    Unknown,
    /// The key is missing, and it was sent while you were not in the room.
    SentBeforeYouJoined,
    /// The key is missing, it was sent before this session existed, and
    /// there is no key backup on the server.
    HistoricalNoBackup,
    /// The key is missing, it was sent before this session existed, and
    /// the key backup this session cannot use yet is there to verify for.
    HistoricalUnverified,
    /// The sender withholds its key from sessions that are not verified.
    WithheldUnverified,
    /// The sender withheld its key from this session (blocked it, or could
    /// not reach it).
    WithheldBySender,
    /// The sender's identity was verified, and has changed since.
    SenderIdentityChanged,
    /// Sent from a session its owner has not signed, which the trust
    /// requirement refuses.
    UnsignedDevice,
    /// Sent from a session that cannot be found securely, which the trust
    /// requirement refuses.
    UnknownDevice,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RoomMessagePresentationKind {
    Text,
    Notice,
    Emote,
}

/// An sRGB colour; `"#rrggbb"` in lowercase hex on the wire.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TextColor {
    pub red: u8,
    pub green: u8,
    pub blue: u8,
}

impl TextColor {
    /// Parses exactly `#rrggbb` in lowercase hex, the wire form.
    fn from_wire(text: &str) -> Option<Self> {
        let hex = text.strip_prefix('#')?;
        if hex.len() != 6
            || !hex
                .bytes()
                .all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'))
        {
            return None;
        }
        let channel = |index: usize| {
            u8::from_str_radix(&hex[index..index + 2], 16).expect("checked lowercase hex digits")
        };
        Some(Self {
            red: channel(0),
            green: channel(2),
            blue: channel(4),
        })
    }
}

impl fmt::Display for TextColor {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "#{:02x}{:02x}{:02x}",
            self.red, self.green, self.blue
        )
    }
}

impl serde::Serialize for TextColor {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.collect_str(self)
    }
}

impl<'de> serde::Deserialize<'de> for TextColor {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let text = String::deserialize(deserializer)?;
        Self::from_wire(&text).ok_or_else(|| {
            serde::de::Error::custom(format!(
                "invalid text color {text:?}: expected #rrggbb in lowercase hex"
            ))
        })
    }
}

/// How a run of message text is drawn. `TextStyle::default()` is plain.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TextStyle {
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub strikethrough: bool,
    pub foreground: Option<TextColor>,
    pub background: Option<TextColor>,
}

/// `style` applies to `body[start..end]`, in UTF-8 byte offsets.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TextStyleSpan {
    pub start: usize,
    pub end: usize,
    pub style: TextStyle,
}

/// Checks that `styles` describe `body` in the one canonical way: sorted,
/// non-overlapping, non-empty spans on char boundaries, none plain, no two
/// touching spans with the same style (they are one span), and none
/// covering a `\n`, so styles never cross a line.
pub fn check_text_styles(body: &str, styles: &[TextStyleSpan]) -> Result<(), String> {
    let mut previous: Option<&TextStyleSpan> = None;
    for span in styles {
        let TextStyleSpan { start, end, style } = span;
        if start >= end {
            return Err(format!("empty text style span {start}..{end}"));
        }
        if *end > body.len() {
            return Err(format!(
                "text style span {start}..{end} is past the body's {} bytes",
                body.len()
            ));
        }
        if !body.is_char_boundary(*start) || !body.is_char_boundary(*end) {
            return Err(format!(
                "text style span {start}..{end} is not on char boundaries"
            ));
        }
        if *style == TextStyle::default() {
            return Err(format!("text style span {start}..{end} is plain"));
        }
        if body[*start..*end].contains('\n') {
            return Err(format!("text style span {start}..{end} crosses a line"));
        }
        if let Some(previous) = previous {
            if previous.end > *start {
                return Err(format!(
                    "text style span {start}..{end} overlaps or precedes {}..{}",
                    previous.start, previous.end
                ));
            }
            if previous.end == *start && previous.style == *style {
                return Err(format!(
                    "text style spans {}..{} and {start}..{end} have the same style and must be one",
                    previous.start, previous.end
                ));
            }
        }
        previous = Some(span);
    }
    Ok(())
}

/// `body[start..end]`, in UTF-8 byte offsets, links to the user `user_id`:
/// a pill.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UserLinkSpan {
    pub start: usize,
    pub end: usize,
    pub user_id: String,
}

/// Checks that `links` describe `body` in the one canonical way: sorted,
/// non-overlapping, non-empty spans on char boundaries, none covering a
/// `\n`, each naming a valid user ID. Unlike styles, two spans may touch:
/// adjacent pills are two links.
pub fn check_user_links(body: &str, links: &[UserLinkSpan]) -> Result<(), String> {
    let mut previous_end = 0;
    for UserLinkSpan {
        start,
        end,
        user_id,
    } in links
    {
        if start >= end {
            return Err(format!("empty user link span {start}..{end}"));
        }
        if *end > body.len() {
            return Err(format!(
                "user link span {start}..{end} is past the body's {} bytes",
                body.len()
            ));
        }
        if !body.is_char_boundary(*start) || !body.is_char_boundary(*end) {
            return Err(format!(
                "user link span {start}..{end} is not on char boundaries"
            ));
        }
        if body[*start..*end].contains('\n') {
            return Err(format!("user link span {start}..{end} crosses a line"));
        }
        if *start < previous_end {
            return Err(format!(
                "user link span {start}..{end} overlaps or precedes the one ending at {previous_end}"
            ));
        }
        if let Err(err) = <&matrix_sdk::ruma::UserId>::try_from(user_id.as_str()) {
            return Err(format!(
                "user link span {start}..{end} names an invalid user ID {user_id:?}: {err}"
            ));
        }
        previous_end = *end;
    }
    Ok(())
}

/// A text, notice or emote message. `styles` format `body` and
/// `user_links` say where it links to users; they pass
/// [`check_text_styles`] and [`check_user_links`], which the wire form
/// enforces.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(try_from = "WireRoomMessageActivity")]
pub struct RoomMessageActivity {
    pub event_id: Option<String>,
    pub transaction_id: Option<String>,
    pub sender: RoomUserProfile,
    pub presentation: RoomMessagePresentationKind,
    pub body: String,
    pub styles: Vec<TextStyleSpan>,
    pub user_links: Vec<UserLinkSpan>,
    pub send_state: RoomMessageSendState,
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct WireRoomMessageActivity {
    event_id: Option<String>,
    transaction_id: Option<String>,
    sender: RoomUserProfile,
    presentation: RoomMessagePresentationKind,
    body: String,
    styles: Vec<TextStyleSpan>,
    user_links: Vec<UserLinkSpan>,
    send_state: RoomMessageSendState,
}

impl TryFrom<WireRoomMessageActivity> for RoomMessageActivity {
    type Error = String;

    fn try_from(message: WireRoomMessageActivity) -> Result<Self, Self::Error> {
        check_text_styles(&message.body, &message.styles).map_err(|err| {
            format!(
                "invalid styles for message (event {:?}, transaction {:?}): {err}",
                message.event_id, message.transaction_id
            )
        })?;
        check_user_links(&message.body, &message.user_links).map_err(|err| {
            format!(
                "invalid user links for message (event {:?}, transaction {:?}): {err}",
                message.event_id, message.transaction_id
            )
        })?;
        Ok(Self {
            event_id: message.event_id,
            transaction_id: message.transaction_id,
            sender: message.sender,
            presentation: message.presentation,
            body: message.body,
            styles: message.styles,
            user_links: message.user_links,
            send_state: message.send_state,
        })
    }
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
pub struct RoomPollActivity {
    pub event_id: String,
    pub sender: RoomUserProfile,
    pub content: RoomPollContent,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum RoomPollContent {
    /// The question and its answers, in order, as plain text.
    Start {
        question: String,
        answers: Vec<String>,
        /// How many answers a vote may choose.
        max_selections: u64,
        /// The votes show only once the poll ends (an undisclosed poll).
        results_hidden: bool,
    },
    /// `text` is the end event's own text: its results, as the sender's
    /// client wrote them.
    End {
        poll_start_event_id: String,
        text: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RoomVerificationRequestActivity {
    pub event_id: String,
    pub sender: RoomUserProfile,
    /// The user ID asked to verify.
    pub to: String,
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
    /// An `m.room.name` change; `None` when the name was removed (empty).
    Name {
        name: Option<String>,
    },
    /// An `m.room.join_rules` change.
    JoinRule {
        rule: RoomJoinRule,
    },
    /// An `m.room.encryption` event: the room turned encryption on.
    Encryption,
}

/// A room's join rule (`m.room.join_rules`).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum RoomJoinRule {
    Public,
    Invite,
    Knock,
    /// Open to members of the rooms its allow list names (its spaces).
    Restricted,
    KnockRestricted,
    Private,
    /// A rule the spec does not define, by its name.
    Other(String),
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
    /// The member's nick changed (see [`RoomUserProfile::nick`]); a display
    /// name change that leaves the nick the same is no change.
    NickChanged {
        old: String,
        new: String,
    },
    AvatarChanged,
}

/// A change to an earlier activity. A `MessageReplaced` kind's styles and
/// user links pass [`check_text_styles`] and [`check_user_links`], which
/// the wire form enforces.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(try_from = "WireRoomActivityUpdate")]
pub struct RoomActivityUpdate {
    pub room_id: String,
    pub target: RoomActivityUpdateTarget,
    pub kind: RoomActivityUpdateKind,
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct WireRoomActivityUpdate {
    room_id: String,
    target: RoomActivityUpdateTarget,
    kind: RoomActivityUpdateKind,
}

impl TryFrom<WireRoomActivityUpdate> for RoomActivityUpdate {
    type Error = String;

    fn try_from(update: WireRoomActivityUpdate) -> Result<Self, Self::Error> {
        if let RoomActivityUpdateKind::MessageReplaced {
            replacement_event_id,
            body,
            styles,
            user_links,
            ..
        } = &update.kind
        {
            check_text_styles(body, styles).map_err(|err| {
                format!("invalid styles for replacement {replacement_event_id}: {err}")
            })?;
            check_user_links(body, user_links).map_err(|err| {
                format!("invalid user links for replacement {replacement_event_id}: {err}")
            })?;
        }
        Ok(Self {
            room_id: update.room_id,
            target: update.target,
            kind: update.kind,
        })
    }
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
    /// Where the next older page starts; `None` when the room has no older
    /// history.
    pub end: Option<String>,
    pub events: Vec<RoomHistoryBackfillEvent>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RetryDecryptionRequest {
    pub room_id: String,
    /// The events the room's window shows that could not be decrypted
    /// (`RoomUnableToDecryptActivity::event_id`); at least one.
    pub event_ids: Vec<String>,
}

/// Events of a room the core could not decrypt when they arrived, read
/// again: because their keys came, or for `RetryDecryption`. Each is as
/// backfill would map it, oldest first. An `Activity` replaces the line of
/// the same event that could not be decrypted: with its message, or, still
/// undecryptable, with its current cause. An `ActivityUpdate` is what a
/// decrypted event does to another: an edit, a reaction. An event can be
/// reported again, decrypted all along, when the SDK refreshes its
/// encryption info: replacing only lines that could not be decrypted and
/// applying updates idempotently makes that a no-op.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RoomEventsDecrypted {
    pub room_id: String,
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

/// The answer to `Command::FetchRoomEvent`; a fetch always ends in one.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RoomEventFetch {
    pub room_id: String,
    pub event_id: String,
    pub result: RoomEventFetchResult,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum RoomEventFetchResult {
    /// The event as the timeline line it would be.
    Loaded {
        activity: Box<RoomActivity>,
        /// The event's edits and reactions the client knows (the server's
        /// bundled edit, and the event cache's), each targeting it; a
        /// quote shows the latest edit.
        updates: Vec<RoomActivityUpdate>,
    },
    Failed {
        message: String,
    },
}

/// The answer to `Command::ListPublicRooms`; a request always ends in one.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PublicRoomsPage {
    pub request: ListPublicRoomsRequest,
    pub result: PublicRoomsResult,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum PublicRoomsResult {
    /// The rooms in the server's order.
    Loaded {
        rooms: Vec<PublicRoom>,
        /// Where the next page starts; `None` on the last page.
        next_batch: Option<String>,
        /// The server's estimate of the rooms matching in all.
        total_estimate: Option<u64>,
    },
    Failed {
        message: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PublicRoom {
    pub room_id: String,
    pub canonical_alias: Option<String>,
    pub name: Option<String>,
    pub topic: Option<String>,
    pub joined_members: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RoomJoined {
    pub room_id: String,
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
    /// As [`RoomUserProfile::nick`]; a member always has one.
    pub nick: String,
    /// As [`RoomUserProfile::nick_ambiguous`].
    pub nick_ambiguous: bool,
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
    /// When the read marker's event was sent (`origin_server_ts`), if the
    /// room's event cache has it: the UI places a marker on an event it
    /// does not show by this time.
    pub fully_read_timestamp_ms: Option<u64>,
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
    pub session: E2eeSessionState,
}

/// Whether this session is verified: whether your cross-signing identity
/// signs it, as the SDK says.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum E2eeSessionState {
    /// Not known yet, before the SDK has read this session's own device.
    Unknown,
    Verified,
    /// Your identity does not sign this session (yet).
    Unverified,
    /// Unverified, and your account has no cross-signing identity to
    /// verify it with.
    NoCrossSigning,
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
        styles: Vec<TextStyleSpan>,
        user_links: Vec<UserLinkSpan>,
    },
    ReactionAdded {
        reaction_event_id: String,
        sender: String,
        timestamp_ms: u64,
        key: String,
    },
    /// The target is the redacted event, whatever it is: a line, a
    /// reaction, an edit. The UI knows what it shows it as.
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
    /// The other side: the user who asked, or the user asked. The own user
    /// for a verification between your own sessions.
    pub user_id: String,
    pub direction: VerificationDirection,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum VerificationDirection {
    /// The other side's session asked; Accept or Cancel answers.
    Incoming { from_device: String },
    /// This session asked; the other side accepts. `to_device` is the one
    /// device asked, `None` for a request to all your other sessions or to
    /// another user's identity.
    Outgoing { to_device: Option<String> },
}

/// A user's sessions and how far this session trusts them, as the crypto
/// store says after a fresh `/keys/query`.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UserDevices {
    pub user_id: String,
    pub identity: UserIdentityState,
    /// This session first, then the most recently seen, then by device ID.
    pub devices: Vec<DeviceSummary>,
}

/// The user's cross-signing identity, as this session trusts it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UserIdentityState {
    /// The user has not set up cross-signing.
    NoCrossSigning,
    Verified,
    Unverified,
    /// The user was verified, and their identity has changed since.
    VerificationViolation,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DeviceSummary {
    pub device_id: String,
    pub display_name: Option<String>,
    /// Whether the crypto store trusts the device: verified directly, or
    /// cross-signed by a verified identity. For this session: whether its
    /// own cross-signing identity signs it.
    pub verified: bool,
    pub this_session: bool,
    /// When the server last saw the session; your own sessions only.
    pub last_seen_ms: Option<u64>,
}

/// Members of a joined encrypted room whose identity trust changed, from
/// the SDK's room identity stream. The first for a room lists its members
/// whose identity is not pinned or verified now (you excluded; omitted
/// when there are none); later ones are transitions.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RoomIdentityChanges {
    pub room_id: String,
    pub changes: Vec<IdentityChange>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IdentityChange {
    pub user_id: String,
    pub trust: IdentityTrust,
}

/// How far this session trusts a user's identity, as the SDK's room
/// identity stream reports it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IdentityTrust {
    Verified,
    /// The first identity seen for the user, or one the user accepted.
    /// Also reported when a user with a changed identity leaves the room.
    Pinned,
    /// The identity changed since it was pinned.
    PinViolation,
    /// The identity changed since you verified it.
    VerificationViolation,
}

/// How an identity reset that waited for browser approval ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum E2eeIdentityResetOutcome {
    /// Your new identity is on the server, and signs this session.
    Done,
    /// `CancelE2eeIdentityReset` stopped it; the server keeps your old
    /// identity.
    Cancelled,
}

/// Whom a verification asks. A device: that device, to-device. No device
/// and the own user: your other cross-signed sessions, to-device. No device
/// and another user: their cross-signing identity, in your DM with them,
/// which the SDK creates if there is none.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RequestVerificationRequest {
    pub user_id: String,
    pub device_id: Option<String>,
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
    /// The other side asked (`VerificationDirection::Incoming`).
    Requested,
    /// This session asked (`VerificationDirection::Outgoing`); waiting for
    /// the other side to accept.
    Sent,
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
    Sent {},
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
            VerificationStateSerde::Sent {} => Self::Sent,
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

/// A presence state, as the server reports it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PresenceKind {
    Online,
    Unavailable,
    /// Also what a server without presence reports for everyone.
    Offline,
}

/// The answer to `SetAway`.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AwayChanged {
    /// The away message, `None` when you are back.
    pub message: Option<String>,
    /// Your presence as the server reports it after the change: not
    /// `Unavailable` after an away when the server keeps no presence, or
    /// another of your sessions keeps you online.
    pub seen_as: PresenceKind,
}

/// A user's presence as the sync reports it: their state and status
/// message, without the times, which age as soon as they are read.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PresenceChange {
    pub user_id: String,
    pub kind: PresenceKind,
    /// `None` without a status message, or with an empty one.
    pub status_msg: Option<String>,
}

/// A user's global profile, as their homeserver gives it.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct UserProfile {
    pub user_id: String,
    /// `None` when the profile has no display name, or an empty one.
    pub display_name: Option<String>,
    /// `None` when the server does not let you see it (you share no room
    /// with the user) or has no presence endpoint.
    pub presence: Option<UserPresence>,
}

/// A user's presence, as their homeserver gives it.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UserPresence {
    pub kind: PresenceKind,
    /// `None` without a status message, or with an empty one.
    pub status_msg: Option<String>,
    /// How long ago the user was last active, in milliseconds.
    pub last_active_ago_ms: Option<u64>,
    /// Whether the server counts the user as active right now.
    pub currently_active: Option<bool>,
}

impl<'de> serde::Deserialize<'de> for UserProfile {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(serde::Deserialize)]
        #[serde(deny_unknown_fields)]
        struct WireUserProfile {
            user_id: String,
            display_name: serde_json::Value,
            presence: Option<UserPresence>,
        }

        let profile = WireUserProfile::deserialize(deserializer)?;
        let display_name = match profile.display_name {
            serde_json::Value::Null => None,
            serde_json::Value::String(name) => Some(name),
            name => {
                return Err(serde::de::Error::custom(format!(
                    "invalid display_name for {}: expected string or null, got {name}",
                    profile.user_id
                )));
            }
        };
        Ok(Self {
            user_id: profile.user_id,
            display_name,
            presence: profile.presence,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct CoreErrorEvent {
    pub operation: Operation,
    /// The room the failed request was about; `None` for account-wide
    /// operations. Always `Some` for an operation that names a room.
    pub room_id: Option<String>,
    pub message: String,
}

impl<'de> serde::Deserialize<'de> for CoreErrorEvent {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(serde::Deserialize)]
        #[serde(deny_unknown_fields)]
        struct WireCoreErrorEvent {
            operation: Operation,
            room_id: serde_json::Value,
            message: String,
        }

        let error = WireCoreErrorEvent::deserialize(deserializer)?;
        let room_id = match error.room_id {
            serde_json::Value::Null => None,
            serde_json::Value::String(room_id) => Some(room_id),
            room_id => {
                return Err(serde::de::Error::custom(format!(
                    "invalid room_id for {:?} error: expected string or null, got {room_id}",
                    error.operation
                )));
            }
        };
        Ok(Self {
            operation: error.operation,
            room_id,
            message: error.message,
        })
    }
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
    CreateRoom,
    LeaveRoom,
    KickRoomMember,
    BanRoomMember,
    UnbanRoomMember,
    InviteRoomMember,
    SetRoomMemberPowerLevel,
    SetDisplayName,
    SetPresence,
    RejectRoomInvite,
    SetRoomTopic,
    FetchRoomSettings,
    UpdateRoomSettings,
    SetRoomNotificationMode,
    HighlightWords,
    AddHighlightWord,
    RemoveHighlightWord,
    IgnoredUsers,
    IgnoreUser,
    UnignoreUser,
    FetchUserProfile,
    ListUserDevices,
    AcceptUserIdentity,
    SignOutDevice,
    RenameDevice,
    SendMessage,
    SendFile,
    EditMessage,
    SendReaction,
    RedactEvent,
    MarkRoomRead,
    SetReadMarker,
    SetTyping,
    BackfillRoomHistory,
    FetchRoomMedia,
    FetchRoomEvent,
    ListPublicRooms,
    RoomList,
    RoomReadState,
    /// The send queue itself: restoring its local echoes, or a missed
    /// update. Errors of one send are that send's operation.
    SendQueue,
    E2eeStatus,
    SetupE2eeRecovery,
    RecoverE2ee,
    ResetE2eeIdentity,
    ExportRoomKeys,
    ImportRoomKeys,
    /// Decrypting events late, as their keys come.
    RetryDecryption,
    /// Answering a verification: accept, cancel, confirm or reject.
    Verification,
    RequestVerification,
    Logout,
    Shutdown,
}

/// Why continuous sync ended. The session stays signed in either way.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum SyncStopReason {
    /// `Command::StopSync`.
    Requested,
    /// The sync loop ended with this error: the network, the server or a
    /// timeout. Starting sync again may work.
    Lost(String),
    /// The server no longer accepts this session's access token
    /// (`M_UNKNOWN_TOKEN`): it was signed out elsewhere or expired. Starting
    /// sync again cannot work; signing in again can.
    SessionEnded(String),
}

/// Why logging out would lose room keys: no key backup that a new session
/// can open would hold them all.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RoomKeysAtRisk {
    /// Recovery is not set up: a new session has no way to the backup key.
    RecoveryOff,
    /// Recovery lacks some of the account's secrets.
    RecoveryIncomplete,
    /// The server has not said yet whether recovery is set up.
    RecoveryUnknown,
    /// Key backup is off: the backup does not get this session's keys.
    BackupOff,
    /// Uploading the keys to the backup failed, for this reason.
    BackupFailed(String),
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Event {
    Started,
    /// The core finished one command: every event it emitted for it is
    /// ahead of this one, so the UI knows the core has answered everything
    /// it was sent (`CommandSender::sent`). Work the command left running
    /// (sync, the send queue, an identity reset waiting for approval)
    /// answers later, on its own. Never in a fixture or a replayed load:
    /// only the core acks its own commands.
    #[serde(skip)]
    CommandHandled,
    /// A browser sign-in waits for the user to sign in on this page, which
    /// sends the browser back to mirx; `LoggedIn` or a `Login` error
    /// follows, or `LoginCancelled`.
    SignInUrl {
        url: String,
    },
    /// The answer to `CancelLogin`: the core is signed out and idle.
    LoginCancelled,
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
    /// The sync task's first successful response: the server answers.
    /// Once per `SyncStarted`, after the response's rooms, E2EE status
    /// and presence have been sent.
    SyncConnected,
    SyncStopped(SyncStopReason),
    SyncedRooms(Vec<RoomSummary>),
    RoomActivity(Box<RoomActivity>),
    RoomActivityUpdate(RoomActivityUpdate),
    RoomHistoryBackfill(RoomHistoryBackfill),
    RoomEventsDecrypted(RoomEventsDecrypted),
    /// Keys may have come that the core cannot match to events: the key
    /// backup became available (after `RecoverE2ee`, or on another
    /// session), or the core missed key notices. Answer with
    /// `RetryDecryption` for the lines shown that could not be decrypted.
    DecryptionRetryWanted,
    RoomMedia(RoomMedia),
    RoomEvent(RoomEventFetch),
    PublicRooms(PublicRoomsPage),
    /// A join the user asked for (`JoinRoom`, `AcceptRoomInvite`,
    /// `CreateDirectRoom`) succeeded; it follows the `SyncedRooms` that
    /// lists the room.
    RoomJoined(RoomJoined),
    RoomTyping(RoomTyping),
    RoomMembers(RoomMembers),
    RoomReadState(RoomReadState),
    /// The user's highlight words, in push-rule order: with the first room
    /// snapshot and whenever the push rules change.
    HighlightWords(Vec<HighlightWord>),
    /// The user IDs on the account's ignore list (`m.ignored_user_list`),
    /// sorted: with the first room snapshot, and whenever it changes. The
    /// server stops sending ignored users' events.
    IgnoredUsers(Vec<String>),
    /// The answer to `FetchUserProfile`.
    UserProfile(UserProfile),
    /// The answer to `FetchRoomSettings`.
    RoomSettings(RoomSettings),
    AwayChanged(AwayChanged),
    /// Users' presence: from each sync that carries some, and with the
    /// session's first room snapshot for every joined room's members the
    /// store has presence for. Never empty.
    PresenceChanged(Vec<PresenceChange>),
    /// The answer to `ListUserDevices`.
    UserDevices(UserDevices),
    RoomIdentityChanges(RoomIdentityChanges),
    /// The answer to `AcceptUserIdentity`.
    UserIdentityAccepted(String),
    /// The answer to `SignOutDevice`: the server signed the session out.
    DeviceSignedOut(String),
    /// The answer to `SignOutDevice` from an OAuth 2.0 session: the session
    /// is signed out on this account page, once the user confirms there.
    /// The core is not told when that happens; `/devices` shows it.
    DeviceSignOutApproval {
        device_id: String,
        url: String,
    },
    /// The answer to `RenameDevice`.
    DeviceRenamed {
        device_id: String,
        display_name: String,
    },
    /// The session's E2EE status: its first from the server (once the
    /// session reached it), a command's answer, or a change since the last
    /// one told. A change read without asking the server keeps the last
    /// answer's `exists_on_server`.
    E2eeStatus(E2eeStatus),
    E2eeRecoverySetupSucceeded(E2eeRecoverySetupSucceeded),
    E2eeRecoverySucceeded(E2eeRecoverySucceeded),
    /// The server wants the identity reset approved on this page; the core
    /// asks again every half second for up to two minutes, and answers
    /// with `E2eeIdentityReset` or a `ResetE2eeIdentity` error.
    E2eeIdentityResetApproval {
        url: String,
    },
    /// The answer to `ResetE2eeIdentity`; an `E2eeStatus` follows.
    E2eeIdentityReset(E2eeIdentityResetOutcome),
    /// The answer to `ExportRoomKeys`: the file is written.
    RoomKeysExported {
        path: String,
    },
    /// The answer to `ImportRoomKeys`: `imported` of the file's `total`
    /// keys were new to the session.
    RoomKeysImported {
        path: String,
        imported: u64,
        total: u64,
    },
    VerificationRequest(VerificationRequest),
    VerificationState(VerificationStateEvent),
    /// The answer to `Logout` without `discard_room_keys`: logging out now
    /// would lose room keys, for this reason. The session stays signed in,
    /// syncing as before.
    LogoutWouldLoseRoomKeys(RoomKeysAtRisk),
    /// A `Logout` went past its checks: the core waits for the key backup
    /// and then the server, with sync stopped. `LoggedOut`,
    /// `LogoutWouldLoseRoomKeys` or a `Logout` error follows. Commands
    /// sent meanwhile wait for its end, but `StopSync`, which stops it:
    /// then `LogoutCancelled` follows.
    LoggingOut,
    /// The answer to a `StopSync` during a logout: the logout stopped, and
    /// the session stays signed in with sync stopped. The server may have
    /// ended the session already, if its answer was on its way.
    LogoutCancelled,
    LoggedOut,
    Stopped,
    Error(CoreErrorEvent),
}

#[cfg(test)]
mod tests {
    use super::*;

    const RED: TextColor = TextColor {
        red: 0xff,
        green: 0,
        blue: 0,
    };

    fn bold() -> TextStyle {
        TextStyle {
            bold: true,
            ..TextStyle::default()
        }
    }

    fn span(start: usize, end: usize, style: TextStyle) -> TextStyleSpan {
        TextStyleSpan { start, end, style }
    }

    #[test]
    fn room_key_file_requests_redact_the_passphrase() {
        let export = format!(
            "{:?}",
            Command::ExportRoomKeys(ExportRoomKeysRequest {
                path: r"C:\keys.txt".to_owned(),
                passphrase: "hunter2".to_owned(),
            })
        );
        assert_eq!(
            export,
            r#"ExportRoomKeys(ExportRoomKeysRequest { path: "C:\\keys.txt", passphrase: "<redacted>" })"#
        );
        let import = format!(
            "{:?}",
            Command::ImportRoomKeys(ImportRoomKeysRequest {
                path: r"C:\keys.txt".to_owned(),
                passphrase: "hunter2".to_owned(),
            })
        );
        assert!(!import.contains("hunter2"), "{import}");
        assert!(import.contains("<redacted>"), "{import}");
        let imported = Event::RoomKeysImported {
            path: r"C:\keys.txt".to_owned(),
            imported: 2,
            total: 3,
        };
        let json = serde_json::to_string(&imported).unwrap();
        assert_eq!(
            json,
            r#"{"RoomKeysImported":{"path":"C:\\keys.txt","imported":2,"total":3}}"#
        );
        assert_eq!(serde_json::from_str::<Event>(&json).unwrap(), imported);
    }

    /// Fixtures replay Log Out's refusal and the logout under way.
    #[test]
    fn logout_events_round_trip_through_serde() {
        for (event, wire) in [
            (Event::LoggingOut, r#""LoggingOut""#),
            (Event::LogoutCancelled, r#""LogoutCancelled""#),
            (
                Event::LogoutWouldLoseRoomKeys(RoomKeysAtRisk::RecoveryOff),
                r#"{"LogoutWouldLoseRoomKeys":"recovery_off"}"#,
            ),
            (
                Event::LogoutWouldLoseRoomKeys(RoomKeysAtRisk::BackupFailed("why".to_owned())),
                r#"{"LogoutWouldLoseRoomKeys":{"backup_failed":"why"}}"#,
            ),
        ] {
            assert_eq!(serde_json::to_string(&event).unwrap(), wire);
            assert_eq!(serde_json::from_str::<Event>(wire).unwrap(), event);
        }
    }

    #[test]
    fn late_decryption_events_round_trip() {
        let decrypted = Event::RoomEventsDecrypted(RoomEventsDecrypted {
            room_id: "!room:example.org".to_owned(),
            events: Vec::new(),
        });
        let json = serde_json::to_string(&decrypted).unwrap();
        assert_eq!(
            json,
            r#"{"RoomEventsDecrypted":{"room_id":"!room:example.org","events":[]}}"#
        );
        assert_eq!(serde_json::from_str::<Event>(&json).unwrap(), decrypted);
        let json = serde_json::to_string(&Event::DecryptionRetryWanted).unwrap();
        assert_eq!(json, r#""DecryptionRetryWanted""#);
        assert_eq!(
            serde_json::from_str::<Event>(&json).unwrap(),
            Event::DecryptionRetryWanted
        );
        let retry = RetryDecryptionRequest {
            room_id: "!room:example.org".to_owned(),
            event_ids: vec!["$secret".to_owned()],
        };
        let json = serde_json::to_string(&retry).unwrap();
        assert_eq!(
            json,
            r#"{"room_id":"!room:example.org","event_ids":["$secret"]}"#
        );
        assert_eq!(
            serde_json::from_str::<RetryDecryptionRequest>(&json).unwrap(),
            retry
        );
        assert!(
            serde_json::from_value::<RetryDecryptionRequest>(serde_json::json!({
                "room_id": "!room:example.org",
                "session_ids": ["SESSION"],
            }))
            .is_err()
        );
        assert!(
            serde_json::from_value::<RetryDecryptionRequest>(serde_json::json!({
                "room_id": "!room:example.org",
                "event_ids": [],
                "extra": true,
            }))
            .is_err()
        );
    }

    #[test]
    fn sign_out_requests_redact_the_password() {
        let request = SignOutDeviceRequest {
            device_id: "PH0NE".to_owned(),
            password: Some("hunter2".to_owned()),
        };
        let debug = format!("{:?}", Command::SignOutDevice(request.clone()));
        assert!(!debug.contains("hunter2"), "{debug}");
        assert_eq!(
            debug,
            r#"SignOutDevice(SignOutDeviceRequest { device_id: "PH0NE", password: Some("<redacted>") })"#
        );
        assert!(
            serde_json::from_value::<SignOutDeviceRequest>(serde_json::json!({
                "device_id": "PH0NE",
                "password": null,
                "extra": true,
            }))
            .is_err()
        );
        assert!(
            serde_json::from_value::<RenameDeviceRequest>(serde_json::json!({
                "device_id": "PH0NE",
                "display_name": "Phone",
                "extra": true,
            }))
            .is_err()
        );
    }

    #[test]
    fn identity_reset_requests_redact_the_password() {
        let request = ResetE2eeIdentityRequest {
            password: Some("hunter2".to_owned()),
        };
        let debug = format!("{:?}", Command::ResetE2eeIdentity(request.clone()));
        assert!(!debug.contains("hunter2"), "{debug}");
        assert!(debug.contains("<redacted>"), "{debug}");
        assert_eq!(
            format!("{:?}", ResetE2eeIdentityRequest { password: None }),
            "ResetE2eeIdentityRequest { password: None }"
        );
        assert_eq!(
            serde_json::from_value::<ResetE2eeIdentityRequest>(
                serde_json::to_value(&request).unwrap()
            )
            .unwrap(),
            request
        );
    }

    #[test]
    fn identity_dtos_round_trip_and_reject_unknown_fields() {
        let changes = RoomIdentityChanges {
            room_id: "!ops:example.org".to_owned(),
            changes: vec![IdentityChange {
                user_id: "@bob:example.org".to_owned(),
                trust: IdentityTrust::VerificationViolation,
            }],
        };
        let json = serde_json::json!({
            "room_id": "!ops:example.org",
            "changes": [{ "user_id": "@bob:example.org", "trust": "verification_violation" }],
        });
        assert_eq!(serde_json::to_value(&changes).unwrap(), json);
        assert_eq!(
            serde_json::from_value::<RoomIdentityChanges>(json).unwrap(),
            changes
        );
        assert!(
            serde_json::from_value::<IdentityChange>(serde_json::json!({
                "user_id": "@bob:example.org", "trust": "pinned", "extra": true,
            }))
            .is_err()
        );
        assert_eq!(
            serde_json::to_value(Event::E2eeIdentityReset(
                E2eeIdentityResetOutcome::Cancelled
            ))
            .unwrap(),
            serde_json::json!({ "E2eeIdentityReset": "cancelled" })
        );
    }

    #[test]
    fn text_colors_are_lowercase_rrggbb_on_the_wire() {
        let color = TextColor {
            red: 0x0a,
            green: 0xbc,
            blue: 0xff,
        };
        assert_eq!(
            serde_json::to_value(color).unwrap(),
            serde_json::json!("#0abcff")
        );
        assert_eq!(
            serde_json::from_value::<TextColor>(serde_json::json!("#0abcff")).unwrap(),
            color
        );
        for invalid in [
            "#0ABCFF", "0abcff", "#abc", "#0abcff0", "#0abcfg", "", "#+abcff",
        ] {
            assert!(
                serde_json::from_value::<TextColor>(serde_json::json!(invalid)).is_err(),
                "{invalid:?} must be rejected"
            );
        }
        assert!(serde_json::from_value::<TextColor>(serde_json::json!(16711680)).is_err());
    }

    #[test]
    fn text_styles_round_trip_and_reject_unknown_fields() {
        let style = TextStyle {
            italic: true,
            foreground: Some(RED),
            ..TextStyle::default()
        };
        let json = serde_json::json!({
            "bold": false,
            "italic": true,
            "underline": false,
            "strikethrough": false,
            "foreground": "#ff0000",
            "background": null,
        });
        assert_eq!(serde_json::to_value(style).unwrap(), json);
        assert_eq!(serde_json::from_value::<TextStyle>(json).unwrap(), style);
        assert!(
            serde_json::from_value::<TextStyle>(serde_json::json!({
                "bold": false, "italic": true, "underline": false, "strikethrough": false,
                "foreground": null, "background": null, "monospace": true,
            }))
            .is_err()
        );
    }

    #[test]
    fn check_text_styles_accepts_canonical_spans() {
        let italic = TextStyle {
            italic: true,
            ..TextStyle::default()
        };
        assert_eq!(check_text_styles("", &[]), Ok(()));
        assert_eq!(
            check_text_styles(
                "héllo world\nnext",
                &[
                    span(0, 6, bold()),
                    span(6, 12, italic),
                    span(13, 17, bold())
                ]
            ),
            Ok(())
        );
    }

    #[test]
    fn check_text_styles_rejects_each_invariant_violation() {
        let italic = TextStyle {
            italic: true,
            ..TextStyle::default()
        };
        let body = "héllo world\nnext";
        for (styles, expected) in [
            (vec![span(3, 3, bold())], "empty"),
            (vec![span(4, 2, bold())], "empty"),
            (vec![span(0, 18, bold())], "past the body"),
            (vec![span(0, 2, bold())], "char boundaries"),
            (vec![span(2, 4, bold())], "char boundaries"),
            (vec![span(0, 3, TextStyle::default())], "plain"),
            (vec![span(7, 14, bold())], "crosses a line"),
            (vec![span(0, 5, bold()), span(4, 6, italic)], "overlaps"),
            (vec![span(6, 9, bold()), span(0, 3, italic)], "overlaps"),
            (vec![span(0, 3, bold()), span(3, 6, bold())], "must be one"),
        ] {
            let err = check_text_styles(body, &styles).unwrap_err();
            assert!(err.contains(expected), "{styles:?}: {err}");
        }
    }

    fn user_link(start: usize, end: usize, user_id: &str) -> UserLinkSpan {
        UserLinkSpan {
            start,
            end,
            user_id: user_id.to_owned(),
        }
    }

    #[test]
    fn user_links_round_trip_and_reject_unknown_fields() {
        let json = serde_json::json!({ "start": 3, "end": 8, "user_id": "@alice:example.org" });
        let link = user_link(3, 8, "@alice:example.org");
        assert_eq!(serde_json::to_value(&link).unwrap(), json);
        assert_eq!(serde_json::from_value::<UserLinkSpan>(json).unwrap(), link);
        assert!(
            serde_json::from_value::<UserLinkSpan>(serde_json::json!({
                "start": 3, "end": 8, "user_id": "@alice:example.org", "room": true,
            }))
            .is_err()
        );
    }

    #[test]
    fn check_user_links_accepts_canonical_spans() {
        assert_eq!(check_user_links("", &[]), Ok(()));
        assert_eq!(
            check_user_links(
                "Ärger,Bob\nCarol",
                &[
                    user_link(0, 6, "@aerger:example.org"),
                    user_link(6, 7, "@comma:example.org"),
                    user_link(7, 10, "@bob:example.org"),
                    user_link(11, 16, "@carol:example.org"),
                ]
            ),
            Ok(()),
            "spans may touch"
        );
    }

    #[test]
    fn check_user_links_rejects_each_invariant_violation() {
        let body = "Ärger Bob\nCarol";
        let alice = "@alice:example.org";
        for (links, expected) in [
            (vec![user_link(3, 3, alice)], "empty"),
            (vec![user_link(4, 2, alice)], "empty"),
            (vec![user_link(0, 17, alice)], "past the body"),
            (vec![user_link(1, 6, alice)], "char boundaries"),
            (vec![user_link(7, 13, alice)], "crosses a line"),
            (
                vec![user_link(0, 7, alice), user_link(6, 10, alice)],
                "overlaps",
            ),
            (
                vec![user_link(7, 10, alice), user_link(0, 6, alice)],
                "overlaps",
            ),
            (vec![user_link(7, 10, "bob")], "invalid user ID"),
            (vec![user_link(7, 10, "")], "invalid user ID"),
        ] {
            let err = check_user_links(body, &links).unwrap_err();
            assert!(err.contains(expected), "{links:?}: {err}");
        }
    }

    fn message_json(styles: serde_json::Value) -> serde_json::Value {
        serde_json::json!({
            "event_id": "$event:example.org",
            "transaction_id": null,
            "sender": {
                "user_id": "@alice:example.org",
                "nick": null,
                "nick_ambiguous": false,
            },
            "presentation": "text",
            "body": "hi there",
            "styles": styles,
            "user_links": [],
            "send_state": { "state": "sent" },
        })
    }

    #[test]
    fn message_activities_carry_checked_styles_and_user_links() {
        let message = serde_json::from_value::<RoomMessageActivity>(message_json(
            serde_json::json!([{ "start": 0, "end": 2, "style": bold() }]),
        ))
        .unwrap();
        assert_eq!(message.styles, vec![span(0, 2, bold())]);
        assert_eq!(
            serde_json::to_value(&message).unwrap(),
            message_json(serde_json::json!([{ "start": 0, "end": 2, "style": bold() }]))
        );

        let err = serde_json::from_value::<RoomMessageActivity>(message_json(
            serde_json::json!([{ "start": 0, "end": 20, "style": bold() }]),
        ))
        .unwrap_err()
        .to_string();
        assert!(err.contains("$event:example.org"), "{err}");
        assert!(err.contains("past the body"), "{err}");

        let mut missing = message_json(serde_json::json!([]));
        missing.as_object_mut().unwrap().remove("styles");
        let err = serde_json::from_value::<RoomMessageActivity>(missing)
            .unwrap_err()
            .to_string();
        assert!(err.contains("missing field `styles`"), "{err}");

        let mut linked = message_json(serde_json::json!([]));
        linked["user_links"] =
            serde_json::json!([{ "start": 3, "end": 8, "user_id": "@bob:example.org" }]);
        let message = serde_json::from_value::<RoomMessageActivity>(linked.clone()).unwrap();
        assert_eq!(
            message.user_links,
            vec![user_link(3, 8, "@bob:example.org")]
        );
        assert_eq!(serde_json::to_value(&message).unwrap(), linked);
        linked["user_links"][0]["end"] = serde_json::json!(9);
        let err = serde_json::from_value::<RoomMessageActivity>(linked)
            .unwrap_err()
            .to_string();
        assert!(err.contains("invalid user links"), "{err}");
        assert!(err.contains("past the body"), "{err}");

        let mut missing = message_json(serde_json::json!([]));
        missing.as_object_mut().unwrap().remove("user_links");
        let err = serde_json::from_value::<RoomMessageActivity>(missing)
            .unwrap_err()
            .to_string();
        assert!(err.contains("missing field `user_links`"), "{err}");

        let mut unknown = message_json(serde_json::json!([]));
        unknown["unexpected"] = serde_json::json!(true);
        assert!(serde_json::from_value::<RoomMessageActivity>(unknown).is_err());
    }

    #[test]
    fn message_replacements_carry_checked_styles_and_user_links() {
        let update = |styles: serde_json::Value| {
            serde_json::json!({
                "room_id": "!ops:example.org",
                "target": { "type": "event", "event_id": "$original:example.org" },
                "kind": {
                    "type": "message_replaced",
                    "replacement_event_id": "$edit:example.org",
                    "sender": "@alice:example.org",
                    "timestamp_ms": 1700000001000u64,
                    "presentation": "text",
                    "body": "edited",
                    "styles": styles,
                    "user_links": [],
                },
            })
        };
        let parsed = serde_json::from_value::<RoomActivityUpdate>(update(serde_json::json!([
            { "start": 0, "end": 6, "style": bold() }
        ])))
        .unwrap();
        assert!(matches!(
            parsed.kind,
            RoomActivityUpdateKind::MessageReplaced { ref styles, .. }
                if *styles == vec![span(0, 6, bold())]
        ));

        let err = serde_json::from_value::<RoomActivityUpdate>(update(serde_json::json!([
            { "start": 0, "end": 6, "style": TextStyle::default() }
        ])))
        .unwrap_err()
        .to_string();
        assert!(err.contains("$edit:example.org"), "{err}");
        assert!(err.contains("plain"), "{err}");

        let mut linked = update(serde_json::json!([]));
        linked["kind"]["user_links"] =
            serde_json::json!([{ "start": 0, "end": 6, "user_id": "not a user" }]);
        let err = serde_json::from_value::<RoomActivityUpdate>(linked)
            .unwrap_err()
            .to_string();
        assert!(err.contains("$edit:example.org"), "{err}");
        assert!(err.contains("invalid user ID"), "{err}");

        let mut unknown = update(serde_json::json!([]));
        unknown["unexpected"] = serde_json::json!(true);
        assert!(serde_json::from_value::<RoomActivityUpdate>(unknown).is_err());
    }
}
