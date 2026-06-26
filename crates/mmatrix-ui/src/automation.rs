#![allow(dead_code)]

use mmatrix_core::{
    CoreErrorEvent, Event, Operation, RoomSummary, TimelineText, VerificationRequest,
};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub(crate) const PROTOCOL_VERSION: u32 = 1;
pub(crate) const COMMAND_ID_PING: u32 = 1;
pub(crate) const COMMAND_ID_GET_VERSION: u32 = 2;
pub(crate) const COMMAND_ID_WAIT_IDLE: u32 = 3;
pub(crate) const COMMAND_ID_DUMP_MODEL: u32 = 4;
pub(crate) const COMMAND_ID_DUMP_TIMELINE: u32 = 5;
pub(crate) const COMMAND_ID_LIST_ROOMS: u32 = 6;
pub(crate) const COMMAND_ID_SELECT_ROOM: u32 = 7;
pub(crate) const COMMAND_ID_SEND_TEXT: u32 = 8;
pub(crate) const COMMAND_ID_LOAD_SYNTHETIC_STATE: u32 = 9;

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
        args: LoadSyntheticStateArgs,
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
pub(crate) struct LoadSyntheticStateArgs {
    pub(crate) json: SyntheticState,
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
pub(crate) struct PongResponse {
    pub(crate) pong: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
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
pub(crate) struct WaitIdleResponse {
    pub(crate) idle: bool,
    pub(crate) event_generation: u64,
    pub(crate) pending_core_events: u64,
    pub(crate) synthetic_load_in_progress: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct DumpTimelineResponse {
    pub(crate) room_id: String,
    pub(crate) lines: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct ListRoomsResponse {
    pub(crate) rooms: Vec<DumpRoom>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct SelectRoomResponse {
    pub(crate) selected_room: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct SendTextResponse {
    pub(crate) accepted: bool,
    pub(crate) room_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct LoadSyntheticStateResponse {
    pub(crate) loaded: bool,
    pub(crate) rooms: usize,
    pub(crate) timeline_events: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct DumpModel {
    pub(crate) status: String,
    pub(crate) rooms: Vec<DumpRoom>,
    pub(crate) last_timeline: Option<DumpTimelineLine>,
    pub(crate) last_verification: Option<DumpVerification>,
    pub(crate) last_error: Option<DumpError>,
    pub(crate) selected_room: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct DumpRoom {
    pub(crate) room_id: String,
    pub(crate) display_name: String,
    pub(crate) encrypted: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct DumpTimelineLine {
    pub(crate) room_id: String,
    pub(crate) body: String,
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct SyntheticState {
    pub(crate) rooms: Vec<SyntheticRoom>,
    pub(crate) timelines: Vec<SyntheticTimeline>,
    pub(crate) selected_room: Option<String>,
    pub(crate) verification: Option<SyntheticVerification>,
    pub(crate) error: Option<SyntheticError>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct SyntheticRoom {
    pub(crate) room_id: String,
    pub(crate) display_name: String,
    pub(crate) encrypted: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct SyntheticTimeline {
    pub(crate) room_id: String,
    pub(crate) lines: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct SyntheticVerification {
    pub(crate) flow_id: String,
    pub(crate) user_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct SyntheticError {
    pub(crate) operation: String,
    pub(crate) message: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SyntheticLoad {
    pub(crate) events: Vec<Event>,
    pub(crate) selected_room: Option<String>,
    pub(crate) room_count: usize,
    pub(crate) timeline_event_count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum SyntheticStateError {
    SelectedRoomNotFound(String),
    TimelineRoomNotFound(String),
    UnsupportedOperation(String),
}

impl SyntheticState {
    pub(crate) fn validate(&self) -> Result<(), SyntheticStateError> {
        if let Some(room_id) = &self.selected_room
            && !self.rooms.iter().any(|room| room.room_id == *room_id)
        {
            return Err(SyntheticStateError::SelectedRoomNotFound(room_id.clone()));
        }

        for timeline in &self.timelines {
            if !self
                .rooms
                .iter()
                .any(|room| room.room_id == timeline.room_id)
            {
                return Err(SyntheticStateError::TimelineRoomNotFound(
                    timeline.room_id.clone(),
                ));
            }
        }

        if let Some(error) = &self.error
            && parse_operation_code(&error.operation).is_none()
        {
            return Err(SyntheticStateError::UnsupportedOperation(
                error.operation.clone(),
            ));
        }

        Ok(())
    }

    pub(crate) fn to_synthetic_load(&self) -> Result<SyntheticLoad, SyntheticStateError> {
        self.validate()?;

        let mut events = Vec::new();
        events.push(Event::Started);
        events.push(Event::SyncedRooms(
            self.rooms
                .iter()
                .map(|room| RoomSummary {
                    room_id: room.room_id.clone(),
                    display_name: room.display_name.clone(),
                    encrypted: room.encrypted,
                })
                .collect(),
        ));

        let mut timeline_event_count = 0;
        for timeline in &self.timelines {
            for line in &timeline.lines {
                events.push(Event::TimelineText(TimelineText {
                    room_id: timeline.room_id.clone(),
                    body: line.clone(),
                }));
                timeline_event_count += 1;
            }
        }

        if let Some(verification) = &self.verification {
            events.push(Event::VerificationRequest(VerificationRequest {
                flow_id: verification.flow_id.clone(),
                user_id: verification.user_id.clone(),
            }));
        }

        if let Some(error) = &self.error {
            events.push(Event::Error(CoreErrorEvent {
                operation: parse_operation_code(&error.operation)
                    .expect("synthetic error operation must validate before conversion"),
                message: error.message.clone(),
            }));
        }

        Ok(SyntheticLoad {
            events,
            selected_room: self.selected_room.clone(),
            room_count: self.rooms.len(),
            timeline_event_count,
        })
    }
}

pub(crate) fn operation_code(operation: Operation) -> &'static str {
    match operation {
        Operation::Login => "login",
        Operation::RestoreSession => "restore_session",
        Operation::StartSync => "start_sync",
        Operation::SendText => "send_text",
        Operation::Logout => "logout",
        Operation::Shutdown => "shutdown",
    }
}

fn parse_operation_code(operation: &str) -> Option<Operation> {
    match operation {
        "login" => Some(Operation::Login),
        "restore_session" => Some(Operation::RestoreSession),
        "start_sync" => Some(Operation::StartSync),
        "send_text" => Some(Operation::SendText),
        "logout" => Some(Operation::Logout),
        "shutdown" => Some(Operation::Shutdown),
        _ => None,
    }
}

pub(crate) fn dump_error_from_core(error: &CoreErrorEvent) -> DumpError {
    DumpError {
        operation: operation_code(error.operation).to_owned(),
        message: error.message.clone(),
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
            args: parse_args_required::<LoadSyntheticStateArgs>(&envelope.id, envelope.args)?,
        }),
    }
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
        CoreErrorEvent, Event, Operation, RoomSummary, TimelineText, VerificationRequest,
    };
    use serde_json::{Value, json};

    use super::{
        COMMAND_ID_DUMP_MODEL, COMMAND_ID_DUMP_TIMELINE, COMMAND_ID_GET_VERSION,
        COMMAND_ID_LIST_ROOMS, COMMAND_ID_LOAD_SYNTHETIC_STATE, COMMAND_ID_PING,
        COMMAND_ID_SELECT_ROOM, COMMAND_ID_SEND_TEXT, COMMAND_ID_WAIT_IDLE, CommandKind,
        DumpTimelineArgs, DumpTimelineResponse, GetVersionResponse, ListRoomsResponse,
        LoadSyntheticStateArgs, NoArgs, ParsedCommand, PongResponse, ResponseEnvelope,
        SelectRoomArgs, SelectRoomResponse, SendTextArgs, SendTextResponse, SyntheticError,
        SyntheticLoad, SyntheticRoom, SyntheticState, SyntheticStateError, SyntheticTimeline,
        SyntheticVerification, WaitIdleArgs, WaitIdleResponse, failure_response,
        parse_command_request, success_response,
    };

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
                br#"{"id":"load","args":{"json":{"rooms":[],"timelines":[],"selected_room":null,"verification":null,"error":null}}}"#,
            )
            .unwrap(),
            ParsedCommand::LoadSyntheticState {
                id: "load".to_owned(),
                args: LoadSyntheticStateArgs {
                    json: SyntheticState {
                        rooms: Vec::new(),
                        timelines: Vec::new(),
                        selected_room: None,
                        verification: None,
                        error: None,
                    },
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
    fn rejects_unknown_selected_room_before_conversion() {
        let state = SyntheticState {
            rooms: vec![SyntheticRoom {
                room_id: "!known:example".to_owned(),
                display_name: "Known".to_owned(),
                encrypted: false,
            }],
            timelines: Vec::new(),
            selected_room: Some("!missing:example".to_owned()),
            verification: None,
            error: None,
        };

        assert_eq!(
            state.to_synthetic_load(),
            Err(SyntheticStateError::SelectedRoomNotFound(
                "!missing:example".to_owned()
            ))
        );
    }

    #[test]
    fn rejects_unknown_timeline_room_before_conversion() {
        let state = SyntheticState {
            rooms: vec![SyntheticRoom {
                room_id: "!known:example".to_owned(),
                display_name: "Known".to_owned(),
                encrypted: false,
            }],
            timelines: vec![SyntheticTimeline {
                room_id: "!missing:example".to_owned(),
                lines: vec!["hello".to_owned()],
            }],
            selected_room: None,
            verification: None,
            error: None,
        };

        assert_eq!(
            state.to_synthetic_load(),
            Err(SyntheticStateError::TimelineRoomNotFound(
                "!missing:example".to_owned()
            ))
        );
    }

    #[test]
    fn converts_valid_synthetic_state_to_ordered_core_events() {
        let state = SyntheticState {
            rooms: vec![
                SyntheticRoom {
                    room_id: "!one:example".to_owned(),
                    display_name: "One".to_owned(),
                    encrypted: false,
                },
                SyntheticRoom {
                    room_id: "!two:example".to_owned(),
                    display_name: "Two".to_owned(),
                    encrypted: true,
                },
            ],
            timelines: vec![
                SyntheticTimeline {
                    room_id: "!two:example".to_owned(),
                    lines: vec!["first".to_owned(), "second".to_owned()],
                },
                SyntheticTimeline {
                    room_id: "!one:example".to_owned(),
                    lines: vec!["third".to_owned()],
                },
            ],
            selected_room: Some("!two:example".to_owned()),
            verification: Some(SyntheticVerification {
                flow_id: "flow-1".to_owned(),
                user_id: "@alice:example".to_owned(),
            }),
            error: Some(SyntheticError {
                operation: "send_text".to_owned(),
                message: "send failed".to_owned(),
            }),
        };

        let load = state.to_synthetic_load().unwrap();

        assert_eq!(load.selected_room, Some("!two:example".to_owned()));
        assert_eq!(load.room_count, 2);
        assert_eq!(load.timeline_event_count, 3);
        assert_eq!(
            load.events,
            vec![
                Event::Started,
                Event::SyncedRooms(vec![
                    RoomSummary {
                        room_id: "!one:example".to_owned(),
                        display_name: "One".to_owned(),
                        encrypted: false,
                    },
                    RoomSummary {
                        room_id: "!two:example".to_owned(),
                        display_name: "Two".to_owned(),
                        encrypted: true,
                    },
                ]),
                Event::TimelineText(TimelineText {
                    room_id: "!two:example".to_owned(),
                    body: "first".to_owned(),
                }),
                Event::TimelineText(TimelineText {
                    room_id: "!two:example".to_owned(),
                    body: "second".to_owned(),
                }),
                Event::TimelineText(TimelineText {
                    room_id: "!one:example".to_owned(),
                    body: "third".to_owned(),
                }),
                Event::VerificationRequest(VerificationRequest {
                    flow_id: "flow-1".to_owned(),
                    user_id: "@alice:example".to_owned(),
                }),
                Event::Error(CoreErrorEvent {
                    operation: Operation::SendText,
                    message: "send failed".to_owned(),
                }),
            ]
        );
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
                    "protocol_version":1,
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
                "timeline",
                DumpTimelineResponse {
                    room_id: "!room:example".to_owned(),
                    lines: vec!["a".to_owned(), "b".to_owned()],
                },
            ))
            .unwrap(),
            json!({
                "id":"timeline",
                "ok":true,
                "json":{"room_id":"!room:example","lines":["a","b"]}
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
                    }],
                },
            ))
            .unwrap(),
            json!({
                "id":"rooms",
                "ok":true,
                "json":{"rooms":[{"room_id":"!room:example","display_name":"Room","encrypted":true}]}
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
                "load",
                super::LoadSyntheticStateResponse {
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
    }

    #[test]
    fn no_args_is_default_constructible() {
        assert_eq!(NoArgs::default(), NoArgs {});
    }

    #[test]
    fn synthetic_load_retains_counts() {
        let load = SyntheticLoad {
            events: vec![Event::Started],
            selected_room: None,
            room_count: 1,
            timeline_event_count: 0,
        };

        assert_eq!(load.room_count, 1);
        assert_eq!(load.timeline_event_count, 0);
    }
}
