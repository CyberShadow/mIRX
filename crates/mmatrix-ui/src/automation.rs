#![allow(dead_code)]

use mmatrix_core::{
    CoreErrorEvent, Event, Operation, RoomSummary, StoredSession, TimelineText, VerificationRequest,
};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub(crate) const PROTOCOL_VERSION: u32 = 8;
pub(crate) const COMMAND_ID_PING: u32 = 1;
pub(crate) const COMMAND_ID_GET_VERSION: u32 = 2;
pub(crate) const COMMAND_ID_WAIT_IDLE: u32 = 3;
pub(crate) const COMMAND_ID_DUMP_MODEL: u32 = 4;
pub(crate) const COMMAND_ID_DUMP_TIMELINE: u32 = 5;
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CommandKind {
    Ping,
    GetVersion,
    WaitIdle,
    DumpModel,
    DumpTimeline,
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
}

impl CommandKind {
    pub(crate) fn from_id(command_id: u32) -> Option<Self> {
        match command_id {
            COMMAND_ID_PING => Some(Self::Ping),
            COMMAND_ID_GET_VERSION => Some(Self::GetVersion),
            COMMAND_ID_WAIT_IDLE => Some(Self::WaitIdle),
            COMMAND_ID_DUMP_MODEL => Some(Self::DumpModel),
            COMMAND_ID_DUMP_TIMELINE => Some(Self::DumpTimeline),
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
    DumpTimeline {
        id: String,
        args: DumpTimelineArgs,
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
}

impl ParsedCommand {
    pub(crate) fn kind(&self) -> CommandKind {
        match self {
            Self::Ping { .. } => CommandKind::Ping,
            Self::GetVersion { .. } => CommandKind::GetVersion,
            Self::WaitIdle { .. } => CommandKind::WaitIdle,
            Self::DumpModel { .. } => CommandKind::DumpModel,
            Self::DumpTimeline { .. } => CommandKind::DumpTimeline,
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
pub(crate) struct DumpTimelineArgs {
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
pub(crate) struct DumpTimelineResponse {
    pub(crate) room_id: String,
    pub(crate) entries: Vec<DumpTimelineEntry>,
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
    Room { room_id: String },
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
pub(crate) struct MdiView {
    pub(crate) identity: ViewIdentity,
    pub(crate) title: String,
    pub(crate) active: bool,
    pub(crate) lines: Vec<String>,
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
    pub(crate) last_timeline: Option<DumpTimelineLine>,
    pub(crate) last_verification: Option<DumpVerification>,
    pub(crate) last_error: Option<DumpError>,
    pub(crate) selected_room: Option<String>,
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
pub(crate) struct DumpRoom {
    pub(crate) room_id: String,
    pub(crate) display_name: String,
    pub(crate) encrypted: bool,
    pub(crate) is_space: bool,
    pub(crate) space_children: Vec<DumpSpaceChild>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct DumpTimelineLine {
    pub(crate) room_id: String,
    pub(crate) sender: String,
    pub(crate) timestamp_ms: u64,
    pub(crate) body: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum DumpTimelineEntry {
    Text(DumpTimelineLine),
    RoomState(DumpRoomStateLine),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct DumpRoomStateLine {
    pub(crate) room_id: String,
    pub(crate) sender: String,
    pub(crate) timestamp_ms: u64,
    pub(crate) event_kind: DumpRoomStateEventKind,
    pub(crate) state_key: String,
    pub(crate) content: DumpRoomStateContent,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum DumpRoomStateEventKind {
    Topic,
    Member,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum DumpRoomStateContent {
    Topic(DumpRoomTopicState),
    Member(DumpRoomMemberState),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct DumpRoomTopicState {
    pub(crate) topic: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct DumpRoomMemberState {
    pub(crate) membership: String,
    pub(crate) displayname: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, tag = "type", rename_all = "snake_case")]
enum DumpTimelineEntrySerde {
    Text {
        room_id: String,
        sender: String,
        timestamp_ms: u64,
        body: String,
    },
    RoomState {
        room_id: String,
        sender: String,
        timestamp_ms: u64,
        event_kind: DumpRoomStateEventKind,
        state_key: String,
        content: DumpRoomStateContent,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, tag = "type", rename_all = "snake_case")]
enum DumpRoomStateContentSerde {
    Topic {
        topic: String,
    },
    Member {
        membership: String,
        displayname: Option<String>,
    },
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DumpRoomStateLineSerde {
    room_id: String,
    sender: String,
    timestamp_ms: u64,
    event_kind: DumpRoomStateEventKind,
    state_key: String,
    content: DumpRoomStateContent,
}

impl Serialize for DumpTimelineEntry {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        match self {
            Self::Text(line) => DumpTimelineEntrySerde::Text {
                room_id: line.room_id.clone(),
                sender: line.sender.clone(),
                timestamp_ms: line.timestamp_ms,
                body: line.body.clone(),
            },
            Self::RoomState(line) => DumpTimelineEntrySerde::RoomState {
                room_id: line.room_id.clone(),
                sender: line.sender.clone(),
                timestamp_ms: line.timestamp_ms,
                event_kind: line.event_kind,
                state_key: line.state_key.clone(),
                content: line.content.clone(),
            },
        }
        .serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for DumpTimelineEntry {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        match DumpTimelineEntrySerde::deserialize(deserializer)? {
            DumpTimelineEntrySerde::Text {
                room_id,
                sender,
                timestamp_ms,
                body,
            } => Ok(Self::Text(DumpTimelineLine {
                room_id,
                sender,
                timestamp_ms,
                body,
            })),
            DumpTimelineEntrySerde::RoomState {
                room_id,
                sender,
                timestamp_ms,
                event_kind,
                state_key,
                content,
            } => Ok(Self::RoomState(
                validate_dump_room_state_line(DumpRoomStateLineSerde {
                    room_id,
                    sender,
                    timestamp_ms,
                    event_kind,
                    state_key,
                    content,
                })
                .map_err(serde::de::Error::custom)?,
            )),
        }
    }
}

impl Serialize for DumpRoomStateContent {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        match self {
            Self::Topic(topic) => DumpRoomStateContentSerde::Topic {
                topic: topic.topic.clone(),
            },
            Self::Member(member) => DumpRoomStateContentSerde::Member {
                membership: member.membership.clone(),
                displayname: member.displayname.clone(),
            },
        }
        .serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for DumpRoomStateContent {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        match DumpRoomStateContentSerde::deserialize(deserializer)? {
            DumpRoomStateContentSerde::Topic { topic } => {
                Ok(Self::Topic(DumpRoomTopicState { topic }))
            }
            DumpRoomStateContentSerde::Member {
                membership,
                displayname,
            } => Ok(Self::Member(DumpRoomMemberState {
                membership,
                displayname,
            })),
        }
    }
}

impl<'de> Deserialize<'de> for DumpRoomStateLine {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let dto = DumpRoomStateLineSerde::deserialize(deserializer)?;
        validate_dump_room_state_line(dto).map_err(serde::de::Error::custom)
    }
}

fn validate_dump_room_state_line(
    dto: DumpRoomStateLineSerde,
) -> Result<DumpRoomStateLine, &'static str> {
    if !dump_room_state_content_matches_kind(dto.event_kind, &dto.content) {
        return Err("room state event_kind must match content type");
    }

    Ok(DumpRoomStateLine {
        room_id: dto.room_id,
        sender: dto.sender,
        timestamp_ms: dto.timestamp_ms,
        event_kind: dto.event_kind,
        state_key: dto.state_key,
        content: dto.content,
    })
}

fn dump_room_state_content_matches_kind(
    kind: DumpRoomStateEventKind,
    content: &DumpRoomStateContent,
) -> bool {
    matches!(
        (kind, content),
        (
            DumpRoomStateEventKind::Topic,
            DumpRoomStateContent::Topic(_)
        ) | (
            DumpRoomStateEventKind::Member,
            DumpRoomStateContent::Member(_)
        )
    )
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct DumpVerification {
    pub(crate) flow_id: String,
    pub(crate) user_id: String,
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
        Operation::SendText => "send_text",
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

pub(crate) fn dump_timeline_line_from_text(timeline: &TimelineText) -> DumpTimelineLine {
    DumpTimelineLine {
        room_id: timeline.room_id.clone(),
        sender: timeline.sender.clone(),
        timestamp_ms: timeline.timestamp_ms,
        body: timeline.body.clone(),
    }
}

pub(crate) fn dump_verification_from_request(request: &VerificationRequest) -> DumpVerification {
    DumpVerification {
        flow_id: request.flow_id.clone(),
        user_id: request.user_id.clone(),
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
        CommandKind::DumpTimeline => Ok(ParsedCommand::DumpTimeline {
            id: envelope.id.clone(),
            args: parse_args_required::<DumpTimelineArgs>(&envelope.id, envelope.args)?,
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

#[cfg(test)]
mod tests {
    use mmatrix_core::{
        CoreErrorEvent, Event, Operation, RoomSummary, SpaceChildSummary, StoredSession,
        TimelineText, VerificationRequest,
    };
    use serde_json::{Value, json};

    use super::{
        AcceptedResponse, COMMAND_ID_DUMP_COMPOSE_TEXT, COMMAND_ID_DUMP_MODEL,
        COMMAND_ID_DUMP_SELECTED_NAVIGATION_PATH, COMMAND_ID_DUMP_TIMELINE,
        COMMAND_ID_DUMP_UI_STATE, COMMAND_ID_GET_VERSION, COMMAND_ID_LIST_ROOMS,
        COMMAND_ID_LOAD_SYNTHETIC_STATE, COMMAND_ID_LOGIN, COMMAND_ID_LOGOUT, COMMAND_ID_PING,
        COMMAND_ID_RESTORE_SAVED_SESSION, COMMAND_ID_RESTORE_SESSION,
        COMMAND_ID_SELECT_NAVIGATION_PATH, COMMAND_ID_SELECT_ROOM, COMMAND_ID_SELECT_VIEW,
        COMMAND_ID_SEND_COMPOSE_KEY, COMMAND_ID_SEND_TEXT, COMMAND_ID_SET_COMPOSE_TEXT,
        COMMAND_ID_START_SYNC, COMMAND_ID_WAIT_IDLE, CommandKind, ComposeKey, ComposeTextResponse,
        DumpModel, DumpRoomStateContent, DumpRoomStateEventKind, DumpRoomStateLine,
        DumpRoomTopicState, DumpTimelineArgs, DumpTimelineEntry, DumpTimelineLine,
        DumpTimelineResponse, GetVersionResponse, LifecycleState, ListRoomsResponse,
        LoadEventsArgs, LoginArgs, MdiState, MdiView, NavigationNode, NavigationPathSegment,
        NoArgs, ParsedCommand, PongResponse, ResponseEnvelope, RestoreSessionArgs,
        SelectNavigationPathArgs, SelectRoomArgs, SelectRoomResponse, SelectViewArgs,
        SelectViewResponse, SelectedNavigationPathResponse, SendComposeKeyArgs, SendTextArgs,
        SendTextResponse, SetComposeTextArgs, StatusBarState, SwitchbarItem, SwitchbarState,
        UiState, ViewIdentity, WaitIdleArgs, WaitIdleResponse, failure_response,
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
            CommandKind::from_id(COMMAND_ID_DUMP_TIMELINE),
            Some(CommandKind::DumpTimeline)
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
            CommandKind::from_id(COMMAND_ID_LOGIN - 1),
            Some(CommandKind::SelectView)
        );
        assert_eq!(
            CommandKind::from_id(COMMAND_ID_DUMP_SELECTED_NAVIGATION_PATH + 1),
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
                COMMAND_ID_DUMP_TIMELINE,
                br#"{"id":"timeline","args":{"room_id":null}}"#,
            )
            .unwrap(),
            ParsedCommand::DumpTimeline {
                id: "timeline".to_owned(),
                args: DumpTimelineArgs { room_id: None },
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
                br#"{"id":"load","args":{"events":["Started",{"SyncedRooms":[{"room_id":"!room:example","display_name":"Room","encrypted":true,"is_space":false,"space_children":[]}]},{"TimelineText":{"room_id":"!room:example","sender":"@alice:example","timestamp_ms":1700000000000,"body":"hello"}}]}}"#,
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
                            space_children: Vec::new(),
                        }]),
                        Event::TimelineText(TimelineText {
                            room_id: "!room:example".to_owned(),
                            sender: "@alice:example".to_owned(),
                            timestamp_ms: 1_700_000_000_000,
                            body: "hello".to_owned(),
                        }),
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
                space_children: Vec::new(),
            }]),
            Event::VerificationRequest(VerificationRequest {
                flow_id: "flow-1".to_owned(),
                user_id: "@alice:example".to_owned(),
            }),
            Event::Error(CoreErrorEvent {
                operation: Operation::SendText,
                message: "send failed".to_owned(),
            }),
        ];

        assert_eq!(
            serde_json::to_value(&events).unwrap(),
            json!([
                "Started",
                {"SyncedRooms":[{"room_id":"!one:example","display_name":"One","encrypted":false,"is_space":false,"space_children":[]}]},
                {"VerificationRequest":{"flow_id":"flow-1","user_id":"@alice:example"}},
                {"Error":{"operation":"SendText","message":"send failed"}}
            ])
        );
    }

    #[test]
    fn load_command_rejects_unknown_fields_inside_event_payloads() {
        let error = parse_command_request(
            COMMAND_ID_LOAD_SYNTHETIC_STATE,
            br#"{"id":"load","args":{"events":["Started",{"SyncedRooms":[{"room_id":"!one:example","display_name":"One","encrypted":false,"is_space":false,"space_children":[],"unexpected":true}]}]}}"#,
        )
        .unwrap_err();

        assert_eq!(
            serde_json::to_value(error).unwrap(),
            json!({
                "id":"load",
                "ok":false,
                "err":{
                    "code":"invalid_args",
                    "message":"invalid command args: unknown field `unexpected`, expected one of `room_id`, `display_name`, `encrypted`, `is_space`, `space_children`"
                }
            })
        );

        let error = parse_command_request(
            COMMAND_ID_LOAD_SYNTHETIC_STATE,
            br#"{"id":"load","args":{"events":["Started",{"SyncedRooms":[{"room_id":"!space:example","display_name":"Space","encrypted":false,"is_space":true,"space_children":[{"room_id":"!one:example","suggested":true,"order":"aaa","unexpected":true}]}]}]}}"#,
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
    fn response_dtos_match_documented_shapes() {
        assert_eq!(
            serde_json::to_value(success_response("version", GetVersionResponse::current()))
                .unwrap(),
            json!({
                "id":"version",
                "ok":true,
                "json":{
                    "protocol_version":8,
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
                        space_children: Vec::new(),
                    }],
                    last_timeline: None,
                    last_verification: None,
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
                    "rooms":[{"room_id":"!room:example","display_name":"Room","encrypted":true,"is_space":false,"space_children":[]}],
                    "last_timeline":null,
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
                DumpTimelineResponse {
                    room_id: "!room:example".to_owned(),
                    entries: vec![
                        DumpTimelineEntry::Text(DumpTimelineLine {
                            room_id: "!room:example".to_owned(),
                            sender: "@alice:example".to_owned(),
                            timestamp_ms: 1_700_000_000_000,
                            body: "hello".to_owned(),
                        }),
                        DumpTimelineEntry::RoomState(DumpRoomStateLine {
                            room_id: "!room:example".to_owned(),
                            sender: "@alice:example".to_owned(),
                            timestamp_ms: 1_700_000_060_000,
                            event_kind: DumpRoomStateEventKind::Topic,
                            state_key: String::new(),
                            content: DumpRoomStateContent::Topic(DumpRoomTopicState {
                                topic: "incident review".to_owned(),
                            }),
                        }),
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
                            "type":"text",
                            "room_id":"!room:example",
                            "sender":"@alice:example",
                            "timestamp_ms":1700000000000u64,
                            "body":"hello"
                        },
                        {
                            "type":"room_state",
                            "room_id":"!room:example",
                            "sender":"@alice:example",
                            "timestamp_ms":1700000060000u64,
                            "event_kind":"topic",
                            "state_key":"",
                            "content":{
                                "type":"topic",
                                "topic":"incident review"
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
                        space_children: Vec::new(),
                    }],
                },
            ))
            .unwrap(),
            json!({
                "id":"rooms",
                "ok":true,
                "json":{"rooms":[{"room_id":"!room:example","display_name":"Room","encrypted":true,"is_space":false,"space_children":[]}]}
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
                            },
                            MdiView {
                                identity: ViewIdentity::Room {
                                    room_id: "!room:example".to_owned(),
                                },
                                title: "Room".to_owned(),
                                active: true,
                                lines: vec!["hello".to_owned()],
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
                        ],
                    },
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
                                "lines":["status: synced"]
                            },
                            {
                                "identity":{"kind":"room","room_id":"!room:example"},
                                "title":"Room",
                                "active":true,
                                "lines":["hello"]
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
                            }
                        ]
                    }
                }
            })
        );
        assert_eq!(
            serde_json::to_value(success_response(
                "select-view",
                SelectViewResponse {
                    active: ViewIdentity::Status,
                },
            ))
            .unwrap(),
            json!({
                "id":"select-view",
                "ok":true,
                "json":{"active":{"kind":"status"}}
            })
        );
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
                space_children: vec![super::DumpSpaceChild {
                    room_id: "!child:example".to_owned(),
                    suggested: true,
                    order: Some("aaa".to_owned()),
                }],
            }
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
        assert_eq!(
            super::dump_timeline_line_from_text(&TimelineText {
                room_id: "!room:example".to_owned(),
                sender: "@alice:example".to_owned(),
                timestamp_ms: 1_700_000_000_000,
                body: "hello".to_owned(),
            }),
            super::DumpTimelineLine {
                room_id: "!room:example".to_owned(),
                sender: "@alice:example".to_owned(),
                timestamp_ms: 1_700_000_000_000,
                body: "hello".to_owned(),
            }
        );
        assert!(
            serde_json::from_value::<DumpTimelineEntry>(json!({
                "type":"text",
                "room_id":"!room:example",
                "sender":"@alice:example",
                "timestamp_ms":1700000000000u64,
                "body":"hello",
                "unexpected":true
            }))
            .is_err()
        );
        assert!(
            serde_json::from_value::<DumpRoomStateContent>(json!({
                "type":"topic",
                "topic":"incident review",
                "unexpected":true
            }))
            .is_err()
        );
        assert!(
            serde_json::from_value::<DumpRoomStateLine>(json!({
                "room_id":"!room:example",
                "sender":"@alice:example",
                "timestamp_ms":1700000060000u64,
                "event_kind":"topic",
                "state_key":"",
                "content":{
                    "type":"topic",
                    "topic":"incident review",
                    "unexpected":true
                }
            }))
            .is_err()
        );
        assert!(
            serde_json::from_value::<DumpTimelineEntry>(json!({
                "type":"room_state",
                "room_id":"!ops:example",
                "sender":"@alice:example",
                "timestamp_ms":1700000000000u64,
                "event_kind":"topic",
                "state_key":"",
                "content":{
                    "type":"member",
                    "membership":"join",
                    "displayname":"alice"
                }
            }))
            .is_err()
        );
        assert!(
            serde_json::from_value::<DumpRoomStateLine>(json!({
                "room_id":"!ops:example",
                "sender":"@alice:example",
                "timestamp_ms":1700000000000u64,
                "event_kind":"topic",
                "state_key":"",
                "content":{
                    "type":"member",
                    "membership":"join",
                    "displayname":"alice"
                }
            }))
            .is_err()
        );
        assert_eq!(
            super::dump_verification_from_request(&VerificationRequest {
                flow_id: "flow".to_owned(),
                user_id: "@alice:example".to_owned(),
            }),
            super::DumpVerification {
                flow_id: "flow".to_owned(),
                user_id: "@alice:example".to_owned(),
            }
        );
        assert_eq!(
            super::dump_error_from_core(&CoreErrorEvent {
                operation: Operation::SendText,
                message: "failed".to_owned(),
            }),
            super::DumpError {
                operation: "send_text".to_owned(),
                message: "failed".to_owned(),
            }
        );
    }
}
