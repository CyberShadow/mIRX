#![cfg_attr(windows, allow(unsafe_op_in_unsafe_fn))]

use std::ffi::{OsStr, OsString};
use std::fmt;
use std::path::{Path, PathBuf};
use std::time::Duration;

use mmatrix_core::{E2eeStatus, RoomMembership};
#[cfg(windows)]
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
#[cfg(windows)]
use serde_json::Value;

#[cfg(windows)]
const FRAME_CLASS_NAME: &str = "mmatrixWin32Frame";
#[cfg(windows)]
const TIMELINE_CLASS_NAME: &str = "mmatrixWin32Timeline";
#[cfg(windows)]
const COMMAND_ID_PING: u32 = 1;
#[cfg(windows)]
const COMMAND_ID_GET_VERSION: u32 = 2;
#[cfg(windows)]
const COMMAND_ID_WAIT_IDLE: u32 = 3;
#[cfg(windows)]
const COMMAND_ID_DUMP_MODEL: u32 = 4;
#[cfg(windows)]
const COMMAND_ID_DUMP_ROOM_ACTIVITY: u32 = 5;
#[cfg(windows)]
const COMMAND_ID_LIST_ROOMS: u32 = 6;
#[cfg(windows)]
const COMMAND_ID_SELECT_ROOM: u32 = 7;
#[cfg(windows)]
const COMMAND_ID_SEND_TEXT: u32 = 8;
#[cfg(windows)]
const COMMAND_ID_SEND_FILE: u32 = 39;
#[cfg(windows)]
const COMMAND_ID_LOAD_SYNTHETIC_STATE: u32 = 9;
#[cfg(windows)]
const COMMAND_ID_DUMP_UI_STATE: u32 = 10;
#[cfg(windows)]
const COMMAND_ID_SELECT_VIEW: u32 = 11;
#[cfg(windows)]
const COMMAND_ID_LOGIN: u32 = 12;
#[cfg(windows)]
const COMMAND_ID_RESTORE_SESSION: u32 = 13;
#[cfg(windows)]
const COMMAND_ID_RESTORE_SAVED_SESSION: u32 = 14;
#[cfg(windows)]
const COMMAND_ID_START_SYNC: u32 = 15;
#[cfg(windows)]
const COMMAND_ID_LOGOUT: u32 = 16;
#[cfg(windows)]
const COMMAND_ID_DUMP_COMPOSE_TEXT: u32 = 17;
#[cfg(windows)]
const COMMAND_ID_SET_COMPOSE_TEXT: u32 = 18;
#[cfg(windows)]
const COMMAND_ID_SEND_COMPOSE_KEY: u32 = 19;
#[cfg(windows)]
const COMMAND_ID_SELECT_NAVIGATION_PATH: u32 = 20;
#[cfg(windows)]
const COMMAND_ID_DUMP_SELECTED_NAVIGATION_PATH: u32 = 21;
#[cfg(windows)]
const COMMAND_ID_APPEND_SYNTHETIC_EVENTS: u32 = 22;
#[cfg(windows)]
const COMMAND_ID_RECOVER_E2EE: u32 = 23;
#[cfg(windows)]
const COMMAND_ID_SETUP_E2EE_RECOVERY: u32 = 40;
#[cfg(windows)]
const COMMAND_ID_ACCEPT_VERIFICATION: u32 = 24;
#[cfg(windows)]
const COMMAND_ID_CANCEL_VERIFICATION: u32 = 25;
#[cfg(windows)]
const COMMAND_ID_CONFIRM_VERIFICATION_SAS: u32 = 26;
#[cfg(windows)]
const COMMAND_ID_REJECT_VERIFICATION_SAS: u32 = 27;
#[cfg(windows)]
const COMMAND_ID_FIND_ACTIVE_VIEW: u32 = 28;
#[cfg(windows)]
const COMMAND_ID_DUMP_VERIFICATION_MENU_STATE: u32 = 29;
#[cfg(windows)]
const COMMAND_ID_DUMP_ROOM_MENU_STATE: u32 = 30;
#[cfg(windows)]
const COMMAND_ID_DUMP_MESSAGE_MENU_STATE: u32 = 31;
#[cfg(windows)]
const COMMAND_ID_SELECT_TIMELINE_EVENT: u32 = 32;
#[cfg(windows)]
const COMMAND_ID_ACCEPT_INVITE_VIA_ROOM_MENU: u32 = 33;
#[cfg(windows)]
const COMMAND_ID_REJECT_INVITE_VIA_ROOM_MENU: u32 = 34;
#[cfg(windows)]
const COMMAND_ID_PART_ROOM_VIA_ROOM_MENU: u32 = 35;
#[cfg(windows)]
const COMMAND_ID_REPLY_TO_SELECTED_MESSAGE_VIA_MENU: u32 = 36;
#[cfg(windows)]
const COMMAND_ID_REDACT_SELECTED_MESSAGE_VIA_MENU: u32 = 37;
#[cfg(windows)]
const COMMAND_ID_SAVE_ATTACHMENT_VIA_MESSAGE_MENU: u32 = 38;
#[cfg(windows)]
const RESPONSE_MAPPING_CAPACITY: usize = 4 * 1024 * 1024;
#[cfg(windows)]
const RESPONSE_JSON_CAPACITY: usize = RESPONSE_MAPPING_CAPACITY - std::mem::size_of::<u32>();
const DEFAULT_LAUNCH_TIMEOUT: Duration = Duration::from_secs(10);
const DEFAULT_POLL_INTERVAL: Duration = Duration::from_millis(50);
#[cfg(windows)]
const SCREENSHOT_CAPTURE_DIR_WIN_ENV: &str = "MMATRIX_TESTKIT_CAPTURE_DIR_WIN";
#[cfg(windows)]
const SCREENSHOT_REQUEST_TIMEOUT: Duration = Duration::from_secs(30);
#[cfg(windows)]
const MDI_ID: i32 = 1002;
#[cfg(windows)]
const VIEW_CONTENT_ID: i32 = 2001;
#[cfg(windows)]
const WM_TIMELINE_GET_TOP_LINE: u32 = 0x8000 + 100;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FrameSize {
    pub width: i32,
    pub height: i32,
}

impl FrameSize {
    pub fn parse_arg(value: &str) -> Result<FrameSize, String> {
        let mut parts = value.split('x');
        let width = parts
            .next()
            .ok_or_else(|| format!("invalid frame size {value:?}: expected WIDTHxHEIGHT"))?;
        let height = parts
            .next()
            .ok_or_else(|| format!("invalid frame size {value:?}: expected WIDTHxHEIGHT"))?;
        if width.is_empty() || height.is_empty() || parts.next().is_some() {
            return Err(format!(
                "invalid frame size {value:?}: expected WIDTHxHEIGHT"
            ));
        }

        let width = width.parse::<i32>().map_err(|_| {
            format!("invalid frame size {value:?}: width must be a positive integer")
        })?;
        let height = height.parse::<i32>().map_err(|_| {
            format!("invalid frame size {value:?}: height must be a positive integer")
        })?;
        if width <= 0 || height <= 0 {
            return Err(format!(
                "invalid frame size {value:?}: dimensions must be positive"
            ));
        }

        Ok(FrameSize { width, height })
    }
}

impl fmt::Display for FrameSize {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}x{}", self.width, self.height)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LaunchOptions {
    pub startup_timeout: Duration,
    pub local_app_data: Option<PathBuf>,
    pub frame_size: Option<FrameSize>,
}

impl Default for LaunchOptions {
    fn default() -> Self {
        Self {
            startup_timeout: DEFAULT_LAUNCH_TIMEOUT,
            local_app_data: None,
            frame_size: None,
        }
    }
}

#[cfg(any(test, windows))]
fn launch_args(args: &[OsString], frame_size: Option<FrameSize>) -> Vec<OsString> {
    let mut final_args = args.to_vec();
    if !final_args
        .iter()
        .any(|arg| arg == OsStr::new("--automation") || arg == OsStr::new("--render-state"))
    {
        final_args.push(OsString::from("--automation"));
    }
    if let Some(frame_size) = frame_size {
        final_args.push(OsString::from("--frame-size"));
        final_args.push(OsString::from(frame_size.to_string()));
    }
    final_args
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StoredSession {
    pub user_id: String,
    pub device_id: String,
    pub access_token: String,
    pub refresh_token: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LoginArgs {
    pub homeserver_url: String,
    pub username: String,
    pub password: String,
    #[serde(default = "default_device_display_name")]
    pub device_display_name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RestoreSessionArgs {
    pub homeserver_url: String,
    pub session: StoredSession,
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RecoverE2eeArgs {
    pub recovery_secret: String,
}

impl fmt::Debug for RecoverE2eeArgs {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RecoverE2eeArgs")
            .field("recovery_secret", &"<redacted>")
            .finish()
    }
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SetupE2eeRecoveryArgs {
    pub passphrase: Option<String>,
    pub wait_for_backups_to_upload: bool,
}

impl fmt::Debug for SetupE2eeRecoveryArgs {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SetupE2eeRecoveryArgs")
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

fn default_device_display_name() -> String {
    "mmatrix-ui".to_owned()
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VerificationActionArgs {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub flow_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FindActiveViewArgs {
    pub query: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FindActiveViewResult {
    pub active: ViewIdentity,
    pub query: String,
    pub matched: bool,
    pub match_count: usize,
    pub current_match_index: Option<usize>,
    pub selected_text: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Model {
    pub status: String,
    pub lifecycle_state: String,
    pub rooms: Vec<Room>,
    pub last_activity: Option<RoomMessageActivityLine>,
    pub e2ee_status: Option<E2eeStatus>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_e2ee_recovery_setup_result: Option<E2eeRecoverySetupResult>,
    pub last_verification: Option<Verification>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub submitted_membership_commands: Vec<SubmittedMembershipCommand>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub submitted_direct_room_commands: Vec<SubmittedDirectRoomCommand>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub submitted_room_topic_commands: Vec<SubmittedRoomTopicCommand>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub submitted_reaction_commands: Vec<SubmittedReactionCommand>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub submitted_edit_commands: Vec<SubmittedEditCommand>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub submitted_reply_commands: Vec<SubmittedReplyCommand>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub submitted_redaction_commands: Vec<SubmittedRedactionCommand>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub submitted_history_commands: Vec<SubmittedHistoryCommand>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub submitted_verification_actions: Vec<SubmittedVerificationAction>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub submitted_recovery_commands: Vec<SubmittedRecoveryCommand>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub submitted_setup_recovery_commands: Vec<SubmittedSetupRecoveryCommand>,
    pub last_error: Option<UiError>,
    pub selected_room: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SubmittedMembershipCommand {
    pub action: MembershipCommandAction,
    pub target: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SubmittedDirectRoomCommand {
    pub user_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SubmittedRoomTopicCommand {
    pub room_id: String,
    pub topic: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SubmittedReactionCommand {
    pub room_id: String,
    pub target_event_id: String,
    pub key: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SubmittedEditCommand {
    pub room_id: String,
    pub target_event_id: String,
    pub body: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SubmittedReplyCommand {
    pub room_id: String,
    pub target_event_id: String,
    pub body: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thread: Option<SubmittedReplyThreadTarget>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SubmittedReplyThreadTarget {
    pub thread_root_event_id: String,
    pub latest_event_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SubmittedRedactionCommand {
    pub room_id: String,
    pub event_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SubmittedHistoryCommand {
    pub room_id: String,
    pub from: Option<String>,
    pub limit: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MembershipCommandAction {
    Join,
    Leave,
    AcceptInvite,
    RejectInvite,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SubmittedVerificationAction {
    pub action: VerificationActionKind,
    pub flow_id: String,
    pub user_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SubmittedRecoveryCommand {
    pub secret_redacted: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SubmittedSetupRecoveryCommand {
    pub passphrase_redacted: bool,
    pub wait_for_backups_to_upload: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct E2eeRecoverySetupResult {
    pub recovery_key_present: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VerificationActionKind {
    Accept,
    Cancel,
    ConfirmSas,
    RejectSas,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SpaceChild {
    pub room_id: String,
    pub suggested: bool,
    pub order: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RoomTag {
    pub tag: String,
    pub order: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Room {
    pub room_id: String,
    pub display_name: String,
    pub topic: Option<String>,
    pub membership: RoomMembership,
    pub encrypted: bool,
    pub is_space: bool,
    pub is_direct: bool,
    pub direct_user_ids: Vec<String>,
    pub tags: Vec<RoomTag>,
    pub space_children: Vec<SpaceChild>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RoomMessageActivityLine {
    pub room_id: String,
    pub activity_id: String,
    pub timestamp_ms: u64,
    pub event_id: Option<String>,
    pub transaction_id: Option<String>,
    pub sender: String,
    pub presentation: RoomMessagePresentationKind,
    pub body: String,
    pub send_state: RoomMessageSendState,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RoomActivity {
    pub room_id: String,
    pub activity_id: String,
    pub timestamp_ms: u64,
    #[serde(default)]
    pub relation: Option<RoomActivityRelation>,
    #[serde(default)]
    pub reactions: Vec<RoomReactionSummary>,
    pub kind: RoomActivityKind,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RoomReactionSummary {
    pub key: String,
    pub count: u64,
    pub senders: Vec<String>,
    pub reaction_event_ids: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum RoomActivityKind {
    Message(RoomMessageActivity),
    Image(Box<RoomImageActivity>),
    File(Box<RoomFileActivity>),
    Audio(Box<RoomAudioActivity>),
    Video(Box<RoomVideoActivity>),
    UnsupportedMessage(UnsupportedRoomMessageActivity),
    StateNotice(RoomStateNoticeActivity),
    Redacted(RoomRedactedActivity),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RoomMessagePresentationKind {
    Text,
    Notice,
    Emote,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RoomMessageActivity {
    pub event_id: Option<String>,
    pub transaction_id: Option<String>,
    pub sender: String,
    pub presentation: RoomMessagePresentationKind,
    pub body: String,
    pub send_state: RoomMessageSendState,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RoomImageActivity {
    pub event_id: String,
    pub sender: String,
    pub body: String,
    pub filename: Option<String>,
    pub source: RoomMediaSource,
    pub info: Option<RoomImageInfo>,
    pub media_state: Option<RoomImageMediaState>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RoomFileActivity {
    pub event_id: String,
    pub sender: String,
    pub body: String,
    pub filename: Option<String>,
    pub source: RoomMediaSource,
    pub info: Option<RoomFileInfo>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RoomAudioActivity {
    pub event_id: String,
    pub sender: String,
    pub body: String,
    pub filename: Option<String>,
    pub source: RoomMediaSource,
    pub info: Option<RoomAudioInfo>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RoomVideoActivity {
    pub event_id: String,
    pub sender: String,
    pub body: String,
    pub filename: Option<String>,
    pub source: RoomMediaSource,
    pub info: Option<RoomVideoInfo>,
    pub media_state: Option<RoomImageMediaState>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum RoomImageMediaState {
    Pending,
    Loaded { width: u32, height: u32 },
    Failed { message: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RoomEncryptedFileKey {
    pub kty: String,
    pub key_ops: Vec<String>,
    pub alg: String,
    pub k: String,
    pub ext: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RoomEncryptedFileHashes {
    pub sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BackfillRoomHistoryRequest {
    pub room_id: String,
    pub from: Option<String>,
    pub limit: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FetchRoomImageMediaRequest {
    pub room_id: String,
    pub activity_id: String,
    pub event_id: String,
    pub source: RoomMediaSource,
    pub format: RoomImageMediaFormat,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum RoomImageMediaFormat {
    Thumbnail { width: u32, height: u32 },
    File,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RoomImageInfo {
    pub mimetype: Option<String>,
    pub size: Option<u64>,
    pub width: Option<u64>,
    pub height: Option<u64>,
    pub is_animated: Option<bool>,
    pub thumbnail_source: Option<RoomMediaSource>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RoomFileInfo {
    pub mimetype: Option<String>,
    pub size: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RoomAudioInfo {
    pub mimetype: Option<String>,
    pub size: Option<u64>,
    pub duration_ms: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RoomVideoInfo {
    pub mimetype: Option<String>,
    pub size: Option<u64>,
    pub duration_ms: Option<u64>,
    pub width: Option<u64>,
    pub height: Option<u64>,
    pub thumbnail_source: Option<RoomMediaSource>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UnsupportedRoomMessageActivity {
    pub event_id: String,
    pub sender: String,
    pub msgtype: String,
    pub body: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case", deny_unknown_fields)]
pub enum RoomMessageSendState {
    Pending,
    Sent,
    Failed { message: String, recoverable: bool },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RoomStateNoticeActivity {
    pub event_id: String,
    pub sender: String,
    pub state_key: String,
    pub content: RoomStateNoticeContent,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RoomRedactedActivity {
    pub event_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RoomActivityUpdate {
    pub room_id: String,
    pub target: RoomActivityUpdateTarget,
    pub kind: RoomActivityUpdateKind,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum RoomActivityUpdateTarget {
    Transaction { transaction_id: String },
    Event { event_id: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum RoomHistoryBackfillEvent {
    Activity(Box<RoomActivity>),
    ActivityUpdate(RoomActivityUpdate),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RoomHistoryBackfill {
    pub room_id: String,
    pub start: String,
    pub end: Option<String>,
    pub events: Vec<RoomHistoryBackfillEvent>,
}

impl RoomActivity {
    pub fn contains(&self, substr: &str) -> bool {
        serde_json::to_string(self).is_ok_and(|json| json.contains(substr))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RoomImageMedia {
    pub room_id: String,
    pub activity_id: String,
    pub event_id: String,
    pub kind: RoomImageMediaKind,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum RoomImageMediaKind {
    Loaded {
        format: RoomImageMediaFormat,
        bytes: Vec<u8>,
    },
    Failed {
        format: RoomImageMediaFormat,
        message: String,
    },
}

impl RoomImageMedia {
    pub fn contains(&self, substr: &str) -> bool {
        serde_json::to_string(self).is_ok_and(|json| json.contains(substr))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Verification {
    pub flow_id: String,
    pub user_id: String,
    pub state: VerificationState,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
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

#[derive(Deserialize)]
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

impl<'de> Deserialize<'de> for VerificationState {
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VerificationEmoji {
    pub symbol: String,
    pub description: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VerificationDecimals {
    pub first: u16,
    pub second: u16,
    pub third: u16,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UiError {
    pub operation: String,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProtocolVersion {
    pub protocol_version: u32,
    pub app_version: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UiState {
    pub navigation: NavigationNode,
    pub mdi: MdiState,
    pub status_bar: StatusBarState,
    pub switchbar: SwitchbarState,
    pub active_find: Option<UiFindState>,
    pub rooms: Vec<UiRoomState>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VerificationMenuState {
    pub accept_enabled: bool,
    pub cancel_enabled: bool,
    pub confirm_sas_enabled: bool,
    pub reject_sas_enabled: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RoomMenuState {
    pub accept_invite_enabled: bool,
    pub reject_invite_enabled: bool,
    pub part_room_enabled: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MessageMenuState {
    pub reply_enabled: bool,
    pub redact_enabled: bool,
    pub save_attachment_enabled: bool,
    pub selected_event_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UiFindState {
    pub active: ViewIdentity,
    pub query: String,
    pub matched: bool,
    pub match_count: usize,
    pub current_match_index: Option<usize>,
    pub selected_text: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UiRoomState {
    pub room_id: String,
    pub display_name: String,
    pub membership: RoomMembership,
    pub encrypted: bool,
    pub is_space: bool,
    pub is_direct: bool,
    pub direct_user_ids: Vec<String>,
    pub tags: Vec<RoomTag>,
    pub space_children: Vec<SpaceChild>,
    pub has_activity: bool,
    pub read_target_event_id: Option<String>,
    pub read_state: Option<UiRoomReadState>,
    pub history_backfill: UiRoomHistoryBackfillState,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case", deny_unknown_fields)]
pub enum UiRoomHistoryBackfillState {
    Idle,
    Loading {
        from: Option<String>,
    },
    Failed {
        from: Option<String>,
        message: String,
    },
    Exhausted,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UiRoomReadState {
    pub latest_active_event_id: Option<String>,
    pub fully_read_event_id: Option<String>,
    pub marked_unread: bool,
    pub num_unread: u64,
    pub num_notifications: u64,
    pub num_mentions: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NavigationNode {
    pub text: String,
    #[serde(default)]
    pub has_activity: bool,
    pub children: Vec<NavigationNode>,
}

impl NavigationNode {
    pub fn find_by_exact_text_path<'a>(&'a self, path: &[&str]) -> Option<&'a NavigationNode> {
        find_navigation_node_by_path(self, path, |text, segment| text == segment)
    }

    pub fn find_by_substring_path<'a>(&'a self, path: &[&str]) -> Option<&'a NavigationNode> {
        find_navigation_node_by_path(self, path, |text, segment| text.contains(segment))
    }
}

fn find_navigation_node_by_path<'a, F>(
    node: &'a NavigationNode,
    path: &[&str],
    matches: F,
) -> Option<&'a NavigationNode>
where
    F: Copy + Fn(&str, &str) -> bool,
{
    let (segment, remaining) = path.split_first()?;
    if !matches(&node.text, segment) {
        return None;
    }
    if remaining.is_empty() {
        return Some(node);
    }

    let mut found = None;
    for child in &node.children {
        if let Some(match_node) = find_navigation_node_by_path(child, remaining, matches) {
            if found.is_some() {
                return None;
            }
            found = Some(match_node);
        }
    }

    found
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NavigationPathSegment {
    pub text: String,
    pub same_text_index: u32,
}

impl NavigationPathSegment {
    pub fn first(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            same_text_index: 0,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MdiState {
    pub active: ViewIdentity,
    pub views: Vec<MdiView>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StatusBarState {
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SwitchbarState {
    pub items: Vec<SwitchbarItem>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ViewIdentity {
    Status,
    Room {
        room_id: String,
    },
    Thread {
        room_id: String,
        thread_root_event_id: String,
    },
}

impl<'de> Deserialize<'de> for ViewIdentity {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(rename_all = "snake_case")]
        enum Kind {
            Status,
            Room,
            Thread,
        }

        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct StatusIdentity {
            #[serde(rename = "kind")]
            _kind: Kind,
        }

        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct RoomIdentity {
            #[serde(rename = "kind")]
            _kind: Kind,
            room_id: String,
        }

        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct ThreadIdentity {
            #[serde(rename = "kind")]
            _kind: Kind,
            room_id: String,
            thread_root_event_id: String,
        }

        let value = serde_json::Value::deserialize(deserializer)?;
        let kind = value
            .get("kind")
            .ok_or_else(|| serde::de::Error::missing_field("kind"))?
            .clone();

        match serde_json::from_value::<Kind>(kind).map_err(serde::de::Error::custom)? {
            Kind::Status => {
                serde_json::from_value::<StatusIdentity>(value)
                    .map_err(serde::de::Error::custom)?;
                Ok(Self::Status)
            }
            Kind::Room => serde_json::from_value::<RoomIdentity>(value)
                .map(|view| {
                    let RoomIdentity { _kind, room_id } = view;
                    Self::Room { room_id }
                })
                .map_err(serde::de::Error::custom),
            Kind::Thread => serde_json::from_value::<ThreadIdentity>(value)
                .map(|view| {
                    let ThreadIdentity {
                        _kind,
                        room_id,
                        thread_root_event_id,
                    } = view;
                    Self::Thread {
                        room_id,
                        thread_root_event_id,
                    }
                })
                .map_err(serde::de::Error::custom),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ComposeKey {
    Return,
    Up,
    Down,
}

impl<'de> Deserialize<'de> for ComposeKey {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(rename_all = "snake_case")]
        enum Kind {
            Return,
            Up,
            Down,
        }

        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct ComposeKeyPayload {
            #[serde(rename = "kind")]
            kind: Kind,
        }

        let value = serde_json::Value::deserialize(deserializer)?;
        if value.get("kind").is_none() {
            return Err(serde::de::Error::missing_field("kind"));
        }

        serde_json::from_value::<ComposeKeyPayload>(value)
            .map(|payload| match payload.kind {
                Kind::Return => Self::Return,
                Kind::Up => Self::Up,
                Kind::Down => Self::Down,
            })
            .map_err(serde::de::Error::custom)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RoomMemberView {
    pub user_id: String,
    pub display_name: Option<String>,
    pub label: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MdiView {
    pub identity: ViewIdentity,
    pub title: String,
    pub active: bool,
    pub lines: Vec<String>,
    pub members: Vec<RoomMemberView>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SwitchbarItem {
    pub identity: ViewIdentity,
    pub title: String,
    pub active: bool,
    pub has_activity: bool,
}

impl UiState {
    pub fn switchbar_item(&self, identity: &ViewIdentity) -> Option<&SwitchbarItem> {
        self.switchbar.item(identity)
    }
}

impl SwitchbarState {
    pub fn item(&self, identity: &ViewIdentity) -> Option<&SwitchbarItem> {
        self.items.iter().find(|item| &item.identity == identity)
    }

    pub fn active_item(&self) -> Option<&SwitchbarItem> {
        self.items.iter().find(|item| item.active)
    }
}

#[derive(Debug)]
pub enum TestkitError {
    UnsupportedPlatform(&'static str),
    Timeout(String),
    Launch(String),
    Transport(String),
    Protocol { code: String, message: String },
    InvalidResponse(String),
    Io(std::io::Error),
    Json(serde_json::Error),
}

impl fmt::Display for TestkitError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnsupportedPlatform(message) => f.write_str(message),
            Self::Timeout(message)
            | Self::Launch(message)
            | Self::Transport(message)
            | Self::InvalidResponse(message) => f.write_str(message),
            Self::Protocol { code, message } => write!(f, "protocol error {code}: {message}"),
            Self::Io(error) => write!(f, "io error: {error}"),
            Self::Json(error) => write!(f, "json error: {error}"),
        }
    }
}

impl std::error::Error for TestkitError {}

impl From<std::io::Error> for TestkitError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error)
    }
}

impl From<serde_json::Error> for TestkitError {
    fn from(error: serde_json::Error) -> Self {
        Self::Json(error)
    }
}

#[derive(Debug)]
pub struct MmatrixApp {
    inner: imp::AppHandle,
}

impl MmatrixApp {
    pub fn find(timeout: Duration) -> Result<Self, TestkitError> {
        imp::AppHandle::find(timeout).map(|inner| Self { inner })
    }

    pub fn find_by_pid(pid: u32, timeout: Duration) -> Result<Self, TestkitError> {
        imp::AppHandle::find_by_pid(pid, timeout).map(|inner| Self { inner })
    }

    pub fn launch(
        exe_path: impl AsRef<Path>,
        args: &[impl AsRef<OsStr>],
    ) -> Result<Self, TestkitError> {
        Self::launch_with_options(exe_path, args, LaunchOptions::default())
    }

    pub fn launch_with_options(
        exe_path: impl AsRef<Path>,
        args: &[impl AsRef<OsStr>],
        options: LaunchOptions,
    ) -> Result<Self, TestkitError> {
        let owned_args = args
            .iter()
            .map(|arg| arg.as_ref().to_os_string())
            .collect::<Vec<_>>();
        imp::AppHandle::launch(exe_path.as_ref(), &owned_args, options).map(|inner| Self { inner })
    }

    pub fn ping(&self) -> Result<(), TestkitError> {
        self.inner.ping()
    }

    pub fn get_version(&self) -> Result<ProtocolVersion, TestkitError> {
        self.inner.get_version()
    }

    pub fn pid(&self) -> u32 {
        self.inner.pid()
    }

    pub fn wait_idle(&self, timeout: Duration) -> Result<(), TestkitError> {
        self.inner.wait_idle(timeout)
    }

    pub fn dump_model(&self) -> Result<Model, TestkitError> {
        self.inner.dump_model()
    }

    pub fn dump_ui_state(&self) -> Result<UiState, TestkitError> {
        self.inner.dump_ui_state()
    }

    pub fn dump_verification_menu_state(&self) -> Result<VerificationMenuState, TestkitError> {
        self.inner.dump_verification_menu_state()
    }

    pub fn dump_room_menu_state(&self) -> Result<RoomMenuState, TestkitError> {
        self.inner.dump_room_menu_state()
    }

    pub fn dump_message_menu_state(&self) -> Result<MessageMenuState, TestkitError> {
        self.inner.dump_message_menu_state()
    }

    pub fn select_timeline_event(&self, event_id: &str) -> Result<(), TestkitError> {
        self.inner.select_timeline_event(event_id)
    }

    pub fn accept_invite_via_room_menu(&self) -> Result<(), TestkitError> {
        self.inner.accept_invite_via_room_menu()
    }

    pub fn reject_invite_via_room_menu(&self) -> Result<(), TestkitError> {
        self.inner.reject_invite_via_room_menu()
    }

    pub fn part_room_via_room_menu(&self) -> Result<(), TestkitError> {
        self.inner.part_room_via_room_menu()
    }

    pub fn reply_to_selected_message_via_menu(&self) -> Result<(), TestkitError> {
        self.inner.reply_to_selected_message_via_menu()
    }

    pub fn redact_selected_message_via_menu(&self) -> Result<(), TestkitError> {
        self.inner.redact_selected_message_via_menu()
    }

    pub fn save_attachment_via_message_menu(&self) -> Result<(), TestkitError> {
        self.inner.save_attachment_via_message_menu()
    }

    pub fn room_activity(&self, room_id: Option<&str>) -> Result<Vec<RoomActivity>, TestkitError> {
        self.inner.room_activity(room_id)
    }

    pub fn list_rooms(&self) -> Result<Vec<Room>, TestkitError> {
        self.inner.list_rooms()
    }

    pub fn select_room(&self, room_id: &str) -> Result<(), TestkitError> {
        self.inner.select_room(room_id)
    }

    pub fn select_view(&self, view: ViewIdentity) -> Result<(), TestkitError> {
        self.inner.select_view(view)
    }

    pub fn select_navigation_path(
        &self,
        path: &[NavigationPathSegment],
    ) -> Result<(), TestkitError> {
        self.inner.select_navigation_path(path)
    }

    pub fn select_navigation_labels(&self, path: &[&str]) -> Result<(), TestkitError> {
        self.inner.select_navigation_labels(path)
    }

    pub fn selected_navigation_path(&self) -> Result<Vec<NavigationPathSegment>, TestkitError> {
        self.inner.selected_navigation_path()
    }

    pub fn select_status_view(&self) -> Result<(), TestkitError> {
        self.inner.select_status_view()
    }

    pub fn find_active_view(&self, query: &str) -> Result<FindActiveViewResult, TestkitError> {
        self.inner.find_active_view(query)
    }

    pub fn trigger_active_timeline_history_backfill(&self) -> Result<(), TestkitError> {
        self.inner.trigger_active_timeline_history_backfill()
    }

    pub fn send_text(&self, room_id: Option<&str>, body: &str) -> Result<(), TestkitError> {
        self.send_text_target(room_id, body).map(|_| ())
    }

    pub fn send_text_target(
        &self,
        room_id: Option<&str>,
        body: &str,
    ) -> Result<String, TestkitError> {
        self.inner.send_text_target(room_id, body)
    }

    pub fn send_file(&self, path: &str) -> Result<String, TestkitError> {
        self.send_file_target(None, path, None, None)
    }

    pub fn send_file_target(
        &self,
        room_id: Option<&str>,
        path: &str,
        filename: Option<&str>,
        content_type: Option<&str>,
    ) -> Result<String, TestkitError> {
        self.inner
            .send_file_target(room_id, path, filename, content_type)
    }

    pub fn compose_text(&self) -> Result<String, TestkitError> {
        self.inner.compose_text()
    }

    pub fn set_compose_text(&self, text: &str) -> Result<(), TestkitError> {
        self.inner.set_compose_text(text)
    }

    pub fn send_compose_key(&self, key: ComposeKey) -> Result<(), TestkitError> {
        self.inner.send_compose_key(key)
    }

    #[cfg(windows)]
    pub fn automation_frame_hwnd(&self) -> isize {
        self.inner.frame_hwnd() as isize
    }

    pub fn login(
        &self,
        homeserver_url: &str,
        username: &str,
        password: &str,
        device_display_name: Option<&str>,
    ) -> Result<(), TestkitError> {
        self.inner.login(LoginArgs {
            homeserver_url: homeserver_url.to_owned(),
            username: username.to_owned(),
            password: password.to_owned(),
            device_display_name: device_display_name.unwrap_or("mmatrix-ui").to_owned(),
        })
    }

    pub fn restore_session(
        &self,
        homeserver_url: &str,
        session: StoredSession,
    ) -> Result<(), TestkitError> {
        self.inner.restore_session(RestoreSessionArgs {
            homeserver_url: homeserver_url.to_owned(),
            session,
        })
    }

    pub fn restore_saved_session(&self) -> Result<(), TestkitError> {
        self.inner.restore_saved_session()
    }

    pub fn start_sync(&self) -> Result<(), TestkitError> {
        self.inner.start_sync()
    }

    pub fn logout(&self) -> Result<(), TestkitError> {
        self.inner.logout()
    }

    pub fn recover_e2ee(&self, recovery_secret: &str) -> Result<(), TestkitError> {
        self.inner.recover_e2ee(RecoverE2eeArgs {
            recovery_secret: recovery_secret.to_owned(),
        })
    }

    pub fn setup_e2ee_recovery(
        &self,
        passphrase: Option<&str>,
        wait_for_backups_to_upload: bool,
    ) -> Result<(), TestkitError> {
        self.inner.setup_e2ee_recovery(SetupE2eeRecoveryArgs {
            passphrase: passphrase.map(str::to_owned),
            wait_for_backups_to_upload,
        })
    }

    pub fn accept_verification(&self) -> Result<(), TestkitError> {
        self.inner
            .accept_verification(VerificationActionArgs::default())
    }

    pub fn accept_verification_flow(
        &self,
        flow_id: impl Into<String>,
        user_id: impl Into<String>,
    ) -> Result<(), TestkitError> {
        self.inner.accept_verification(VerificationActionArgs {
            flow_id: Some(flow_id.into()),
            user_id: Some(user_id.into()),
        })
    }

    pub fn cancel_verification(&self) -> Result<(), TestkitError> {
        self.inner
            .cancel_verification(VerificationActionArgs::default())
    }

    pub fn cancel_verification_flow(
        &self,
        flow_id: impl Into<String>,
        user_id: impl Into<String>,
    ) -> Result<(), TestkitError> {
        self.inner.cancel_verification(VerificationActionArgs {
            flow_id: Some(flow_id.into()),
            user_id: Some(user_id.into()),
        })
    }

    pub fn confirm_verification_sas(&self) -> Result<(), TestkitError> {
        self.inner
            .confirm_verification_sas(VerificationActionArgs::default())
    }

    pub fn confirm_verification_sas_flow(
        &self,
        flow_id: impl Into<String>,
        user_id: impl Into<String>,
    ) -> Result<(), TestkitError> {
        self.inner.confirm_verification_sas(VerificationActionArgs {
            flow_id: Some(flow_id.into()),
            user_id: Some(user_id.into()),
        })
    }

    pub fn reject_verification_sas(&self) -> Result<(), TestkitError> {
        self.inner
            .reject_verification_sas(VerificationActionArgs::default())
    }

    pub fn reject_verification_sas_flow(
        &self,
        flow_id: impl Into<String>,
        user_id: impl Into<String>,
    ) -> Result<(), TestkitError> {
        self.inner.reject_verification_sas(VerificationActionArgs {
            flow_id: Some(flow_id.into()),
            user_id: Some(user_id.into()),
        })
    }

    pub fn load_events(&self, events_json: &str) -> Result<(), TestkitError> {
        self.inner.load_events(events_json)
    }

    pub fn load_events_from_file(&self, path: impl AsRef<Path>) -> Result<(), TestkitError> {
        self.inner.load_events_from_file(path.as_ref())
    }

    pub fn append_synthetic_events(&self, events_json: &str) -> Result<(), TestkitError> {
        self.inner.append_synthetic_events(events_json)
    }

    pub fn wait_for_room_activity_contains(
        &self,
        room_id: Option<&str>,
        substr: &str,
        timeout: Duration,
    ) -> Result<(), TestkitError> {
        self.inner
            .wait_for_room_activity_contains(room_id, substr, timeout)
    }

    pub fn screenshot(&self, path: impl AsRef<Path>) -> Result<(), TestkitError> {
        self.inner.screenshot(path.as_ref())
    }

    pub fn wait_for_lifecycle_state(
        &self,
        expected: &str,
        timeout: Duration,
    ) -> Result<(), TestkitError> {
        let deadline = std::time::Instant::now() + timeout;
        loop {
            let remaining = deadline.saturating_duration_since(std::time::Instant::now());
            self.wait_idle(remaining)?;
            if self.dump_model()?.lifecycle_state == expected {
                return Ok(());
            }
            if std::time::Instant::now() >= deadline {
                return Err(TestkitError::Timeout(format!(
                    "lifecycle state never reached {expected:?}"
                )));
            }
            std::thread::sleep(DEFAULT_POLL_INTERVAL);
        }
    }

    pub fn wait_for_min_rooms(
        &self,
        min_rooms: usize,
        timeout: Duration,
    ) -> Result<(), TestkitError> {
        let deadline = std::time::Instant::now() + timeout;
        loop {
            let remaining = deadline.saturating_duration_since(std::time::Instant::now());
            self.wait_idle(remaining)?;
            if self.list_rooms()?.len() >= min_rooms {
                return Ok(());
            }
            if std::time::Instant::now() >= deadline {
                return Err(TestkitError::Timeout(format!(
                    "room list never reached {min_rooms} room(s)"
                )));
            }
            std::thread::sleep(DEFAULT_POLL_INTERVAL);
        }
    }
}

#[cfg(windows)]
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct EmptyArgs {}

#[cfg(windows)]
#[derive(Serialize)]
#[serde(deny_unknown_fields)]
struct SelectTimelineEventArgs<'a> {
    event_id: &'a str,
}

#[cfg(windows)]
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct RequestEnvelope<'a, T> {
    id: &'a str,
    args: T,
}

#[cfg(windows)]
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct WaitIdleArgs {
    timeout_ms: u64,
}

#[cfg(windows)]
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct DumpRoomActivityArgs<'a> {
    room_id: Option<&'a str>,
}

#[cfg(windows)]
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct SelectRoomArgs<'a> {
    room_id: &'a str,
}

#[cfg(windows)]
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct SendTextArgs<'a> {
    room_id: Option<&'a str>,
    body: &'a str,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SendFileArgs<'a> {
    pub room_id: Option<&'a str>,
    pub path: &'a str,
    pub filename: Option<&'a str>,
    pub content_type: Option<&'a str>,
}

#[cfg(windows)]
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct SetComposeTextArgs<'a> {
    text: &'a str,
}

#[cfg(windows)]
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct SendComposeKeyArgs {
    key: ComposeKey,
}

#[cfg(windows)]
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct LoadEventsArgs {
    events: Value,
}

#[cfg(windows)]
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
struct SelectViewArgs {
    view: ViewIdentity,
}

#[cfg(windows)]
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
struct FindActiveViewRequestArgs<'a> {
    query: &'a str,
}

#[cfg(windows)]
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
struct SelectNavigationPathArgs<'a> {
    path: &'a [NavigationPathSegment],
}

#[cfg(windows)]
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawResponseEnvelope<T> {
    id: Option<String>,
    ok: bool,
    json: Option<T>,
    err: Option<ProtocolFailure>,
}

#[cfg(windows)]
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
struct ProtocolFailure {
    code: String,
    message: String,
}

#[cfg(windows)]
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
struct PongResponse {
    pong: bool,
}

#[cfg(windows)]
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
struct WaitIdleResponse {
    idle: bool,
    event_generation: u64,
    pending_core_events: u64,
    synthetic_load_in_progress: bool,
}

#[cfg(windows)]
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
struct DumpRoomActivityResponse {
    room_id: String,
    entries: Vec<RoomActivity>,
}

#[cfg(windows)]
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
struct ListRoomsResponse {
    rooms: Vec<Room>,
}

#[cfg(windows)]
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
struct SelectRoomResponse {
    selected_room: String,
}

#[cfg(windows)]
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
struct SendTextResponse {
    accepted: bool,
    room_id: String,
}

#[cfg(windows)]
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
struct ComposeTextResponse {
    text: String,
}

#[cfg(windows)]
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
struct AcceptedResponse {
    accepted: bool,
}

#[cfg(windows)]
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
struct SelectedNavigationPathResponse {
    path: Vec<NavigationPathSegment>,
}

#[cfg(windows)]
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
struct LoadEventsResponse {
    loaded: bool,
    rooms: usize,
    timeline_events: usize,
}

#[cfg(windows)]
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
struct SelectViewResponse {
    active: ViewIdentity,
}

#[cfg(not(windows))]
mod imp {
    use super::*;

    #[derive(Debug)]
    pub(crate) struct AppHandle;

    impl AppHandle {
        pub(crate) fn find(_timeout: Duration) -> Result<Self, TestkitError> {
            Err(TestkitError::UnsupportedPlatform(
                "mmatrix-testkit requires the Windows target",
            ))
        }

        pub(crate) fn find_by_pid(_pid: u32, _timeout: Duration) -> Result<Self, TestkitError> {
            Err(TestkitError::UnsupportedPlatform(
                "mmatrix-testkit requires the Windows target",
            ))
        }

        pub(crate) fn launch(
            _exe_path: &Path,
            _args: &[OsString],
            _options: LaunchOptions,
        ) -> Result<Self, TestkitError> {
            Err(TestkitError::UnsupportedPlatform(
                "mmatrix-testkit requires the Windows target",
            ))
        }

        pub(crate) fn ping(&self) -> Result<(), TestkitError> {
            Err(TestkitError::UnsupportedPlatform(
                "mmatrix-testkit requires the Windows target",
            ))
        }

        pub(crate) fn get_version(&self) -> Result<ProtocolVersion, TestkitError> {
            Err(TestkitError::UnsupportedPlatform(
                "mmatrix-testkit requires the Windows target",
            ))
        }

        pub(crate) fn pid(&self) -> u32 {
            0
        }

        pub(crate) fn wait_idle(&self, _timeout: Duration) -> Result<(), TestkitError> {
            Err(TestkitError::UnsupportedPlatform(
                "mmatrix-testkit requires the Windows target",
            ))
        }

        pub(crate) fn dump_model(&self) -> Result<Model, TestkitError> {
            Err(TestkitError::UnsupportedPlatform(
                "mmatrix-testkit requires the Windows target",
            ))
        }

        pub(crate) fn dump_ui_state(&self) -> Result<UiState, TestkitError> {
            Err(TestkitError::UnsupportedPlatform(
                "mmatrix-testkit requires the Windows target",
            ))
        }

        pub(crate) fn dump_verification_menu_state(
            &self,
        ) -> Result<VerificationMenuState, TestkitError> {
            Err(TestkitError::UnsupportedPlatform(
                "mmatrix-testkit requires the Windows target",
            ))
        }

        pub(crate) fn dump_room_menu_state(&self) -> Result<RoomMenuState, TestkitError> {
            Err(TestkitError::UnsupportedPlatform(
                "mmatrix-testkit requires the Windows target",
            ))
        }

        pub(crate) fn dump_message_menu_state(&self) -> Result<MessageMenuState, TestkitError> {
            Err(TestkitError::UnsupportedPlatform(
                "mmatrix-testkit requires the Windows target",
            ))
        }

        pub(crate) fn select_timeline_event(&self, _event_id: &str) -> Result<(), TestkitError> {
            Err(TestkitError::UnsupportedPlatform(
                "mmatrix-testkit requires the Windows target",
            ))
        }

        pub(crate) fn accept_invite_via_room_menu(&self) -> Result<(), TestkitError> {
            Err(TestkitError::UnsupportedPlatform(
                "mmatrix-testkit requires the Windows target",
            ))
        }

        pub(crate) fn reject_invite_via_room_menu(&self) -> Result<(), TestkitError> {
            Err(TestkitError::UnsupportedPlatform(
                "mmatrix-testkit requires the Windows target",
            ))
        }

        pub(crate) fn part_room_via_room_menu(&self) -> Result<(), TestkitError> {
            Err(TestkitError::UnsupportedPlatform(
                "mmatrix-testkit requires the Windows target",
            ))
        }

        pub(crate) fn reply_to_selected_message_via_menu(&self) -> Result<(), TestkitError> {
            Err(TestkitError::UnsupportedPlatform(
                "mmatrix-testkit requires the Windows target",
            ))
        }

        pub(crate) fn redact_selected_message_via_menu(&self) -> Result<(), TestkitError> {
            Err(TestkitError::UnsupportedPlatform(
                "mmatrix-testkit requires the Windows target",
            ))
        }

        pub(crate) fn save_attachment_via_message_menu(&self) -> Result<(), TestkitError> {
            Err(TestkitError::UnsupportedPlatform(
                "mmatrix-testkit requires the Windows target",
            ))
        }

        pub(crate) fn room_activity(
            &self,
            _room_id: Option<&str>,
        ) -> Result<Vec<RoomActivity>, TestkitError> {
            Err(TestkitError::UnsupportedPlatform(
                "mmatrix-testkit requires the Windows target",
            ))
        }

        pub(crate) fn list_rooms(&self) -> Result<Vec<Room>, TestkitError> {
            Err(TestkitError::UnsupportedPlatform(
                "mmatrix-testkit requires the Windows target",
            ))
        }

        pub(crate) fn select_room(&self, _room_id: &str) -> Result<(), TestkitError> {
            Err(TestkitError::UnsupportedPlatform(
                "mmatrix-testkit requires the Windows target",
            ))
        }

        pub(crate) fn select_view(&self, _view: ViewIdentity) -> Result<(), TestkitError> {
            Err(TestkitError::UnsupportedPlatform(
                "mmatrix-testkit requires the Windows target",
            ))
        }

        pub(crate) fn select_navigation_path(
            &self,
            _path: &[NavigationPathSegment],
        ) -> Result<(), TestkitError> {
            Err(TestkitError::UnsupportedPlatform(
                "mmatrix-testkit requires the Windows target",
            ))
        }

        pub(crate) fn select_navigation_labels(&self, _path: &[&str]) -> Result<(), TestkitError> {
            Err(TestkitError::UnsupportedPlatform(
                "mmatrix-testkit requires the Windows target",
            ))
        }

        pub(crate) fn selected_navigation_path(
            &self,
        ) -> Result<Vec<NavigationPathSegment>, TestkitError> {
            Err(TestkitError::UnsupportedPlatform(
                "mmatrix-testkit requires the Windows target",
            ))
        }

        pub(crate) fn select_status_view(&self) -> Result<(), TestkitError> {
            Err(TestkitError::UnsupportedPlatform(
                "mmatrix-testkit requires the Windows target",
            ))
        }

        pub(crate) fn find_active_view(
            &self,
            _query: &str,
        ) -> Result<FindActiveViewResult, TestkitError> {
            Err(TestkitError::UnsupportedPlatform(
                "mmatrix-testkit requires the Windows target",
            ))
        }

        pub(crate) fn trigger_active_timeline_history_backfill(&self) -> Result<(), TestkitError> {
            Err(TestkitError::UnsupportedPlatform(
                "mmatrix-testkit requires the Windows target",
            ))
        }

        pub(crate) fn send_text_target(
            &self,
            _room_id: Option<&str>,
            _body: &str,
        ) -> Result<String, TestkitError> {
            Err(TestkitError::UnsupportedPlatform(
                "mmatrix-testkit requires the Windows target",
            ))
        }

        pub(crate) fn send_file_target(
            &self,
            _room_id: Option<&str>,
            _path: &str,
            _filename: Option<&str>,
            _content_type: Option<&str>,
        ) -> Result<String, TestkitError> {
            Err(TestkitError::UnsupportedPlatform(
                "mmatrix-testkit requires the Windows target",
            ))
        }

        pub(crate) fn compose_text(&self) -> Result<String, TestkitError> {
            Err(TestkitError::UnsupportedPlatform(
                "mmatrix-testkit requires the Windows target",
            ))
        }

        pub(crate) fn set_compose_text(&self, _text: &str) -> Result<(), TestkitError> {
            Err(TestkitError::UnsupportedPlatform(
                "mmatrix-testkit requires the Windows target",
            ))
        }

        pub(crate) fn send_compose_key(&self, _key: ComposeKey) -> Result<(), TestkitError> {
            Err(TestkitError::UnsupportedPlatform(
                "mmatrix-testkit requires the Windows target",
            ))
        }

        pub(crate) fn login(&self, _args: LoginArgs) -> Result<(), TestkitError> {
            Err(TestkitError::UnsupportedPlatform(
                "mmatrix-testkit requires the Windows target",
            ))
        }

        pub(crate) fn restore_session(
            &self,
            _args: RestoreSessionArgs,
        ) -> Result<(), TestkitError> {
            Err(TestkitError::UnsupportedPlatform(
                "mmatrix-testkit requires the Windows target",
            ))
        }

        pub(crate) fn restore_saved_session(&self) -> Result<(), TestkitError> {
            Err(TestkitError::UnsupportedPlatform(
                "mmatrix-testkit requires the Windows target",
            ))
        }

        pub(crate) fn start_sync(&self) -> Result<(), TestkitError> {
            Err(TestkitError::UnsupportedPlatform(
                "mmatrix-testkit requires the Windows target",
            ))
        }

        pub(crate) fn logout(&self) -> Result<(), TestkitError> {
            Err(TestkitError::UnsupportedPlatform(
                "mmatrix-testkit requires the Windows target",
            ))
        }

        pub(crate) fn recover_e2ee(&self, _args: RecoverE2eeArgs) -> Result<(), TestkitError> {
            Err(TestkitError::UnsupportedPlatform(
                "mmatrix-testkit requires the Windows target",
            ))
        }

        pub(crate) fn setup_e2ee_recovery(
            &self,
            _args: SetupE2eeRecoveryArgs,
        ) -> Result<(), TestkitError> {
            Err(TestkitError::UnsupportedPlatform(
                "mmatrix-testkit requires the Windows target",
            ))
        }

        pub(crate) fn accept_verification(
            &self,
            _args: VerificationActionArgs,
        ) -> Result<(), TestkitError> {
            Err(TestkitError::UnsupportedPlatform(
                "mmatrix-testkit requires the Windows target",
            ))
        }

        pub(crate) fn cancel_verification(
            &self,
            _args: VerificationActionArgs,
        ) -> Result<(), TestkitError> {
            Err(TestkitError::UnsupportedPlatform(
                "mmatrix-testkit requires the Windows target",
            ))
        }

        pub(crate) fn confirm_verification_sas(
            &self,
            _args: VerificationActionArgs,
        ) -> Result<(), TestkitError> {
            Err(TestkitError::UnsupportedPlatform(
                "mmatrix-testkit requires the Windows target",
            ))
        }

        pub(crate) fn reject_verification_sas(
            &self,
            _args: VerificationActionArgs,
        ) -> Result<(), TestkitError> {
            Err(TestkitError::UnsupportedPlatform(
                "mmatrix-testkit requires the Windows target",
            ))
        }

        pub(crate) fn load_events(&self, _events_json: &str) -> Result<(), TestkitError> {
            Err(TestkitError::UnsupportedPlatform(
                "mmatrix-testkit requires the Windows target",
            ))
        }

        pub(crate) fn load_events_from_file(&self, _path: &Path) -> Result<(), TestkitError> {
            Err(TestkitError::UnsupportedPlatform(
                "mmatrix-testkit requires the Windows target",
            ))
        }

        pub(crate) fn append_synthetic_events(
            &self,
            _events_json: &str,
        ) -> Result<(), TestkitError> {
            Err(TestkitError::UnsupportedPlatform(
                "mmatrix-testkit requires the Windows target",
            ))
        }

        pub(crate) fn wait_for_room_activity_contains(
            &self,
            _room_id: Option<&str>,
            _substr: &str,
            _timeout: Duration,
        ) -> Result<(), TestkitError> {
            Err(TestkitError::UnsupportedPlatform(
                "mmatrix-testkit requires the Windows target",
            ))
        }

        pub(crate) fn screenshot(&self, _path: &Path) -> Result<(), TestkitError> {
            Err(TestkitError::UnsupportedPlatform(
                "mmatrix-testkit requires the Windows target",
            ))
        }
    }
}

#[cfg(windows)]
mod imp {
    use std::fs;
    use std::mem::size_of;
    use std::os::windows::ffi::OsStrExt;
    use std::process::{Child, Command};
    use std::ptr::null_mut;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::thread;
    use std::time::Instant;

    use windows_sys::Win32::Foundation::{CloseHandle, HANDLE, HWND, LPARAM};
    use windows_sys::Win32::System::DataExchange::COPYDATASTRUCT;
    use windows_sys::Win32::System::Memory::{
        FILE_MAP_READ, MEMORY_MAPPED_VIEW_ADDRESS, MapViewOfFile, OpenFileMappingW, UnmapViewOfFile,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        EnumWindows, FindWindowExW, GetClassNameW, GetDlgItem, GetWindowThreadProcessId,
        SB_PAGEDOWN, SB_PAGEUP, SendMessageW, WM_COPYDATA, WM_GETTEXT, WM_GETTEXTLENGTH,
        WM_MDIGETACTIVE, WM_VSCROLL,
    };

    use super::*;

    static REQUEST_COUNTER: AtomicU64 = AtomicU64::new(1);

    #[derive(Debug)]
    pub(crate) struct AppHandle {
        hwnd: HWND,
        pid: u32,
        response_mapping: HANDLE,
        response_view: *mut u8,
        owned_child: Option<Child>,
    }

    impl Drop for AppHandle {
        fn drop(&mut self) {
            unsafe {
                if !self.response_view.is_null() {
                    UnmapViewOfFile(MEMORY_MAPPED_VIEW_ADDRESS {
                        Value: self.response_view.cast(),
                    });
                }
                if !self.response_mapping.is_null() {
                    CloseHandle(self.response_mapping);
                }
            }

            if let Some(child) = self.owned_child.as_mut() {
                match child.try_wait() {
                    Ok(Some(_)) => {}
                    Ok(None) => {
                        let _ = child.kill();
                        let _ = child.wait();
                    }
                    Err(_) => {
                        let _ = child.kill();
                        let _ = child.wait();
                    }
                }
            }
        }
    }

    impl AppHandle {
        pub(crate) fn frame_hwnd(&self) -> HWND {
            self.hwnd
        }

        pub(crate) fn find(timeout: Duration) -> Result<Self, TestkitError> {
            Self::find_by_pid_internal(None, timeout)
        }

        pub(crate) fn find_by_pid(pid: u32, timeout: Duration) -> Result<Self, TestkitError> {
            Self::find_by_pid_internal(Some(pid), timeout)
        }

        fn find_by_pid_internal(
            pid_filter: Option<u32>,
            timeout: Duration,
        ) -> Result<Self, TestkitError> {
            let deadline = Instant::now() + timeout;
            let mut last_error = None;
            while Instant::now() < deadline {
                match attach_to_window(pid_filter) {
                    Ok(app) => match app.ping() {
                        Ok(()) => return Ok(app),
                        Err(error) => last_error = Some(error),
                    },
                    Err(error) => last_error = Some(error),
                }
                thread::sleep(DEFAULT_POLL_INTERVAL);
            }

            Err(TestkitError::Timeout(match last_error {
                Some(error) => format!("timed out waiting for automation window: {error}"),
                None => "timed out waiting for automation window".to_owned(),
            }))
        }

        pub(crate) fn pid(&self) -> u32 {
            self.pid
        }

        pub(crate) fn launch(
            exe_path: &Path,
            args: &[OsString],
            options: LaunchOptions,
        ) -> Result<Self, TestkitError> {
            let final_args = launch_args(args, options.frame_size);
            let mut command = Command::new(exe_path);
            command.args(&final_args);
            if let Some(local_app_data) = options.local_app_data.as_ref() {
                command.env("LOCALAPPDATA", local_app_data);
            }
            let mut child = command.spawn().map_err(|error| {
                TestkitError::Launch(format!("failed to launch {}: {error}", exe_path.display()))
            })?;
            let child_pid = child.id();
            let deadline = Instant::now() + options.startup_timeout;
            let mut last_error = None;

            while Instant::now() < deadline {
                match attach_to_window(Some(child_pid)) {
                    Ok(mut app) => match app.ping() {
                        Ok(()) => {
                            app.owned_child = Some(child);
                            return Ok(app);
                        }
                        Err(error) => last_error = Some(error),
                    },
                    Err(error) => last_error = Some(error),
                }

                if let Some(status) = child.try_wait()? {
                    return Err(TestkitError::Launch(format!(
                        "mmatrix-ui exited before automation was ready with status {status}"
                    )));
                }

                thread::sleep(DEFAULT_POLL_INTERVAL);
            }

            let _ = child.kill();
            let _ = child.wait();
            Err(TestkitError::Timeout(match last_error {
                Some(error) => {
                    format!("timed out waiting for launched mmatrix-ui automation window: {error}")
                }
                None => "timed out waiting for launched mmatrix-ui automation window".to_owned(),
            }))
        }

        pub(crate) fn ping(&self) -> Result<(), TestkitError> {
            let response: PongResponse = self.send_command(COMMAND_ID_PING, EmptyArgs {})?;
            if response.pong {
                Ok(())
            } else {
                Err(TestkitError::InvalidResponse(
                    "PING response did not contain pong=true".to_owned(),
                ))
            }
        }

        pub(crate) fn get_version(&self) -> Result<ProtocolVersion, TestkitError> {
            self.send_command(COMMAND_ID_GET_VERSION, EmptyArgs {})
        }

        pub(crate) fn wait_idle(&self, timeout: Duration) -> Result<(), TestkitError> {
            let deadline = Instant::now() + timeout;
            loop {
                let remaining = deadline.saturating_duration_since(Instant::now());
                let response: WaitIdleResponse = self.send_command(
                    COMMAND_ID_WAIT_IDLE,
                    WaitIdleArgs {
                        timeout_ms: duration_millis_u64(remaining),
                    },
                )?;
                if response.idle {
                    return Ok(());
                }
                if Instant::now() >= deadline {
                    return Err(TestkitError::Timeout(format!(
                        "WAIT_IDLE did not report idle for pid {} hwnd 0x{:x}",
                        self.pid, self.hwnd as usize
                    )));
                }
                thread::sleep(DEFAULT_POLL_INTERVAL);
            }
        }

        pub(crate) fn dump_model(&self) -> Result<Model, TestkitError> {
            self.send_command(COMMAND_ID_DUMP_MODEL, EmptyArgs {})
        }

        pub(crate) fn dump_ui_state(&self) -> Result<UiState, TestkitError> {
            self.send_command(COMMAND_ID_DUMP_UI_STATE, EmptyArgs {})
        }

        pub(crate) fn dump_verification_menu_state(
            &self,
        ) -> Result<VerificationMenuState, TestkitError> {
            self.send_command(COMMAND_ID_DUMP_VERIFICATION_MENU_STATE, EmptyArgs {})
        }

        pub(crate) fn dump_room_menu_state(&self) -> Result<RoomMenuState, TestkitError> {
            self.send_command(COMMAND_ID_DUMP_ROOM_MENU_STATE, EmptyArgs {})
        }

        pub(crate) fn dump_message_menu_state(&self) -> Result<MessageMenuState, TestkitError> {
            self.send_command(COMMAND_ID_DUMP_MESSAGE_MENU_STATE, EmptyArgs {})
        }

        pub(crate) fn select_timeline_event(&self, event_id: &str) -> Result<(), TestkitError> {
            self.send_accepted_command(
                COMMAND_ID_SELECT_TIMELINE_EVENT,
                SelectTimelineEventArgs { event_id },
                "SELECT_TIMELINE_EVENT",
            )
        }

        pub(crate) fn accept_invite_via_room_menu(&self) -> Result<(), TestkitError> {
            self.send_accepted_command(
                COMMAND_ID_ACCEPT_INVITE_VIA_ROOM_MENU,
                EmptyArgs {},
                "ACCEPT_INVITE_VIA_ROOM_MENU",
            )
        }

        pub(crate) fn reject_invite_via_room_menu(&self) -> Result<(), TestkitError> {
            self.send_accepted_command(
                COMMAND_ID_REJECT_INVITE_VIA_ROOM_MENU,
                EmptyArgs {},
                "REJECT_INVITE_VIA_ROOM_MENU",
            )
        }

        pub(crate) fn part_room_via_room_menu(&self) -> Result<(), TestkitError> {
            self.send_accepted_command(
                COMMAND_ID_PART_ROOM_VIA_ROOM_MENU,
                EmptyArgs {},
                "PART_ROOM_VIA_ROOM_MENU",
            )
        }

        pub(crate) fn reply_to_selected_message_via_menu(&self) -> Result<(), TestkitError> {
            self.send_accepted_command(
                COMMAND_ID_REPLY_TO_SELECTED_MESSAGE_VIA_MENU,
                EmptyArgs {},
                "REPLY_TO_SELECTED_MESSAGE_VIA_MENU",
            )
        }

        pub(crate) fn redact_selected_message_via_menu(&self) -> Result<(), TestkitError> {
            self.send_accepted_command(
                COMMAND_ID_REDACT_SELECTED_MESSAGE_VIA_MENU,
                EmptyArgs {},
                "REDACT_SELECTED_MESSAGE_VIA_MENU",
            )
        }

        pub(crate) fn save_attachment_via_message_menu(&self) -> Result<(), TestkitError> {
            self.send_accepted_command(
                COMMAND_ID_SAVE_ATTACHMENT_VIA_MESSAGE_MENU,
                EmptyArgs {},
                "SAVE_ATTACHMENT_VIA_MESSAGE_MENU",
            )
        }

        fn send_accepted_command<Args: Serialize>(
            &self,
            command_id: u32,
            args: Args,
            command_name: &str,
        ) -> Result<(), TestkitError> {
            let response: AcceptedResponse = self.send_command(command_id, args)?;
            if response.accepted {
                Ok(())
            } else {
                Err(TestkitError::InvalidResponse(format!(
                    "{command_name} was not accepted"
                )))
            }
        }

        pub(crate) fn room_activity(
            &self,
            room_id: Option<&str>,
        ) -> Result<Vec<RoomActivity>, TestkitError> {
            let response: DumpRoomActivityResponse = self.send_command(
                COMMAND_ID_DUMP_ROOM_ACTIVITY,
                DumpRoomActivityArgs { room_id },
            )?;
            Ok(response.entries)
        }

        pub(crate) fn list_rooms(&self) -> Result<Vec<Room>, TestkitError> {
            let response: ListRoomsResponse =
                self.send_command(COMMAND_ID_LIST_ROOMS, EmptyArgs {})?;
            Ok(response.rooms)
        }

        pub(crate) fn select_room(&self, room_id: &str) -> Result<(), TestkitError> {
            let response: SelectRoomResponse =
                self.send_command(COMMAND_ID_SELECT_ROOM, SelectRoomArgs { room_id })?;
            if response.selected_room == room_id {
                Ok(())
            } else {
                Err(TestkitError::InvalidResponse(format!(
                    "SELECT_ROOM returned selected_room={}, expected {}",
                    response.selected_room, room_id
                )))
            }
        }

        pub(crate) fn select_view(&self, view: ViewIdentity) -> Result<(), TestkitError> {
            let response: SelectViewResponse = self.send_command(
                COMMAND_ID_SELECT_VIEW,
                SelectViewArgs { view: view.clone() },
            )?;
            if response.active == view {
                Ok(())
            } else {
                Err(TestkitError::InvalidResponse(format!(
                    "SELECT_VIEW returned active={:?}, expected {:?}",
                    response.active, view
                )))
            }
        }

        pub(crate) fn select_navigation_path(
            &self,
            path: &[NavigationPathSegment],
        ) -> Result<(), TestkitError> {
            let response: AcceptedResponse = self.send_command(
                COMMAND_ID_SELECT_NAVIGATION_PATH,
                SelectNavigationPathArgs { path },
            )?;
            if response.accepted {
                Ok(())
            } else {
                Err(TestkitError::InvalidResponse(
                    "SELECT_NAVIGATION_PATH was not accepted".to_owned(),
                ))
            }
        }

        pub(crate) fn select_navigation_labels(&self, path: &[&str]) -> Result<(), TestkitError> {
            let path = path
                .iter()
                .map(|segment| NavigationPathSegment::first(*segment))
                .collect::<Vec<_>>();
            self.select_navigation_path(&path)
        }

        pub(crate) fn selected_navigation_path(
            &self,
        ) -> Result<Vec<NavigationPathSegment>, TestkitError> {
            let response: SelectedNavigationPathResponse =
                self.send_command(COMMAND_ID_DUMP_SELECTED_NAVIGATION_PATH, EmptyArgs {})?;
            Ok(response.path)
        }

        pub(crate) fn select_status_view(&self) -> Result<(), TestkitError> {
            self.select_view(ViewIdentity::Status)
        }

        pub(crate) fn find_active_view(
            &self,
            query: &str,
        ) -> Result<FindActiveViewResult, TestkitError> {
            self.send_command(
                COMMAND_ID_FIND_ACTIVE_VIEW,
                FindActiveViewRequestArgs { query },
            )
        }

        pub(crate) fn trigger_active_timeline_history_backfill(&self) -> Result<(), TestkitError> {
            let active_child = self.active_mdi_child_hwnd()?;
            self.require_active_room_view(active_child)?;
            let content = self.active_mdi_content_hwnd()?;
            let timeline_hwnd = self.child_window_with_class(content, TIMELINE_CLASS_NAME)?;
            let before_top_line = unsafe { timeline_top_line(timeline_hwnd) };

            if before_top_line == 0 {
                unsafe { send_timeline_scroll(timeline_hwnd, SB_PAGEDOWN as usize) };
                let after_page_down = unsafe { timeline_top_line(timeline_hwnd) };
                if after_page_down <= 0 {
                    return Err(TestkitError::InvalidResponse(format!(
                        "page-down did not move the active room timeline away from the top: before={before_top_line}, after={after_page_down}"
                    )));
                }
            }

            unsafe { send_timeline_scroll(timeline_hwnd, SB_PAGEUP as usize) };
            let after_page_up = unsafe { timeline_top_line(timeline_hwnd) };
            if after_page_up != 0 {
                return Err(TestkitError::InvalidResponse(format!(
                    "page-up did not return the active room timeline to the top: top_line={after_page_up}"
                )));
            }

            Ok(())
        }

        pub(crate) fn send_text_target(
            &self,
            room_id: Option<&str>,
            body: &str,
        ) -> Result<String, TestkitError> {
            let response: SendTextResponse =
                self.send_command(COMMAND_ID_SEND_TEXT, SendTextArgs { room_id, body })?;
            if response.accepted {
                Ok(response.room_id)
            } else {
                Err(TestkitError::InvalidResponse(format!(
                    "SEND_TEXT was not accepted for room {}",
                    response.room_id
                )))
            }
        }

        pub(crate) fn send_file_target(
            &self,
            room_id: Option<&str>,
            path: &str,
            filename: Option<&str>,
            content_type: Option<&str>,
        ) -> Result<String, TestkitError> {
            let response: SendTextResponse = self.send_command(
                COMMAND_ID_SEND_FILE,
                SendFileArgs {
                    room_id,
                    path,
                    filename,
                    content_type,
                },
            )?;
            if response.accepted {
                Ok(response.room_id)
            } else {
                Err(TestkitError::InvalidResponse(format!(
                    "SEND_FILE was not accepted for room {}",
                    response.room_id
                )))
            }
        }

        pub(crate) fn compose_text(&self) -> Result<String, TestkitError> {
            let response: ComposeTextResponse =
                self.send_command(COMMAND_ID_DUMP_COMPOSE_TEXT, EmptyArgs {})?;
            Ok(response.text)
        }

        pub(crate) fn set_compose_text(&self, text: &str) -> Result<(), TestkitError> {
            let response: AcceptedResponse =
                self.send_command(COMMAND_ID_SET_COMPOSE_TEXT, SetComposeTextArgs { text })?;
            if response.accepted {
                Ok(())
            } else {
                Err(TestkitError::InvalidResponse(
                    "SET_COMPOSE_TEXT was not accepted".to_owned(),
                ))
            }
        }

        pub(crate) fn send_compose_key(&self, key: ComposeKey) -> Result<(), TestkitError> {
            let response: AcceptedResponse =
                self.send_command(COMMAND_ID_SEND_COMPOSE_KEY, SendComposeKeyArgs { key })?;
            if response.accepted {
                Ok(())
            } else {
                Err(TestkitError::InvalidResponse(
                    "SEND_COMPOSE_KEY was not accepted".to_owned(),
                ))
            }
        }

        pub(crate) fn login(&self, args: LoginArgs) -> Result<(), TestkitError> {
            let response: AcceptedResponse = self.send_command(COMMAND_ID_LOGIN, args)?;
            if response.accepted {
                Ok(())
            } else {
                Err(TestkitError::InvalidResponse(
                    "LOGIN was not accepted".to_owned(),
                ))
            }
        }

        pub(crate) fn restore_session(&self, args: RestoreSessionArgs) -> Result<(), TestkitError> {
            let response: AcceptedResponse = self.send_command(COMMAND_ID_RESTORE_SESSION, args)?;
            if response.accepted {
                Ok(())
            } else {
                Err(TestkitError::InvalidResponse(
                    "RESTORE_SESSION was not accepted".to_owned(),
                ))
            }
        }

        pub(crate) fn restore_saved_session(&self) -> Result<(), TestkitError> {
            let response: AcceptedResponse =
                self.send_command(COMMAND_ID_RESTORE_SAVED_SESSION, EmptyArgs {})?;
            if response.accepted {
                Ok(())
            } else {
                Err(TestkitError::InvalidResponse(
                    "RESTORE_SAVED_SESSION was not accepted".to_owned(),
                ))
            }
        }

        pub(crate) fn start_sync(&self) -> Result<(), TestkitError> {
            let response: AcceptedResponse =
                self.send_command(COMMAND_ID_START_SYNC, EmptyArgs {})?;
            if response.accepted {
                Ok(())
            } else {
                Err(TestkitError::InvalidResponse(
                    "START_SYNC was not accepted".to_owned(),
                ))
            }
        }

        pub(crate) fn logout(&self) -> Result<(), TestkitError> {
            let response: AcceptedResponse = self.send_command(COMMAND_ID_LOGOUT, EmptyArgs {})?;
            if response.accepted {
                Ok(())
            } else {
                Err(TestkitError::InvalidResponse(
                    "LOGOUT was not accepted".to_owned(),
                ))
            }
        }

        pub(crate) fn recover_e2ee(&self, args: RecoverE2eeArgs) -> Result<(), TestkitError> {
            let response: AcceptedResponse = self.send_command(COMMAND_ID_RECOVER_E2EE, args)?;
            if response.accepted {
                Ok(())
            } else {
                Err(TestkitError::InvalidResponse(
                    "RECOVER_E2EE was not accepted".to_owned(),
                ))
            }
        }

        pub(crate) fn setup_e2ee_recovery(
            &self,
            args: SetupE2eeRecoveryArgs,
        ) -> Result<(), TestkitError> {
            let response: AcceptedResponse =
                self.send_command(COMMAND_ID_SETUP_E2EE_RECOVERY, args)?;
            if response.accepted {
                Ok(())
            } else {
                Err(TestkitError::InvalidResponse(
                    "SETUP_E2EE_RECOVERY was not accepted".to_owned(),
                ))
            }
        }

        pub(crate) fn accept_verification(
            &self,
            args: VerificationActionArgs,
        ) -> Result<(), TestkitError> {
            let response: AcceptedResponse =
                self.send_command(COMMAND_ID_ACCEPT_VERIFICATION, args)?;
            if response.accepted {
                Ok(())
            } else {
                Err(TestkitError::InvalidResponse(
                    "ACCEPT_VERIFICATION was not accepted".to_owned(),
                ))
            }
        }

        pub(crate) fn cancel_verification(
            &self,
            args: VerificationActionArgs,
        ) -> Result<(), TestkitError> {
            let response: AcceptedResponse =
                self.send_command(COMMAND_ID_CANCEL_VERIFICATION, args)?;
            if response.accepted {
                Ok(())
            } else {
                Err(TestkitError::InvalidResponse(
                    "CANCEL_VERIFICATION was not accepted".to_owned(),
                ))
            }
        }

        pub(crate) fn confirm_verification_sas(
            &self,
            args: VerificationActionArgs,
        ) -> Result<(), TestkitError> {
            let response: AcceptedResponse =
                self.send_command(COMMAND_ID_CONFIRM_VERIFICATION_SAS, args)?;
            if response.accepted {
                Ok(())
            } else {
                Err(TestkitError::InvalidResponse(
                    "CONFIRM_VERIFICATION_SAS was not accepted".to_owned(),
                ))
            }
        }

        pub(crate) fn reject_verification_sas(
            &self,
            args: VerificationActionArgs,
        ) -> Result<(), TestkitError> {
            let response: AcceptedResponse =
                self.send_command(COMMAND_ID_REJECT_VERIFICATION_SAS, args)?;
            if response.accepted {
                Ok(())
            } else {
                Err(TestkitError::InvalidResponse(
                    "REJECT_VERIFICATION_SAS was not accepted".to_owned(),
                ))
            }
        }

        pub(crate) fn load_events(&self, events_json: &str) -> Result<(), TestkitError> {
            let events = serde_json::from_str(events_json)?;
            let response: LoadEventsResponse =
                self.send_command(COMMAND_ID_LOAD_SYNTHETIC_STATE, LoadEventsArgs { events })?;
            if response.loaded {
                Ok(())
            } else {
                Err(TestkitError::InvalidResponse(format!(
                    "LOAD_SYNTHETIC_STATE reported loaded=false after {} room(s) and {} room activity event(s)",
                    response.rooms, response.timeline_events
                )))
            }
        }

        pub(crate) fn load_events_from_file(&self, path: &Path) -> Result<(), TestkitError> {
            let events_json = fs::read_to_string(path)?;
            self.load_events(&events_json)
        }

        pub(crate) fn append_synthetic_events(
            &self,
            events_json: &str,
        ) -> Result<(), TestkitError> {
            let events = serde_json::from_str(events_json)?;
            let response: LoadEventsResponse = self.send_command(
                COMMAND_ID_APPEND_SYNTHETIC_EVENTS,
                LoadEventsArgs { events },
            )?;
            if response.loaded {
                Ok(())
            } else {
                Err(TestkitError::InvalidResponse(format!(
                    "APPEND_SYNTHETIC_EVENTS reported loaded=false after {} room(s) and {} room activity event(s)",
                    response.rooms, response.timeline_events
                )))
            }
        }

        pub(crate) fn wait_for_room_activity_contains(
            &self,
            room_id: Option<&str>,
            substr: &str,
            timeout: Duration,
        ) -> Result<(), TestkitError> {
            let deadline = Instant::now() + timeout;
            loop {
                let remaining = deadline.saturating_duration_since(Instant::now());
                self.wait_idle(remaining)?;
                let entries = self.room_activity(room_id)?;
                if entries.iter().any(|entry| entry.contains(substr)) {
                    return Ok(());
                }
                if Instant::now() >= deadline {
                    return Err(TestkitError::Timeout(format!(
                        "room activity never contained {:?} for pid {} hwnd 0x{:x}",
                        substr, self.pid, self.hwnd as usize
                    )));
                }
                thread::sleep(DEFAULT_POLL_INTERVAL);
            }
        }

        pub(crate) fn screenshot(&self, path: &Path) -> Result<(), TestkitError> {
            let capture_dir =
                std::env::var_os(SCREENSHOT_CAPTURE_DIR_WIN_ENV).ok_or_else(|| {
                    TestkitError::Launch(format!(
                        "{SCREENSHOT_CAPTURE_DIR_WIN_ENV} was not set by the Wine runner"
                    ))
                })?;
            let request_id = next_request_id();
            let capture_dir = Path::new(&capture_dir);
            let capture_name = format!("capture-{request_id}.png");
            let capture_path = capture_dir.join(&capture_name);
            let request_path = capture_dir.join(format!("{request_id}.request"));
            let result_path = capture_dir.join(format!("{request_id}.result"));

            fs::write(&request_path, format!("{capture_name}\n")).map_err(|error| {
                TestkitError::Launch(format!(
                    "failed to write screenshot request {}: {error}",
                    request_path.display()
                ))
            })?;

            let wait_result = wait_for_file(&result_path, SCREENSHOT_REQUEST_TIMEOUT);
            let result = match wait_result {
                Ok(()) => fs::read_to_string(&result_path).map_err(|error| {
                    TestkitError::Launch(format!(
                        "failed to read screenshot result {}: {error}",
                        result_path.display()
                    ))
                })?,
                Err(error) => {
                    let _ = fs::remove_file(&request_path);
                    return Err(error);
                }
            };

            let outcome = if result.lines().next() == Some("ok") {
                fs::copy(&capture_path, path).map_err(|error| {
                    TestkitError::Launch(format!(
                        "failed to copy screenshot {} to {}: {error}",
                        capture_path.display(),
                        path.display()
                    ))
                })?;
                Ok(())
            } else {
                let message = result.lines().skip(1).collect::<Vec<_>>().join("\n");
                Err(TestkitError::Launch(format!(
                    "host screenshot capture failed for {}: {}",
                    path.display(),
                    if message.is_empty() {
                        "no helper error output".to_owned()
                    } else {
                        message
                    }
                )))
            };

            let _ = fs::remove_file(&request_path);
            let _ = fs::remove_file(&result_path);
            let _ = fs::remove_file(&capture_path);
            outcome
        }

        fn active_mdi_content_hwnd(&self) -> Result<HWND, TestkitError> {
            let active_child = self.active_mdi_child_hwnd()?;
            let content = unsafe { GetDlgItem(active_child, VIEW_CONTENT_ID) };
            if content.is_null() {
                return Err(TestkitError::InvalidResponse(
                    "active MDI child content was not found".to_owned(),
                ));
            }

            Ok(content)
        }

        fn active_mdi_child_hwnd(&self) -> Result<HWND, TestkitError> {
            let mdi_client = unsafe { GetDlgItem(self.hwnd, MDI_ID) };
            if mdi_client.is_null() {
                return Err(TestkitError::InvalidResponse(
                    "MDI client control was not found under the automation frame".to_owned(),
                ));
            }

            let active_child = unsafe { SendMessageW(mdi_client, WM_MDIGETACTIVE, 0, 0) as HWND };
            if active_child.is_null() {
                return Err(TestkitError::InvalidResponse(
                    "active MDI child was not found under the automation frame".to_owned(),
                ));
            }

            Ok(active_child)
        }

        fn require_active_room_view(&self, active_child: HWND) -> Result<(), TestkitError> {
            let active_child_title = unsafe { read_window_text(active_child) };
            let ui_state = self.dump_ui_state()?;
            let active_view = ui_state
                .mdi
                .views
                .iter()
                .find(|view| view.active)
                .ok_or_else(|| {
                    TestkitError::InvalidResponse(
                        "UI state did not include an active MDI view".to_owned(),
                    )
                })?;
            if active_view.title != active_child_title {
                return Err(TestkitError::InvalidResponse(format!(
                    "active MDI child title {:?} did not match active UI view title {:?}",
                    active_child_title, active_view.title
                )));
            }
            match &active_view.identity {
                ViewIdentity::Room { .. } => Ok(()),
                _ => Err(TestkitError::InvalidResponse(format!(
                    "active view is not a room view: {:?}",
                    active_view.identity
                ))),
            }
        }

        fn child_window_with_class(
            &self,
            parent: HWND,
            expected_class_name: &str,
        ) -> Result<HWND, TestkitError> {
            let mut child =
                unsafe { FindWindowExW(parent, 0 as HWND, std::ptr::null(), std::ptr::null()) };
            while !child.is_null() {
                let child_class_name = unsafe { class_name(child) };
                if child_class_name.as_deref() == Some(expected_class_name) {
                    return Ok(child);
                }
                child = unsafe { FindWindowExW(parent, child, std::ptr::null(), std::ptr::null()) };
            }

            Err(TestkitError::InvalidResponse(format!(
                "active MDI child is not a room timeline view: expected child class {expected_class_name} under {}",
                unsafe { class_name(parent) }.unwrap_or_else(|| "<unknown>".to_owned())
            )))
        }

        fn send_command<Args: Serialize, Response: DeserializeOwned>(
            &self,
            command_id: u32,
            args: Args,
        ) -> Result<Response, TestkitError> {
            let request_id = next_request_id();
            let request = RequestEnvelope {
                id: &request_id,
                args,
            };
            let request_bytes = serde_json::to_vec(&request)?;
            let cb_data = u32::try_from(request_bytes.len()).map_err(|_| {
                TestkitError::Transport(format!(
                    "request payload for command id {command_id} exceeded WM_COPYDATA size range"
                ))
            })?;
            let mut request = COPYDATASTRUCT {
                dwData: command_id as usize,
                cbData: cb_data,
                lpData: request_bytes.as_ptr().cast_mut().cast(),
            };

            let send_result = unsafe {
                SendMessageW(
                    self.hwnd,
                    WM_COPYDATA,
                    0,
                    (&mut request as *mut COPYDATASTRUCT) as LPARAM,
                )
            };
            if send_result == 0 {
                return Err(TestkitError::Transport(format!(
                    "SendMessageW returned zero for command id {command_id} to pid {} hwnd 0x{:x}",
                    self.pid, self.hwnd as usize
                )));
            }

            let response_bytes = self.read_response_bytes()?;
            let response: RawResponseEnvelope<Response> = serde_json::from_slice(response_bytes)?;
            if let Some(response_id) = response.id.as_deref()
                && response_id != request_id
            {
                return Err(TestkitError::InvalidResponse(format!(
                    "response id {} did not match request id {}",
                    response_id, request_id
                )));
            }

            if response.ok {
                response.json.ok_or_else(|| {
                    TestkitError::InvalidResponse(format!(
                        "successful response for command id {command_id} did not include json"
                    ))
                })
            } else {
                let failure = response.err.ok_or_else(|| {
                    TestkitError::InvalidResponse(format!(
                        "failed response for command id {command_id} did not include err"
                    ))
                })?;
                Err(TestkitError::Protocol {
                    code: failure.code,
                    message: failure.message,
                })
            }
        }

        fn read_response_bytes(&self) -> Result<&[u8], TestkitError> {
            if self.response_view.is_null() {
                return Err(TestkitError::Transport(format!(
                    "response mapping was not mapped for pid {} hwnd 0x{:x}",
                    self.pid, self.hwnd as usize
                )));
            }

            let prefix_len = size_of::<u32>();
            let buffer = unsafe {
                std::slice::from_raw_parts(self.response_view, RESPONSE_MAPPING_CAPACITY)
            };
            let response_len =
                u32::from_le_bytes(buffer[..prefix_len].try_into().unwrap()) as usize;
            if response_len == 0 {
                return Err(TestkitError::InvalidResponse(
                    "response envelope length prefix was zero".to_owned(),
                ));
            }
            if response_len > RESPONSE_JSON_CAPACITY {
                return Err(TestkitError::InvalidResponse(format!(
                    "response envelope length {} exceeded maximum {}",
                    response_len, RESPONSE_JSON_CAPACITY
                )));
            }

            Ok(&buffer[prefix_len..prefix_len + response_len])
        }
    }

    struct WindowSearch {
        pid_filter: Option<u32>,
        hwnd: HWND,
        pid: u32,
        mapping: HANDLE,
    }

    fn attach_to_window(pid_filter: Option<u32>) -> Result<AppHandle, TestkitError> {
        let mut search = WindowSearch {
            pid_filter,
            hwnd: null_mut(),
            pid: 0,
            mapping: null_mut(),
        };

        unsafe {
            EnumWindows(
                Some(enum_windows_callback),
                (&mut search as *mut WindowSearch) as LPARAM,
            );
        }

        if search.hwnd.is_null() || search.mapping.is_null() {
            return Err(TestkitError::Transport(match pid_filter {
                Some(pid) => format!("no automation-enabled mmatrix window found for pid {pid}"),
                None => "no automation-enabled mmatrix window found".to_owned(),
            }));
        }

        let view = unsafe {
            MapViewOfFile(
                search.mapping,
                FILE_MAP_READ,
                0,
                0,
                RESPONSE_MAPPING_CAPACITY,
            )
        };
        if view.Value.is_null() {
            unsafe {
                CloseHandle(search.mapping);
            }
            return Err(TestkitError::Transport(last_os_error("MapViewOfFile")));
        }

        Ok(AppHandle {
            hwnd: search.hwnd,
            pid: search.pid,
            response_mapping: search.mapping,
            response_view: view.Value.cast(),
            owned_child: None,
        })
    }

    unsafe extern "system" fn enum_windows_callback(hwnd: HWND, lparam: LPARAM) -> i32 {
        let search = &mut *(lparam as *mut WindowSearch);
        let Some(class_name) = class_name(hwnd) else {
            return 1;
        };
        if class_name != FRAME_CLASS_NAME {
            return 1;
        }

        let mut pid = 0;
        GetWindowThreadProcessId(hwnd, &mut pid);
        if pid == 0 {
            return 1;
        }
        if let Some(expected_pid) = search.pid_filter
            && pid != expected_pid
        {
            return 1;
        }

        let mapping_name = wide_null(&response_mapping_name(pid, hwnd));
        let mapping = OpenFileMappingW(FILE_MAP_READ, 0, mapping_name.as_ptr());
        if mapping.is_null() {
            return 1;
        }

        search.hwnd = hwnd;
        search.pid = pid;
        search.mapping = mapping;
        0
    }

    unsafe fn class_name(hwnd: HWND) -> Option<String> {
        let mut buffer = [0u16; 128];
        let len = GetClassNameW(hwnd, buffer.as_mut_ptr(), buffer.len() as i32);
        if len <= 0 {
            return None;
        }
        Some(String::from_utf16_lossy(&buffer[..len as usize]))
    }

    fn response_mapping_name(pid: u32, hwnd: HWND) -> String {
        format!(
            "Local\\mmatrix-ui-automation-response-{pid}-{:x}",
            hwnd as usize
        )
    }

    fn next_request_id() -> String {
        format!(
            "request-{}",
            REQUEST_COUNTER.fetch_add(1, Ordering::Relaxed)
        )
    }

    unsafe fn timeline_top_line(hwnd: HWND) -> i32 {
        unsafe { SendMessageW(hwnd, WM_TIMELINE_GET_TOP_LINE, 0, 0) as i32 }
    }

    unsafe fn send_timeline_scroll(hwnd: HWND, command: usize) {
        unsafe { SendMessageW(hwnd, WM_VSCROLL, command, 0) };
    }

    unsafe fn read_window_text(hwnd: HWND) -> String {
        let len = unsafe { SendMessageW(hwnd, WM_GETTEXTLENGTH, 0, 0) };
        let mut buffer = vec![0u16; len as usize + 1];
        unsafe { SendMessageW(hwnd, WM_GETTEXT, buffer.len(), buffer.as_mut_ptr() as isize) };
        String::from_utf16_lossy(&buffer[..len as usize])
    }

    fn duration_millis_u64(duration: Duration) -> u64 {
        duration.as_millis().min(u64::MAX as u128) as u64
    }

    fn wait_for_file(path: &Path, timeout: Duration) -> Result<(), TestkitError> {
        let deadline = Instant::now() + timeout;
        while Instant::now() < deadline {
            if path.is_file() {
                return Ok(());
            }
            thread::sleep(DEFAULT_POLL_INTERVAL);
        }

        Err(TestkitError::Timeout(format!(
            "timed out waiting for screenshot result {}",
            path.display()
        )))
    }

    fn wide_null(text: &str) -> Vec<u16> {
        OsStr::new(text).encode_wide().chain([0]).collect()
    }

    fn last_os_error(operation: &str) -> String {
        let error = std::io::Error::last_os_error();
        format!("{operation} failed: {error}")
    }
}

#[cfg(test)]
mod tests {
    use super::{FrameSize, launch_args};
    use std::ffi::OsString;

    #[test]
    fn launch_args_add_automation_by_default() {
        assert_eq!(launch_args(&[], None), vec![OsString::from("--automation")]);
    }

    #[test]
    fn launch_args_preserve_automatic_automation_when_frame_size_is_requested() {
        assert_eq!(
            launch_args(
                &[],
                Some(FrameSize {
                    width: 640,
                    height: 768,
                }),
            ),
            vec![
                OsString::from("--automation"),
                OsString::from("--frame-size"),
                OsString::from("640x768"),
            ]
        );
    }

    #[test]
    fn launch_args_preserve_existing_automation_behavior_with_render_state() {
        assert_eq!(
            launch_args(
                &[
                    OsString::from("--render-state"),
                    OsString::from("state.json"),
                ],
                Some(FrameSize {
                    width: 640,
                    height: 768,
                }),
            ),
            vec![
                OsString::from("--render-state"),
                OsString::from("state.json"),
                OsString::from("--frame-size"),
                OsString::from("640x768"),
            ]
        );
    }

    #[test]
    fn launch_args_append_frame_size_only_when_requested() {
        assert_eq!(
            launch_args(
                &[OsString::from("--automation")],
                Some(FrameSize {
                    width: 640,
                    height: 768,
                }),
            ),
            vec![
                OsString::from("--automation"),
                OsString::from("--frame-size"),
                OsString::from("640x768"),
            ]
        );
        assert_eq!(
            launch_args(&[OsString::from("--automation")], None),
            vec![OsString::from("--automation")]
        );
    }
}
