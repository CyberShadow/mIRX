#![allow(dead_code)]

use std::fmt;

use mmatrix_core::{
    CoreErrorEvent, E2eeStatus, Event, Operation, RoomActivity, RoomActivityKind,
    RoomActivityRelation, RoomImageActivity, RoomImageInfo, RoomMediaSource, RoomMessageActivity,
    RoomMessagePresentationKind, RoomMessageSendState, RoomReadState, RoomStateNoticeActivity,
    RoomStateNoticeContent, RoomSummary, RoomTagSummary, StoredSession,
    UnsupportedRoomMessageActivity, VerificationState, VerificationStateEvent,
};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub(crate) const PROTOCOL_VERSION: u32 = 15;
pub(crate) const COMMAND_ID_PING: u32 = 1;
pub(crate) const COMMAND_ID_GET_VERSION: u32 = 2;
pub(crate) const COMMAND_ID_WAIT_IDLE: u32 = 3;
pub(crate) const COMMAND_ID_DUMP_MODEL: u32 = 4;
pub(crate) const COMMAND_ID_DUMP_ROOM_ACTIVITY: u32 = 5;
pub(crate) const COMMAND_ID_LIST_ROOMS: u32 = 6;
pub(crate) const COMMAND_ID_SELECT_ROOM: u32 = 7;
pub(crate) const COMMAND_ID_SEND_TEXT: u32 = 8;
pub(crate) const COMMAND_ID_LOAD_SYNTHETIC_STATE: u32 = 9;
pub(crate) const COMMAND_ID_DUMP_UI_STATE: u32 = 10;
pub(crate) const COMMAND_ID_SELECT_VIEW: u32 = 11;
pub(crate) const COMMAND_ID_LOGIN: u32 = 12;
pub(crate) const COMMAND_ID_RESTORE_SESSION: u32 = 13;
pub(crate) const COMMAND_ID_RESTORE_SAVED_SESSION: u32 = 14;
pub(crate) const COMMAND_ID_START_SYNC: u32 = 15;
pub(crate) const COMMAND_ID_LOGOUT: u32 = 16;
pub(crate) const COMMAND_ID_DUMP_COMPOSE_TEXT: u32 = 17;
pub(crate) const COMMAND_ID_SET_COMPOSE_TEXT: u32 = 18;
pub(crate) const COMMAND_ID_SEND_COMPOSE_KEY: u32 = 19;
pub(crate) const COMMAND_ID_SELECT_NAVIGATION_PATH: u32 = 20;
pub(crate) const COMMAND_ID_DUMP_SELECTED_NAVIGATION_PATH: u32 = 21;
pub(crate) const COMMAND_ID_APPEND_SYNTHETIC_EVENTS: u32 = 22;
pub(crate) const COMMAND_ID_RECOVER_E2EE: u32 = 23;
pub(crate) const COMMAND_ID_ACCEPT_VERIFICATION: u32 = 24;
pub(crate) const COMMAND_ID_CANCEL_VERIFICATION: u32 = 25;
pub(crate) const COMMAND_ID_CONFIRM_VERIFICATION_SAS: u32 = 26;
pub(crate) const COMMAND_ID_REJECT_VERIFICATION_SAS: u32 = 27;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CommandKind {
    Ping,
    GetVersion,
    WaitIdle,
    DumpModel,
    DumpRoomActivity,
    ListRooms,
    SelectRoom,
    SendText,
    LoadSyntheticState,
    DumpUiState,
    SelectView,
    Login,
    RestoreSession,
    RestoreSavedSession,
    StartSync,
    Logout,
    DumpComposeText,
    SetComposeText,
    SendComposeKey,
    SelectNavigationPath,
    DumpSelectedNavigationPath,
    AppendSyntheticEvents,
    RecoverE2ee,
    AcceptVerification,
    CancelVerification,
    ConfirmVerificationSas,
    RejectVerificationSas,
}

impl CommandKind {
    pub(crate) fn from_id(command_id: u32) -> Option<Self> {
        match command_id {
            COMMAND_ID_PING => Some(Self::Ping),
            COMMAND_ID_GET_VERSION => Some(Self::GetVersion),
            COMMAND_ID_WAIT_IDLE => Some(Self::WaitIdle),
            COMMAND_ID_DUMP_MODEL => Some(Self::DumpModel),
            COMMAND_ID_DUMP_ROOM_ACTIVITY => Some(Self::DumpRoomActivity),
            COMMAND_ID_LIST_ROOMS => Some(Self::ListRooms),
            COMMAND_ID_SELECT_ROOM => Some(Self::SelectRoom),
            COMMAND_ID_SEND_TEXT => Some(Self::SendText),
            COMMAND_ID_LOAD_SYNTHETIC_STATE => Some(Self::LoadSyntheticState),
            COMMAND_ID_DUMP_UI_STATE => Some(Self::DumpUiState),
            COMMAND_ID_SELECT_VIEW => Some(Self::SelectView),
            COMMAND_ID_LOGIN => Some(Self::Login),
            COMMAND_ID_RESTORE_SESSION => Some(Self::RestoreSession),
            COMMAND_ID_RESTORE_SAVED_SESSION => Some(Self::RestoreSavedSession),
            COMMAND_ID_START_SYNC => Some(Self::StartSync),
            COMMAND_ID_LOGOUT => Some(Self::Logout),
            COMMAND_ID_DUMP_COMPOSE_TEXT => Some(Self::DumpComposeText),
            COMMAND_ID_SET_COMPOSE_TEXT => Some(Self::SetComposeText),
            COMMAND_ID_SEND_COMPOSE_KEY => Some(Self::SendComposeKey),
            COMMAND_ID_SELECT_NAVIGATION_PATH => Some(Self::SelectNavigationPath),
            COMMAND_ID_DUMP_SELECTED_NAVIGATION_PATH => Some(Self::DumpSelectedNavigationPath),
            COMMAND_ID_APPEND_SYNTHETIC_EVENTS => Some(Self::AppendSyntheticEvents),
            COMMAND_ID_RECOVER_E2EE => Some(Self::RecoverE2ee),
            COMMAND_ID_ACCEPT_VERIFICATION => Some(Self::AcceptVerification),
            COMMAND_ID_CANCEL_VERIFICATION => Some(Self::CancelVerification),
            COMMAND_ID_CONFIRM_VERIFICATION_SAS => Some(Self::ConfirmVerificationSas),
            COMMAND_ID_REJECT_VERIFICATION_SAS => Some(Self::RejectVerificationSas),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ParsedCommand {
    Ping {
        id: String,
    },
    GetVersion {
        id: String,
    },
    WaitIdle {
        id: String,
        args: WaitIdleArgs,
    },
    DumpModel {
        id: String,
    },
    DumpRoomActivity {
        id: String,
        args: DumpRoomActivityArgs,
    },
    ListRooms {
        id: String,
    },
    SelectRoom {
        id: String,
        args: SelectRoomArgs,
    },
    SendText {
        id: String,
        args: SendTextArgs,
    },
    LoadSyntheticState {
        id: String,
        args: LoadEventsArgs,
    },
    DumpUiState {
        id: String,
    },
    SelectView {
        id: String,
        args: SelectViewArgs,
    },
    Login {
        id: String,
        args: LoginArgs,
    },
    RestoreSession {
        id: String,
        args: RestoreSessionArgs,
    },
    RestoreSavedSession {
        id: String,
    },
    StartSync {
        id: String,
    },
    Logout {
        id: String,
    },
    DumpComposeText {
        id: String,
    },
    SetComposeText {
        id: String,
        args: SetComposeTextArgs,
    },
    SendComposeKey {
        id: String,
        args: SendComposeKeyArgs,
    },
    SelectNavigationPath {
        id: String,
        args: SelectNavigationPathArgs,
    },
    DumpSelectedNavigationPath {
        id: String,
    },
    AppendSyntheticEvents {
        id: String,
        args: LoadEventsArgs,
    },
    RecoverE2ee {
        id: String,
        args: RecoverE2eeArgs,
    },
    AcceptVerification {
        id: String,
        args: VerificationActionArgs,
    },
    CancelVerification {
        id: String,
        args: VerificationActionArgs,
    },
    ConfirmVerificationSas {
        id: String,
        args: VerificationActionArgs,
    },
    RejectVerificationSas {
        id: String,
        args: VerificationActionArgs,
    },
}

impl ParsedCommand {
    pub(crate) fn kind(&self) -> CommandKind {
        match self {
            Self::Ping { .. } => CommandKind::Ping,
            Self::GetVersion { .. } => CommandKind::GetVersion,
            Self::WaitIdle { .. } => CommandKind::WaitIdle,
            Self::DumpModel { .. } => CommandKind::DumpModel,
            Self::DumpRoomActivity { .. } => CommandKind::DumpRoomActivity,
            Self::ListRooms { .. } => CommandKind::ListRooms,
            Self::SelectRoom { .. } => CommandKind::SelectRoom,
            Self::SendText { .. } => CommandKind::SendText,
            Self::LoadSyntheticState { .. } => CommandKind::LoadSyntheticState,
            Self::DumpUiState { .. } => CommandKind::DumpUiState,
            Self::SelectView { .. } => CommandKind::SelectView,
            Self::Login { .. } => CommandKind::Login,
            Self::RestoreSession { .. } => CommandKind::RestoreSession,
            Self::RestoreSavedSession { .. } => CommandKind::RestoreSavedSession,
            Self::StartSync { .. } => CommandKind::StartSync,
            Self::Logout { .. } => CommandKind::Logout,
            Self::DumpComposeText { .. } => CommandKind::DumpComposeText,
            Self::SetComposeText { .. } => CommandKind::SetComposeText,
            Self::SendComposeKey { .. } => CommandKind::SendComposeKey,
            Self::SelectNavigationPath { .. } => CommandKind::SelectNavigationPath,
            Self::DumpSelectedNavigationPath { .. } => CommandKind::DumpSelectedNavigationPath,
            Self::AppendSyntheticEvents { .. } => CommandKind::AppendSyntheticEvents,
            Self::RecoverE2ee { .. } => CommandKind::RecoverE2ee,
            Self::AcceptVerification { .. } => CommandKind::AcceptVerification,
            Self::CancelVerification { .. } => CommandKind::CancelVerification,
            Self::ConfirmVerificationSas { .. } => CommandKind::ConfirmVerificationSas,
            Self::RejectVerificationSas { .. } => CommandKind::RejectVerificationSas,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawRequestEnvelope {
    id: String,
    #[serde(default)]
    args: Option<Value>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct NoArgs {}

#[derive(Debug, Clone, PartialEq, Eq, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct WaitIdleArgs {
    pub(crate) timeout_ms: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct DumpRoomActivityArgs {
    pub(crate) room_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct SelectRoomArgs {
    pub(crate) room_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct SendTextArgs {
    pub(crate) room_id: Option<String>,
    pub(crate) body: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct SetComposeTextArgs {
    pub(crate) text: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct SendComposeKeyArgs {
    pub(crate) key: ComposeKey,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct SelectNavigationPathArgs {
    pub(crate) path: Vec<NavigationPathSegment>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct LoadEventsArgs {
    pub(crate) events: Vec<Event>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct SelectViewArgs {
    pub(crate) view: ViewIdentity,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct LoginArgs {
    pub(crate) homeserver_url: String,
    pub(crate) username: String,
    pub(crate) password: String,
    #[serde(default = "default_device_display_name")]
    pub(crate) device_display_name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RestoreSessionArgs {
    pub(crate) homeserver_url: String,
    pub(crate) session: StoredSession,
}

#[derive(Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RecoverE2eeArgs {
    pub(crate) recovery_secret: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct VerificationActionArgs {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) flow_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) user_id: Option<String>,
}

impl fmt::Debug for RecoverE2eeArgs {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RecoverE2eeArgs")
            .field("recovery_secret", &"<redacted>")
            .finish()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct ProtocolError {
    pub(crate) code: String,
    pub(crate) message: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct SuccessEnvelope<T> {
    pub(crate) id: String,
    pub(crate) ok: bool,
    pub(crate) json: T,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct FailureEnvelope {
    pub(crate) id: Option<String>,
    pub(crate) ok: bool,
    pub(crate) err: ProtocolError,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(untagged)]
pub(crate) enum ResponseEnvelope<T> {
    Success(SuccessEnvelope<T>),
    Failure(FailureEnvelope),
}

pub(crate) fn success_response<T>(id: impl Into<String>, json: T) -> ResponseEnvelope<T> {
    ResponseEnvelope::Success(SuccessEnvelope {
        id: id.into(),
        ok: true,
        json,
    })
}

pub(crate) fn failure_response(
    id: Option<String>,
    code: impl Into<String>,
    message: impl Into<String>,
) -> FailureEnvelope {
    FailureEnvelope {
        id,
        ok: false,
        err: ProtocolError {
            code: code.into(),
            message: message.into(),
        },
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct PongResponse {
    pub(crate) pong: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct GetVersionResponse {
    pub(crate) protocol_version: u32,
    pub(crate) app_version: String,
}

impl GetVersionResponse {
    pub(crate) fn current() -> Self {
        Self {
            protocol_version: PROTOCOL_VERSION,
            app_version: env!("CARGO_PKG_VERSION").to_owned(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct WaitIdleResponse {
    pub(crate) idle: bool,
    pub(crate) event_generation: u64,
    pub(crate) pending_core_events: u64,
    pub(crate) synthetic_load_in_progress: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct DumpRoomActivityResponse {
    pub(crate) room_id: String,
    pub(crate) entries: Vec<DumpRoomActivity>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ListRoomsResponse {
    pub(crate) rooms: Vec<DumpRoom>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct SelectRoomResponse {
    pub(crate) selected_room: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct SendTextResponse {
    pub(crate) accepted: bool,
    pub(crate) room_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ComposeTextResponse {
    pub(crate) text: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct AcceptedResponse {
    pub(crate) accepted: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct NavigationPathSegment {
    pub(crate) text: String,
    pub(crate) same_text_index: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct SelectedNavigationPathResponse {
    pub(crate) path: Vec<NavigationPathSegment>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct LoadEventsResponse {
    pub(crate) loaded: bool,
    pub(crate) rooms: usize,
    pub(crate) timeline_events: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct UiState {
    pub(crate) navigation: NavigationNode,
    pub(crate) mdi: MdiState,
    pub(crate) status_bar: StatusBarState,
    pub(crate) switchbar: SwitchbarState,
    pub(crate) rooms: Vec<UiRoomState>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct UiRoomState {
    pub(crate) room_id: String,
    pub(crate) display_name: String,
    pub(crate) encrypted: bool,
    pub(crate) is_space: bool,
    pub(crate) is_direct: bool,
    pub(crate) direct_user_ids: Vec<String>,
    pub(crate) tags: Vec<DumpRoomTag>,
    pub(crate) space_children: Vec<DumpSpaceChild>,
    pub(crate) has_activity: bool,
    pub(crate) read_target_event_id: Option<String>,
    pub(crate) read_state: Option<UiRoomReadState>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct UiRoomReadState {
    pub(crate) latest_active_event_id: Option<String>,
    pub(crate) fully_read_event_id: Option<String>,
    pub(crate) marked_unread: bool,
    pub(crate) num_unread: u64,
    pub(crate) num_notifications: u64,
    pub(crate) num_mentions: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct NavigationNode {
    pub(crate) text: String,
    pub(crate) children: Vec<NavigationNode>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct MdiState {
    pub(crate) active: ViewIdentity,
    pub(crate) views: Vec<MdiView>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct StatusBarState {
    pub(crate) text: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct SwitchbarState {
    pub(crate) items: Vec<SwitchbarItem>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(crate) enum ViewIdentity {
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

        let value = Value::deserialize(deserializer)?;
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
pub(crate) enum ComposeKey {
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

        let value = Value::deserialize(deserializer)?;
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
pub(crate) struct RoomMemberView {
    pub(crate) user_id: String,
    pub(crate) display_name: Option<String>,
    pub(crate) label: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct MdiView {
    pub(crate) identity: ViewIdentity,
    pub(crate) title: String,
    pub(crate) active: bool,
    pub(crate) lines: Vec<String>,
    pub(crate) members: Vec<RoomMemberView>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct SwitchbarItem {
    pub(crate) identity: ViewIdentity,
    pub(crate) title: String,
    pub(crate) active: bool,
    pub(crate) has_activity: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct SelectViewResponse {
    pub(crate) active: ViewIdentity,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum LifecycleState {
    #[default]
    Offline,
    CoreStarted,
    LoggingIn,
    Restoring,
    Authenticated,
    Syncing,
    Ready,
    LoggedOut,
    Stopped,
    Error,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct DumpModel {
    pub(crate) status: String,
    pub(crate) lifecycle_state: LifecycleState,
    pub(crate) rooms: Vec<DumpRoom>,
    pub(crate) last_activity: Option<DumpRoomMessageActivityLine>,
    pub(crate) e2ee_status: Option<E2eeStatus>,
    pub(crate) last_verification: Option<DumpVerification>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) submitted_membership_commands: Vec<DumpSubmittedMembershipCommand>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) submitted_direct_room_commands: Vec<DumpSubmittedDirectRoomCommand>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) submitted_room_topic_commands: Vec<DumpSubmittedRoomTopicCommand>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) submitted_verification_actions: Vec<DumpSubmittedVerificationAction>,
    pub(crate) last_error: Option<DumpError>,
    pub(crate) selected_room: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct DumpSubmittedMembershipCommand {
    pub(crate) action: DumpMembershipCommandAction,
    pub(crate) target: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct DumpSubmittedDirectRoomCommand {
    pub(crate) user_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct DumpSubmittedRoomTopicCommand {
    pub(crate) room_id: String,
    pub(crate) topic: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum DumpMembershipCommandAction {
    Join,
    Leave,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct DumpSubmittedVerificationAction {
    pub(crate) action: DumpVerificationActionKind,
    pub(crate) flow_id: String,
    pub(crate) user_id: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum DumpVerificationActionKind {
    Accept,
    Cancel,
    ConfirmSas,
    RejectSas,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct DumpSpaceChild {
    pub(crate) room_id: String,
    pub(crate) suggested: bool,
    pub(crate) order: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct DumpRoomTag {
    pub(crate) tag: String,
    pub(crate) order: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct DumpRoom {
    pub(crate) room_id: String,
    pub(crate) display_name: String,
    pub(crate) encrypted: bool,
    pub(crate) is_space: bool,
    pub(crate) is_direct: bool,
    pub(crate) direct_user_ids: Vec<String>,
    pub(crate) tags: Vec<DumpRoomTag>,
    pub(crate) space_children: Vec<DumpSpaceChild>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct DumpRoomMessageActivityLine {
    pub(crate) room_id: String,
    pub(crate) activity_id: String,
    pub(crate) timestamp_ms: u64,
    pub(crate) event_id: Option<String>,
    pub(crate) transaction_id: Option<String>,
    pub(crate) sender: String,
    pub(crate) presentation: RoomMessagePresentationKind,
    pub(crate) body: String,
    pub(crate) send_state: DumpRoomMessageSendState,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct DumpRoomActivity {
    pub(crate) room_id: String,
    pub(crate) activity_id: String,
    pub(crate) timestamp_ms: u64,
    pub(crate) relation: Option<DumpRoomActivityRelation>,
    pub(crate) kind: DumpRoomActivityKind,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum DumpRoomActivityRelation {
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
pub(crate) enum DumpRoomActivityKind {
    Message(DumpRoomMessageActivity),
    Image(DumpRoomImageActivity),
    UnsupportedMessage(DumpUnsupportedRoomMessageActivity),
    StateNotice(DumpRoomStateNoticeActivity),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct DumpRoomMessageActivity {
    pub(crate) event_id: Option<String>,
    pub(crate) transaction_id: Option<String>,
    pub(crate) sender: String,
    pub(crate) presentation: RoomMessagePresentationKind,
    pub(crate) body: String,
    pub(crate) send_state: DumpRoomMessageSendState,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct DumpRoomImageActivity {
    pub(crate) event_id: String,
    pub(crate) sender: String,
    pub(crate) body: String,
    pub(crate) filename: Option<String>,
    pub(crate) source: DumpRoomMediaSource,
    pub(crate) info: Option<DumpRoomImageInfo>,
    pub(crate) media_state: Option<DumpRoomImageMediaState>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum DumpRoomImageMediaState {
    Pending,
    Loaded { width: u32, height: u32 },
    Failed { message: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum DumpRoomMediaSource {
    Plain { url: String },
    Encrypted { url: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct DumpRoomImageInfo {
    pub(crate) mimetype: Option<String>,
    pub(crate) size: Option<u64>,
    pub(crate) width: Option<u64>,
    pub(crate) height: Option<u64>,
    pub(crate) is_animated: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct DumpUnsupportedRoomMessageActivity {
    pub(crate) event_id: String,
    pub(crate) sender: String,
    pub(crate) msgtype: String,
    pub(crate) body: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum DumpRoomMessageSendState {
    Pending,
    Sent,
    Failed { message: String, recoverable: bool },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct DumpRoomStateNoticeActivity {
    pub(crate) event_id: String,
    pub(crate) sender: String,
    pub(crate) state_key: String,
    pub(crate) content: DumpRoomStateNoticeContent,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, tag = "type", rename_all = "snake_case")]
pub(crate) enum DumpRoomStateNoticeContent {
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
pub(crate) struct DumpVerification {
    pub(crate) flow_id: String,
    pub(crate) user_id: String,
    pub(crate) state: VerificationState,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct DumpError {
    pub(crate) operation: String,
    pub(crate) message: String,
}

pub(crate) fn operation_code(operation: Operation) -> &'static str {
    match operation {
        Operation::Login => "login",
        Operation::RestoreSession => "restore_session",
        Operation::SessionUpdate => "session_update",
        Operation::StartSync => "start_sync",
        Operation::JoinRoom => "join_room",
        Operation::CreateDirectRoom => "create_direct_room",
        Operation::LeaveRoom => "leave_room",
        Operation::SetRoomTopic => "set_room_topic",
        Operation::SendMessage => "send_text",
        Operation::MarkRoomRead => "mark_room_read",
        Operation::FetchRoomImageMedia => "fetch_room_image_media",
        Operation::RoomList => "room_list",
        Operation::RoomReadState => "room_read_state",
        Operation::E2eeStatus => "e2ee_status",
        Operation::RecoverE2ee => "recover_e2ee",
        Operation::Verification => "verification",
        Operation::Logout => "logout",
        Operation::Shutdown => "shutdown",
    }
}

pub(crate) fn dump_error_from_core(error: &CoreErrorEvent) -> DumpError {
    DumpError {
        operation: operation_code(error.operation).to_owned(),
        message: error.message.clone(),
    }
}

pub(crate) fn dump_room_from_summary(room: &RoomSummary) -> DumpRoom {
    DumpRoom {
        room_id: room.room_id.clone(),
        display_name: room.display_name.clone(),
        encrypted: room.encrypted,
        is_space: room.is_space,
        is_direct: room.is_direct,
        direct_user_ids: room.direct_user_ids.clone(),
        tags: room.tags.iter().map(dump_room_tag_from_summary).collect(),
        space_children: room
            .space_children
            .iter()
            .map(|child| DumpSpaceChild {
                room_id: child.room_id.clone(),
                suggested: child.suggested,
                order: child.order.clone(),
            })
            .collect(),
    }
}

fn dump_room_tag_from_summary(tag: &RoomTagSummary) -> DumpRoomTag {
    DumpRoomTag {
        tag: tag.tag.clone(),
        order: tag.order.clone(),
    }
}

pub(crate) fn dump_ui_room_read_state_from_core(read_state: &RoomReadState) -> UiRoomReadState {
    UiRoomReadState {
        latest_active_event_id: read_state.latest_active_event_id.clone(),
        fully_read_event_id: read_state.fully_read_event_id.clone(),
        marked_unread: read_state.marked_unread,
        num_unread: read_state.num_unread,
        num_notifications: read_state.num_notifications,
        num_mentions: read_state.num_mentions,
    }
}

pub(crate) fn dump_room_activity_from_activity(activity: &RoomActivity) -> DumpRoomActivity {
    DumpRoomActivity {
        room_id: activity.room_id.clone(),
        activity_id: activity.activity_id.clone(),
        timestamp_ms: activity.timestamp_ms,
        relation: activity
            .relation
            .as_ref()
            .map(dump_room_activity_relation_from_core),
        kind: dump_room_activity_kind_from_core(&activity.kind),
    }
}

fn dump_room_activity_relation_from_core(
    relation: &RoomActivityRelation,
) -> DumpRoomActivityRelation {
    match relation {
        RoomActivityRelation::Reply {
            in_reply_to_event_id,
        } => DumpRoomActivityRelation::Reply {
            in_reply_to_event_id: in_reply_to_event_id.clone(),
        },
        RoomActivityRelation::Thread {
            thread_root_event_id,
            in_reply_to_event_id,
            is_falling_back,
        } => DumpRoomActivityRelation::Thread {
            thread_root_event_id: thread_root_event_id.clone(),
            in_reply_to_event_id: in_reply_to_event_id.clone(),
            is_falling_back: *is_falling_back,
        },
    }
}

pub(crate) fn dump_room_message_activity_line_from_activity(
    activity: &RoomActivity,
) -> Option<DumpRoomMessageActivityLine> {
    let RoomActivityKind::Message(message) = &activity.kind else {
        return None;
    };
    Some(DumpRoomMessageActivityLine {
        room_id: activity.room_id.clone(),
        activity_id: activity.activity_id.clone(),
        timestamp_ms: activity.timestamp_ms,
        event_id: message.event_id.clone(),
        transaction_id: message.transaction_id.clone(),
        sender: message.sender.clone(),
        presentation: message.presentation,
        body: message.body.clone(),
        send_state: dump_room_message_send_state_from_core(&message.send_state),
    })
}

pub(crate) fn dump_room_activity_kind_from_core(kind: &RoomActivityKind) -> DumpRoomActivityKind {
    match kind {
        RoomActivityKind::Message(message) => {
            DumpRoomActivityKind::Message(dump_room_message_activity_from_core(message))
        }
        RoomActivityKind::Image(image) => {
            DumpRoomActivityKind::Image(dump_room_image_activity_from_core(image))
        }
        RoomActivityKind::UnsupportedMessage(message) => DumpRoomActivityKind::UnsupportedMessage(
            dump_unsupported_room_message_activity_from_core(message),
        ),
        RoomActivityKind::StateNotice(notice) => {
            DumpRoomActivityKind::StateNotice(dump_room_state_notice_activity_from_core(notice))
        }
    }
}

pub(crate) fn dump_room_message_activity_from_core(
    message: &RoomMessageActivity,
) -> DumpRoomMessageActivity {
    DumpRoomMessageActivity {
        event_id: message.event_id.clone(),
        transaction_id: message.transaction_id.clone(),
        sender: message.sender.clone(),
        presentation: message.presentation,
        body: message.body.clone(),
        send_state: dump_room_message_send_state_from_core(&message.send_state),
    }
}

pub(crate) fn dump_room_message_send_state_from_core(
    send_state: &RoomMessageSendState,
) -> DumpRoomMessageSendState {
    match send_state {
        RoomMessageSendState::Pending => DumpRoomMessageSendState::Pending,
        RoomMessageSendState::Sent => DumpRoomMessageSendState::Sent,
        RoomMessageSendState::Failed {
            message,
            recoverable,
        } => DumpRoomMessageSendState::Failed {
            message: message.clone(),
            recoverable: *recoverable,
        },
    }
}

pub(crate) fn dump_room_image_activity_from_core(
    image: &RoomImageActivity,
) -> DumpRoomImageActivity {
    DumpRoomImageActivity {
        event_id: image.event_id.clone(),
        sender: image.sender.clone(),
        body: image.body.clone(),
        filename: image.filename.clone(),
        source: dump_room_media_source_from_core(&image.source),
        info: image.info.as_ref().map(dump_room_image_info_from_core),
        media_state: None,
    }
}

pub(crate) fn dump_room_media_source_from_core(source: &RoomMediaSource) -> DumpRoomMediaSource {
    match source {
        RoomMediaSource::Plain { url } => DumpRoomMediaSource::Plain { url: url.clone() },
        RoomMediaSource::Encrypted { url } => DumpRoomMediaSource::Encrypted { url: url.clone() },
    }
}

pub(crate) fn dump_room_image_info_from_core(info: &RoomImageInfo) -> DumpRoomImageInfo {
    DumpRoomImageInfo {
        mimetype: info.mimetype.clone(),
        size: info.size,
        width: info.width,
        height: info.height,
        is_animated: info.is_animated,
    }
}

pub(crate) fn dump_unsupported_room_message_activity_from_core(
    message: &UnsupportedRoomMessageActivity,
) -> DumpUnsupportedRoomMessageActivity {
    DumpUnsupportedRoomMessageActivity {
        event_id: message.event_id.clone(),
        sender: message.sender.clone(),
        msgtype: message.msgtype.clone(),
        body: message.body.clone(),
    }
}

pub(crate) fn dump_room_state_notice_activity_from_core(
    notice: &RoomStateNoticeActivity,
) -> DumpRoomStateNoticeActivity {
    DumpRoomStateNoticeActivity {
        event_id: notice.event_id.clone(),
        sender: notice.sender.clone(),
        state_key: notice.state_key.clone(),
        content: dump_room_state_notice_content_from_core(&notice.content),
    }
}

pub(crate) fn dump_room_state_notice_content_from_core(
    content: &RoomStateNoticeContent,
) -> DumpRoomStateNoticeContent {
    match content {
        RoomStateNoticeContent::Topic { topic } => DumpRoomStateNoticeContent::Topic {
            topic: topic.clone(),
        },
        RoomStateNoticeContent::Member {
            membership,
            displayname,
        } => DumpRoomStateNoticeContent::Member {
            membership: membership.clone(),
            displayname: displayname.clone(),
        },
    }
}

pub(crate) fn dump_verification_from_state_event(
    event: &VerificationStateEvent,
) -> DumpVerification {
    DumpVerification {
        flow_id: event.flow_id.clone(),
        user_id: event.user_id.clone(),
        state: event.state.clone(),
    }
}

pub(crate) fn parse_command_request(
    command_id: u32,
    request_json: &[u8],
) -> Result<ParsedCommand, FailureEnvelope> {
    let envelope = parse_request_envelope(request_json)?;
    let id = envelope.id.clone();

    let command = match CommandKind::from_id(command_id) {
        Some(command) => command,
        None => {
            return Err(failure_response(
                Some(id),
                "unknown_command",
                format!("unknown command id {command_id}"),
            ));
        }
    };

    match command {
        CommandKind::Ping => {
            parse_args_with_default::<NoArgs>(&envelope.id, envelope.args)?;
            Ok(ParsedCommand::Ping { id: envelope.id })
        }
        CommandKind::GetVersion => {
            parse_args_with_default::<NoArgs>(&envelope.id, envelope.args)?;
            Ok(ParsedCommand::GetVersion { id: envelope.id })
        }
        CommandKind::WaitIdle => Ok(ParsedCommand::WaitIdle {
            id: envelope.id.clone(),
            args: parse_args_with_default::<WaitIdleArgs>(&envelope.id, envelope.args)?,
        }),
        CommandKind::DumpModel => {
            parse_args_with_default::<NoArgs>(&envelope.id, envelope.args)?;
            Ok(ParsedCommand::DumpModel { id: envelope.id })
        }
        CommandKind::DumpRoomActivity => Ok(ParsedCommand::DumpRoomActivity {
            id: envelope.id.clone(),
            args: parse_args_required::<DumpRoomActivityArgs>(&envelope.id, envelope.args)?,
        }),
        CommandKind::ListRooms => {
            parse_args_with_default::<NoArgs>(&envelope.id, envelope.args)?;
            Ok(ParsedCommand::ListRooms { id: envelope.id })
        }
        CommandKind::SelectRoom => Ok(ParsedCommand::SelectRoom {
            id: envelope.id.clone(),
            args: parse_args_required::<SelectRoomArgs>(&envelope.id, envelope.args)?,
        }),
        CommandKind::SendText => Ok(ParsedCommand::SendText {
            id: envelope.id.clone(),
            args: parse_args_required::<SendTextArgs>(&envelope.id, envelope.args)?,
        }),
        CommandKind::LoadSyntheticState => Ok(ParsedCommand::LoadSyntheticState {
            id: envelope.id.clone(),
            args: parse_args_required::<LoadEventsArgs>(&envelope.id, envelope.args)?,
        }),
        CommandKind::DumpUiState => {
            parse_args_with_default::<NoArgs>(&envelope.id, envelope.args)?;
            Ok(ParsedCommand::DumpUiState { id: envelope.id })
        }
        CommandKind::SelectView => Ok(ParsedCommand::SelectView {
            id: envelope.id.clone(),
            args: parse_args_required::<SelectViewArgs>(&envelope.id, envelope.args)?,
        }),
        CommandKind::Login => Ok(ParsedCommand::Login {
            id: envelope.id.clone(),
            args: parse_args_required::<LoginArgs>(&envelope.id, envelope.args)?,
        }),
        CommandKind::RestoreSession => Ok(ParsedCommand::RestoreSession {
            id: envelope.id.clone(),
            args: parse_args_required::<RestoreSessionArgs>(&envelope.id, envelope.args)?,
        }),
        CommandKind::RestoreSavedSession => {
            parse_args_with_default::<NoArgs>(&envelope.id, envelope.args)?;
            Ok(ParsedCommand::RestoreSavedSession { id: envelope.id })
        }
        CommandKind::StartSync => {
            parse_args_with_default::<NoArgs>(&envelope.id, envelope.args)?;
            Ok(ParsedCommand::StartSync { id: envelope.id })
        }
        CommandKind::Logout => {
            parse_args_with_default::<NoArgs>(&envelope.id, envelope.args)?;
            Ok(ParsedCommand::Logout { id: envelope.id })
        }
        CommandKind::DumpComposeText => {
            parse_args_with_default::<NoArgs>(&envelope.id, envelope.args)?;
            Ok(ParsedCommand::DumpComposeText { id: envelope.id })
        }
        CommandKind::SetComposeText => Ok(ParsedCommand::SetComposeText {
            id: envelope.id.clone(),
            args: parse_args_required::<SetComposeTextArgs>(&envelope.id, envelope.args)?,
        }),
        CommandKind::SendComposeKey => Ok(ParsedCommand::SendComposeKey {
            id: envelope.id.clone(),
            args: parse_args_required::<SendComposeKeyArgs>(&envelope.id, envelope.args)?,
        }),
        CommandKind::SelectNavigationPath => Ok(ParsedCommand::SelectNavigationPath {
            id: envelope.id.clone(),
            args: parse_args_required::<SelectNavigationPathArgs>(&envelope.id, envelope.args)?,
        }),
        CommandKind::DumpSelectedNavigationPath => {
            parse_args_with_default::<NoArgs>(&envelope.id, envelope.args)?;
            Ok(ParsedCommand::DumpSelectedNavigationPath { id: envelope.id })
        }
        CommandKind::AppendSyntheticEvents => Ok(ParsedCommand::AppendSyntheticEvents {
            id: envelope.id.clone(),
            args: parse_args_required::<LoadEventsArgs>(&envelope.id, envelope.args)?,
        }),
        CommandKind::RecoverE2ee => Ok(ParsedCommand::RecoverE2ee {
            id: envelope.id.clone(),
            args: parse_recover_e2ee_args(&envelope.id, envelope.args)?,
        }),
        CommandKind::AcceptVerification => Ok(ParsedCommand::AcceptVerification {
            id: envelope.id.clone(),
            args: parse_verification_action_args(&envelope.id, envelope.args)?,
        }),
        CommandKind::CancelVerification => Ok(ParsedCommand::CancelVerification {
            id: envelope.id.clone(),
            args: parse_verification_action_args(&envelope.id, envelope.args)?,
        }),
        CommandKind::ConfirmVerificationSas => Ok(ParsedCommand::ConfirmVerificationSas {
            id: envelope.id.clone(),
            args: parse_verification_action_args(&envelope.id, envelope.args)?,
        }),
        CommandKind::RejectVerificationSas => Ok(ParsedCommand::RejectVerificationSas {
            id: envelope.id.clone(),
            args: parse_verification_action_args(&envelope.id, envelope.args)?,
        }),
    }
}

fn default_device_display_name() -> String {
    "mmatrix-ui".to_owned()
}

fn parse_request_envelope(request_json: &[u8]) -> Result<RawRequestEnvelope, FailureEnvelope> {
    serde_json::from_slice(request_json).map_err(|err| {
        failure_response(
            None,
            "invalid_request",
            format!("failed to parse request envelope: {err}"),
        )
    })
}

fn parse_args_required<T: DeserializeOwned>(
    id: &str,
    args: Option<Value>,
) -> Result<T, FailureEnvelope> {
    let args = args.ok_or_else(|| {
        failure_response(
            Some(id.to_owned()),
            "invalid_args",
            "missing args object for command",
        )
    })?;
    serde_json::from_value(args).map_err(|err| {
        failure_response(
            Some(id.to_owned()),
            "invalid_args",
            format!("invalid command args: {err}"),
        )
    })
}

fn parse_args_with_default<T: DeserializeOwned + Default>(
    id: &str,
    args: Option<Value>,
) -> Result<T, FailureEnvelope> {
    match args {
        Some(args) => serde_json::from_value(args).map_err(|err| {
            failure_response(
                Some(id.to_owned()),
                "invalid_args",
                format!("invalid command args: {err}"),
            )
        }),
        None => Ok(T::default()),
    }
}

fn parse_recover_e2ee_args(
    id: &str,
    args: Option<Value>,
) -> Result<RecoverE2eeArgs, FailureEnvelope> {
    let args = args.ok_or_else(|| {
        failure_response(
            Some(id.to_owned()),
            "invalid_args",
            "missing args object for recover_e2ee command",
        )
    })?;
    let Value::Object(args) = args else {
        return Err(failure_response(
            Some(id.to_owned()),
            "invalid_args",
            "invalid recover_e2ee args: expected object",
        ));
    };
    if args.keys().any(|key| key.as_str() != "recovery_secret") {
        return Err(failure_response(
            Some(id.to_owned()),
            "invalid_args",
            "invalid recover_e2ee args: unexpected field; expected only `recovery_secret`",
        ));
    }
    let recovery_secret = args.get("recovery_secret").ok_or_else(|| {
        failure_response(
            Some(id.to_owned()),
            "invalid_args",
            "invalid recover_e2ee args: missing field `recovery_secret`",
        )
    })?;
    let Value::String(recovery_secret) = recovery_secret else {
        return Err(failure_response(
            Some(id.to_owned()),
            "invalid_args",
            "invalid recover_e2ee args: `recovery_secret` must be a string",
        ));
    };
    Ok(RecoverE2eeArgs {
        recovery_secret: recovery_secret.clone(),
    })
}

fn parse_verification_action_args(
    id: &str,
    args: Option<Value>,
) -> Result<VerificationActionArgs, FailureEnvelope> {
    let args = parse_args_with_default::<VerificationActionArgs>(id, args)?;
    if args.flow_id.is_some() != args.user_id.is_some() {
        return Err(failure_response(
            Some(id.to_owned()),
            "invalid_args",
            "invalid verification action args: `flow_id` and `user_id` must be provided together",
        ));
    }
    Ok(args)
}

#[cfg(test)]
mod tests {
    use mmatrix_core::{
        CoreErrorEvent, E2eeBackupState, E2eeBackupStatus, E2eeCrossSigningStatus,
        E2eeRecoveryState, E2eeStatus, Event, Operation, RoomActivity, RoomActivityKind,
        RoomActivityRelation, RoomImageActivity, RoomImageInfo, RoomMediaSource,
        RoomMessageActivity, RoomMessagePresentationKind, RoomMessageSendState, RoomReadState,
        RoomSummary, RoomTagSummary, SpaceChildSummary, StoredSession,
        UnsupportedRoomMessageActivity, VerificationRequest, VerificationState,
        VerificationStateEvent,
    };
    use serde_json::{Value, json};

    use super::{
        AcceptedResponse, COMMAND_ID_ACCEPT_VERIFICATION, COMMAND_ID_APPEND_SYNTHETIC_EVENTS,
        COMMAND_ID_CANCEL_VERIFICATION, COMMAND_ID_CONFIRM_VERIFICATION_SAS,
        COMMAND_ID_DUMP_COMPOSE_TEXT, COMMAND_ID_DUMP_MODEL, COMMAND_ID_DUMP_ROOM_ACTIVITY,
        COMMAND_ID_DUMP_SELECTED_NAVIGATION_PATH, COMMAND_ID_DUMP_UI_STATE, COMMAND_ID_GET_VERSION,
        COMMAND_ID_LIST_ROOMS, COMMAND_ID_LOAD_SYNTHETIC_STATE, COMMAND_ID_LOGIN,
        COMMAND_ID_LOGOUT, COMMAND_ID_PING, COMMAND_ID_RECOVER_E2EE,
        COMMAND_ID_REJECT_VERIFICATION_SAS, COMMAND_ID_RESTORE_SAVED_SESSION,
        COMMAND_ID_RESTORE_SESSION, COMMAND_ID_SELECT_NAVIGATION_PATH, COMMAND_ID_SELECT_ROOM,
        COMMAND_ID_SELECT_VIEW, COMMAND_ID_SEND_COMPOSE_KEY, COMMAND_ID_SEND_TEXT,
        COMMAND_ID_SET_COMPOSE_TEXT, COMMAND_ID_START_SYNC, COMMAND_ID_WAIT_IDLE, CommandKind,
        ComposeKey, ComposeTextResponse, DumpModel, DumpRoomActivity, DumpRoomActivityArgs,
        DumpRoomActivityKind, DumpRoomActivityRelation, DumpRoomActivityResponse,
        DumpRoomImageActivity, DumpRoomImageInfo, DumpRoomImageMediaState, DumpRoomMediaSource,
        DumpRoomMessageActivity, DumpRoomMessageActivityLine, DumpRoomMessageSendState,
        DumpRoomStateNoticeActivity, DumpRoomStateNoticeContent, DumpSubmittedRoomTopicCommand,
        DumpUnsupportedRoomMessageActivity, GetVersionResponse, LifecycleState, ListRoomsResponse,
        LoadEventsArgs, LoginArgs, MdiState, MdiView, NavigationNode, NavigationPathSegment,
        NoArgs, PROTOCOL_VERSION, ParsedCommand, PongResponse, RecoverE2eeArgs, ResponseEnvelope,
        RestoreSessionArgs, RoomMemberView, SelectNavigationPathArgs, SelectRoomArgs,
        SelectRoomResponse, SelectViewArgs, SelectViewResponse, SelectedNavigationPathResponse,
        SendComposeKeyArgs, SendTextArgs, SendTextResponse, SetComposeTextArgs, StatusBarState,
        SwitchbarItem, SwitchbarState, UiRoomReadState, UiRoomState, UiState,
        VerificationActionArgs, ViewIdentity, WaitIdleArgs, WaitIdleResponse, failure_response,
        parse_command_request, success_response,
    };

    fn stored_session() -> StoredSession {
        StoredSession {
            user_id: "@alice:example".to_owned(),
            device_id: "DEVICE".to_owned(),
            access_token: "secret-token".to_owned(),
            refresh_token: Some("refresh-token".to_owned()),
        }
    }

    fn sample_e2ee_status() -> E2eeStatus {
        E2eeStatus {
            recovery: E2eeRecoveryState::Incomplete,
            backup: E2eeBackupStatus {
                state: E2eeBackupState::Enabled,
                enabled_locally: true,
                exists_on_server: Some(true),
            },
            cross_signing: Some(E2eeCrossSigningStatus {
                has_master: true,
                has_self_signing: true,
                has_user_signing: false,
                complete: false,
            }),
        }
    }

    #[test]
    fn protocol_version_is_15() {
        assert_eq!(PROTOCOL_VERSION, 15);
    }

    #[test]
    fn command_ids_parse_expected_commands() {
        assert_eq!(
            CommandKind::from_id(COMMAND_ID_PING),
            Some(CommandKind::Ping)
        );
        assert_eq!(
            CommandKind::from_id(COMMAND_ID_GET_VERSION),
            Some(CommandKind::GetVersion)
        );
        assert_eq!(
            CommandKind::from_id(COMMAND_ID_WAIT_IDLE),
            Some(CommandKind::WaitIdle)
        );
        assert_eq!(
            CommandKind::from_id(COMMAND_ID_DUMP_MODEL),
            Some(CommandKind::DumpModel)
        );
        assert_eq!(
            CommandKind::from_id(COMMAND_ID_DUMP_ROOM_ACTIVITY),
            Some(CommandKind::DumpRoomActivity)
        );
        assert_eq!(
            CommandKind::from_id(COMMAND_ID_LIST_ROOMS),
            Some(CommandKind::ListRooms)
        );
        assert_eq!(
            CommandKind::from_id(COMMAND_ID_SELECT_ROOM),
            Some(CommandKind::SelectRoom)
        );
        assert_eq!(
            CommandKind::from_id(COMMAND_ID_SEND_TEXT),
            Some(CommandKind::SendText)
        );
        assert_eq!(
            CommandKind::from_id(COMMAND_ID_LOAD_SYNTHETIC_STATE),
            Some(CommandKind::LoadSyntheticState)
        );
        assert_eq!(
            CommandKind::from_id(COMMAND_ID_DUMP_UI_STATE),
            Some(CommandKind::DumpUiState)
        );
        assert_eq!(
            CommandKind::from_id(COMMAND_ID_SELECT_VIEW),
            Some(CommandKind::SelectView)
        );
        assert_eq!(
            CommandKind::from_id(COMMAND_ID_LOGIN),
            Some(CommandKind::Login)
        );
        assert_eq!(
            CommandKind::from_id(COMMAND_ID_RESTORE_SESSION),
            Some(CommandKind::RestoreSession)
        );
        assert_eq!(
            CommandKind::from_id(COMMAND_ID_RESTORE_SAVED_SESSION),
            Some(CommandKind::RestoreSavedSession)
        );
        assert_eq!(
            CommandKind::from_id(COMMAND_ID_START_SYNC),
            Some(CommandKind::StartSync)
        );
        assert_eq!(
            CommandKind::from_id(COMMAND_ID_LOGOUT),
            Some(CommandKind::Logout)
        );
        assert_eq!(
            CommandKind::from_id(COMMAND_ID_DUMP_COMPOSE_TEXT),
            Some(CommandKind::DumpComposeText)
        );
        assert_eq!(
            CommandKind::from_id(COMMAND_ID_SET_COMPOSE_TEXT),
            Some(CommandKind::SetComposeText)
        );
        assert_eq!(
            CommandKind::from_id(COMMAND_ID_SEND_COMPOSE_KEY),
            Some(CommandKind::SendComposeKey)
        );
        assert_eq!(
            CommandKind::from_id(COMMAND_ID_SELECT_NAVIGATION_PATH),
            Some(CommandKind::SelectNavigationPath)
        );
        assert_eq!(
            CommandKind::from_id(COMMAND_ID_DUMP_SELECTED_NAVIGATION_PATH),
            Some(CommandKind::DumpSelectedNavigationPath)
        );
        assert_eq!(
            CommandKind::from_id(COMMAND_ID_APPEND_SYNTHETIC_EVENTS),
            Some(CommandKind::AppendSyntheticEvents)
        );
        assert_eq!(
            CommandKind::from_id(COMMAND_ID_RECOVER_E2EE),
            Some(CommandKind::RecoverE2ee)
        );
        assert_eq!(
            CommandKind::from_id(COMMAND_ID_ACCEPT_VERIFICATION),
            Some(CommandKind::AcceptVerification)
        );
        assert_eq!(
            CommandKind::from_id(COMMAND_ID_CANCEL_VERIFICATION),
            Some(CommandKind::CancelVerification)
        );
        assert_eq!(
            CommandKind::from_id(COMMAND_ID_CONFIRM_VERIFICATION_SAS),
            Some(CommandKind::ConfirmVerificationSas)
        );
        assert_eq!(
            CommandKind::from_id(COMMAND_ID_REJECT_VERIFICATION_SAS),
            Some(CommandKind::RejectVerificationSas)
        );
        assert_eq!(
            CommandKind::from_id(COMMAND_ID_LOGIN - 1),
            Some(CommandKind::SelectView)
        );
        assert_eq!(
            CommandKind::from_id(COMMAND_ID_REJECT_VERIFICATION_SAS + 1),
            None
        );
    }

    #[test]
    fn parses_all_command_argument_shapes() {
        assert_eq!(
            parse_command_request(COMMAND_ID_PING, br#"{"id":"ping","args":{}}"#).unwrap(),
            ParsedCommand::Ping {
                id: "ping".to_owned(),
            }
        );
        assert_eq!(
            parse_command_request(COMMAND_ID_GET_VERSION, br#"{"id":"version"}"#).unwrap(),
            ParsedCommand::GetVersion {
                id: "version".to_owned(),
            }
        );
        assert_eq!(
            parse_command_request(
                COMMAND_ID_WAIT_IDLE,
                br#"{"id":"idle","args":{"timeout_ms":250}}"#,
            )
            .unwrap(),
            ParsedCommand::WaitIdle {
                id: "idle".to_owned(),
                args: WaitIdleArgs {
                    timeout_ms: Some(250),
                },
            }
        );
        assert_eq!(
            parse_command_request(COMMAND_ID_DUMP_MODEL, br#"{"id":"model","args":{}}"#).unwrap(),
            ParsedCommand::DumpModel {
                id: "model".to_owned(),
            }
        );
        assert_eq!(
            parse_command_request(
                COMMAND_ID_DUMP_ROOM_ACTIVITY,
                br#"{"id":"timeline","args":{"room_id":null}}"#,
            )
            .unwrap(),
            ParsedCommand::DumpRoomActivity {
                id: "timeline".to_owned(),
                args: DumpRoomActivityArgs { room_id: None },
            }
        );
        assert_eq!(
            parse_command_request(COMMAND_ID_LIST_ROOMS, br#"{"id":"rooms","args":{}}"#).unwrap(),
            ParsedCommand::ListRooms {
                id: "rooms".to_owned(),
            }
        );
        assert_eq!(
            parse_command_request(
                COMMAND_ID_SELECT_ROOM,
                br#"{"id":"select","args":{"room_id":"!room:example"}}"#,
            )
            .unwrap(),
            ParsedCommand::SelectRoom {
                id: "select".to_owned(),
                args: SelectRoomArgs {
                    room_id: "!room:example".to_owned(),
                },
            }
        );
        assert_eq!(
            parse_command_request(
                COMMAND_ID_SEND_TEXT,
                br#"{"id":"send","args":{"room_id":"!room:example","body":"hello"}}"#,
            )
            .unwrap(),
            ParsedCommand::SendText {
                id: "send".to_owned(),
                args: SendTextArgs {
                    room_id: Some("!room:example".to_owned()),
                    body: "hello".to_owned(),
                },
            }
        );
        assert_eq!(
            parse_command_request(
                COMMAND_ID_LOAD_SYNTHETIC_STATE,
                br#"{"id":"load","args":{"events":["Started",{"SyncedRooms":[{"room_id":"!room:example","display_name":"Room","encrypted":true,"is_space":false,"is_direct":false,"direct_user_ids":[],"tags":[],"space_children":[]}]},{"RoomActivity":{"room_id":"!room:example","activity_id":"event:$event:example","timestamp_ms":1700000000000,"relation":null,"kind":{"type":"message","event_id":"$event:example","transaction_id":null,"sender":"@alice:example","presentation":"text","body":"hello","send_state":{"state":"sent"}}}}]}}"#,
            )
            .unwrap(),
            ParsedCommand::LoadSyntheticState {
                id: "load".to_owned(),
                args: LoadEventsArgs {
                    events: vec![
                        Event::Started,
                        Event::SyncedRooms(vec![RoomSummary {
                            room_id: "!room:example".to_owned(),
                            display_name: "Room".to_owned(),
                            encrypted: true,
                            is_space: false,
                            is_direct: false,
                            direct_user_ids: Vec::new(),
                            tags: Vec::new(),
                            space_children: Vec::new(),
                        }]),
                        Event::RoomActivity(Box::new(RoomActivity {
                            room_id: "!room:example".to_owned(),
                            activity_id: "event:$event:example".to_owned(),
                            timestamp_ms: 1_700_000_000_000,
                            relation: None,
                            kind: RoomActivityKind::Message(RoomMessageActivity {
                                event_id: Some("$event:example".to_owned()),
                                transaction_id: None,
                                sender: "@alice:example".to_owned(),
                                presentation: mmatrix_core::RoomMessagePresentationKind::Text,
                                body: "hello".to_owned(),
                                send_state: RoomMessageSendState::Sent,
                            }),
                        })),
                    ],
                },
            }
        );
        assert_eq!(
            parse_command_request(COMMAND_ID_DUMP_UI_STATE, br#"{"id":"ui","args":{}}"#).unwrap(),
            ParsedCommand::DumpUiState {
                id: "ui".to_owned(),
            }
        );
        assert_eq!(
            parse_command_request(
                COMMAND_ID_SELECT_VIEW,
                br#"{"id":"status","args":{"view":{"kind":"status"}}}"#,
            )
            .unwrap(),
            ParsedCommand::SelectView {
                id: "status".to_owned(),
                args: SelectViewArgs {
                    view: ViewIdentity::Status,
                },
            }
        );
        assert_eq!(
            parse_command_request(
                COMMAND_ID_SELECT_VIEW,
                br#"{"id":"room-view","args":{"view":{"kind":"room","room_id":"!room:example"}}}"#,
            )
            .unwrap(),
            ParsedCommand::SelectView {
                id: "room-view".to_owned(),
                args: SelectViewArgs {
                    view: ViewIdentity::Room {
                        room_id: "!room:example".to_owned(),
                    },
                },
            }
        );
        assert_eq!(
            parse_command_request(
                COMMAND_ID_SELECT_VIEW,
                br#"{"id":"thread-view","args":{"view":{"kind":"thread","room_id":"!room:example","thread_root_event_id":"$thread-root:example"}}}"#,
            )
            .unwrap(),
            ParsedCommand::SelectView {
                id: "thread-view".to_owned(),
                args: SelectViewArgs {
                    view: ViewIdentity::Thread {
                        room_id: "!room:example".to_owned(),
                        thread_root_event_id: "$thread-root:example".to_owned(),
                    },
                },
            }
        );
        assert_eq!(
            parse_command_request(
                COMMAND_ID_LOGIN,
                br#"{"id":"login","args":{"homeserver_url":"https://matrix.example","username":"@alice:example","password":"secret","device_display_name":"desktop"}}"#,
            )
            .unwrap(),
            ParsedCommand::Login {
                id: "login".to_owned(),
                args: LoginArgs {
                    homeserver_url: "https://matrix.example".to_owned(),
                    username: "@alice:example".to_owned(),
                    password: "secret".to_owned(),
                    device_display_name: "desktop".to_owned(),
                },
            }
        );
        assert_eq!(
            parse_command_request(
                COMMAND_ID_RESTORE_SESSION,
                br#"{"id":"restore","args":{"homeserver_url":"https://matrix.example","session":{"user_id":"@alice:example","device_id":"DEVICE","access_token":"secret-token","refresh_token":"refresh-token"}}}"#,
            )
            .unwrap(),
            ParsedCommand::RestoreSession {
                id: "restore".to_owned(),
                args: RestoreSessionArgs {
                    homeserver_url: "https://matrix.example".to_owned(),
                    session: stored_session(),
                },
            }
        );
        assert_eq!(
            parse_command_request(
                COMMAND_ID_RESTORE_SAVED_SESSION,
                br#"{"id":"restore-saved"}"#,
            )
            .unwrap(),
            ParsedCommand::RestoreSavedSession {
                id: "restore-saved".to_owned(),
            }
        );
        assert_eq!(
            parse_command_request(COMMAND_ID_START_SYNC, br#"{"id":"start-sync","args":{}}"#)
                .unwrap(),
            ParsedCommand::StartSync {
                id: "start-sync".to_owned(),
            }
        );
        assert_eq!(
            parse_command_request(COMMAND_ID_LOGOUT, br#"{"id":"logout"}"#).unwrap(),
            ParsedCommand::Logout {
                id: "logout".to_owned(),
            }
        );
        assert_eq!(
            parse_command_request(
                COMMAND_ID_DUMP_COMPOSE_TEXT,
                br#"{"id":"compose-text","args":{}}"#
            )
            .unwrap(),
            ParsedCommand::DumpComposeText {
                id: "compose-text".to_owned(),
            }
        );
        assert_eq!(
            parse_command_request(
                COMMAND_ID_SET_COMPOSE_TEXT,
                br#"{"id":"set-compose","args":{"text":"draft"}}"#,
            )
            .unwrap(),
            ParsedCommand::SetComposeText {
                id: "set-compose".to_owned(),
                args: SetComposeTextArgs {
                    text: "draft".to_owned(),
                },
            }
        );
        for (request_json, expected_key) in [
            (
                br#"{"id":"compose-key","args":{"key":{"kind":"return"}}}"#.as_slice(),
                ComposeKey::Return,
            ),
            (
                br#"{"id":"compose-key","args":{"key":{"kind":"up"}}}"#.as_slice(),
                ComposeKey::Up,
            ),
            (
                br#"{"id":"compose-key","args":{"key":{"kind":"down"}}}"#.as_slice(),
                ComposeKey::Down,
            ),
        ] {
            assert_eq!(
                parse_command_request(COMMAND_ID_SEND_COMPOSE_KEY, request_json).unwrap(),
                ParsedCommand::SendComposeKey {
                    id: "compose-key".to_owned(),
                    args: SendComposeKeyArgs { key: expected_key },
                }
            );
        }
        assert_eq!(
            parse_command_request(
                COMMAND_ID_SELECT_NAVIGATION_PATH,
                br#"{"id":"select-nav","args":{"path":[{"text":"mmatrix","same_text_index":0},{"text":"rooms: 4","same_text_index":0},{"text":"Work","same_text_index":0},{"text":"Shared","same_text_index":0}]}}"#,
            )
            .unwrap(),
            ParsedCommand::SelectNavigationPath {
                id: "select-nav".to_owned(),
                args: SelectNavigationPathArgs {
                    path: vec![
                        NavigationPathSegment {
                            text: "mmatrix".to_owned(),
                            same_text_index: 0,
                        },
                        NavigationPathSegment {
                            text: "rooms: 4".to_owned(),
                            same_text_index: 0,
                        },
                        NavigationPathSegment {
                            text: "Work".to_owned(),
                            same_text_index: 0,
                        },
                        NavigationPathSegment {
                            text: "Shared".to_owned(),
                            same_text_index: 0,
                        },
                    ],
                },
            }
        );
        assert_eq!(
            parse_command_request(
                COMMAND_ID_DUMP_SELECTED_NAVIGATION_PATH,
                br#"{"id":"selected-nav","args":{}}"#,
            )
            .unwrap(),
            ParsedCommand::DumpSelectedNavigationPath {
                id: "selected-nav".to_owned(),
            }
        );
        assert_eq!(
            parse_command_request(
                COMMAND_ID_APPEND_SYNTHETIC_EVENTS,
                br#"{"id":"append","args":{"events":[{"RoomActivityUpdate":{"room_id":"!room:example","transaction_id":"txn-1","kind":{"type":"message_sent","event_id":"$event:example"}}}]}}"#,
            )
            .unwrap(),
            ParsedCommand::AppendSyntheticEvents {
                id: "append".to_owned(),
                args: LoadEventsArgs {
                    events: vec![Event::RoomActivityUpdate(mmatrix_core::RoomActivityUpdate {
                        room_id: "!room:example".to_owned(),
                        transaction_id: "txn-1".to_owned(),
                        kind: mmatrix_core::RoomActivityUpdateKind::MessageSent {
                            event_id: "$event:example".to_owned(),
                        },
                    })],
                },
            }
        );
        assert_eq!(
            parse_command_request(
                COMMAND_ID_APPEND_SYNTHETIC_EVENTS,
                br#"{"id":"append","args":{"events":[{"RoomReadState":{"room_id":"!ops:example","latest_active_event_id":"$latest:example","fully_read_event_id":"$fully-read:example","marked_unread":true,"num_unread":7,"num_notifications":3,"num_mentions":1}}]}}"#,
            )
            .unwrap(),
            ParsedCommand::AppendSyntheticEvents {
                id: "append".to_owned(),
                args: LoadEventsArgs {
                    events: vec![Event::RoomReadState(RoomReadState {
                        room_id: "!ops:example".to_owned(),
                        latest_active_event_id: Some("$latest:example".to_owned()),
                        fully_read_event_id: Some("$fully-read:example".to_owned()),
                        marked_unread: true,
                        num_unread: 7,
                        num_notifications: 3,
                        num_mentions: 1,
                    })],
                },
            }
        );
        assert_eq!(
            parse_command_request(
                COMMAND_ID_RECOVER_E2EE,
                br#"{"id":"recover","args":{"recovery_secret":"correct horse battery staple"}}"#,
            )
            .unwrap(),
            ParsedCommand::RecoverE2ee {
                id: "recover".to_owned(),
                args: RecoverE2eeArgs {
                    recovery_secret: "correct horse battery staple".to_owned(),
                },
            }
        );
        assert_eq!(
            parse_command_request(
                COMMAND_ID_ACCEPT_VERIFICATION,
                br#"{"id":"accept-verification"}"#,
            )
            .unwrap(),
            ParsedCommand::AcceptVerification {
                id: "accept-verification".to_owned(),
                args: VerificationActionArgs::default(),
            }
        );
        assert_eq!(
            parse_command_request(
                COMMAND_ID_CANCEL_VERIFICATION,
                br#"{"id":"cancel-verification","args":{"flow_id":"flow-1","user_id":"@alice:example"}}"#,
            )
            .unwrap(),
            ParsedCommand::CancelVerification {
                id: "cancel-verification".to_owned(),
                args: VerificationActionArgs {
                    flow_id: Some("flow-1".to_owned()),
                    user_id: Some("@alice:example".to_owned()),
                },
            }
        );
        assert_eq!(
            parse_command_request(
                COMMAND_ID_CONFIRM_VERIFICATION_SAS,
                br#"{"id":"confirm-verification","args":{}}"#,
            )
            .unwrap(),
            ParsedCommand::ConfirmVerificationSas {
                id: "confirm-verification".to_owned(),
                args: VerificationActionArgs::default(),
            }
        );
        assert_eq!(
            parse_command_request(
                COMMAND_ID_REJECT_VERIFICATION_SAS,
                br#"{"id":"reject-verification","args":{"flow_id":"flow-2","user_id":"@bob:example"}}"#,
            )
            .unwrap(),
            ParsedCommand::RejectVerificationSas {
                id: "reject-verification".to_owned(),
                args: VerificationActionArgs {
                    flow_id: Some("flow-2".to_owned()),
                    user_id: Some("@bob:example".to_owned()),
                },
            }
        );
    }

    #[test]
    fn serializes_success_and_failure_envelopes() {
        let success = success_response("ping", PongResponse { pong: true });
        assert_eq!(
            serde_json::to_value(success).unwrap(),
            json!({"id":"ping","ok":true,"json":{"pong":true}})
        );

        let failure: ResponseEnvelope<PongResponse> = ResponseEnvelope::Failure(failure_response(
            Some("ping".to_owned()),
            "invalid_args",
            "bad args",
        ));
        assert_eq!(
            serde_json::to_value(failure).unwrap(),
            json!({"id":"ping","ok":false,"err":{"code":"invalid_args","message":"bad args"}})
        );
    }

    #[test]
    fn malformed_json_returns_parse_failure_with_null_id() {
        let error = parse_command_request(COMMAND_ID_PING, br#"{"id":"ping","args":"#).unwrap_err();
        let value = serde_json::to_value(error).unwrap();

        assert_eq!(value["id"], Value::Null);
        assert_eq!(value["ok"], json!(false));
        assert_eq!(value["err"]["code"], json!("invalid_request"));
    }

    #[test]
    fn unknown_command_returns_structured_failure() {
        let error = parse_command_request(999, br#"{"id":"mystery","args":{}}"#).unwrap_err();
        assert_eq!(
            serde_json::to_value(error).unwrap(),
            json!({
                "id":"mystery",
                "ok":false,
                "err":{"code":"unknown_command","message":"unknown command id 999"}
            })
        );
    }

    #[test]
    fn load_command_uses_real_event_json_schema() {
        let events = vec![
            Event::Started,
            Event::SyncedRooms(vec![RoomSummary {
                room_id: "!one:example".to_owned(),
                display_name: "One".to_owned(),
                encrypted: false,
                is_space: false,
                is_direct: false,
                direct_user_ids: Vec::new(),
                tags: Vec::new(),
                space_children: Vec::new(),
            }]),
            Event::VerificationRequest(VerificationRequest {
                flow_id: "flow-1".to_owned(),
                user_id: "@alice:example".to_owned(),
            }),
            Event::RoomReadState(RoomReadState {
                room_id: "!ops:example".to_owned(),
                latest_active_event_id: Some("$latest:example".to_owned()),
                fully_read_event_id: Some("$fully-read:example".to_owned()),
                marked_unread: true,
                num_unread: 7,
                num_notifications: 3,
                num_mentions: 1,
            }),
            Event::Error(CoreErrorEvent {
                operation: Operation::SendMessage,
                message: "send failed".to_owned(),
            }),
        ];

        assert_eq!(
            serde_json::to_value(&events).unwrap(),
            json!([
                "Started",
                {"SyncedRooms":[{"room_id":"!one:example","display_name":"One","encrypted":false,"is_space":false,"is_direct":false,"direct_user_ids":[],"tags":[],"space_children":[]}]},
                {"VerificationRequest":{"flow_id":"flow-1","user_id":"@alice:example"}},
                {"RoomReadState":{"room_id":"!ops:example","latest_active_event_id":"$latest:example","fully_read_event_id":"$fully-read:example","marked_unread":true,"num_unread":7,"num_notifications":3,"num_mentions":1}},
                {"Error":{"operation":"SendMessage","message":"send failed"}}
            ])
        );
    }

    #[test]
    fn load_command_parses_room_read_state_events() {
        assert_eq!(
            parse_command_request(
                COMMAND_ID_LOAD_SYNTHETIC_STATE,
                br#"{"id":"load","args":{"events":[{"RoomReadState":{"room_id":"!ops:example","latest_active_event_id":"$latest:example","fully_read_event_id":"$fully-read:example","marked_unread":true,"num_unread":7,"num_notifications":3,"num_mentions":1}}]}}"#,
            )
            .unwrap(),
            ParsedCommand::LoadSyntheticState {
                id: "load".to_owned(),
                args: LoadEventsArgs {
                    events: vec![Event::RoomReadState(RoomReadState {
                        room_id: "!ops:example".to_owned(),
                        latest_active_event_id: Some("$latest:example".to_owned()),
                        fully_read_event_id: Some("$fully-read:example".to_owned()),
                        marked_unread: true,
                        num_unread: 7,
                        num_notifications: 3,
                        num_mentions: 1,
                    })],
                },
            }
        );
    }

    #[test]
    fn load_command_rejects_unknown_fields_inside_event_payloads() {
        let error = parse_command_request(
            COMMAND_ID_LOAD_SYNTHETIC_STATE,
            br#"{"id":"load","args":{"events":["Started",{"SyncedRooms":[{"room_id":"!one:example","display_name":"One","encrypted":false,"is_space":false,"is_direct":false,"direct_user_ids":[],"tags":[],"space_children":[],"unexpected":true}]}]}}"#,
        )
        .unwrap_err();

        assert_eq!(
            serde_json::to_value(error).unwrap(),
            json!({
                "id":"load",
                "ok":false,
                "err":{
                    "code":"invalid_args",
                    "message":"invalid command args: unknown field `unexpected`, expected one of `room_id`, `display_name`, `encrypted`, `is_space`, `is_direct`, `direct_user_ids`, `tags`, `space_children`"
                }
            })
        );

        let error = parse_command_request(
            COMMAND_ID_LOAD_SYNTHETIC_STATE,
            br#"{"id":"load","args":{"events":["Started",{"SyncedRooms":[{"room_id":"!space:example","display_name":"Space","encrypted":false,"is_space":true,"is_direct":false,"direct_user_ids":[],"tags":[],"space_children":[{"room_id":"!one:example","suggested":true,"order":"aaa","unexpected":true}]}]}]}}"#,
        )
        .unwrap_err();

        assert_eq!(
            serde_json::to_value(error).unwrap(),
            json!({
                "id":"load",
                "ok":false,
                "err":{
                    "code":"invalid_args",
                    "message":"invalid command args: unknown field `unexpected`, expected one of `room_id`, `suggested`, `order`"
                }
            })
        );

        let error = parse_command_request(
            COMMAND_ID_LOAD_SYNTHETIC_STATE,
            br#"{"id":"load","args":{"events":["Started",{"SyncedRooms":[{"room_id":"!one:example","display_name":"One","encrypted":false,"is_space":false,"is_direct":false,"direct_user_ids":[],"tags":[{"tag":"m.favourite","order":"0.25","unexpected":true}],"space_children":[]}]}]}}"#,
        )
        .unwrap_err();

        assert_eq!(
            serde_json::to_value(error).unwrap(),
            json!({
                "id":"load",
                "ok":false,
                "err":{
                    "code":"invalid_args",
                    "message":"invalid command args: unknown field `unexpected`, expected `tag` or `order`"
                }
            })
        );

        let error = parse_command_request(
            COMMAND_ID_LOAD_SYNTHETIC_STATE,
            br#"{"id":"load","args":{"events":[{"RoomReadState":{"room_id":"!ops:example","latest_active_event_id":"$latest:example","fully_read_event_id":"$fully-read:example","marked_unread":true,"num_unread":7,"num_notifications":3,"num_mentions":1,"unexpected":true}}]}}"#,
        )
        .unwrap_err();

        assert_eq!(
            serde_json::to_value(error).unwrap(),
            json!({
                "id":"load",
                "ok":false,
                "err":{
                    "code":"invalid_args",
                    "message":"invalid command args: unknown field `unexpected`, expected one of `room_id`, `latest_active_event_id`, `fully_read_event_id`, `marked_unread`, `num_unread`, `num_notifications`, `num_mentions`"
                }
            })
        );
    }

    #[test]
    fn select_view_rejects_unknown_fields() {
        let error = parse_command_request(
            COMMAND_ID_SELECT_VIEW,
            br#"{"id":"select","args":{"view":{"kind":"status","unexpected":true}}}"#,
        )
        .unwrap_err();
        let value = serde_json::to_value(error).unwrap();
        assert_eq!(value["id"], json!("select"));
        assert_eq!(value["ok"], json!(false));
        assert_eq!(value["err"]["code"], json!("invalid_args"));
        assert!(
            value["err"]["message"]
                .as_str()
                .unwrap()
                .contains("unknown field `unexpected`")
        );

        let error = parse_command_request(
            COMMAND_ID_SELECT_VIEW,
            br#"{"id":"select","args":{"view":{"kind":"room","room_id":"!room:example","unexpected":true}}}"#,
        )
        .unwrap_err();
        let value = serde_json::to_value(error).unwrap();
        assert_eq!(value["id"], json!("select"));
        assert_eq!(value["ok"], json!(false));
        assert_eq!(value["err"]["code"], json!("invalid_args"));
        assert!(
            value["err"]["message"]
                .as_str()
                .unwrap()
                .contains("unknown field `unexpected`")
        );

        let error = parse_command_request(
            COMMAND_ID_SELECT_VIEW,
            br#"{"id":"select","args":{"view":{"kind":"status"},"unexpected":true}}"#,
        )
        .unwrap_err();
        let value = serde_json::to_value(error).unwrap();
        assert_eq!(value["id"], json!("select"));
        assert_eq!(value["ok"], json!(false));
        assert_eq!(value["err"]["code"], json!("invalid_args"));
        assert!(
            value["err"]["message"]
                .as_str()
                .unwrap()
                .contains("unknown field `unexpected`")
        );
    }

    #[test]
    fn send_compose_key_rejects_unknown_and_malformed_payloads() {
        for request in [
            br#"{"id":"compose-key","args":{"key":{"kind":"left"}}}"#.as_slice(),
            br#"{"id":"compose-key","args":{"key":{"unexpected":true}}}"#.as_slice(),
            br#"{"id":"compose-key","args":{"key":{"kind":"return","unexpected":true}}}"#
                .as_slice(),
            br#"{"id":"compose-key","args":{"key":{"kind":"return"},"unexpected":true}}"#
                .as_slice(),
        ] {
            let error = parse_command_request(COMMAND_ID_SEND_COMPOSE_KEY, request).unwrap_err();
            let value = serde_json::to_value(error).unwrap();
            assert_eq!(value["id"], json!("compose-key"));
            assert_eq!(value["ok"], json!(false));
            assert_eq!(value["err"]["code"], json!("invalid_args"));
        }
    }

    #[test]
    fn set_compose_text_rejects_unknown_fields() {
        let error = parse_command_request(
            COMMAND_ID_SET_COMPOSE_TEXT,
            br#"{"id":"set-compose","args":{"text":"draft","unexpected":true}}"#,
        )
        .unwrap_err();
        let value = serde_json::to_value(error).unwrap();
        assert_eq!(value["id"], json!("set-compose"));
        assert_eq!(value["ok"], json!(false));
        assert_eq!(value["err"]["code"], json!("invalid_args"));
        assert!(
            value["err"]["message"]
                .as_str()
                .unwrap()
                .contains("unknown field `unexpected`")
        );
    }

    #[test]
    fn select_navigation_path_rejects_unknown_fields() {
        let error = parse_command_request(
            COMMAND_ID_SELECT_NAVIGATION_PATH,
            br#"{"id":"select-nav","args":{"path":[{"text":"mmatrix","same_text_index":0}],"unexpected":true}}"#,
        )
        .unwrap_err();
        let value = serde_json::to_value(error).unwrap();
        assert_eq!(value["id"], json!("select-nav"));
        assert_eq!(value["ok"], json!(false));
        assert_eq!(value["err"]["code"], json!("invalid_args"));
        assert!(
            value["err"]["message"]
                .as_str()
                .unwrap()
                .contains("unknown field `unexpected`")
        );
    }

    #[test]
    fn login_defaults_device_display_name_when_omitted() {
        assert_eq!(
            parse_command_request(
                COMMAND_ID_LOGIN,
                br#"{"id":"login","args":{"homeserver_url":"https://matrix.example","username":"@alice:example","password":"secret"}}"#,
            )
            .unwrap(),
            ParsedCommand::Login {
                id: "login".to_owned(),
                args: LoginArgs {
                    homeserver_url: "https://matrix.example".to_owned(),
                    username: "@alice:example".to_owned(),
                    password: "secret".to_owned(),
                    device_display_name: "mmatrix-ui".to_owned(),
                },
            }
        );
    }

    #[test]
    fn lifecycle_payload_commands_require_args() {
        let login_error = parse_command_request(COMMAND_ID_LOGIN, br#"{"id":"login"}"#)
            .expect_err("missing login args must fail");
        assert_eq!(
            serde_json::to_value(login_error).unwrap()["err"]["code"],
            json!("invalid_args")
        );

        let restore_error =
            parse_command_request(COMMAND_ID_RESTORE_SESSION, br#"{"id":"restore"}"#)
                .expect_err("missing restore args must fail");
        assert_eq!(
            serde_json::to_value(restore_error).unwrap()["err"]["code"],
            json!("invalid_args")
        );
    }

    #[test]
    fn lifecycle_commands_reject_unknown_fields() {
        let login_error = parse_command_request(
            COMMAND_ID_LOGIN,
            br#"{"id":"login","args":{"homeserver_url":"https://matrix.example","username":"@alice:example","password":"secret","unexpected":true}}"#,
        )
        .expect_err("login unknown field must fail");
        assert!(
            serde_json::to_value(login_error).unwrap()["err"]["message"]
                .as_str()
                .unwrap()
                .contains("unknown field `unexpected`")
        );

        let restore_error = parse_command_request(
            COMMAND_ID_RESTORE_SESSION,
            br#"{"id":"restore","args":{"homeserver_url":"https://matrix.example","session":{"user_id":"@alice:example","device_id":"DEVICE","access_token":"secret-token","refresh_token":null,"unexpected":true}}}"#,
        )
        .expect_err("restore nested unknown field must fail");
        assert!(
            serde_json::to_value(restore_error).unwrap()["err"]["message"]
                .as_str()
                .unwrap()
                .contains("unknown field `unexpected`")
        );

        let restore_args_error = parse_command_request(
            COMMAND_ID_RESTORE_SESSION,
            br#"{"id":"restore","args":{"homeserver_url":"https://matrix.example","session":{"user_id":"@alice:example","device_id":"DEVICE","access_token":"secret-token","refresh_token":null},"unexpected":true}}"#,
        )
        .expect_err("restore args unknown field must fail");
        assert!(
            serde_json::to_value(restore_args_error).unwrap()["err"]["message"]
                .as_str()
                .unwrap()
                .contains("unknown field `unexpected`")
        );
    }

    #[test]
    fn no_arg_lifecycle_commands_reject_unexpected_args() {
        for (command_id, request_id) in [
            (COMMAND_ID_RESTORE_SAVED_SESSION, "restore-saved"),
            (COMMAND_ID_START_SYNC, "start-sync"),
            (COMMAND_ID_LOGOUT, "logout"),
            (COMMAND_ID_DUMP_COMPOSE_TEXT, "compose-text"),
        ] {
            let request = format!(r#"{{"id":"{request_id}","args":{{"unexpected":true}}}}"#);
            let error = parse_command_request(command_id, request.as_bytes())
                .expect_err("unexpected args must fail");
            assert_eq!(
                serde_json::to_value(error).unwrap()["err"]["code"],
                json!("invalid_args")
            );
        }
    }

    #[test]
    fn recover_e2ee_rejects_unknown_fields() {
        let error = parse_command_request(
            COMMAND_ID_RECOVER_E2EE,
            br#"{"id":"recover","args":{"recovery_secret":"secret","unexpected":true}}"#,
        )
        .expect_err("unknown fields must fail");
        let value = serde_json::to_value(error).unwrap();
        assert_eq!(value["id"], json!("recover"));
        assert_eq!(value["ok"], json!(false));
        assert_eq!(value["err"]["code"], json!("invalid_args"));
        assert_eq!(
            value["err"]["message"],
            json!("invalid recover_e2ee args: unexpected field; expected only `recovery_secret`")
        );
    }

    #[test]
    fn recover_e2ee_shape_errors_do_not_echo_secret() {
        let error = parse_command_request(
            COMMAND_ID_RECOVER_E2EE,
            br#"{"id":"recover","args":"sample recovery secret"}"#,
        )
        .expect_err("non-object args must fail");
        let debug = format!("{error:?}");
        let value = serde_json::to_value(error).unwrap();

        assert!(!debug.contains("sample recovery secret"));
        assert_eq!(value["id"], json!("recover"));
        assert_eq!(value["ok"], json!(false));
        assert_eq!(value["err"]["code"], json!("invalid_args"));
        assert_eq!(
            value["err"]["message"],
            json!("invalid recover_e2ee args: expected object")
        );
        assert!(
            !value["err"]["message"]
                .as_str()
                .unwrap()
                .contains("sample recovery secret")
        );
    }

    #[test]
    fn recover_e2ee_unknown_field_name_does_not_echo_secret() {
        let error = parse_command_request(
            COMMAND_ID_RECOVER_E2EE,
            br#"{"id":"recover","args":{"sample recovery secret":true}}"#,
        )
        .expect_err("unexpected field must fail");
        let debug = format!("{error:?}");
        let value = serde_json::to_value(error).unwrap();

        assert!(!debug.contains("sample recovery secret"));
        assert_eq!(value["id"], json!("recover"));
        assert_eq!(value["ok"], json!(false));
        assert_eq!(value["err"]["code"], json!("invalid_args"));
        assert_eq!(
            value["err"]["message"],
            json!("invalid recover_e2ee args: unexpected field; expected only `recovery_secret`")
        );
        assert!(
            !value["err"]["message"]
                .as_str()
                .unwrap()
                .contains("sample recovery secret")
        );
    }

    #[test]
    fn select_view_related_payloads_accept_thread_identities() {
        let select_view_response = SelectViewResponse {
            active: ViewIdentity::Thread {
                room_id: "!room:example".to_owned(),
                thread_root_event_id: "$thread-root:example".to_owned(),
            },
        };
        let select_view_response_json = json!({
            "active": {
                "kind": "thread",
                "room_id": "!room:example",
                "thread_root_event_id": "$thread-root:example",
            }
        });
        assert_eq!(
            serde_json::from_value::<SelectViewResponse>(select_view_response_json.clone())
                .expect("deserialize thread select-view response"),
            select_view_response
        );
        assert_eq!(
            serde_json::to_value(&select_view_response)
                .expect("serialize thread select-view response"),
            select_view_response_json
        );

        let mdi_view = MdiView {
            identity: ViewIdentity::Thread {
                room_id: "!room:example".to_owned(),
                thread_root_event_id: "$thread-root:example".to_owned(),
            },
            title: "Thread".to_owned(),
            active: true,
            lines: vec!["reply".to_owned()],
            members: Vec::new(),
        };
        let mdi_view_json = json!({
            "identity": {
                "kind": "thread",
                "room_id": "!room:example",
                "thread_root_event_id": "$thread-root:example",
            },
            "title": "Thread",
            "active": true,
            "lines": ["reply"],
            "members": [],
        });
        assert_eq!(
            serde_json::from_value::<MdiView>(mdi_view_json.clone())
                .expect("deserialize thread mdi view"),
            mdi_view
        );
        assert_eq!(
            serde_json::to_value(&mdi_view).expect("serialize thread mdi view"),
            mdi_view_json
        );

        let switchbar_item = SwitchbarItem {
            identity: ViewIdentity::Thread {
                room_id: "!room:example".to_owned(),
                thread_root_event_id: "$thread-root:example".to_owned(),
            },
            title: "Thread".to_owned(),
            active: false,
            has_activity: true,
        };
        let switchbar_item_json = json!({
            "identity": {
                "kind": "thread",
                "room_id": "!room:example",
                "thread_root_event_id": "$thread-root:example",
            },
            "title": "Thread",
            "active": false,
            "has_activity": true,
        });
        assert_eq!(
            serde_json::from_value::<SwitchbarItem>(switchbar_item_json.clone())
                .expect("deserialize thread switchbar item"),
            switchbar_item
        );
        assert_eq!(
            serde_json::to_value(&switchbar_item).expect("serialize thread switchbar item"),
            switchbar_item_json
        );
    }

    #[test]
    fn select_view_related_payloads_reject_unknown_fields_on_thread_identities() {
        let error = parse_command_request(
            COMMAND_ID_SELECT_VIEW,
            br#"{"id":"thread-view","args":{"view":{"kind":"thread","room_id":"!room:example","thread_root_event_id":"$thread-root:example","unexpected":true}}}"#,
        )
        .expect_err("thread select-view unknown field must fail");
        assert!(
            serde_json::to_value(error).unwrap()["err"]["message"]
                .as_str()
                .unwrap()
                .contains("unknown field `unexpected`")
        );

        assert!(
            serde_json::from_value::<SelectViewResponse>(json!({
                "active": {
                    "kind": "thread",
                    "room_id": "!room:example",
                    "thread_root_event_id": "$thread-root:example",
                    "unexpected": true,
                }
            }))
            .is_err()
        );
        assert!(
            serde_json::from_value::<MdiView>(json!({
                "identity": {
                    "kind": "thread",
                    "room_id": "!room:example",
                    "thread_root_event_id": "$thread-root:example",
                    "unexpected": true,
                },
                "title": "Thread",
                "active": true,
                "lines": [],
                "members": [],
            }))
            .is_err()
        );
        assert!(
            serde_json::from_value::<SwitchbarItem>(json!({
                "identity": {
                    "kind": "thread",
                    "room_id": "!room:example",
                    "thread_root_event_id": "$thread-root:example",
                    "unexpected": true,
                },
                "title": "Thread",
                "active": false,
                "has_activity": true,
            }))
            .is_err()
        );
    }

    #[test]
    fn verification_action_commands_reject_partial_explicit_flow_args() {
        for (command_id, request_id, request) in [
            (
                COMMAND_ID_ACCEPT_VERIFICATION,
                "accept-verification",
                br#"{"id":"accept-verification","args":{"flow_id":"flow-1"}}"#.as_slice(),
            ),
            (
                COMMAND_ID_CANCEL_VERIFICATION,
                "cancel-verification",
                br#"{"id":"cancel-verification","args":{"user_id":"@alice:example"}}"#.as_slice(),
            ),
            (
                COMMAND_ID_CONFIRM_VERIFICATION_SAS,
                "confirm-verification",
                br#"{"id":"confirm-verification","args":{"flow_id":"flow-1"}}"#.as_slice(),
            ),
            (
                COMMAND_ID_REJECT_VERIFICATION_SAS,
                "reject-verification",
                br#"{"id":"reject-verification","args":{"user_id":"@alice:example"}}"#.as_slice(),
            ),
        ] {
            let error = parse_command_request(command_id, request).unwrap_err();
            assert_eq!(
                serde_json::to_value(error).unwrap(),
                json!({
                    "id": request_id,
                    "ok": false,
                    "err": {
                        "code": "invalid_args",
                        "message": "invalid verification action args: `flow_id` and `user_id` must be provided together"
                    }
                })
            );
        }
    }

    #[test]
    fn verification_action_commands_reject_unknown_fields() {
        for (command_id, request_id) in [
            (COMMAND_ID_ACCEPT_VERIFICATION, "accept-verification"),
            (COMMAND_ID_CANCEL_VERIFICATION, "cancel-verification"),
            (COMMAND_ID_CONFIRM_VERIFICATION_SAS, "confirm-verification"),
            (COMMAND_ID_REJECT_VERIFICATION_SAS, "reject-verification"),
        ] {
            let request = format!(
                r#"{{"id":"{request_id}","args":{{"flow_id":"flow-1","user_id":"@alice:example","unexpected":true}}}}"#
            );
            let error = parse_command_request(command_id, request.as_bytes()).unwrap_err();
            let value = serde_json::to_value(error).unwrap();
            assert_eq!(value["id"], json!(request_id));
            assert_eq!(value["ok"], json!(false));
            assert_eq!(value["err"]["code"], json!("invalid_args"));
            assert!(
                value["err"]["message"]
                    .as_str()
                    .unwrap()
                    .contains("unknown field `unexpected`")
            );
        }
    }

    #[test]
    fn recover_e2ee_debug_redacts_secret() {
        let command = ParsedCommand::RecoverE2ee {
            id: "recover".to_owned(),
            args: RecoverE2eeArgs {
                recovery_secret: "sample recovery secret".to_owned(),
            },
        };
        let debug = format!("{command:?}");
        assert!(!debug.contains("sample recovery secret"));
        assert!(debug.contains("<redacted>"));
    }

    #[test]
    fn response_dtos_match_documented_shapes() {
        assert_eq!(
            serde_json::to_value(success_response("version", GetVersionResponse::current()))
                .unwrap(),
            json!({
                "id":"version",
                "ok":true,
                "json":{
                    "protocol_version":15,
                    "app_version":env!("CARGO_PKG_VERSION")
                }
            })
        );
        assert_eq!(
            serde_json::to_value(success_response(
                "idle",
                WaitIdleResponse {
                    idle: true,
                    event_generation: 7,
                    pending_core_events: 0,
                    synthetic_load_in_progress: false,
                },
            ))
            .unwrap(),
            json!({
                "id":"idle",
                "ok":true,
                "json":{
                    "idle":true,
                    "event_generation":7,
                    "pending_core_events":0,
                    "synthetic_load_in_progress":false
                }
            })
        );
        assert_eq!(
            serde_json::to_value(success_response(
                "model",
                DumpModel {
                    status: "ready".to_owned(),
                    lifecycle_state: LifecycleState::Ready,
                    rooms: vec![super::DumpRoom {
                        room_id: "!room:example".to_owned(),
                        display_name: "Room".to_owned(),
                        encrypted: true,
                        is_space: false,
                        is_direct: true,
                        direct_user_ids: vec!["@alice:example".to_owned()],
                        tags: vec![super::DumpRoomTag {
                            tag: "m.favourite".to_owned(),
                            order: Some("0.25".to_owned()),
                        }],
                        space_children: Vec::new(),
                    }],
                    last_activity: None,
                    e2ee_status: Some(sample_e2ee_status()),
                    last_verification: None,
                    submitted_membership_commands: Vec::new(),
                    submitted_direct_room_commands: Vec::new(),
                    submitted_room_topic_commands: Vec::new(),
                    submitted_verification_actions: Vec::new(),
                    last_error: None,
                    selected_room: Some("!room:example".to_owned()),
                },
            ))
            .unwrap(),
            json!({
                "id":"model",
                "ok":true,
                "json":{
                    "status":"ready",
                    "lifecycle_state":"ready",
                    "rooms":[{"room_id":"!room:example","display_name":"Room","encrypted":true,"is_space":false,"is_direct":true,"direct_user_ids":["@alice:example"],"tags":[{"tag":"m.favourite","order":"0.25"}],"space_children":[]}],
                    "last_activity":null,
                    "e2ee_status":{
                        "recovery":"incomplete",
                        "backup":{
                            "state":"enabled",
                            "enabled_locally":true,
                            "exists_on_server":true
                        },
                        "cross_signing":{
                            "has_master":true,
                            "has_self_signing":true,
                            "has_user_signing":false,
                            "complete":false
                        }
                    },
                    "last_verification":null,
                    "last_error":null,
                    "selected_room":"!room:example"
                }
            })
        );
        assert_eq!(
            serde_json::to_value(success_response(
                "selected-nav",
                SelectedNavigationPathResponse {
                    path: vec![
                        NavigationPathSegment {
                            text: "mmatrix".to_owned(),
                            same_text_index: 0,
                        },
                        NavigationPathSegment {
                            text: "rooms: 4".to_owned(),
                            same_text_index: 0,
                        },
                        NavigationPathSegment {
                            text: "Work".to_owned(),
                            same_text_index: 0,
                        },
                    ],
                },
            ))
            .unwrap(),
            json!({
                "id":"selected-nav",
                "ok":true,
                "json":{
                    "path":[
                        {"text":"mmatrix","same_text_index":0},
                        {"text":"rooms: 4","same_text_index":0},
                        {"text":"Work","same_text_index":0}
                    ]
                }
            })
        );
        assert_eq!(
            serde_json::to_value(success_response(
                "timeline",
                DumpRoomActivityResponse {
                    room_id: "!room:example".to_owned(),
                    entries: vec![
                        DumpRoomActivity {
                            room_id: "!room:example".to_owned(),
                            activity_id: "txn:txn-1".to_owned(),
                            timestamp_ms: 1_700_000_000_000,
                            relation: Some(DumpRoomActivityRelation::Thread {
                                thread_root_event_id: "$thread-root:example".to_owned(),
                                in_reply_to_event_id: Some("$reply:example".to_owned()),
                                is_falling_back: false,
                            }),
                            kind: DumpRoomActivityKind::Message(DumpRoomMessageActivity {
                                event_id: Some("$event:example".to_owned()),
                                transaction_id: Some("txn-1".to_owned()),
                                sender: "@alice:example".to_owned(),
                                presentation: RoomMessagePresentationKind::Text,
                                body: "hello".to_owned(),
                                send_state: DumpRoomMessageSendState::Failed {
                                    message: "send failed".to_owned(),
                                    recoverable: true,
                                },
                            }),
                        },
                        DumpRoomActivity {
                            room_id: "!room:example".to_owned(),
                            activity_id: "event:$image:example".to_owned(),
                            timestamp_ms: 1_700_000_030_000,
                            relation: None,
                            kind: DumpRoomActivityKind::Image(DumpRoomImageActivity {
                                event_id: "$image:example".to_owned(),
                                sender: "@alice:example".to_owned(),
                                body: "diagram.png".to_owned(),
                                filename: Some("diagram.png".to_owned()),
                                source: DumpRoomMediaSource::Plain {
                                    url: "mxc://example.org/diagram".to_owned(),
                                },
                                info: Some(DumpRoomImageInfo {
                                    mimetype: Some("image/png".to_owned()),
                                    size: Some(12_345),
                                    width: Some(640),
                                    height: Some(480),
                                    is_animated: Some(false),
                                }),
                                media_state: None,
                            }),
                        },
                        DumpRoomActivity {
                            room_id: "!room:example".to_owned(),
                            activity_id: "state:$state-1".to_owned(),
                            timestamp_ms: 1_700_000_060_000,
                            relation: None,
                            kind: DumpRoomActivityKind::StateNotice(DumpRoomStateNoticeActivity {
                                event_id: "$state-1".to_owned(),
                                sender: "@alice:example".to_owned(),
                                state_key: String::new(),
                                content: DumpRoomStateNoticeContent::Topic {
                                    topic: "incident review".to_owned(),
                                },
                            }),
                        },
                    ],
                },
            ))
            .unwrap(),
            json!({
                "id":"timeline",
                "ok":true,
                "json":{
                    "room_id":"!room:example",
                    "entries":[
                        {
                            "room_id":"!room:example",
                            "activity_id":"txn:txn-1",
                            "timestamp_ms":1700000000000u64,
                            "relation":{
                                "type":"thread",
                                "thread_root_event_id":"$thread-root:example",
                                "in_reply_to_event_id":"$reply:example",
                                "is_falling_back":false
                            },
                            "kind":{
                                "type":"message",
                                "event_id":"$event:example",
                                "transaction_id":"txn-1",
                                "sender":"@alice:example",
                                "presentation":"text",
                                "body":"hello",
                                "send_state":{"state":"failed","message":"send failed","recoverable":true}
                            }
                        },
                        {
                            "room_id":"!room:example",
                            "activity_id":"event:$image:example",
                            "timestamp_ms":1700000030000u64,
                            "relation":null,
                            "kind":{
                                "type":"image",
                                "event_id":"$image:example",
                                "sender":"@alice:example",
                                "body":"diagram.png",
                                "filename":"diagram.png",
                                "source":{
                                    "type":"plain",
                                    "url":"mxc://example.org/diagram"
                                },
                                "info":{
                                    "mimetype":"image/png",
                                    "size":12345u64,
                                    "width":640u64,
                                    "height":480u64,
                                    "is_animated":false
                                },
                                "media_state":null
                            }
                        },
                        {
                            "room_id":"!room:example",
                            "activity_id":"state:$state-1",
                            "timestamp_ms":1700000060000u64,
                            "relation":null,
                            "kind":{
                                "type":"state_notice",
                                "event_id":"$state-1",
                                "sender":"@alice:example",
                                "state_key":"",
                                "content":{"type":"topic","topic":"incident review"}
                            }
                        }
                    ]
                }
            })
        );
        assert_eq!(
            serde_json::to_value(success_response(
                "rooms",
                ListRoomsResponse {
                    rooms: vec![super::DumpRoom {
                        room_id: "!room:example".to_owned(),
                        display_name: "Room".to_owned(),
                        encrypted: true,
                        is_space: false,
                        is_direct: true,
                        direct_user_ids: vec!["@alice:example".to_owned()],
                        tags: vec![super::DumpRoomTag {
                            tag: "m.favourite".to_owned(),
                            order: Some("0.25".to_owned()),
                        }],
                        space_children: Vec::new(),
                    }],
                },
            ))
            .unwrap(),
            json!({
                "id":"rooms",
                "ok":true,
                "json":{"rooms":[{"room_id":"!room:example","display_name":"Room","encrypted":true,"is_space":false,"is_direct":true,"direct_user_ids":["@alice:example"],"tags":[{"tag":"m.favourite","order":"0.25"}],"space_children":[]}]}
            })
        );
        assert_eq!(
            serde_json::to_value(success_response(
                "select",
                SelectRoomResponse {
                    selected_room: "!room:example".to_owned(),
                },
            ))
            .unwrap(),
            json!({
                "id":"select",
                "ok":true,
                "json":{"selected_room":"!room:example"}
            })
        );
        assert_eq!(
            serde_json::to_value(success_response(
                "send",
                SendTextResponse {
                    accepted: true,
                    room_id: "!room:example".to_owned(),
                },
            ))
            .unwrap(),
            json!({
                "id":"send",
                "ok":true,
                "json":{"accepted":true,"room_id":"!room:example"}
            })
        );
        assert_eq!(
            serde_json::to_value(success_response(
                "compose-text",
                ComposeTextResponse {
                    text: "draft".to_owned(),
                },
            ))
            .unwrap(),
            json!({
                "id":"compose-text",
                "ok":true,
                "json":{"text":"draft"}
            })
        );
        assert_eq!(
            serde_json::to_value(success_response(
                "login",
                AcceptedResponse { accepted: true },
            ))
            .unwrap(),
            json!({
                "id":"login",
                "ok":true,
                "json":{"accepted":true}
            })
        );
        assert_eq!(
            serde_json::to_value(success_response(
                "load",
                super::LoadEventsResponse {
                    loaded: true,
                    rooms: 2,
                    timeline_events: 3,
                },
            ))
            .unwrap(),
            json!({
                "id":"load",
                "ok":true,
                "json":{"loaded":true,"rooms":2,"timeline_events":3}
            })
        );
        assert_eq!(
            serde_json::to_value(success_response(
                "ui",
                UiState {
                    navigation: NavigationNode {
                        text: "mmatrix".to_owned(),
                        children: vec![
                            NavigationNode {
                                text: "status".to_owned(),
                                children: vec![],
                            },
                            NavigationNode {
                                text: "rooms: 1".to_owned(),
                                children: vec![NavigationNode {
                                    text: "Room".to_owned(),
                                    children: vec![],
                                }],
                            },
                        ],
                    },
                    mdi: MdiState {
                        active: ViewIdentity::Room {
                            room_id: "!room:example".to_owned(),
                        },
                        views: vec![
                            MdiView {
                                identity: ViewIdentity::Status,
                                title: "Status".to_owned(),
                                active: false,
                                lines: vec!["status: synced".to_owned()],
                                members: Vec::new(),
                            },
                            MdiView {
                                identity: ViewIdentity::Room {
                                    room_id: "!room:example".to_owned(),
                                },
                                title: "Room".to_owned(),
                                active: true,
                                lines: vec!["hello".to_owned()],
                                members: vec![RoomMemberView {
                                    user_id: "@alice:example".to_owned(),
                                    display_name: Some("Alice".to_owned()),
                                    label: "Alice".to_owned(),
                                }],
                            },
                            MdiView {
                                identity: ViewIdentity::Thread {
                                    room_id: "!room:example".to_owned(),
                                    thread_root_event_id: "$thread-root:example".to_owned(),
                                },
                                title: "Thread".to_owned(),
                                active: false,
                                lines: vec!["reply".to_owned()],
                                members: Vec::new(),
                            },
                        ],
                    },
                    status_bar: StatusBarState {
                        text: "synced 1 room(s)".to_owned(),
                    },
                    switchbar: SwitchbarState {
                        items: vec![
                            SwitchbarItem {
                                identity: ViewIdentity::Status,
                                title: "Status".to_owned(),
                                active: false,
                                has_activity: false,
                            },
                            SwitchbarItem {
                                identity: ViewIdentity::Room {
                                    room_id: "!room:example".to_owned(),
                                },
                                title: "Room".to_owned(),
                                active: true,
                                has_activity: true,
                            },
                            SwitchbarItem {
                                identity: ViewIdentity::Thread {
                                    room_id: "!room:example".to_owned(),
                                    thread_root_event_id: "$thread-root:example".to_owned(),
                                },
                                title: "Thread".to_owned(),
                                active: false,
                                has_activity: true,
                            },
                        ],
                    },
                    rooms: vec![UiRoomState {
                        room_id: "!room:example".to_owned(),
                        display_name: "Room".to_owned(),
                        encrypted: true,
                        is_space: false,
                        is_direct: true,
                        direct_user_ids: vec!["@alice:example".to_owned()],
                        tags: vec![super::DumpRoomTag {
                            tag: "m.favourite".to_owned(),
                            order: Some("0.25".to_owned()),
                        }],
                        space_children: Vec::new(),
                        has_activity: true,
                        read_target_event_id: Some("$event:example".to_owned()),
                        read_state: Some(UiRoomReadState {
                            latest_active_event_id: Some("$latest:example".to_owned()),
                            fully_read_event_id: Some("$fully-read:example".to_owned()),
                            marked_unread: true,
                            num_unread: 7,
                            num_notifications: 3,
                            num_mentions: 1,
                        }),
                    }],
                },
            ))
            .unwrap(),
            json!({
                "id":"ui",
                "ok":true,
                "json":{
                    "navigation":{
                        "text":"mmatrix",
                        "children":[
                            {"text":"status","children":[]},
                            {"text":"rooms: 1","children":[{"text":"Room","children":[]}]}
                        ]
                    },
                    "mdi":{
                        "active":{"kind":"room","room_id":"!room:example"},
                        "views":[
                            {
                                "identity":{"kind":"status"},
                                "title":"Status",
                                "active":false,
                                "lines":["status: synced"],
                                "members":[]
                            },
                            {
                                "identity":{"kind":"room","room_id":"!room:example"},
                                "title":"Room",
                                "active":true,
                                "lines":["hello"],
                                "members":[
                                    {
                                        "user_id":"@alice:example",
                                        "display_name":"Alice",
                                        "label":"Alice"
                                    }
                                ]
                            },
                            {
                                "identity":{"kind":"thread","room_id":"!room:example","thread_root_event_id":"$thread-root:example"},
                                "title":"Thread",
                                "active":false,
                                "lines":["reply"],
                                "members":[]
                            }
                        ]
                    },
                    "status_bar":{
                        "text":"synced 1 room(s)"
                    },
                    "switchbar":{
                        "items":[
                            {
                                "identity":{"kind":"status"},
                                "title":"Status",
                                "active":false,
                                "has_activity":false
                            },
                            {
                                "identity":{"kind":"room","room_id":"!room:example"},
                                "title":"Room",
                                "active":true,
                                "has_activity":true
                            },
                            {
                                "identity":{"kind":"thread","room_id":"!room:example","thread_root_event_id":"$thread-root:example"},
                                "title":"Thread",
                                "active":false,
                                "has_activity":true
                            }
                        ]
                    },
                    "rooms":[
                        {
                            "room_id":"!room:example",
                            "display_name":"Room",
                            "encrypted":true,
                            "is_space":false,
                            "is_direct":true,
                            "direct_user_ids":["@alice:example"],
                            "tags":[
                                {
                                    "tag":"m.favourite",
                                    "order":"0.25"
                                }
                            ],
                            "space_children":[],
                            "has_activity":true,
                            "read_target_event_id":"$event:example",
                            "read_state":{
                                "latest_active_event_id":"$latest:example",
                                "fully_read_event_id":"$fully-read:example",
                                "marked_unread":true,
                                "num_unread":7,
                                "num_notifications":3,
                                "num_mentions":1
                            }
                        }
                    ]
                }
            })
        );
        assert_eq!(
            serde_json::to_value(success_response(
                "select-view",
                SelectViewResponse {
                    active: ViewIdentity::Thread {
                        room_id: "!room:example".to_owned(),
                        thread_root_event_id: "$thread-root:example".to_owned(),
                    },
                },
            ))
            .unwrap(),
            json!({
                "id":"select-view",
                "ok":true,
                "json":{"active":{"kind":"thread","room_id":"!room:example","thread_root_event_id":"$thread-root:example"}}
            })
        );
    }

    #[test]
    fn dump_model_round_trips_e2ee_status_and_rejects_nested_unknown_fields() {
        let model = DumpModel {
            status: "ready".to_owned(),
            lifecycle_state: LifecycleState::Ready,
            rooms: Vec::new(),
            last_activity: None,
            e2ee_status: Some(sample_e2ee_status()),
            last_verification: None,
            submitted_membership_commands: Vec::new(),
            submitted_direct_room_commands: Vec::new(),
            submitted_room_topic_commands: Vec::new(),
            submitted_verification_actions: Vec::new(),
            last_error: None,
            selected_room: None,
        };
        let json = serde_json::to_value(&model).unwrap();
        assert_eq!(serde_json::from_value::<DumpModel>(json).unwrap(), model);

        for invalid_json in [
            json!({
                "status":"ready",
                "lifecycle_state":"ready",
                "rooms":[],
                "last_activity":null,
                "e2ee_status":{
                    "recovery":"incomplete",
                    "backup":{"state":"enabled","enabled_locally":true,"exists_on_server":true},
                    "cross_signing":{"has_master":true,"has_self_signing":true,"has_user_signing":false,"complete":false},
                    "unexpected":true
                },
                "last_verification":null,
                "last_error":null,
                "selected_room":null
            }),
            json!({
                "status":"ready",
                "lifecycle_state":"ready",
                "rooms":[],
                "last_activity":null,
                "e2ee_status":{
                    "recovery":"incomplete",
                    "backup":{"state":"enabled","enabled_locally":true,"exists_on_server":true,"unexpected":true},
                    "cross_signing":{"has_master":true,"has_self_signing":true,"has_user_signing":false,"complete":false}
                },
                "last_verification":null,
                "last_error":null,
                "selected_room":null
            }),
            json!({
                "status":"ready",
                "lifecycle_state":"ready",
                "rooms":[],
                "last_activity":null,
                "e2ee_status":{
                    "recovery":"incomplete",
                    "backup":{"state":"enabled","enabled_locally":true,"exists_on_server":true},
                    "cross_signing":{"has_master":true,"has_self_signing":true,"has_user_signing":false,"complete":false,"unexpected":true}
                },
                "last_verification":null,
                "last_error":null,
                "selected_room":null
            }),
        ] {
            assert!(serde_json::from_value::<DumpModel>(invalid_json).is_err());
        }
    }

    #[test]
    fn dump_model_round_trips_submitted_room_topic_commands_and_rejects_unknown_nested_fields() {
        let model = DumpModel {
            status: "ready".to_owned(),
            lifecycle_state: LifecycleState::Ready,
            rooms: Vec::new(),
            last_activity: None,
            e2ee_status: None,
            last_verification: None,
            submitted_membership_commands: Vec::new(),
            submitted_direct_room_commands: Vec::new(),
            submitted_room_topic_commands: vec![DumpSubmittedRoomTopicCommand {
                room_id: "!ops:example".to_owned(),
                topic: "incident review".to_owned(),
            }],
            submitted_verification_actions: Vec::new(),
            last_error: None,
            selected_room: Some("!ops:example".to_owned()),
        };
        let json = serde_json::to_value(&model).unwrap();
        assert_eq!(serde_json::from_value::<DumpModel>(json).unwrap(), model);

        let error = serde_json::from_value::<DumpModel>(json!({
            "status":"ready",
            "lifecycle_state":"ready",
            "rooms":[],
            "last_activity":null,
            "e2ee_status":null,
            "last_verification":null,
            "submitted_membership_commands":[],
            "submitted_direct_room_commands":[],
            "submitted_room_topic_commands":[
                {
                    "room_id":"!ops:example",
                    "topic":"incident review",
                    "unexpected":true
                }
            ],
            "last_error":null,
            "selected_room":"!ops:example"
        }))
        .unwrap_err();

        assert!(error.to_string().contains("unknown field `unexpected`"));
    }

    #[test]
    fn no_args_is_default_constructible() {
        assert_eq!(NoArgs::default(), NoArgs {});
    }

    #[test]
    fn dump_helpers_copy_automation_payload_fields() {
        assert_eq!(
            super::dump_room_from_summary(&RoomSummary {
                room_id: "!room:example".to_owned(),
                display_name: "Room".to_owned(),
                encrypted: true,
                is_space: true,
                is_direct: true,
                direct_user_ids: vec!["@alice:example".to_owned()],
                tags: vec![
                    RoomTagSummary {
                        tag: "m.favourite".to_owned(),
                        order: Some("0.25".to_owned()),
                    },
                    RoomTagSummary {
                        tag: "u.work".to_owned(),
                        order: None,
                    },
                ],
                space_children: vec![SpaceChildSummary {
                    room_id: "!child:example".to_owned(),
                    suggested: true,
                    order: Some("aaa".to_owned()),
                }],
            }),
            super::DumpRoom {
                room_id: "!room:example".to_owned(),
                display_name: "Room".to_owned(),
                encrypted: true,
                is_space: true,
                is_direct: true,
                direct_user_ids: vec!["@alice:example".to_owned()],
                tags: vec![
                    super::DumpRoomTag {
                        tag: "m.favourite".to_owned(),
                        order: Some("0.25".to_owned()),
                    },
                    super::DumpRoomTag {
                        tag: "u.work".to_owned(),
                        order: None,
                    },
                ],
                space_children: vec![super::DumpSpaceChild {
                    room_id: "!child:example".to_owned(),
                    suggested: true,
                    order: Some("aaa".to_owned()),
                }],
            }
        );
        assert!(
            serde_json::from_value::<super::DumpRoomTag>(json!({
                "tag":"m.favourite",
                "order":"0.25",
                "unexpected":true
            }))
            .is_err()
        );
        assert!(
            serde_json::from_value::<super::DumpSpaceChild>(json!({
                "room_id":"!child:example",
                "suggested":true,
                "order":"aaa",
                "unexpected":true
            }))
            .is_err()
        );
        assert!(
            serde_json::from_value::<super::UiState>(json!({
                "navigation":{"text":"mmatrix","children":[]},
                "mdi":{"active":{"kind":"status"},"views":[]},
                "status_bar":{"text":"ready"},
                "switchbar":{"items":[]},
                "unexpected":true
            }))
            .is_err()
        );
        assert!(
            serde_json::from_value::<super::UiState>(json!({
                "navigation":{"text":"mmatrix","children":[]},
                "mdi":{"active":{"kind":"status"},"views":[]},
                "status_bar":{"text":"ready"},
                "switchbar":{"items":[]}
            }))
            .is_err()
        );
        assert!(
            serde_json::from_value::<super::UiRoomState>(json!({
                "room_id":"!room:example",
                "display_name":"Room",
                "encrypted":true,
                "is_space":false,
                "is_direct":false,
                "direct_user_ids":[],
                "tags":[],
                "space_children":[],
                "has_activity":false,
                "read_target_event_id":null,
                "read_state":null,
                "unexpected":true
            }))
            .is_err()
        );
        assert!(
            serde_json::from_value::<super::UiRoomReadState>(json!({
                "latest_active_event_id":null,
                "fully_read_event_id":null,
                "marked_unread":false,
                "num_unread":0,
                "num_notifications":0,
                "num_mentions":0,
                "unexpected":true
            }))
            .is_err()
        );
        assert_eq!(
            super::dump_ui_room_read_state_from_core(&RoomReadState {
                room_id: "!room:example".to_owned(),
                latest_active_event_id: Some("$latest:example".to_owned()),
                fully_read_event_id: Some("$fully-read:example".to_owned()),
                marked_unread: true,
                num_unread: 7,
                num_notifications: 3,
                num_mentions: 1,
            }),
            super::UiRoomReadState {
                latest_active_event_id: Some("$latest:example".to_owned()),
                fully_read_event_id: Some("$fully-read:example".to_owned()),
                marked_unread: true,
                num_unread: 7,
                num_notifications: 3,
                num_mentions: 1,
            }
        );
        let activity = RoomActivity {
            room_id: "!room:example".to_owned(),
            activity_id: "event:$event:example".to_owned(),
            timestamp_ms: 1_700_000_000_000,
            relation: Some(RoomActivityRelation::Thread {
                thread_root_event_id: "$thread-root:example".to_owned(),
                in_reply_to_event_id: Some("$reply:example".to_owned()),
                is_falling_back: true,
            }),
            kind: RoomActivityKind::Message(RoomMessageActivity {
                event_id: Some("$event:example".to_owned()),
                transaction_id: Some("txn-1".to_owned()),
                sender: "@alice:example".to_owned(),
                presentation: mmatrix_core::RoomMessagePresentationKind::Text,
                body: "hello".to_owned(),
                send_state: RoomMessageSendState::Sent,
            }),
        };
        assert_eq!(
            super::dump_room_activity_from_activity(&activity),
            DumpRoomActivity {
                room_id: "!room:example".to_owned(),
                activity_id: "event:$event:example".to_owned(),
                timestamp_ms: 1_700_000_000_000,
                relation: Some(DumpRoomActivityRelation::Thread {
                    thread_root_event_id: "$thread-root:example".to_owned(),
                    in_reply_to_event_id: Some("$reply:example".to_owned()),
                    is_falling_back: true,
                }),
                kind: DumpRoomActivityKind::Message(DumpRoomMessageActivity {
                    event_id: Some("$event:example".to_owned()),
                    transaction_id: Some("txn-1".to_owned()),
                    sender: "@alice:example".to_owned(),
                    presentation: RoomMessagePresentationKind::Text,
                    body: "hello".to_owned(),
                    send_state: DumpRoomMessageSendState::Sent,
                }),
            }
        );
        assert_eq!(
            super::dump_room_message_activity_line_from_activity(&activity),
            Some(DumpRoomMessageActivityLine {
                room_id: "!room:example".to_owned(),
                activity_id: "event:$event:example".to_owned(),
                timestamp_ms: 1_700_000_000_000,
                event_id: Some("$event:example".to_owned()),
                transaction_id: Some("txn-1".to_owned()),
                sender: "@alice:example".to_owned(),
                presentation: RoomMessagePresentationKind::Text,
                body: "hello".to_owned(),
                send_state: DumpRoomMessageSendState::Sent,
            })
        );
        let unsupported_activity = RoomActivity {
            room_id: "!room:example".to_owned(),
            activity_id: "event:$audio1:example.org".to_owned(),
            timestamp_ms: 1_710_000_000_000,
            relation: None,
            kind: RoomActivityKind::UnsupportedMessage(UnsupportedRoomMessageActivity {
                event_id: "$audio1:example.org".to_owned(),
                sender: "@alice:example.org".to_owned(),
                msgtype: "m.audio".to_owned(),
                body: "voice-note.ogg".to_owned(),
            }),
        };
        assert_eq!(
            super::dump_room_activity_from_activity(&unsupported_activity),
            DumpRoomActivity {
                room_id: "!room:example".to_owned(),
                activity_id: "event:$audio1:example.org".to_owned(),
                timestamp_ms: 1_710_000_000_000,
                relation: None,
                kind: DumpRoomActivityKind::UnsupportedMessage(
                    DumpUnsupportedRoomMessageActivity {
                        event_id: "$audio1:example.org".to_owned(),
                        sender: "@alice:example.org".to_owned(),
                        msgtype: "m.audio".to_owned(),
                        body: "voice-note.ogg".to_owned(),
                    }
                ),
            }
        );
        assert_eq!(
            serde_json::to_value(super::dump_room_activity_from_activity(
                &unsupported_activity
            ))
            .expect("serialize unsupported dump room activity"),
            json!({
                "room_id":"!room:example",
                "activity_id":"event:$audio1:example.org",
                "timestamp_ms":1710000000000u64,
                "relation":null,
                "kind":{
                    "type":"unsupported_message",
                    "event_id":"$audio1:example.org",
                    "sender":"@alice:example.org",
                    "msgtype":"m.audio",
                    "body":"voice-note.ogg"
                }
            })
        );
        assert_eq!(
            super::dump_room_message_activity_line_from_activity(&unsupported_activity),
            None
        );
        let image_activity = RoomActivity {
            room_id: "!room:example".to_owned(),
            activity_id: "event:$image1:example.org".to_owned(),
            timestamp_ms: 1_710_000_010_000,
            relation: None,
            kind: RoomActivityKind::Image(RoomImageActivity {
                event_id: "$image1:example.org".to_owned(),
                sender: "@alice:example.org".to_owned(),
                body: "diagram.png".to_owned(),
                filename: Some("diagram.png".to_owned()),
                source: RoomMediaSource::Encrypted {
                    url: "mxc://example.org/diagram".to_owned(),
                },
                info: Some(RoomImageInfo {
                    mimetype: Some("image/png".to_owned()),
                    size: Some(12_345),
                    width: Some(640),
                    height: Some(480),
                    is_animated: Some(false),
                }),
            }),
        };
        assert_eq!(
            super::dump_room_activity_from_activity(&image_activity),
            DumpRoomActivity {
                room_id: "!room:example".to_owned(),
                activity_id: "event:$image1:example.org".to_owned(),
                timestamp_ms: 1_710_000_010_000,
                relation: None,
                kind: DumpRoomActivityKind::Image(DumpRoomImageActivity {
                    event_id: "$image1:example.org".to_owned(),
                    sender: "@alice:example.org".to_owned(),
                    body: "diagram.png".to_owned(),
                    filename: Some("diagram.png".to_owned()),
                    source: DumpRoomMediaSource::Encrypted {
                        url: "mxc://example.org/diagram".to_owned(),
                    },
                    info: Some(DumpRoomImageInfo {
                        mimetype: Some("image/png".to_owned()),
                        size: Some(12_345),
                        width: Some(640),
                        height: Some(480),
                        is_animated: Some(false),
                    }),
                    media_state: None,
                }),
            }
        );
        assert_eq!(
            serde_json::to_value(super::dump_room_activity_from_activity(&image_activity))
                .expect("serialize image dump room activity"),
            json!({
                "room_id":"!room:example",
                "activity_id":"event:$image1:example.org",
                "timestamp_ms":1710000010000u64,
                "relation":null,
                "kind":{
                    "type":"image",
                    "event_id":"$image1:example.org",
                    "sender":"@alice:example.org",
                    "body":"diagram.png",
                    "filename":"diagram.png",
                    "source":{
                        "type":"encrypted",
                        "url":"mxc://example.org/diagram"
                    },
                    "info":{
                        "mimetype":"image/png",
                        "size":12345u64,
                        "width":640u64,
                        "height":480u64,
                        "is_animated":false
                    },
                    "media_state":null
                }
            })
        );
        assert_eq!(
            super::dump_room_message_activity_line_from_activity(&image_activity),
            None
        );
        assert!(
            serde_json::from_value::<DumpRoomActivity>(json!({
                "room_id":"!room:example",
                "activity_id":"event:$event:example",
                "timestamp_ms":1700000000000u64,
                "kind":{
                    "type":"message",
                    "event_id":"$event:example",
                    "transaction_id":"txn-1",
                    "sender":"@alice:example",
                    "presentation":"text",
                    "body":"hello",
                    "send_state":{"state":"sent"}
                },
                "unexpected":true
            }))
            .is_err()
        );
        assert!(
            serde_json::from_value::<DumpRoomImageActivity>(json!({
                "event_id":"$image1:example.org",
                "sender":"@alice:example.org",
                "body":"diagram.png",
                "filename":"diagram.png",
                "source":{"type":"plain","url":"mxc://example.org/diagram"},
                "info":{"mimetype":"image/png","size":12345u64,"width":640u64,"height":480u64,"is_animated":false},
                "unexpected":true
            }))
            .is_err()
        );
        assert!(
            serde_json::from_value::<DumpRoomImageMediaState>(json!({
                "type":"failed",
                "message":"unavailable",
                "unexpected":true
            }))
            .is_err()
        );
        assert!(
            serde_json::from_value::<DumpRoomMediaSource>(json!({
                "type":"plain",
                "url":"mxc://example.org/diagram",
                "unexpected":true
            }))
            .is_err()
        );
        assert!(
            serde_json::from_value::<DumpRoomImageInfo>(json!({
                "mimetype":"image/png",
                "size":12345u64,
                "width":640u64,
                "height":480u64,
                "is_animated":false,
                "unexpected":true
            }))
            .is_err()
        );
        assert!(
            serde_json::from_value::<DumpRoomStateNoticeContent>(json!({
                "type":"topic",
                "topic":"incident review",
                "unexpected":true
            }))
            .is_err()
        );
        assert!(
            serde_json::from_value::<DumpRoomActivity>(json!({
                "room_id":"!ops:example",
                "activity_id":"state:$state-1",
                "timestamp_ms":1700000000000u64,
                "kind":{
                    "type":"state_notice",
                    "event_id":"$state-1",
                    "sender":"@alice:example",
                    "state_key":"",
                    "content":{
                        "type":"member",
                        "membership":"join",
                        "displayname":"alice"
                    }
                }
            }))
            .is_ok()
        );
        assert!(
            serde_json::from_value::<DumpRoomActivity>(json!({
                "room_id":"!ops:example",
                "activity_id":"state:$state-1",
                "timestamp_ms":1700000000000u64,
                "kind":{
                    "type":"state_notice",
                    "event_id":"$state-1",
                    "sender":"@alice:example",
                    "state_key":"",
                    "content":{
                        "type":"topic",
                        "topic":"incident review",
                        "unexpected":true
                    }
                }
            }))
            .is_err()
        );
        assert_eq!(
            super::dump_verification_from_state_event(&VerificationStateEvent {
                flow_id: "flow".to_owned(),
                user_id: "@alice:example".to_owned(),
                state: VerificationState::Requested,
            }),
            super::DumpVerification {
                flow_id: "flow".to_owned(),
                user_id: "@alice:example".to_owned(),
                state: VerificationState::Requested,
            }
        );
        assert_eq!(
            serde_json::to_value(super::DumpVerification {
                flow_id: "flow".to_owned(),
                user_id: "@alice:example".to_owned(),
                state: VerificationState::Cancelled {
                    reason: "timed out".to_owned(),
                    code: "m.timeout".to_owned(),
                    cancelled_by_us: false,
                },
            })
            .unwrap(),
            json!({
                "flow_id":"flow",
                "user_id":"@alice:example",
                "state":{
                    "state":"cancelled",
                    "reason":"timed out",
                    "code":"m.timeout",
                    "cancelled_by_us":false
                }
            })
        );
        assert_eq!(
            super::dump_error_from_core(&CoreErrorEvent {
                operation: Operation::SendMessage,
                message: "failed".to_owned(),
            }),
            super::DumpError {
                operation: "send_text".to_owned(),
                message: "failed".to_owned(),
            }
        );
        assert_eq!(
            super::dump_error_from_core(&CoreErrorEvent {
                operation: Operation::MarkRoomRead,
                message: "failed".to_owned(),
            }),
            super::DumpError {
                operation: "mark_room_read".to_owned(),
                message: "failed".to_owned(),
            }
        );
    }
}
