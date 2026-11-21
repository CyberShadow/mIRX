#![allow(dead_code)]

use std::fmt;

use mirx_core::{
    CoreErrorEvent, E2eeStatus, Event, HistoryFrom, Operation, RoomActivity, RoomActivityKind,
    RoomActivityRelation, RoomAudioActivity, RoomAudioInfo, RoomFileActivity, RoomFileInfo,
    RoomImageActivity, RoomImageInfo, RoomMediaSource, RoomMemberChange, RoomMemberRank,
    RoomMembership, RoomMessageActivity, RoomMessagePresentationKind, RoomMessageSendState,
    RoomNotificationMode, RoomPowerLevelValueChange, RoomReadState, RoomRedactedActivity,
    RoomStateNoticeActivity, RoomStateNoticeContent, RoomSummary, RoomTagSummary,
    RoomVideoActivity, RoomVideoInfo, StoredSession, UnsupportedRoomMessageActivity,
    VerificationState, VerificationStateEvent,
};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub(crate) const PROTOCOL_VERSION: u32 = 69;
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
pub(crate) const COMMAND_ID_FIND_ACTIVE_VIEW: u32 = 28;
pub(crate) const COMMAND_ID_DUMP_VERIFICATION_MENU_STATE: u32 = 29;
pub(crate) const COMMAND_ID_DUMP_ROOM_MENU_STATE: u32 = 30;
pub(crate) const COMMAND_ID_DUMP_MESSAGE_MENU_STATE: u32 = 31;
pub(crate) const COMMAND_ID_SELECT_TIMELINE_EVENT: u32 = 32;
pub(crate) const COMMAND_ID_ACCEPT_INVITE_VIA_ROOM_MENU: u32 = 33;
pub(crate) const COMMAND_ID_REJECT_INVITE_VIA_ROOM_MENU: u32 = 34;
pub(crate) const COMMAND_ID_PART_ROOM_VIA_ROOM_MENU: u32 = 35;
pub(crate) const COMMAND_ID_REPLY_TO_SELECTED_MESSAGE_VIA_MENU: u32 = 36;
pub(crate) const COMMAND_ID_REDACT_SELECTED_MESSAGE_VIA_MENU: u32 = 37;
pub(crate) const COMMAND_ID_SAVE_ATTACHMENT_VIA_MESSAGE_MENU: u32 = 38;
pub(crate) const COMMAND_ID_SEND_FILE: u32 = 39;
pub(crate) const COMMAND_ID_SETUP_E2EE_RECOVERY: u32 = 40;
pub(crate) const COMMAND_ID_EDIT_SELECTED_MESSAGE_VIA_MENU: u32 = 41;
pub(crate) const COMMAND_ID_REACT_TO_SELECTED_MESSAGE_VIA_MENU: u32 = 42;
pub(crate) const COMMAND_ID_OPEN_NAVIGATION_CONTEXT_MENU: u32 = 45;
pub(crate) const COMMAND_ID_OPEN_SELECTED_NAVIGATION_CONTEXT_MENU: u32 = 46;
pub(crate) const COMMAND_ID_OPEN_TIMELINE_CONTEXT_MENU: u32 = 47;
pub(crate) const COMMAND_ID_OPEN_SELECTED_TIMELINE_CONTEXT_MENU: u32 = 48;
pub(crate) const COMMAND_ID_DUMP_MEMBER_MENU_STATE: u32 = 49;
pub(crate) const COMMAND_ID_SELECT_MEMBER: u32 = 50;
pub(crate) const COMMAND_ID_QUERY_SELECTED_MEMBER_VIA_MENU: u32 = 51;
pub(crate) const COMMAND_ID_MENTION_SELECTED_MEMBER_VIA_MENU: u32 = 52;
pub(crate) const COMMAND_ID_OPEN_MEMBER_CONTEXT_MENU: u32 = 53;
pub(crate) const COMMAND_ID_OPEN_SELECTED_MEMBER_CONTEXT_MENU: u32 = 54;
pub(crate) const COMMAND_ID_ARRANGE_WINDOWS_VIA_WINDOW_MENU: u32 = 55;
pub(crate) const COMMAND_ID_SET_VIEW_WINDOW_STATE: u32 = 56;
pub(crate) const COMMAND_ID_SELECT_NAVIGATION_VIEW: u32 = 57;
pub(crate) const COMMAND_ID_OPEN_DIALOG: u32 = 58;
pub(crate) const COMMAND_ID_SELECT_OPTIONS_PAGE: u32 = 59;
pub(crate) const COMMAND_ID_SET_NAVIGATION_EXPANDED: u32 = 60;
pub(crate) const COMMAND_ID_CHOOSE_TRAY_MENU_ITEM: u32 = 61;
pub(crate) const COMMAND_ID_CRASH: u32 = 62;

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
    SetupE2eeRecovery,
    AcceptVerification,
    CancelVerification,
    ConfirmVerificationSas,
    RejectVerificationSas,
    FindActiveView,
    DumpVerificationMenuState,
    DumpRoomMenuState,
    DumpMessageMenuState,
    SelectTimelineEvent,
    AcceptInviteViaRoomMenu,
    RejectInviteViaRoomMenu,
    PartRoomViaRoomMenu,
    ReplyToSelectedMessageViaMenu,
    EditSelectedMessageViaMenu,
    ReactToSelectedMessageViaMenu,
    RedactSelectedMessageViaMenu,
    SaveAttachmentViaMessageMenu,
    SendFile,
    OpenNavigationContextMenu,
    OpenSelectedNavigationContextMenu,
    OpenTimelineContextMenu,
    OpenSelectedTimelineContextMenu,
    DumpMemberMenuState,
    SelectMember,
    QuerySelectedMemberViaMenu,
    MentionSelectedMemberViaMenu,
    OpenMemberContextMenu,
    OpenSelectedMemberContextMenu,
    ArrangeWindowsViaWindowMenu,
    SetViewWindowState,
    SelectNavigationView,
    OpenDialog,
    SelectOptionsPage,
    SetNavigationExpanded,
    ChooseTrayMenuItem,
    Crash,
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
            COMMAND_ID_SETUP_E2EE_RECOVERY => Some(Self::SetupE2eeRecovery),
            COMMAND_ID_ACCEPT_VERIFICATION => Some(Self::AcceptVerification),
            COMMAND_ID_CANCEL_VERIFICATION => Some(Self::CancelVerification),
            COMMAND_ID_CONFIRM_VERIFICATION_SAS => Some(Self::ConfirmVerificationSas),
            COMMAND_ID_REJECT_VERIFICATION_SAS => Some(Self::RejectVerificationSas),
            COMMAND_ID_FIND_ACTIVE_VIEW => Some(Self::FindActiveView),
            COMMAND_ID_DUMP_VERIFICATION_MENU_STATE => Some(Self::DumpVerificationMenuState),
            COMMAND_ID_DUMP_ROOM_MENU_STATE => Some(Self::DumpRoomMenuState),
            COMMAND_ID_DUMP_MESSAGE_MENU_STATE => Some(Self::DumpMessageMenuState),
            COMMAND_ID_SELECT_TIMELINE_EVENT => Some(Self::SelectTimelineEvent),
            COMMAND_ID_ACCEPT_INVITE_VIA_ROOM_MENU => Some(Self::AcceptInviteViaRoomMenu),
            COMMAND_ID_REJECT_INVITE_VIA_ROOM_MENU => Some(Self::RejectInviteViaRoomMenu),
            COMMAND_ID_PART_ROOM_VIA_ROOM_MENU => Some(Self::PartRoomViaRoomMenu),
            COMMAND_ID_REPLY_TO_SELECTED_MESSAGE_VIA_MENU => {
                Some(Self::ReplyToSelectedMessageViaMenu)
            }
            COMMAND_ID_EDIT_SELECTED_MESSAGE_VIA_MENU => Some(Self::EditSelectedMessageViaMenu),
            COMMAND_ID_REACT_TO_SELECTED_MESSAGE_VIA_MENU => {
                Some(Self::ReactToSelectedMessageViaMenu)
            }
            COMMAND_ID_REDACT_SELECTED_MESSAGE_VIA_MENU => Some(Self::RedactSelectedMessageViaMenu),
            COMMAND_ID_SAVE_ATTACHMENT_VIA_MESSAGE_MENU => Some(Self::SaveAttachmentViaMessageMenu),
            COMMAND_ID_SEND_FILE => Some(Self::SendFile),
            COMMAND_ID_OPEN_NAVIGATION_CONTEXT_MENU => Some(Self::OpenNavigationContextMenu),
            COMMAND_ID_OPEN_SELECTED_NAVIGATION_CONTEXT_MENU => {
                Some(Self::OpenSelectedNavigationContextMenu)
            }
            COMMAND_ID_OPEN_TIMELINE_CONTEXT_MENU => Some(Self::OpenTimelineContextMenu),
            COMMAND_ID_OPEN_SELECTED_TIMELINE_CONTEXT_MENU => {
                Some(Self::OpenSelectedTimelineContextMenu)
            }
            COMMAND_ID_DUMP_MEMBER_MENU_STATE => Some(Self::DumpMemberMenuState),
            COMMAND_ID_SELECT_MEMBER => Some(Self::SelectMember),
            COMMAND_ID_QUERY_SELECTED_MEMBER_VIA_MENU => Some(Self::QuerySelectedMemberViaMenu),
            COMMAND_ID_MENTION_SELECTED_MEMBER_VIA_MENU => Some(Self::MentionSelectedMemberViaMenu),
            COMMAND_ID_OPEN_MEMBER_CONTEXT_MENU => Some(Self::OpenMemberContextMenu),
            COMMAND_ID_OPEN_SELECTED_MEMBER_CONTEXT_MENU => {
                Some(Self::OpenSelectedMemberContextMenu)
            }
            COMMAND_ID_ARRANGE_WINDOWS_VIA_WINDOW_MENU => Some(Self::ArrangeWindowsViaWindowMenu),
            COMMAND_ID_SET_VIEW_WINDOW_STATE => Some(Self::SetViewWindowState),
            COMMAND_ID_SELECT_NAVIGATION_VIEW => Some(Self::SelectNavigationView),
            COMMAND_ID_OPEN_DIALOG => Some(Self::OpenDialog),
            COMMAND_ID_SELECT_OPTIONS_PAGE => Some(Self::SelectOptionsPage),
            COMMAND_ID_SET_NAVIGATION_EXPANDED => Some(Self::SetNavigationExpanded),
            COMMAND_ID_CHOOSE_TRAY_MENU_ITEM => Some(Self::ChooseTrayMenuItem),
            COMMAND_ID_CRASH => Some(Self::Crash),
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
    SendFile {
        id: String,
        args: SendFileArgs,
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
    /// The compose commands act on the active window's editbox, like mIRC's
    /// `/editbox -a`.
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
    SetupE2eeRecovery {
        id: String,
        args: SetupE2eeRecoveryArgs,
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
    FindActiveView {
        id: String,
        args: FindActiveViewArgs,
    },
    DumpVerificationMenuState {
        id: String,
    },
    DumpRoomMenuState {
        id: String,
    },
    DumpMessageMenuState {
        id: String,
    },
    SelectTimelineEvent {
        id: String,
        args: SelectTimelineEventArgs,
    },
    AcceptInviteViaRoomMenu {
        id: String,
    },
    RejectInviteViaRoomMenu {
        id: String,
    },
    PartRoomViaRoomMenu {
        id: String,
    },
    ReplyToSelectedMessageViaMenu {
        id: String,
    },
    EditSelectedMessageViaMenu {
        id: String,
    },
    ReactToSelectedMessageViaMenu {
        id: String,
    },
    RedactSelectedMessageViaMenu {
        id: String,
    },
    SaveAttachmentViaMessageMenu {
        id: String,
    },
    OpenNavigationContextMenu {
        id: String,
        args: OpenNavigationContextMenuArgs,
    },
    OpenSelectedNavigationContextMenu {
        id: String,
        args: OpenSelectedNavigationContextMenuArgs,
    },
    OpenTimelineContextMenu {
        id: String,
        args: OpenTimelineContextMenuArgs,
    },
    OpenSelectedTimelineContextMenu {
        id: String,
        args: OpenSelectedTimelineContextMenuArgs,
    },
    DumpMemberMenuState {
        id: String,
    },
    SelectMember {
        id: String,
        args: SelectMemberArgs,
    },
    QuerySelectedMemberViaMenu {
        id: String,
    },
    MentionSelectedMemberViaMenu {
        id: String,
    },
    OpenMemberContextMenu {
        id: String,
        args: OpenMemberContextMenuArgs,
    },
    OpenSelectedMemberContextMenu {
        id: String,
        args: OpenSelectedMemberContextMenuArgs,
    },
    ArrangeWindowsViaWindowMenu {
        id: String,
        args: ArrangeWindowsArgs,
    },
    SetViewWindowState {
        id: String,
        args: SetViewWindowStateArgs,
    },
    /// Selects the treebar item of a view, as a click on it does.
    SelectNavigationView {
        id: String,
        args: SelectViewArgs,
    },
    /// Opens a dialog as the user's gesture does. It is posted: the dialog
    /// runs a modal loop, which must not block the request.
    OpenDialog {
        id: String,
        args: OpenDialogArgs,
    },
    /// Selects an Options tree item, as a click on it does.
    SelectOptionsPage {
        id: String,
        args: SelectOptionsPageArgs,
    },
    /// Expands or collapses a treebar item, as its button does.
    SetNavigationExpanded {
        id: String,
        args: SetNavigationExpandedArgs,
    },
    /// Chooses an item of the tray icon's popup by its label, as a click on
    /// it does.
    ChooseTrayMenuItem {
        id: String,
        args: ChooseTrayMenuItemArgs,
    },
    /// Panics on the UI thread with the given message: the crash reporter's
    /// test hook.
    Crash {
        id: String,
        args: CrashArgs,
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
            Self::SendFile { .. } => CommandKind::SendFile,
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
            Self::SetupE2eeRecovery { .. } => CommandKind::SetupE2eeRecovery,
            Self::AcceptVerification { .. } => CommandKind::AcceptVerification,
            Self::CancelVerification { .. } => CommandKind::CancelVerification,
            Self::ConfirmVerificationSas { .. } => CommandKind::ConfirmVerificationSas,
            Self::RejectVerificationSas { .. } => CommandKind::RejectVerificationSas,
            Self::FindActiveView { .. } => CommandKind::FindActiveView,
            Self::DumpVerificationMenuState { .. } => CommandKind::DumpVerificationMenuState,
            Self::DumpRoomMenuState { .. } => CommandKind::DumpRoomMenuState,
            Self::DumpMessageMenuState { .. } => CommandKind::DumpMessageMenuState,
            Self::SelectTimelineEvent { .. } => CommandKind::SelectTimelineEvent,
            Self::AcceptInviteViaRoomMenu { .. } => CommandKind::AcceptInviteViaRoomMenu,
            Self::RejectInviteViaRoomMenu { .. } => CommandKind::RejectInviteViaRoomMenu,
            Self::PartRoomViaRoomMenu { .. } => CommandKind::PartRoomViaRoomMenu,
            Self::ReplyToSelectedMessageViaMenu { .. } => {
                CommandKind::ReplyToSelectedMessageViaMenu
            }
            Self::EditSelectedMessageViaMenu { .. } => CommandKind::EditSelectedMessageViaMenu,
            Self::ReactToSelectedMessageViaMenu { .. } => {
                CommandKind::ReactToSelectedMessageViaMenu
            }
            Self::RedactSelectedMessageViaMenu { .. } => CommandKind::RedactSelectedMessageViaMenu,
            Self::SaveAttachmentViaMessageMenu { .. } => CommandKind::SaveAttachmentViaMessageMenu,
            Self::OpenNavigationContextMenu { .. } => CommandKind::OpenNavigationContextMenu,
            Self::OpenSelectedNavigationContextMenu { .. } => {
                CommandKind::OpenSelectedNavigationContextMenu
            }
            Self::OpenTimelineContextMenu { .. } => CommandKind::OpenTimelineContextMenu,
            Self::OpenSelectedTimelineContextMenu { .. } => {
                CommandKind::OpenSelectedTimelineContextMenu
            }
            Self::DumpMemberMenuState { .. } => CommandKind::DumpMemberMenuState,
            Self::SelectMember { .. } => CommandKind::SelectMember,
            Self::QuerySelectedMemberViaMenu { .. } => CommandKind::QuerySelectedMemberViaMenu,
            Self::MentionSelectedMemberViaMenu { .. } => CommandKind::MentionSelectedMemberViaMenu,
            Self::OpenMemberContextMenu { .. } => CommandKind::OpenMemberContextMenu,
            Self::OpenSelectedMemberContextMenu { .. } => {
                CommandKind::OpenSelectedMemberContextMenu
            }
            Self::ArrangeWindowsViaWindowMenu { .. } => CommandKind::ArrangeWindowsViaWindowMenu,
            Self::SetViewWindowState { .. } => CommandKind::SetViewWindowState,
            Self::SelectNavigationView { .. } => CommandKind::SelectNavigationView,
            Self::OpenDialog { .. } => CommandKind::OpenDialog,
            Self::SelectOptionsPage { .. } => CommandKind::SelectOptionsPage,
            Self::SetNavigationExpanded { .. } => CommandKind::SetNavigationExpanded,
            Self::ChooseTrayMenuItem { .. } => CommandKind::ChooseTrayMenuItem,
            Self::Crash { .. } => CommandKind::Crash,
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
pub(crate) struct SelectTimelineEventArgs {
    pub(crate) event_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct SelectMemberArgs {
    pub(crate) user_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct SendTextArgs {
    pub(crate) room_id: Option<String>,
    pub(crate) body: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct SendFileArgs {
    pub(crate) room_id: Option<String>,
    pub(crate) path: String,
    pub(crate) filename: Option<String>,
    pub(crate) content_type: Option<String>,
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ContextMenuCommand {
    AcceptInvite,
    RejectInvite,
    PartRoom,
    NotifyAllMessages,
    NotifyMentionsAndKeywords,
    NotifyMute,
    ReplyMessage,
    GoToOriginal,
    EditMessage,
    ReactMessage,
    RedactMessage,
    SaveAttachment,
    WhoisMember,
    DevicesMember,
    VerifyMember,
    AcceptIdentityMember,
    QueryMember,
    MentionMember,
    IgnoreMember,
    UnignoreMember,
    OpMember,
    DeopMember,
    VoiceMember,
    DevoiceMember,
    KickMember,
    BanMember,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct OpenNavigationContextMenuArgs {
    pub(crate) path: Vec<NavigationPathSegment>,
    pub(crate) command: ContextMenuCommand,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct OpenSelectedNavigationContextMenuArgs {
    pub(crate) command: ContextMenuCommand,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct OpenTimelineContextMenuArgs {
    pub(crate) event_id: String,
    pub(crate) command: ContextMenuCommand,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct OpenSelectedTimelineContextMenuArgs {
    pub(crate) command: ContextMenuCommand,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct OpenMemberContextMenuArgs {
    pub(crate) user_id: String,
    pub(crate) command: ContextMenuCommand,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct OpenSelectedMemberContextMenuArgs {
    pub(crate) command: ContextMenuCommand,
}

/// The Window menu's arrangements of the MDI windows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum MdiArrangement {
    Cascade,
    TileHorizontal,
    TileVertical,
    ArrangeIcons,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ArrangeWindowsArgs {
    pub(crate) arrangement: MdiArrangement,
}

/// The dialogs automation opens, each as the user's gesture does.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum AutomationDialog {
    /// Tools > Options...
    Options,
    /// Tools > Colors...
    Colors,
    /// File > Select Server...
    SelectServer,
    /// Help > About mirx
    About,
    /// Commands > Join room...
    CommandsJoin,
    /// Tools > Recover E2EE...
    RecoverE2ee,
    /// Tools > Set Up E2EE Recovery...
    SetupE2eeRecovery,
    /// Tools > Reset E2EE Identity...
    ResetE2ee,
    /// The active window's close box, which asks before leaving a room.
    CloseActiveView,
    /// Return in the active window's editbox, for a typed command that
    /// opens a dialog (`/devices delete <session>`); fails on an empty one.
    ComposeReturn,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct OpenDialogArgs {
    pub(crate) dialog: AutomationDialog,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ChooseTrayMenuItemArgs {
    pub(crate) label: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct CrashArgs {
    pub(crate) message: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct SelectOptionsPageArgs {
    /// The tree item's path, `IRC/Messages`.
    pub(crate) path: String,
}

/// A treebar item: a view's (`{"view":{...}}`) or a space folder's
/// (`{"space":"!id"}`); the first such item when there are several.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum NavigationNodeKey {
    View(ViewIdentity),
    Space(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct SetNavigationExpandedArgs {
    pub(crate) node: NavigationNodeKey,
    pub(crate) expanded: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum MdiWindowState {
    Maximized,
    Normal,
    Minimized,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct SetViewWindowStateArgs {
    pub(crate) view: ViewIdentity,
    pub(crate) state: MdiWindowState,
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
pub(crate) struct FindActiveViewArgs {
    pub(crate) query: String,
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

#[derive(Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct SetupE2eeRecoveryArgs {
    pub(crate) passphrase: Option<String>,
    pub(crate) wait_for_backups_to_upload: bool,
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
    /// Commands sent to the core that it has not acked yet, counting acks
    /// still in `pending_core_events`.
    pub(crate) pending_core_commands: u64,
    /// A message posted to the UI thread (input, a menu command, a
    /// follow-up a handler posted) waits to be dispatched.
    pub(crate) posted_messages_pending: bool,
    pub(crate) synthetic_load_in_progress: bool,
}

/// What one WAIT_IDLE probe saw, before the verdict.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct IdleProbe {
    pub(crate) event_generation: u64,
    pub(crate) pending_core_events: u64,
    pub(crate) pending_core_commands: u64,
    pub(crate) posted_messages_pending: bool,
    pub(crate) synthetic_load_in_progress: bool,
    /// The event generation the previous probe saw.
    pub(crate) previous_generation: Option<u64>,
}

impl WaitIdleResponse {
    /// Idle: the core has answered every command it was sent and the UI
    /// applied those answers, nothing posted waits, no synthetic load runs,
    /// and no event arrived since the previous probe (for the core's
    /// background events, such as sync, that no command accounts for).
    pub(crate) fn from_probe(probe: IdleProbe) -> Self {
        Self {
            idle: probe.pending_core_events == 0
                && probe.pending_core_commands == 0
                && !probe.posted_messages_pending
                && !probe.synthetic_load_in_progress
                && probe.previous_generation == Some(probe.event_generation),
            event_generation: probe.event_generation,
            pending_core_events: probe.pending_core_events,
            pending_core_commands: probe.pending_core_commands,
            posted_messages_pending: probe.posted_messages_pending,
            synthetic_load_in_progress: probe.synthetic_load_in_progress,
        }
    }
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
pub(crate) struct DumpVerificationMenuState {
    pub(crate) accept_enabled: bool,
    pub(crate) cancel_enabled: bool,
    pub(crate) confirm_sas_enabled: bool,
    pub(crate) reject_sas_enabled: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct DumpRoomMenuState {
    pub(crate) accept_invite_enabled: bool,
    pub(crate) reject_invite_enabled: bool,
    pub(crate) part_room_enabled: bool,
    /// The mode Notifications checks, if any.
    pub(crate) notification_mode: Option<RoomNotificationMode>,
    pub(crate) notification_items_enabled: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct DumpMessageMenuState {
    pub(crate) reply_enabled: bool,
    pub(crate) edit_enabled: bool,
    pub(crate) react_enabled: bool,
    pub(crate) redact_enabled: bool,
    pub(crate) save_attachment_enabled: bool,
    /// The selected message is a reply with a quote line to go to.
    pub(crate) go_to_original_enabled: bool,
    pub(crate) selected_event_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct DumpMemberMenuState {
    pub(crate) whois_enabled: bool,
    pub(crate) devices_enabled: bool,
    pub(crate) verify_enabled: bool,
    pub(crate) accept_identity_enabled: bool,
    pub(crate) query_enabled: bool,
    pub(crate) mention_enabled: bool,
    pub(crate) ignore_enabled: bool,
    pub(crate) unignore_enabled: bool,
    pub(crate) op_enabled: bool,
    pub(crate) deop_enabled: bool,
    pub(crate) voice_enabled: bool,
    pub(crate) devoice_enabled: bool,
    pub(crate) kick_enabled: bool,
    pub(crate) ban_enabled: bool,
    pub(crate) selected_user_id: Option<String>,
    pub(crate) selected_label: Option<String>,
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
    /// The view the selected item stands for; `None` for a folder.
    pub(crate) view: Option<ViewIdentity>,
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
    pub(crate) frame_layout: UiFrameLayout,
    /// The frame's dots per inch, which its layout is at.
    pub(crate) dpi: u32,
    pub(crate) active_find: Option<UiFindState>,
    pub(crate) rooms: Vec<UiRoomState>,
    pub(crate) text_font: UiTextFont,
    pub(crate) timestamp: UiTimestamp,
    /// Tools > Colors and the switchbar's activity colours.
    pub(crate) colors: UiColors,
    /// The labels of the active window's window menu, separators omitted.
    /// A submenu's items follow it, as `Submenu/Item`.
    pub(crate) active_system_menu_items: Vec<String>,
    /// The labels of the active window's checked window menu items.
    pub(crate) active_system_menu_checked: Vec<String>,
    /// The labels of the active window's greyed window menu items.
    pub(crate) active_system_menu_disabled: Vec<String>,
    /// The frame's menubar as opening each menu would show it: MDI's
    /// maximized-child buttons and separators omitted.
    pub(crate) menu_bar: Vec<UiMenu>,
    /// What the active window's editbox replies to or edits.
    pub(crate) compose_target: Option<UiComposeTarget>,
    /// The Rooms List window, read back from its list control.
    pub(crate) room_list: Option<UiRoomList>,
    /// The Options dialog while it is open, read back from its controls.
    pub(crate) options_dialog: Option<UiOptionsDialog>,
    /// The Colors dialog while it is open.
    pub(crate) colors_dialog: Option<UiColorsDialog>,
    /// The colour index while it shows.
    pub(crate) color_index: Option<UiColorIndex>,
    /// The tray icon and the frame's place behind it.
    pub(crate) tray: UiTray,
    /// The active window's timeline; the Rooms List has none.
    pub(crate) active_timeline: Option<UiActiveTimeline>,
    /// The account's ignore list as the server last listed it; `None`
    /// until the first list.
    pub(crate) ignored_users: Option<Vec<String>>,
    /// How much refresh work the UI has done since it started.
    pub(crate) refresh_counters: UiRefreshCounters,
}

/// Counts of the refresh path's costly work since the UI started. They only
/// grow: tests compare two dumps.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct UiRefreshCounters {
    /// Rows built for a window (Status, room, thread).
    pub(crate) timeline_row_builds: u64,
    /// Log rows wrapped, each measured in the text font.
    pub(crate) rows_wrapped: u64,
    pub(crate) treebar_rebuilds: u64,
    pub(crate) switchbar_rebuilds: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct UiTray {
    /// The shell holds mirx's icon.
    pub(crate) icon: bool,
    /// The frame is hidden in the tray.
    pub(crate) hidden: bool,
    /// The icon's popup, top to bottom.
    pub(crate) menu: Vec<UiTrayMenuItem>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct UiTrayMenuItem {
    pub(crate) label: String,
    /// The window had new messages.
    pub(crate) checked: bool,
}

/// The active window's timeline: the lines it shows now.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct UiActiveTimeline {
    pub(crate) top_line: usize,
    /// Each line on screen, top to bottom, with where to click it in the
    /// timeline's client coordinates.
    pub(crate) visible_lines: Vec<UiTimelineLine>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct UiTimelineLine {
    /// Empty for an image preview's lines.
    pub(crate) text: String,
    pub(crate) kind: UiTimelineLineKind,
    /// The line's colour where its styles set none, `#rrggbb`: its colour
    /// item's.
    pub(crate) color: String,
    /// How `text` is formatted, at offsets in it: a message's own styles.
    pub(crate) styles: Vec<mirx_core::TextStyleSpan>,
    /// The event the line belongs to; clicking it selects that event.
    pub(crate) event_id: Option<String>,
    pub(crate) click_point: UiPoint,
    /// The client x where `text` starts.
    pub(crate) text_left: i32,
    /// The links of the line's row that the line shows, at offsets in
    /// `text`: a link that wraps is in each of its lines.
    pub(crate) links: Vec<UiTimelineLink>,
}

/// A link in a timeline line: `text[start..end]` shows part or all of it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct UiTimelineLink {
    pub(crate) start: usize,
    pub(crate) end: usize,
    /// The whole link as written.
    pub(crate) url: String,
    /// The user a nick or pill links to; `None` for web and room links.
    pub(crate) user_id: Option<String>,
}

/// What a timeline line shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum UiTimelineLineKind {
    Event,
    Info,
    Error,
    Typing,
    ReactionSummary,
    ReplyQuote,
    Message,
    Image,
    File,
    Audio,
    Video,
    Unsupported,
    UnableToDecrypt,
    StateNotice,
    Plain,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct UiOptionsDialog {
    /// Every tree item's path, `Connect/Options`, top to bottom.
    pub(crate) tree: Vec<String>,
    /// The selected item's path.
    pub(crate) selected: String,
    /// The Connect page's controls.
    pub(crate) connect: UiConnectOptions,
    /// The ids of the shown page's controls.
    pub(crate) visible_controls: Vec<u16>,
    /// The Highlight page's list, top to bottom.
    pub(crate) highlight_items: Vec<String>,
    /// Whether the Highlight page's list, word box and Add are enabled
    /// (signed in).
    pub(crate) highlight_controls_enabled: bool,
    /// Whether the Highlight page's Delete is enabled (an item selected).
    pub(crate) highlight_delete_enabled: bool,
    /// The Display page's switchbar Event, Message and Highlight colours.
    pub(crate) activity_colors: UiActivityColors,
    /// The Logging page's checkboxes.
    pub(crate) logging: UiLoggingOptions,
    /// The Tray page's checkboxes.
    pub(crate) tray: UiTrayOptions,
    /// The Tips page's checkboxes.
    pub(crate) tips: UiTipsOptions,
}

/// Options > Connect > Options: its checkboxes, the Retry and Delay
/// fields as typed, and whether those are enabled (retry on).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct UiConnectOptions {
    pub(crate) startup: bool,
    pub(crate) reconnect: bool,
    pub(crate) retry: bool,
    pub(crate) retry_times: String,
    pub(crate) retry_delay: String,
    pub(crate) retry_fields_enabled: bool,
}

/// Options > Display > Tray's checkboxes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct UiTrayOptions {
    pub(crate) always: bool,
    pub(crate) startup: bool,
    pub(crate) minimize: bool,
    pub(crate) single_click: bool,
}

/// Options > Display > Tips' checkboxes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct UiTipsOptions {
    pub(crate) highlights: bool,
    pub(crate) queries: bool,
    pub(crate) channels: bool,
}

/// Options > IRC > Logging's checkboxes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct UiLoggingOptions {
    /// Automatically log channels.
    pub(crate) channels: bool,
    /// Automatically log queries.
    pub(crate) queries: bool,
    pub(crate) strip_codes: bool,
    pub(crate) timestamp: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct UiRoomList {
    /// The header's column titles.
    pub(crate) columns: Vec<String>,
    /// Every row's cells in display order, as the list shows them.
    pub(crate) rows: Vec<Vec<String>>,
    pub(crate) selected: Option<usize>,
    /// Where to click each column header, in the header's client
    /// coordinates.
    pub(crate) column_click_points: Vec<UiPoint>,
    /// Where to click each row the list shows now, from its top row, in the
    /// list's client coordinates.
    pub(crate) visible_row_click_points: Vec<UiPoint>,
    /// The display index of the list's top row.
    pub(crate) top_row: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct UiPoint {
    pub(crate) x: i32,
    pub(crate) y: i32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct UiRect {
    pub(crate) x: i32,
    pub(crate) y: i32,
    pub(crate) width: i32,
    pub(crate) height: i32,
}

/// Where the frame's child windows are, read back from them, in the
/// frame's client coordinates. A hidden bar is `None`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct UiFrameLayout {
    pub(crate) switchbar: Option<UiRect>,
    pub(crate) treebar: Option<UiRect>,
    pub(crate) mdi_client: UiRect,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct UiComposeTarget {
    pub(crate) kind: UiComposeTargetKind,
    pub(crate) event_id: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum UiComposeTargetKind {
    Reply,
    Edit,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct UiMenu {
    /// With its `&` mnemonic, as `GetMenuItemInfoW` returns it.
    pub(crate) label: String,
    pub(crate) items: Vec<UiMenuItem>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct UiMenuItem {
    /// With its `&` mnemonic and any `\t` accelerator text.
    pub(crate) label: String,
    pub(crate) enabled: bool,
    pub(crate) checked: bool,
}

/// The text font as the active window draws it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct UiTextFont {
    /// The face GDI selected, which may differ from the one asked for.
    pub(crate) face: String,
    pub(crate) points: u8,
    pub(crate) bold: bool,
    /// The active timeline's line height in pixels.
    pub(crate) line_height: i32,
    /// The active editbox's height in pixels.
    pub(crate) edit_height: i32,
    /// The text font's average character width (`tmAveCharWidth`): every
    /// character's, in a fixed-pitch face.
    pub(crate) average_char_width: i32,
    /// The fallback faces, in order, that draw characters the text font
    /// has no glyph for.
    pub(crate) fallback_faces: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct UiTimestamp {
    pub(crate) show: bool,
    pub(crate) format: String,
}

/// The colour scheme, as `mirx.ini [colors]` and `[switchbar]` hold it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct UiColors {
    /// Every item of the Colors dialog, in its order.
    pub(crate) items: Vec<UiColorItem>,
    pub(crate) switchbar: UiActivityColors,
}

/// A Colors dialog item's palette index and the colour that is.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct UiColorItem {
    /// Its `mirx.ini` key: `background`, `join`, `editbox_text`, ...
    pub(crate) item: String,
    pub(crate) index: u8,
    /// `#rrggbb`.
    pub(crate) color: String,
}

/// The Colors dialog: what it shows, and where to click it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct UiColorsDialog {
    /// The picked item's `mirx.ini` key.
    pub(crate) selected: String,
    /// The scheme as edited.
    pub(crate) items: Vec<UiColorItem>,
    /// Palette boxes 0-15, in the palette control's client coordinates.
    pub(crate) palette_cells: Vec<UiRect>,
    /// Where a click in the preview picks each item, in its order.
    pub(crate) preview_items: Vec<UiColorsPreviewItem>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct UiColorsPreviewItem {
    pub(crate) item: String,
    /// In the preview control's client coordinates.
    pub(crate) click_point: UiPoint,
}

/// The colour index over the editbox while a `^K` colour number is typed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct UiColorIndex {
    /// Which number of the code is being typed.
    pub(crate) part: UiColorCodePart,
    /// The popup, in screen coordinates.
    pub(crate) rect: UiRect,
    /// Boxes 0-15, in the popup's client coordinates.
    pub(crate) cells: Vec<UiRect>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum UiColorCodePart {
    Foreground,
    Background,
}

/// Options > Display's Event, Message and Highlight palette indices.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct UiActivityColors {
    pub(crate) event: u8,
    pub(crate) message: u8,
    pub(crate) highlight: u8,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct UiFindState {
    pub(crate) active: ViewIdentity,
    pub(crate) query: String,
    pub(crate) matched: bool,
    pub(crate) match_count: usize,
    pub(crate) current_match_index: Option<usize>,
    pub(crate) selected_text: Option<String>,
    pub(crate) history_search: UiFindHistorySearchState,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum UiFindHistorySearchState {
    LocalOnly,
    Loading,
    Exhausted,
    Failed { message: String },
}

/// What happened in a view the user is not looking at, in mIRC's
/// switchbar/treebar colour precedence: a higher level replaces a lower one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ViewActivityLevel {
    /// Joins, parts, topic and power-level changes.
    Event,
    /// Someone talked.
    Message,
    /// A line the user's push rules highlight, e.g. one mentioning them.
    Highlight,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct UiRoomState {
    pub(crate) room_id: String,
    pub(crate) display_name: String,
    pub(crate) membership: RoomMembership,
    pub(crate) encrypted: bool,
    pub(crate) is_space: bool,
    pub(crate) is_direct: bool,
    pub(crate) direct_user_ids: Vec<String>,
    pub(crate) tags: Vec<DumpRoomTag>,
    pub(crate) space_children: Vec<DumpSpaceChild>,
    pub(crate) activity: Option<ViewActivityLevel>,
    pub(crate) read_target_event_id: Option<String>,
    pub(crate) read_state: Option<UiRoomReadState>,
    pub(crate) history_backfill: UiRoomHistoryBackfillState,
    /// Rows built for the room's window; 0 without one.
    pub(crate) timeline_row_builds: u64,
    /// Lines the room's windows keep: its loaded messages and echo lines.
    pub(crate) buffer_lines: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum UiRoomHistoryBackfillState {
    Idle { next: HistoryFrom },
    Loading { from: HistoryFrom },
    Failed { from: HistoryFrom, message: String },
    Exhausted,
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
    /// The view the item stands for; `None` for a folder.
    pub(crate) view: Option<ViewIdentity>,
    /// The space a folder stands for; `None` for a view.
    pub(crate) space_id: Option<String>,
    /// Whether the item shows its children; `None` for an item without.
    pub(crate) expanded: Option<bool>,
    /// The colour the item is drawn with: its view's activity, or for a
    /// collapsed item the highest under it. `None` for items without
    /// activity and for expanded folders.
    pub(crate) activity: Option<ViewActivityLevel>,
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
    /// How many rows the buttons wrap onto.
    pub(crate) rows: u32,
    pub(crate) items: Vec<SwitchbarItem>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(crate) enum ViewIdentity {
    Status,
    /// The Rooms List window (`/list`).
    RoomList,
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
            RoomList,
            Room,
            Thread,
        }

        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct KindOnlyIdentity {
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
                serde_json::from_value::<KindOnlyIdentity>(value)
                    .map_err(serde::de::Error::custom)?;
                Ok(Self::Status)
            }
            Kind::RoomList => {
                serde_json::from_value::<KindOnlyIdentity>(value)
                    .map_err(serde::de::Error::custom)?;
                Ok(Self::RoomList)
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
    Tab,
    ShiftTab,
    Escape,
    /// The mIRC formatting code a Ctrl chord types.
    Format {
        key: MircFormatKey,
    },
}

/// The mIRC formatting chords: Ctrl+B, K, U, I, O, R and E.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum MircFormatKey {
    Bold,
    Color,
    Underline,
    Italic,
    Reset,
    Reverse,
    Strikethrough,
}

impl<'de> Deserialize<'de> for ComposeKey {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        // Struct variants, so that serde rejects unknown fields on every
        // key; it does not for unit variants of a tagged enum.
        #[derive(Deserialize)]
        #[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
        enum Wire {
            Return {},
            Up {},
            Down {},
            Tab {},
            ShiftTab {},
            Escape {},
            Format { key: MircFormatKey },
        }

        Ok(match Wire::deserialize(deserializer)? {
            Wire::Return {} => Self::Return,
            Wire::Up {} => Self::Up,
            Wire::Down {} => Self::Down,
            Wire::Tab {} => Self::Tab,
            Wire::ShiftTab {} => Self::ShiftTab,
            Wire::Escape {} => Self::Escape,
            Wire::Format { key } => Self::Format { key },
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RoomMemberView {
    pub(crate) user_id: String,
    pub(crate) display_name: Option<String>,
    /// The bare nick; the nick list shows it after the rank prefix.
    pub(crate) label: String,
    pub(crate) rank: RoomMemberRank,
    pub(crate) can_kick: bool,
    pub(crate) can_ban: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct MdiView {
    pub(crate) identity: ViewIdentity,
    pub(crate) title: String,
    pub(crate) active: bool,
    pub(crate) placement: MdiWindowPlacement,
    pub(crate) lines: Vec<String>,
    pub(crate) members: Vec<RoomMemberView>,
    /// The log file the window writes to while it logs (a thread window's
    /// is its room's), as Windows names it.
    pub(crate) log: Option<String>,
}

/// An MDI window's state and its rectangle in MDI client coordinates.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct MdiWindowPlacement {
    pub(crate) state: MdiWindowState,
    pub(crate) x: i32,
    pub(crate) y: i32,
    pub(crate) width: i32,
    pub(crate) height: i32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct SwitchbarItem {
    pub(crate) identity: ViewIdentity,
    pub(crate) title: String,
    pub(crate) active: bool,
    pub(crate) activity: Option<ViewActivityLevel>,
    /// The button, in the switchbar's client coordinates.
    pub(crate) rect: UiRect,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct SelectViewResponse {
    pub(crate) active: ViewIdentity,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct FindActiveViewResponse {
    pub(crate) active: ViewIdentity,
    pub(crate) query: String,
    pub(crate) matched: bool,
    pub(crate) match_count: usize,
    pub(crate) current_match_index: Option<usize>,
    pub(crate) selected_text: Option<String>,
    pub(crate) history_search: UiFindHistorySearchState,
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

/// Where the account is on mIRC's connect/disconnect axis: File > Connect
/// and Disconnect move it, and the File menu greys what does not apply.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ConnectionState {
    #[default]
    SignedOut,
    /// A login or session restore is in flight.
    SigningIn,
    /// A browser sign-in waits for the user to finish on the homeserver's
    /// page.
    WaitingForBrowser,
    /// Signed in, not syncing: offline, with the session kept.
    SignedIn,
    /// Signed in, not syncing, and a connect retry is due: mIRC's
    /// `Retry...` delay between attempts.
    WaitingToReconnect,
    Syncing,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct DumpModel {
    pub(crate) status: String,
    pub(crate) lifecycle_state: LifecycleState,
    pub(crate) connection: ConnectionState,
    /// The connect retries since the connection was lost, or since the
    /// user's own Connect.
    pub(crate) reconnect_attempts: u32,
    pub(crate) rooms: Vec<DumpRoom>,
    pub(crate) last_activity: Option<DumpRoomMessageActivityLine>,
    pub(crate) e2ee_status: Option<E2eeStatus>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) last_e2ee_recovery_setup_result: Option<DumpE2eeRecoverySetupResult>,
    pub(crate) last_verification: Option<DumpVerification>,
    /// Every room's identity-change warnings, by room and then user.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) identity_warnings: Vec<DumpIdentityWarning>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) submitted_membership_commands: Vec<DumpSubmittedMembershipCommand>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) submitted_member_moderation_commands: Vec<DumpSubmittedMemberModerationCommand>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) submitted_power_level_commands: Vec<DumpSubmittedPowerLevelCommand>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) submitted_profile_commands: Vec<DumpSubmittedProfileCommand>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) submitted_user_commands: Vec<DumpSubmittedUserCommand>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) submitted_direct_room_commands: Vec<DumpSubmittedDirectRoomCommand>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) submitted_room_topic_commands: Vec<DumpSubmittedRoomTopicCommand>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) submitted_reaction_commands: Vec<DumpSubmittedReactionCommand>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) submitted_edit_commands: Vec<DumpSubmittedEditCommand>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) submitted_reply_commands: Vec<DumpSubmittedReplyCommand>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) submitted_message_commands: Vec<DumpSubmittedMessageCommand>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) submitted_file_commands: Vec<DumpSubmittedFileCommand>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) submitted_notification_commands: Vec<DumpSubmittedNotificationCommand>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) submitted_redaction_commands: Vec<DumpSubmittedRedactionCommand>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) submitted_history_commands: Vec<DumpSubmittedHistoryCommand>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) submitted_verification_actions: Vec<DumpSubmittedVerificationAction>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) submitted_verification_requests: Vec<DumpSubmittedVerificationRequest>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) submitted_recovery_commands: Vec<DumpSubmittedRecoveryCommand>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) submitted_setup_recovery_commands: Vec<DumpSubmittedSetupRecoveryCommand>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) submitted_identity_reset_commands: Vec<DumpSubmittedIdentityReset>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) submitted_room_key_transfers: Vec<DumpSubmittedRoomKeyTransfer>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) submitted_session_commands: Vec<DumpSubmittedSessionCommand>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) submitted_room_list_requests: Vec<DumpSubmittedRoomListRequest>,
    /// The web links handed to the shell, in order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) opened_links: Vec<String>,
    /// The tips the shell took, in order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) shown_tips: Vec<DumpShownTip>,
    pub(crate) last_error: Option<DumpError>,
    pub(crate) selected_room: Option<String>,
}

/// A tip: the window it is about, its title and its line.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct DumpShownTip {
    pub(crate) view: ViewIdentity,
    pub(crate) title: String,
    pub(crate) text: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct DumpSubmittedMembershipCommand {
    pub(crate) action: DumpMembershipCommandAction,
    pub(crate) target: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct DumpSubmittedMemberModerationCommand {
    pub(crate) action: DumpMemberModerationAction,
    pub(crate) room_id: String,
    pub(crate) user_id: String,
    /// Always `None` for invites, which take no reason.
    pub(crate) reason: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum DumpMemberModerationAction {
    Kick,
    Ban,
    Unban,
    Invite,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct DumpSubmittedPowerLevelCommand {
    pub(crate) room_id: String,
    pub(crate) user_id: String,
    pub(crate) role: DumpMemberPowerRole,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum DumpMemberPowerRole {
    Moderator,
    Voiced,
    Default,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct DumpSubmittedProfileCommand {
    pub(crate) display_name: String,
}

/// A request about another user (`/ignore`, `/unignore`, `/whois`,
/// `/devices`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct DumpSubmittedUserCommand {
    pub(crate) action: DumpUserCommandAction,
    pub(crate) user_id: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum DumpUserCommandAction {
    Ignore,
    Unignore,
    FetchProfile,
    ListDevices,
    AcceptIdentity,
}

/// A room directory page asked for (`/list`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct DumpSubmittedRoomListRequest {
    pub(crate) server: Option<String>,
    pub(crate) filter: Option<String>,
    pub(crate) since: Option<String>,
    pub(crate) limit: u32,
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct DumpSubmittedReactionCommand {
    pub(crate) room_id: String,
    pub(crate) target_event_id: String,
    pub(crate) key: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct DumpSubmittedEditCommand {
    pub(crate) room_id: String,
    pub(crate) target_event_id: String,
    pub(crate) body: String,
    pub(crate) styles: Vec<mirx_core::TextStyleSpan>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct DumpSubmittedReplyCommand {
    pub(crate) room_id: String,
    pub(crate) target_event_id: String,
    pub(crate) sender_user_id: String,
    pub(crate) body: String,
    pub(crate) styles: Vec<mirx_core::TextStyleSpan>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) thread: Option<DumpSubmittedReplyThreadTarget>,
}

/// A message the UI sent that replies to nothing; replies are in
/// `submitted_reply_commands`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct DumpSubmittedMessageCommand {
    pub(crate) room_id: String,
    pub(crate) presentation: RoomMessagePresentationKind,
    pub(crate) body: String,
    pub(crate) styles: Vec<mirx_core::TextStyleSpan>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) thread: Option<DumpSubmittedReplyThreadTarget>,
}

/// A notification settings change the UI sent: a room's mode or a
/// highlight word.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum DumpSubmittedNotificationCommand {
    SetRoomNotificationMode {
        room_id: String,
        mode: RoomNotificationMode,
    },
    AddHighlightWord {
        word: String,
    },
    RemoveHighlightWord {
        word: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct DumpSubmittedFileCommand {
    pub(crate) room_id: String,
    pub(crate) path: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct DumpSubmittedReplyThreadTarget {
    pub(crate) thread_root_event_id: String,
    pub(crate) latest_event_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct DumpSubmittedRedactionCommand {
    pub(crate) room_id: String,
    pub(crate) event_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct DumpSubmittedHistoryCommand {
    pub(crate) room_id: String,
    pub(crate) from: HistoryFrom,
    pub(crate) limit: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum DumpMembershipCommandAction {
    Join,
    Leave,
    AcceptInvite,
    RejectInvite,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct DumpSubmittedVerificationAction {
    pub(crate) action: DumpVerificationActionKind,
    pub(crate) flow_id: String,
    pub(crate) user_id: String,
}

/// A verification started (`/verify [nick [device]]`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct DumpSubmittedVerificationRequest {
    pub(crate) user_id: String,
    pub(crate) device_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct DumpSubmittedRecoveryCommand {
    pub(crate) secret_redacted: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct DumpSubmittedSetupRecoveryCommand {
    pub(crate) passphrase_redacted: bool,
    pub(crate) wait_for_backups_to_upload: bool,
}

/// An identity reset (`/e2ee reset`) or its cancel; never the password.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum DumpSubmittedIdentityReset {
    Reset { password_redacted: bool },
    Cancel,
}

/// A room key file export or import (`/e2ee export|import`); never the
/// passphrase.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct DumpSubmittedRoomKeyTransfer {
    pub(crate) direction: DumpRoomKeyTransferDirection,
    pub(crate) path: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum DumpRoomKeyTransferDirection {
    Export,
    Import,
}

/// A `/devices delete` or `/devices rename`; never the password.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum DumpSubmittedSessionCommand {
    SignOut {
        device_id: String,
        password_redacted: bool,
    },
    Rename {
        device_id: String,
        display_name: String,
    },
}

/// A room member whose identity changed since this session accepted or
/// verified it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct DumpIdentityWarning {
    pub(crate) room_id: String,
    pub(crate) user_id: String,
    pub(crate) warning: DumpIdentityWarningKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum DumpIdentityWarningKind {
    Changed,
    VerifiedChanged,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct DumpE2eeRecoverySetupResult {
    pub(crate) recovery_key_present: bool,
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
    pub(crate) topic: Option<String>,
    pub(crate) membership: RoomMembership,
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
    pub(crate) highlight: bool,
    pub(crate) relation: Option<DumpRoomActivityRelation>,
    pub(crate) reactions: Vec<DumpRoomReactionSummary>,
    pub(crate) kind: DumpRoomActivityKind,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct DumpRoomReactionSummary {
    pub(crate) key: String,
    pub(crate) count: u64,
    pub(crate) senders: Vec<String>,
    pub(crate) reaction_event_ids: Vec<String>,
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
    Image(Box<DumpRoomImageActivity>),
    File(Box<DumpRoomFileActivity>),
    Audio(Box<DumpRoomAudioActivity>),
    Video(Box<DumpRoomVideoActivity>),
    UnsupportedMessage(DumpUnsupportedRoomMessageActivity),
    UnableToDecrypt(DumpRoomUnableToDecryptActivity),
    StateNotice(DumpRoomStateNoticeActivity),
    Redacted(DumpRoomRedactedActivity),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct DumpRoomUnableToDecryptActivity {
    pub(crate) event_id: String,
    pub(crate) sender: String,
    pub(crate) session_id: Option<String>,
    pub(crate) reason: mirx_core::RoomUnableToDecryptReason,
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
    pub(crate) media_state: Option<DumpRoomPreviewMediaState>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct DumpRoomFileActivity {
    pub(crate) event_id: String,
    pub(crate) sender: String,
    pub(crate) body: String,
    pub(crate) filename: Option<String>,
    pub(crate) source: DumpRoomMediaSource,
    pub(crate) info: Option<DumpRoomFileInfo>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct DumpRoomAudioActivity {
    pub(crate) event_id: String,
    pub(crate) sender: String,
    pub(crate) body: String,
    pub(crate) filename: Option<String>,
    pub(crate) source: DumpRoomMediaSource,
    pub(crate) info: Option<DumpRoomAudioInfo>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct DumpRoomVideoActivity {
    pub(crate) event_id: String,
    pub(crate) sender: String,
    pub(crate) body: String,
    pub(crate) filename: Option<String>,
    pub(crate) source: DumpRoomMediaSource,
    pub(crate) info: Option<DumpRoomVideoInfo>,
    pub(crate) media_state: Option<DumpRoomPreviewMediaState>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum DumpRoomPreviewMediaState {
    Pending,
    Loaded { width: u32, height: u32 },
    Failed { message: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum DumpRoomMediaSource {
    Plain {
        url: String,
    },
    Encrypted {
        url: String,
        key: DumpRoomEncryptedFileKey,
        iv: String,
        hashes: DumpRoomEncryptedFileHashes,
        v: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct DumpRoomEncryptedFileKey {
    pub(crate) kty: String,
    pub(crate) key_ops: Vec<String>,
    pub(crate) alg: String,
    pub(crate) k: String,
    pub(crate) ext: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct DumpRoomEncryptedFileHashes {
    pub(crate) sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct DumpRoomImageInfo {
    pub(crate) mimetype: Option<String>,
    pub(crate) size: Option<u64>,
    pub(crate) width: Option<u64>,
    pub(crate) height: Option<u64>,
    pub(crate) is_animated: Option<bool>,
    pub(crate) thumbnail_source: Option<DumpRoomMediaSource>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct DumpRoomFileInfo {
    pub(crate) mimetype: Option<String>,
    pub(crate) size: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct DumpRoomAudioInfo {
    pub(crate) mimetype: Option<String>,
    pub(crate) size: Option<u64>,
    pub(crate) duration_ms: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct DumpRoomVideoInfo {
    pub(crate) mimetype: Option<String>,
    pub(crate) size: Option<u64>,
    pub(crate) duration_ms: Option<u64>,
    pub(crate) width: Option<u64>,
    pub(crate) height: Option<u64>,
    pub(crate) thumbnail_source: Option<DumpRoomMediaSource>,
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
#[serde(deny_unknown_fields)]
pub(crate) struct DumpRoomRedactedActivity {
    pub(crate) event_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, tag = "type", rename_all = "snake_case")]
pub(crate) enum DumpRoomStateNoticeContent {
    Topic {
        topic: String,
    },
    Member {
        change: RoomMemberChange,
        target_display_name: Option<String>,
        reason: Option<String>,
    },
    PowerLevels {
        changes: Vec<DumpRoomPowerLevelUserChange>,
        users_default: Option<RoomPowerLevelValueChange>,
        other_permissions_changed: bool,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct DumpRoomPowerLevelUserChange {
    pub(crate) user_id: String,
    pub(crate) old: i64,
    pub(crate) new: i64,
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
        Operation::StopSync => "stop_sync",
        Operation::JoinRoom => "join_room",
        Operation::AcceptRoomInvite => "accept_room_invite",
        Operation::CreateDirectRoom => "create_direct_room",
        Operation::CreateRoom => "create_room",
        Operation::LeaveRoom => "leave_room",
        Operation::RejectRoomInvite => "reject_room_invite",
        Operation::KickRoomMember => "kick_room_member",
        Operation::BanRoomMember => "ban_room_member",
        Operation::UnbanRoomMember => "unban_room_member",
        Operation::InviteRoomMember => "invite_room_member",
        Operation::SetRoomMemberPowerLevel => "set_room_member_power_level",
        Operation::SetDisplayName => "set_display_name",
        Operation::SetRoomTopic => "set_room_topic",
        Operation::SetRoomNotificationMode => "set_room_notification_mode",
        Operation::HighlightWords => "highlight_words",
        Operation::AddHighlightWord => "add_highlight_word",
        Operation::RemoveHighlightWord => "remove_highlight_word",
        Operation::IgnoredUsers => "ignored_users",
        Operation::IgnoreUser => "ignore_user",
        Operation::UnignoreUser => "unignore_user",
        Operation::FetchUserProfile => "fetch_user_profile",
        Operation::ListUserDevices => "list_user_devices",
        Operation::AcceptUserIdentity => "accept_user_identity",
        Operation::SignOutDevice => "sign_out_device",
        Operation::RenameDevice => "rename_device",
        Operation::SendMessage => "send_text",
        Operation::SendFile => "send_file",
        Operation::EditMessage => "edit_message",
        Operation::SendReaction => "send_reaction",
        Operation::RedactEvent => "redact_event",
        Operation::MarkRoomRead => "mark_room_read",
        Operation::BackfillRoomHistory => "backfill_room_history",
        Operation::FetchRoomMedia => "fetch_room_media",
        Operation::FetchRoomEvent => "fetch_room_event",
        Operation::ListPublicRooms => "list_public_rooms",
        Operation::RoomList => "room_list",
        Operation::RoomReadState => "room_read_state",
        Operation::SendQueue => "send_queue",
        Operation::E2eeStatus => "e2ee_status",
        Operation::RecoverE2ee => "recover_e2ee",
        Operation::SetupE2eeRecovery => "setup_e2ee_recovery",
        Operation::ResetE2eeIdentity => "reset_e2ee_identity",
        Operation::RetryDecryption => "retry_decryption",
        Operation::ExportRoomKeys => "export_room_keys",
        Operation::ImportRoomKeys => "import_room_keys",
        Operation::Verification => "verification",
        Operation::RequestVerification => "request_verification",
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
        topic: room.topic.clone(),
        membership: room.membership,
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
        highlight: activity.highlight,
        relation: activity
            .relation
            .as_ref()
            .map(dump_room_activity_relation_from_core),
        reactions: Vec::new(),
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
        sender: message.sender.user_id.clone(),
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
            DumpRoomActivityKind::Image(Box::new(dump_room_image_activity_from_core(image)))
        }
        RoomActivityKind::File(file) => {
            DumpRoomActivityKind::File(Box::new(dump_room_file_activity_from_core(file)))
        }
        RoomActivityKind::Audio(audio) => {
            DumpRoomActivityKind::Audio(Box::new(dump_room_audio_activity_from_core(audio)))
        }
        RoomActivityKind::Video(video) => {
            DumpRoomActivityKind::Video(Box::new(dump_room_video_activity_from_core(video)))
        }
        RoomActivityKind::UnsupportedMessage(message) => DumpRoomActivityKind::UnsupportedMessage(
            dump_unsupported_room_message_activity_from_core(message),
        ),
        RoomActivityKind::UnableToDecrypt(message) => {
            DumpRoomActivityKind::UnableToDecrypt(DumpRoomUnableToDecryptActivity {
                event_id: message.event_id.clone(),
                sender: message.sender.user_id.clone(),
                session_id: message.session_id.clone(),
                reason: message.reason.clone(),
            })
        }
        RoomActivityKind::StateNotice(notice) => {
            DumpRoomActivityKind::StateNotice(dump_room_state_notice_activity_from_core(notice))
        }
        RoomActivityKind::Redacted(redacted) => {
            DumpRoomActivityKind::Redacted(dump_room_redacted_activity_from_core(redacted))
        }
    }
}

pub(crate) fn dump_room_message_activity_from_core(
    message: &RoomMessageActivity,
) -> DumpRoomMessageActivity {
    DumpRoomMessageActivity {
        event_id: message.event_id.clone(),
        transaction_id: message.transaction_id.clone(),
        sender: message.sender.user_id.clone(),
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
        sender: image.sender.user_id.clone(),
        body: image.body.clone(),
        filename: image.filename.clone(),
        source: dump_room_media_source_from_core(&image.source),
        info: image.info.as_ref().map(dump_room_image_info_from_core),
        media_state: None,
    }
}

pub(crate) fn dump_room_file_activity_from_core(file: &RoomFileActivity) -> DumpRoomFileActivity {
    DumpRoomFileActivity {
        event_id: file.event_id.clone(),
        sender: file.sender.user_id.clone(),
        body: file.body.clone(),
        filename: file.filename.clone(),
        source: dump_room_media_source_from_core(&file.source),
        info: file.info.as_ref().map(dump_room_file_info_from_core),
    }
}

pub(crate) fn dump_room_audio_activity_from_core(
    audio: &RoomAudioActivity,
) -> DumpRoomAudioActivity {
    DumpRoomAudioActivity {
        event_id: audio.event_id.clone(),
        sender: audio.sender.user_id.clone(),
        body: audio.body.clone(),
        filename: audio.filename.clone(),
        source: dump_room_media_source_from_core(&audio.source),
        info: audio.info.as_ref().map(dump_room_audio_info_from_core),
    }
}

pub(crate) fn dump_room_video_activity_from_core(
    video: &RoomVideoActivity,
) -> DumpRoomVideoActivity {
    DumpRoomVideoActivity {
        event_id: video.event_id.clone(),
        sender: video.sender.user_id.clone(),
        body: video.body.clone(),
        filename: video.filename.clone(),
        source: dump_room_media_source_from_core(&video.source),
        info: video.info.as_ref().map(dump_room_video_info_from_core),
        media_state: None,
    }
}

pub(crate) fn dump_room_media_source_from_core(source: &RoomMediaSource) -> DumpRoomMediaSource {
    match source {
        RoomMediaSource::Plain { url } => DumpRoomMediaSource::Plain { url: url.clone() },
        RoomMediaSource::Encrypted {
            url,
            key,
            iv,
            hashes,
            v,
        } => DumpRoomMediaSource::Encrypted {
            url: url.clone(),
            key: DumpRoomEncryptedFileKey {
                kty: key.kty.clone(),
                key_ops: key.key_ops.clone(),
                alg: key.alg.clone(),
                k: key.k.clone(),
                ext: key.ext,
            },
            iv: iv.clone(),
            hashes: DumpRoomEncryptedFileHashes {
                sha256: hashes.sha256.clone(),
            },
            v: v.clone(),
        },
    }
}

pub(crate) fn dump_room_image_info_from_core(info: &RoomImageInfo) -> DumpRoomImageInfo {
    DumpRoomImageInfo {
        mimetype: info.mimetype.clone(),
        size: info.size,
        width: info.width,
        height: info.height,
        is_animated: info.is_animated,
        thumbnail_source: info
            .thumbnail_source
            .as_ref()
            .map(dump_room_media_source_from_core),
    }
}

pub(crate) fn dump_room_file_info_from_core(info: &RoomFileInfo) -> DumpRoomFileInfo {
    DumpRoomFileInfo {
        mimetype: info.mimetype.clone(),
        size: info.size,
    }
}

pub(crate) fn dump_room_audio_info_from_core(info: &RoomAudioInfo) -> DumpRoomAudioInfo {
    DumpRoomAudioInfo {
        mimetype: info.mimetype.clone(),
        size: info.size,
        duration_ms: info.duration_ms,
    }
}

pub(crate) fn dump_room_video_info_from_core(info: &RoomVideoInfo) -> DumpRoomVideoInfo {
    DumpRoomVideoInfo {
        mimetype: info.mimetype.clone(),
        size: info.size,
        duration_ms: info.duration_ms,
        width: info.width,
        height: info.height,
        thumbnail_source: info
            .thumbnail_source
            .as_ref()
            .map(dump_room_media_source_from_core),
    }
}

pub(crate) fn dump_unsupported_room_message_activity_from_core(
    message: &UnsupportedRoomMessageActivity,
) -> DumpUnsupportedRoomMessageActivity {
    DumpUnsupportedRoomMessageActivity {
        event_id: message.event_id.clone(),
        sender: message.sender.user_id.clone(),
        msgtype: message.msgtype.clone(),
        body: message.body.clone(),
    }
}

pub(crate) fn dump_room_state_notice_activity_from_core(
    notice: &RoomStateNoticeActivity,
) -> DumpRoomStateNoticeActivity {
    DumpRoomStateNoticeActivity {
        event_id: notice.event_id.clone(),
        sender: notice.sender.user_id.clone(),
        state_key: match &notice.content {
            // Topic and power levels are room state with an empty state key.
            RoomStateNoticeContent::Topic { .. } | RoomStateNoticeContent::PowerLevels { .. } => {
                String::new()
            }
            RoomStateNoticeContent::Member { target, .. } => target.user_id.clone(),
        },
        content: dump_room_state_notice_content_from_core(&notice.content),
    }
}

pub(crate) fn dump_room_redacted_activity_from_core(
    redacted: &RoomRedactedActivity,
) -> DumpRoomRedactedActivity {
    DumpRoomRedactedActivity {
        event_id: redacted.event_id.clone(),
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
            change,
            target,
            reason,
        } => DumpRoomStateNoticeContent::Member {
            change: change.clone(),
            target_display_name: target.display_name.clone(),
            reason: reason.clone(),
        },
        RoomStateNoticeContent::PowerLevels {
            changes,
            users_default,
            other_permissions_changed,
        } => DumpRoomStateNoticeContent::PowerLevels {
            changes: changes
                .iter()
                .map(|change| DumpRoomPowerLevelUserChange {
                    user_id: change.target.user_id.clone(),
                    old: change.old,
                    new: change.new,
                })
                .collect(),
            users_default: users_default.clone(),
            other_permissions_changed: *other_permissions_changed,
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
        CommandKind::SendFile => Ok(ParsedCommand::SendFile {
            id: envelope.id.clone(),
            args: parse_args_required::<SendFileArgs>(&envelope.id, envelope.args)?,
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
        CommandKind::FindActiveView => Ok(ParsedCommand::FindActiveView {
            id: envelope.id.clone(),
            args: parse_args_required::<FindActiveViewArgs>(&envelope.id, envelope.args)?,
        }),
        CommandKind::DumpVerificationMenuState => {
            parse_args_with_default::<NoArgs>(&envelope.id, envelope.args)?;
            Ok(ParsedCommand::DumpVerificationMenuState { id: envelope.id })
        }
        CommandKind::DumpRoomMenuState => {
            parse_args_with_default::<NoArgs>(&envelope.id, envelope.args)?;
            Ok(ParsedCommand::DumpRoomMenuState { id: envelope.id })
        }
        CommandKind::DumpMessageMenuState => {
            parse_args_with_default::<NoArgs>(&envelope.id, envelope.args)?;
            Ok(ParsedCommand::DumpMessageMenuState { id: envelope.id })
        }
        CommandKind::SelectTimelineEvent => Ok(ParsedCommand::SelectTimelineEvent {
            id: envelope.id.clone(),
            args: parse_args_required::<SelectTimelineEventArgs>(&envelope.id, envelope.args)?,
        }),
        CommandKind::AcceptInviteViaRoomMenu => {
            parse_args_with_default::<NoArgs>(&envelope.id, envelope.args)?;
            Ok(ParsedCommand::AcceptInviteViaRoomMenu { id: envelope.id })
        }
        CommandKind::RejectInviteViaRoomMenu => {
            parse_args_with_default::<NoArgs>(&envelope.id, envelope.args)?;
            Ok(ParsedCommand::RejectInviteViaRoomMenu { id: envelope.id })
        }
        CommandKind::PartRoomViaRoomMenu => {
            parse_args_with_default::<NoArgs>(&envelope.id, envelope.args)?;
            Ok(ParsedCommand::PartRoomViaRoomMenu { id: envelope.id })
        }
        CommandKind::ReplyToSelectedMessageViaMenu => {
            parse_args_with_default::<NoArgs>(&envelope.id, envelope.args)?;
            Ok(ParsedCommand::ReplyToSelectedMessageViaMenu { id: envelope.id })
        }
        CommandKind::EditSelectedMessageViaMenu => {
            parse_args_with_default::<NoArgs>(&envelope.id, envelope.args)?;
            Ok(ParsedCommand::EditSelectedMessageViaMenu { id: envelope.id })
        }
        CommandKind::ReactToSelectedMessageViaMenu => {
            parse_args_with_default::<NoArgs>(&envelope.id, envelope.args)?;
            Ok(ParsedCommand::ReactToSelectedMessageViaMenu { id: envelope.id })
        }
        CommandKind::RedactSelectedMessageViaMenu => {
            parse_args_with_default::<NoArgs>(&envelope.id, envelope.args)?;
            Ok(ParsedCommand::RedactSelectedMessageViaMenu { id: envelope.id })
        }
        CommandKind::SaveAttachmentViaMessageMenu => {
            parse_args_with_default::<NoArgs>(&envelope.id, envelope.args)?;
            Ok(ParsedCommand::SaveAttachmentViaMessageMenu { id: envelope.id })
        }
        CommandKind::OpenNavigationContextMenu => Ok(ParsedCommand::OpenNavigationContextMenu {
            id: envelope.id.clone(),
            args: parse_args_required::<OpenNavigationContextMenuArgs>(
                &envelope.id,
                envelope.args,
            )?,
        }),
        CommandKind::OpenSelectedNavigationContextMenu => {
            Ok(ParsedCommand::OpenSelectedNavigationContextMenu {
                id: envelope.id.clone(),
                args: parse_args_required::<OpenSelectedNavigationContextMenuArgs>(
                    &envelope.id,
                    envelope.args,
                )?,
            })
        }
        CommandKind::OpenTimelineContextMenu => Ok(ParsedCommand::OpenTimelineContextMenu {
            id: envelope.id.clone(),
            args: parse_args_required::<OpenTimelineContextMenuArgs>(&envelope.id, envelope.args)?,
        }),
        CommandKind::OpenSelectedTimelineContextMenu => {
            Ok(ParsedCommand::OpenSelectedTimelineContextMenu {
                id: envelope.id.clone(),
                args: parse_args_required::<OpenSelectedTimelineContextMenuArgs>(
                    &envelope.id,
                    envelope.args,
                )?,
            })
        }
        CommandKind::DumpMemberMenuState => {
            parse_args_with_default::<NoArgs>(&envelope.id, envelope.args)?;
            Ok(ParsedCommand::DumpMemberMenuState { id: envelope.id })
        }
        CommandKind::SelectMember => Ok(ParsedCommand::SelectMember {
            id: envelope.id.clone(),
            args: parse_args_required::<SelectMemberArgs>(&envelope.id, envelope.args)?,
        }),
        CommandKind::QuerySelectedMemberViaMenu => {
            parse_args_with_default::<NoArgs>(&envelope.id, envelope.args)?;
            Ok(ParsedCommand::QuerySelectedMemberViaMenu { id: envelope.id })
        }
        CommandKind::MentionSelectedMemberViaMenu => {
            parse_args_with_default::<NoArgs>(&envelope.id, envelope.args)?;
            Ok(ParsedCommand::MentionSelectedMemberViaMenu { id: envelope.id })
        }
        CommandKind::OpenMemberContextMenu => Ok(ParsedCommand::OpenMemberContextMenu {
            id: envelope.id.clone(),
            args: parse_args_required::<OpenMemberContextMenuArgs>(&envelope.id, envelope.args)?,
        }),
        CommandKind::OpenSelectedMemberContextMenu => {
            Ok(ParsedCommand::OpenSelectedMemberContextMenu {
                id: envelope.id.clone(),
                args: parse_args_required::<OpenSelectedMemberContextMenuArgs>(
                    &envelope.id,
                    envelope.args,
                )?,
            })
        }
        CommandKind::ArrangeWindowsViaWindowMenu => {
            Ok(ParsedCommand::ArrangeWindowsViaWindowMenu {
                id: envelope.id.clone(),
                args: parse_args_required::<ArrangeWindowsArgs>(&envelope.id, envelope.args)?,
            })
        }
        CommandKind::SetViewWindowState => Ok(ParsedCommand::SetViewWindowState {
            id: envelope.id.clone(),
            args: parse_args_required::<SetViewWindowStateArgs>(&envelope.id, envelope.args)?,
        }),
        CommandKind::SelectNavigationView => Ok(ParsedCommand::SelectNavigationView {
            id: envelope.id.clone(),
            args: parse_args_required::<SelectViewArgs>(&envelope.id, envelope.args)?,
        }),
        CommandKind::OpenDialog => Ok(ParsedCommand::OpenDialog {
            id: envelope.id.clone(),
            args: parse_args_required::<OpenDialogArgs>(&envelope.id, envelope.args)?,
        }),
        CommandKind::SelectOptionsPage => Ok(ParsedCommand::SelectOptionsPage {
            id: envelope.id.clone(),
            args: parse_args_required::<SelectOptionsPageArgs>(&envelope.id, envelope.args)?,
        }),
        CommandKind::SetNavigationExpanded => Ok(ParsedCommand::SetNavigationExpanded {
            id: envelope.id.clone(),
            args: parse_args_required::<SetNavigationExpandedArgs>(&envelope.id, envelope.args)?,
        }),
        CommandKind::ChooseTrayMenuItem => Ok(ParsedCommand::ChooseTrayMenuItem {
            id: envelope.id.clone(),
            args: parse_args_required::<ChooseTrayMenuItemArgs>(&envelope.id, envelope.args)?,
        }),
        CommandKind::Crash => Ok(ParsedCommand::Crash {
            id: envelope.id.clone(),
            args: parse_args_required::<CrashArgs>(&envelope.id, envelope.args)?,
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
        CommandKind::SetupE2eeRecovery => Ok(ParsedCommand::SetupE2eeRecovery {
            id: envelope.id.clone(),
            args: parse_setup_e2ee_recovery_args(&envelope.id, envelope.args)?,
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
    "mirx-ui".to_owned()
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

fn parse_setup_e2ee_recovery_args(
    id: &str,
    args: Option<Value>,
) -> Result<SetupE2eeRecoveryArgs, FailureEnvelope> {
    let args = args.ok_or_else(|| {
        failure_response(
            Some(id.to_owned()),
            "invalid_args",
            "missing args object for setup_e2ee_recovery command",
        )
    })?;
    let Value::Object(args) = args else {
        return Err(failure_response(
            Some(id.to_owned()),
            "invalid_args",
            "invalid setup_e2ee_recovery args: expected object",
        ));
    };
    if args
        .keys()
        .any(|key| !matches!(key.as_str(), "passphrase" | "wait_for_backups_to_upload"))
    {
        return Err(failure_response(
            Some(id.to_owned()),
            "invalid_args",
            "invalid setup_e2ee_recovery args: unexpected field; expected only `passphrase` and `wait_for_backups_to_upload`",
        ));
    }
    let passphrase = match args.get("passphrase") {
        None | Some(Value::Null) => None,
        Some(Value::String(value)) => Some(value.clone()),
        Some(_) => {
            return Err(failure_response(
                Some(id.to_owned()),
                "invalid_args",
                "invalid setup_e2ee_recovery args: `passphrase` must be a string or null",
            ));
        }
    };
    let Some(Value::Bool(wait_for_backups_to_upload)) = args.get("wait_for_backups_to_upload")
    else {
        return Err(failure_response(
            Some(id.to_owned()),
            "invalid_args",
            "invalid setup_e2ee_recovery args: `wait_for_backups_to_upload` must be a boolean",
        ));
    };
    Ok(SetupE2eeRecoveryArgs {
        passphrase,
        wait_for_backups_to_upload: *wait_for_backups_to_upload,
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
    use mirx_core::{
        CoreErrorEvent, E2eeBackupState, E2eeBackupStatus, E2eeCrossSigningStatus,
        E2eeRecoveryState, E2eeSessionState, E2eeStatus, Event, HistoryFrom, Operation,
        RoomActivity, RoomActivityKind, RoomActivityRelation, RoomAudioActivity, RoomAudioInfo,
        RoomEncryptedFileHashes, RoomEncryptedFileKey, RoomFileActivity, RoomFileInfo,
        RoomImageActivity, RoomImageInfo, RoomMediaSource, RoomMemberRank, RoomMembership,
        RoomMessageActivity, RoomMessagePresentationKind, RoomMessageSendState,
        RoomNotificationMode, RoomReadState, RoomRedactedActivity, RoomSummary, RoomTagSummary,
        RoomVideoActivity, RoomVideoInfo, SessionAuth, SpaceChildSummary, StoredSession,
        UnsupportedRoomMessageActivity, VerificationDirection, VerificationRequest,
        VerificationState, VerificationStateEvent,
    };
    use serde_json::{Value, json};

    use super::{
        AcceptedResponse, ArrangeWindowsArgs, AutomationDialog,
        COMMAND_ID_ACCEPT_INVITE_VIA_ROOM_MENU, COMMAND_ID_ACCEPT_VERIFICATION,
        COMMAND_ID_APPEND_SYNTHETIC_EVENTS, COMMAND_ID_ARRANGE_WINDOWS_VIA_WINDOW_MENU,
        COMMAND_ID_CANCEL_VERIFICATION, COMMAND_ID_CHOOSE_TRAY_MENU_ITEM,
        COMMAND_ID_CONFIRM_VERIFICATION_SAS, COMMAND_ID_CRASH, COMMAND_ID_DUMP_COMPOSE_TEXT,
        COMMAND_ID_DUMP_MEMBER_MENU_STATE, COMMAND_ID_DUMP_MESSAGE_MENU_STATE,
        COMMAND_ID_DUMP_MODEL, COMMAND_ID_DUMP_ROOM_ACTIVITY, COMMAND_ID_DUMP_ROOM_MENU_STATE,
        COMMAND_ID_DUMP_SELECTED_NAVIGATION_PATH, COMMAND_ID_DUMP_UI_STATE,
        COMMAND_ID_DUMP_VERIFICATION_MENU_STATE, COMMAND_ID_EDIT_SELECTED_MESSAGE_VIA_MENU,
        COMMAND_ID_FIND_ACTIVE_VIEW, COMMAND_ID_GET_VERSION, COMMAND_ID_LIST_ROOMS,
        COMMAND_ID_LOAD_SYNTHETIC_STATE, COMMAND_ID_LOGIN, COMMAND_ID_LOGOUT,
        COMMAND_ID_MENTION_SELECTED_MEMBER_VIA_MENU, COMMAND_ID_OPEN_DIALOG,
        COMMAND_ID_OPEN_MEMBER_CONTEXT_MENU, COMMAND_ID_OPEN_NAVIGATION_CONTEXT_MENU,
        COMMAND_ID_OPEN_SELECTED_MEMBER_CONTEXT_MENU,
        COMMAND_ID_OPEN_SELECTED_NAVIGATION_CONTEXT_MENU,
        COMMAND_ID_OPEN_SELECTED_TIMELINE_CONTEXT_MENU, COMMAND_ID_OPEN_TIMELINE_CONTEXT_MENU,
        COMMAND_ID_PART_ROOM_VIA_ROOM_MENU, COMMAND_ID_PING,
        COMMAND_ID_QUERY_SELECTED_MEMBER_VIA_MENU, COMMAND_ID_REACT_TO_SELECTED_MESSAGE_VIA_MENU,
        COMMAND_ID_RECOVER_E2EE, COMMAND_ID_REDACT_SELECTED_MESSAGE_VIA_MENU,
        COMMAND_ID_REJECT_INVITE_VIA_ROOM_MENU, COMMAND_ID_REJECT_VERIFICATION_SAS,
        COMMAND_ID_REPLY_TO_SELECTED_MESSAGE_VIA_MENU, COMMAND_ID_RESTORE_SAVED_SESSION,
        COMMAND_ID_RESTORE_SESSION, COMMAND_ID_SAVE_ATTACHMENT_VIA_MESSAGE_MENU,
        COMMAND_ID_SELECT_MEMBER, COMMAND_ID_SELECT_NAVIGATION_PATH,
        COMMAND_ID_SELECT_NAVIGATION_VIEW, COMMAND_ID_SELECT_OPTIONS_PAGE, COMMAND_ID_SELECT_ROOM,
        COMMAND_ID_SELECT_TIMELINE_EVENT, COMMAND_ID_SELECT_VIEW, COMMAND_ID_SEND_COMPOSE_KEY,
        COMMAND_ID_SEND_FILE, COMMAND_ID_SEND_TEXT, COMMAND_ID_SET_COMPOSE_TEXT,
        COMMAND_ID_SET_NAVIGATION_EXPANDED, COMMAND_ID_SET_VIEW_WINDOW_STATE,
        COMMAND_ID_SETUP_E2EE_RECOVERY, COMMAND_ID_START_SYNC, COMMAND_ID_WAIT_IDLE,
        ChooseTrayMenuItemArgs, CommandKind, ComposeKey, ComposeTextResponse, ConnectionState,
        ContextMenuCommand, CrashArgs, DumpE2eeRecoverySetupResult, DumpMemberModerationAction,
        DumpMemberPowerRole, DumpModel, DumpRoomActivity, DumpRoomActivityArgs,
        DumpRoomActivityKind, DumpRoomActivityRelation, DumpRoomActivityResponse,
        DumpRoomAudioActivity, DumpRoomAudioInfo, DumpRoomEncryptedFileHashes,
        DumpRoomEncryptedFileKey, DumpRoomFileActivity, DumpRoomFileInfo, DumpRoomImageActivity,
        DumpRoomImageInfo, DumpRoomMediaSource, DumpRoomMessageActivity,
        DumpRoomMessageActivityLine, DumpRoomMessageSendState, DumpRoomPreviewMediaState,
        DumpRoomReactionSummary, DumpRoomRedactedActivity, DumpRoomStateNoticeActivity,
        DumpRoomStateNoticeContent, DumpRoomVideoActivity, DumpRoomVideoInfo,
        DumpSubmittedEditCommand, DumpSubmittedHistoryCommand,
        DumpSubmittedMemberModerationCommand, DumpSubmittedPowerLevelCommand,
        DumpSubmittedProfileCommand, DumpSubmittedReactionCommand, DumpSubmittedRedactionCommand,
        DumpSubmittedReplyCommand, DumpSubmittedReplyThreadTarget, DumpSubmittedRoomTopicCommand,
        DumpSubmittedSetupRecoveryCommand, DumpSubmittedUserCommand,
        DumpSubmittedVerificationRequest, DumpUnsupportedRoomMessageActivity,
        DumpUserCommandAction, FindActiveViewArgs, FindActiveViewResponse, GetVersionResponse,
        IdleProbe, LifecycleState, ListRoomsResponse, LoadEventsArgs, LoginArgs, MdiArrangement,
        MdiState, MdiView, MdiWindowPlacement, MdiWindowState, MircFormatKey, NavigationNode,
        NavigationNodeKey, NavigationPathSegment, NoArgs, OpenDialogArgs,
        OpenMemberContextMenuArgs, OpenSelectedMemberContextMenuArgs, PROTOCOL_VERSION,
        ParsedCommand, PongResponse, RecoverE2eeArgs, ResponseEnvelope, RestoreSessionArgs,
        RoomMemberView, SelectMemberArgs, SelectNavigationPathArgs, SelectOptionsPageArgs,
        SelectRoomArgs, SelectRoomResponse, SelectTimelineEventArgs, SelectViewArgs,
        SelectViewResponse, SelectedNavigationPathResponse, SendComposeKeyArgs, SendFileArgs,
        SendTextArgs, SendTextResponse, SetComposeTextArgs, SetNavigationExpandedArgs,
        SetViewWindowStateArgs, SetupE2eeRecoveryArgs, StatusBarState, SwitchbarItem,
        SwitchbarState, UiFindHistorySearchState, UiFindState, UiFrameLayout, UiRect,
        UiRoomHistoryBackfillState, UiRoomReadState, UiRoomState, UiState, VerificationActionArgs,
        ViewActivityLevel, ViewIdentity, WaitIdleArgs, WaitIdleResponse, failure_response,
        parse_command_request, success_response,
    };

    fn user_profile(user_id: &str) -> mirx_core::RoomUserProfile {
        mirx_core::RoomUserProfile {
            user_id: user_id.to_owned(),
            display_name: None,
            display_name_ambiguous: false,
        }
    }
    fn stored_session() -> StoredSession {
        StoredSession {
            user_id: "@alice:example".to_owned(),
            device_id: "DEVICE".to_owned(),
            access_token: "secret-token".to_owned(),
            refresh_token: Some("refresh-token".to_owned()),
            auth: SessionAuth::Matrix,
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
            session: E2eeSessionState::Verified,
        }
    }

    fn sample_encrypted_file_key() -> RoomEncryptedFileKey {
        RoomEncryptedFileKey {
            kty: "oct".to_owned(),
            key_ops: vec!["encrypt".to_owned(), "decrypt".to_owned()],
            alg: "A256CTR".to_owned(),
            k: "base64url-key".to_owned(),
            ext: true,
        }
    }

    fn sample_encrypted_file_hashes() -> RoomEncryptedFileHashes {
        RoomEncryptedFileHashes {
            sha256: "base64-sha256".to_owned(),
        }
    }

    fn sample_encrypted_room_media_source(url: &str) -> RoomMediaSource {
        RoomMediaSource::Encrypted {
            url: url.to_owned(),
            key: sample_encrypted_file_key(),
            iv: "base64-iv".to_owned(),
            hashes: sample_encrypted_file_hashes(),
            v: "v2".to_owned(),
        }
    }

    fn sample_dump_encrypted_room_media_source(url: &str) -> DumpRoomMediaSource {
        DumpRoomMediaSource::Encrypted {
            url: url.to_owned(),
            key: DumpRoomEncryptedFileKey {
                kty: "oct".to_owned(),
                key_ops: vec!["encrypt".to_owned(), "decrypt".to_owned()],
                alg: "A256CTR".to_owned(),
                k: "base64url-key".to_owned(),
                ext: true,
            },
            iv: "base64-iv".to_owned(),
            hashes: DumpRoomEncryptedFileHashes {
                sha256: "base64-sha256".to_owned(),
            },
            v: "v2".to_owned(),
        }
    }

    #[test]
    fn member_command_dumps_deserialize_into_testkit_model() {
        let dump = json!({
            "status":"ready",
            "lifecycle_state":"ready",
            "connection":"syncing", "reconnect_attempts":0,
            "rooms":[],
            "last_activity":null,
            "e2ee_status":null,
            "last_verification":null,
            "last_error":null,
            "selected_room":null
        });
        let mut model = serde_json::from_value::<DumpModel>(dump).unwrap();
        model.submitted_member_moderation_commands = vec![
            DumpSubmittedMemberModerationCommand {
                action: DumpMemberModerationAction::Kick,
                room_id: "!ops:example".to_owned(),
                user_id: "@bob:example".to_owned(),
                reason: Some("spamming".to_owned()),
            },
            DumpSubmittedMemberModerationCommand {
                action: DumpMemberModerationAction::Invite,
                room_id: "!ops:example".to_owned(),
                user_id: "@carol:example".to_owned(),
                reason: None,
            },
        ];
        model.submitted_power_level_commands = vec![DumpSubmittedPowerLevelCommand {
            room_id: "!ops:example".to_owned(),
            user_id: "@bob:example".to_owned(),
            role: DumpMemberPowerRole::Voiced,
        }];
        model.submitted_profile_commands = vec![DumpSubmittedProfileCommand {
            display_name: "New Name".to_owned(),
        }];
        model.submitted_user_commands = vec![DumpSubmittedUserCommand {
            action: DumpUserCommandAction::FetchProfile,
            user_id: "@bob:example".to_owned(),
        }];
        model.submitted_verification_requests = vec![
            DumpSubmittedVerificationRequest {
                user_id: "@me:example".to_owned(),
                device_id: None,
            },
            DumpSubmittedVerificationRequest {
                user_id: "@bob:example".to_owned(),
                device_id: Some("DEV1".to_owned()),
            },
        ];

        let testkit_model =
            serde_json::from_value::<mirx_testkit::Model>(serde_json::to_value(&model).unwrap())
                .unwrap();
        assert_eq!(
            testkit_model.submitted_member_moderation_commands,
            vec![
                mirx_testkit::SubmittedMemberModerationCommand {
                    action: mirx_testkit::MemberModerationAction::Kick,
                    room_id: "!ops:example".to_owned(),
                    user_id: "@bob:example".to_owned(),
                    reason: Some("spamming".to_owned()),
                },
                mirx_testkit::SubmittedMemberModerationCommand {
                    action: mirx_testkit::MemberModerationAction::Invite,
                    room_id: "!ops:example".to_owned(),
                    user_id: "@carol:example".to_owned(),
                    reason: None,
                },
            ]
        );
        assert_eq!(
            testkit_model.submitted_power_level_commands,
            vec![mirx_testkit::SubmittedPowerLevelCommand {
                room_id: "!ops:example".to_owned(),
                user_id: "@bob:example".to_owned(),
                role: mirx_testkit::MemberPowerRole::Voiced,
            }]
        );
        assert_eq!(
            testkit_model.submitted_profile_commands,
            vec![mirx_testkit::SubmittedProfileCommand {
                display_name: "New Name".to_owned(),
            }]
        );
        assert_eq!(
            testkit_model.submitted_user_commands,
            vec![mirx_testkit::SubmittedUserCommand {
                action: mirx_testkit::UserCommandAction::FetchProfile,
                user_id: "@bob:example".to_owned(),
            }]
        );
        assert_eq!(
            testkit_model.submitted_verification_requests,
            vec![
                mirx_testkit::SubmittedVerificationRequest {
                    user_id: "@me:example".to_owned(),
                    device_id: None,
                },
                mirx_testkit::SubmittedVerificationRequest {
                    user_id: "@bob:example".to_owned(),
                    device_id: Some("DEV1".to_owned()),
                },
            ]
        );
    }

    #[test]
    fn identity_dumps_have_their_wire_shapes_and_deserialize_into_testkit_model() {
        let dump = json!({
            "status":"ready",
            "lifecycle_state":"ready",
            "connection":"syncing", "reconnect_attempts":0,
            "rooms":[],
            "last_activity":null,
            "e2ee_status":null,
            "last_verification":null,
            "identity_warnings":[
                {"room_id":"!ops:example","user_id":"@bob:example","warning":"changed"},
                {"room_id":"!ops:example","user_id":"@carol:example","warning":"verified_changed"}
            ],
            "submitted_user_commands":[{"action":"accept_identity","user_id":"@bob:example"}],
            "submitted_identity_reset_commands":[
                {"action":"reset","password_redacted":true},
                {"action":"cancel"}
            ],
            "submitted_room_key_transfers":[
                {"direction":"export","path":"C:\\keys.txt"},
                {"direction":"import","path":"D:\\keys.txt"}
            ],
            "submitted_session_commands":[
                {"action":"sign_out","device_id":"PH0NE","password_redacted":true},
                {"action":"rename","device_id":"PH0NE","display_name":"Work phone"}
            ],
            "last_error":null,
            "selected_room":null
        });
        let model = serde_json::from_value::<DumpModel>(dump.clone()).unwrap();
        assert_eq!(
            model.submitted_identity_reset_commands,
            vec![
                super::DumpSubmittedIdentityReset::Reset {
                    password_redacted: true
                },
                super::DumpSubmittedIdentityReset::Cancel,
            ]
        );
        assert_eq!(
            model.submitted_room_key_transfers,
            vec![
                super::DumpSubmittedRoomKeyTransfer {
                    direction: super::DumpRoomKeyTransferDirection::Export,
                    path: "C:\\keys.txt".to_owned(),
                },
                super::DumpSubmittedRoomKeyTransfer {
                    direction: super::DumpRoomKeyTransferDirection::Import,
                    path: "D:\\keys.txt".to_owned(),
                },
            ]
        );
        assert_eq!(
            model.submitted_session_commands,
            vec![
                super::DumpSubmittedSessionCommand::SignOut {
                    device_id: "PH0NE".to_owned(),
                    password_redacted: true,
                },
                super::DumpSubmittedSessionCommand::Rename {
                    device_id: "PH0NE".to_owned(),
                    display_name: "Work phone".to_owned(),
                },
            ]
        );
        assert_eq!(serde_json::to_value(&model).unwrap(), dump);

        let testkit_model = serde_json::from_value::<mirx_testkit::Model>(dump).unwrap();
        assert_eq!(
            testkit_model.submitted_session_commands,
            vec![
                mirx_testkit::SubmittedSessionCommand::SignOut {
                    device_id: "PH0NE".to_owned(),
                    password_redacted: true,
                },
                mirx_testkit::SubmittedSessionCommand::Rename {
                    device_id: "PH0NE".to_owned(),
                    display_name: "Work phone".to_owned(),
                },
            ]
        );
        assert_eq!(
            testkit_model.identity_warnings,
            vec![
                mirx_testkit::IdentityWarning {
                    room_id: "!ops:example".to_owned(),
                    user_id: "@bob:example".to_owned(),
                    warning: mirx_testkit::IdentityWarningKind::Changed,
                },
                mirx_testkit::IdentityWarning {
                    room_id: "!ops:example".to_owned(),
                    user_id: "@carol:example".to_owned(),
                    warning: mirx_testkit::IdentityWarningKind::VerifiedChanged,
                },
            ]
        );
        assert_eq!(
            testkit_model.submitted_user_commands[0].action,
            mirx_testkit::UserCommandAction::AcceptIdentity
        );
        assert_eq!(
            testkit_model.submitted_identity_reset_commands,
            vec![
                mirx_testkit::SubmittedIdentityReset::Reset {
                    password_redacted: true
                },
                mirx_testkit::SubmittedIdentityReset::Cancel,
            ]
        );
        assert_eq!(
            testkit_model.submitted_room_key_transfers,
            vec![
                mirx_testkit::SubmittedRoomKeyTransfer {
                    direction: mirx_testkit::RoomKeyTransferDirection::Export,
                    path: "C:\\keys.txt".to_owned(),
                },
                mirx_testkit::SubmittedRoomKeyTransfer {
                    direction: mirx_testkit::RoomKeyTransferDirection::Import,
                    path: "D:\\keys.txt".to_owned(),
                },
            ]
        );
    }

    fn idle_probe(previous_generation: Option<u64>) -> IdleProbe {
        IdleProbe {
            event_generation: 7,
            pending_core_events: 0,
            pending_core_commands: 0,
            posted_messages_pending: false,
            synthetic_load_in_progress: false,
            previous_generation,
        }
    }

    #[test]
    fn idle_needs_every_command_answered_nothing_posted_and_a_settled_generation() {
        assert!(WaitIdleResponse::from_probe(idle_probe(Some(7))).idle);
        for probe in [
            // The first probe, or one after an event, settles nothing yet.
            idle_probe(None),
            idle_probe(Some(6)),
            IdleProbe {
                pending_core_events: 1,
                ..idle_probe(Some(7))
            },
            // The core has not answered a command yet: its refusal may be
            // on the way.
            IdleProbe {
                pending_core_commands: 1,
                ..idle_probe(Some(7))
            },
            // Input the driver posted before asking has not been handled.
            IdleProbe {
                posted_messages_pending: true,
                ..idle_probe(Some(7))
            },
            IdleProbe {
                synthetic_load_in_progress: true,
                ..idle_probe(Some(7))
            },
        ] {
            assert!(!WaitIdleResponse::from_probe(probe).idle, "{probe:?}");
        }
    }

    #[test]
    fn protocol_version_is_69() {
        assert_eq!(PROTOCOL_VERSION, 69);
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
            CommandKind::from_id(COMMAND_ID_FIND_ACTIVE_VIEW),
            Some(CommandKind::FindActiveView)
        );
        assert_eq!(
            CommandKind::from_id(COMMAND_ID_DUMP_VERIFICATION_MENU_STATE),
            Some(CommandKind::DumpVerificationMenuState)
        );
        assert_eq!(
            CommandKind::from_id(COMMAND_ID_DUMP_ROOM_MENU_STATE),
            Some(CommandKind::DumpRoomMenuState)
        );
        assert_eq!(
            CommandKind::from_id(COMMAND_ID_DUMP_MESSAGE_MENU_STATE),
            Some(CommandKind::DumpMessageMenuState)
        );
        assert_eq!(
            CommandKind::from_id(COMMAND_ID_OPEN_SELECTED_MEMBER_CONTEXT_MENU),
            Some(CommandKind::OpenSelectedMemberContextMenu)
        );
        assert_eq!(
            CommandKind::from_id(COMMAND_ID_SELECT_TIMELINE_EVENT),
            Some(CommandKind::SelectTimelineEvent)
        );
        assert_eq!(
            CommandKind::from_id(COMMAND_ID_ACCEPT_INVITE_VIA_ROOM_MENU),
            Some(CommandKind::AcceptInviteViaRoomMenu)
        );
        assert_eq!(
            CommandKind::from_id(COMMAND_ID_REJECT_INVITE_VIA_ROOM_MENU),
            Some(CommandKind::RejectInviteViaRoomMenu)
        );
        assert_eq!(
            CommandKind::from_id(COMMAND_ID_PART_ROOM_VIA_ROOM_MENU),
            Some(CommandKind::PartRoomViaRoomMenu)
        );
        assert_eq!(
            CommandKind::from_id(COMMAND_ID_OPEN_NAVIGATION_CONTEXT_MENU),
            Some(CommandKind::OpenNavigationContextMenu)
        );
        assert_eq!(
            CommandKind::from_id(COMMAND_ID_OPEN_SELECTED_NAVIGATION_CONTEXT_MENU),
            Some(CommandKind::OpenSelectedNavigationContextMenu)
        );
        assert_eq!(
            CommandKind::from_id(COMMAND_ID_OPEN_TIMELINE_CONTEXT_MENU),
            Some(CommandKind::OpenTimelineContextMenu)
        );
        assert_eq!(
            CommandKind::from_id(COMMAND_ID_OPEN_SELECTED_TIMELINE_CONTEXT_MENU),
            Some(CommandKind::OpenSelectedTimelineContextMenu)
        );
        assert_eq!(
            CommandKind::from_id(COMMAND_ID_REPLY_TO_SELECTED_MESSAGE_VIA_MENU),
            Some(CommandKind::ReplyToSelectedMessageViaMenu)
        );
        assert_eq!(
            CommandKind::from_id(COMMAND_ID_EDIT_SELECTED_MESSAGE_VIA_MENU),
            Some(CommandKind::EditSelectedMessageViaMenu)
        );
        assert_eq!(
            CommandKind::from_id(COMMAND_ID_REACT_TO_SELECTED_MESSAGE_VIA_MENU),
            Some(CommandKind::ReactToSelectedMessageViaMenu)
        );
        assert_eq!(
            CommandKind::from_id(COMMAND_ID_REDACT_SELECTED_MESSAGE_VIA_MENU),
            Some(CommandKind::RedactSelectedMessageViaMenu)
        );
        assert_eq!(
            CommandKind::from_id(COMMAND_ID_SAVE_ATTACHMENT_VIA_MESSAGE_MENU),
            Some(CommandKind::SaveAttachmentViaMessageMenu)
        );
        assert_eq!(
            CommandKind::from_id(COMMAND_ID_LOGIN - 1),
            Some(CommandKind::SelectView)
        );
        assert_eq!(
            CommandKind::from_id(COMMAND_ID_SEND_FILE),
            Some(CommandKind::SendFile)
        );
        assert_eq!(
            CommandKind::from_id(COMMAND_ID_SETUP_E2EE_RECOVERY),
            Some(CommandKind::SetupE2eeRecovery)
        );
        assert_eq!(
            CommandKind::from_id(COMMAND_ID_ARRANGE_WINDOWS_VIA_WINDOW_MENU),
            Some(CommandKind::ArrangeWindowsViaWindowMenu)
        );
        assert_eq!(
            CommandKind::from_id(COMMAND_ID_SET_VIEW_WINDOW_STATE),
            Some(CommandKind::SetViewWindowState)
        );
        assert_eq!(
            CommandKind::from_id(COMMAND_ID_SELECT_NAVIGATION_VIEW),
            Some(CommandKind::SelectNavigationView)
        );
        assert_eq!(
            CommandKind::from_id(COMMAND_ID_OPEN_DIALOG),
            Some(CommandKind::OpenDialog)
        );
        assert_eq!(
            CommandKind::from_id(COMMAND_ID_SELECT_OPTIONS_PAGE),
            Some(CommandKind::SelectOptionsPage)
        );
        assert_eq!(
            CommandKind::from_id(COMMAND_ID_SET_NAVIGATION_EXPANDED),
            Some(CommandKind::SetNavigationExpanded)
        );
        assert_eq!(
            CommandKind::from_id(COMMAND_ID_CHOOSE_TRAY_MENU_ITEM),
            Some(CommandKind::ChooseTrayMenuItem)
        );
        assert_eq!(
            CommandKind::from_id(COMMAND_ID_CRASH),
            Some(CommandKind::Crash)
        );
        assert_eq!(CommandKind::from_id(COMMAND_ID_CRASH + 1), None);
    }

    #[test]
    fn context_menu_commands_require_strict_valid_args() {
        for (command_id, args) in [
            (
                COMMAND_ID_OPEN_NAVIGATION_CONTEXT_MENU,
                r#"{"path":[],"command":"part_room"}"#,
            ),
            (
                COMMAND_ID_OPEN_SELECTED_NAVIGATION_CONTEXT_MENU,
                r#"{"command":"accept_invite"}"#,
            ),
            (
                COMMAND_ID_OPEN_TIMELINE_CONTEXT_MENU,
                r#"{"event_id":"$event","command":"reply_message"}"#,
            ),
            (
                COMMAND_ID_OPEN_SELECTED_TIMELINE_CONTEXT_MENU,
                r#"{"command":"edit_message"}"#,
            ),
        ] {
            assert!(
                parse_command_request(
                    command_id,
                    format!(r#"{{"id":"context","args":{args}}}"#).as_bytes()
                )
                .is_ok()
            );
            assert!(
                parse_command_request(
                    command_id,
                    br#"{"id":"context","args":{"command":"invalid","extra":true}}"#,
                )
                .is_err()
            );
            assert!(parse_command_request(command_id, br#"{"id":"context"}"#).is_err());
        }
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
                COMMAND_ID_SEND_FILE,
                br#"{"id":"file","args":{"room_id":"!room:example","path":"C:\\tmp\\report.txt","filename":"report.txt","content_type":"application/octet-stream"}}"#,
            )
            .unwrap(),
            ParsedCommand::SendFile {
                id: "file".to_owned(),
                args: SendFileArgs {
                    room_id: Some("!room:example".to_owned()),
                    path: "C:\\tmp\\report.txt".to_owned(),
                    filename: Some("report.txt".to_owned()),
                    content_type: Some("application/octet-stream".to_owned()),
                },
            }
        );
        assert_eq!(
            parse_command_request(
                COMMAND_ID_LOAD_SYNTHETIC_STATE,
                br#"{"id":"load","args":{"events":["Started",{"SyncedRooms":[{"room_id":"!room:example","display_name":"Room","topic":null,"membership":"join","encrypted":true,"is_space":false,"is_direct":false,"direct_user_ids":[],"tags":[],"space_children":[],"notification_mode":"all_messages","canonical_alias":null,"alt_aliases":[]}]},{"RoomActivity":{"room_id":"!room:example","activity_id":"event:$event:example","timestamp_ms":1700000000000,"highlight":false,"relation":null,"kind":{"type":"message","event_id":"$event:example","transaction_id":null,"sender":{"user_id":"@alice:example","display_name":null,"display_name_ambiguous":false},"presentation":"text","body":"hello","styles":[],"user_links":[],"send_state":{"state":"sent"}}}}]}}"#,
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
            topic: None,
                            membership: RoomMembership::Join,
                            encrypted: true,
                            is_space: false,
                            is_direct: false,
                            direct_user_ids: Vec::new(),
                            tags: Vec::new(),
                            space_children: Vec::new(),
                            notification_mode: Some(RoomNotificationMode::AllMessages),
                            canonical_alias: None,
                            alt_aliases: Vec::new(),
                        }]),
                        Event::RoomActivity(Box::new(RoomActivity {
                            room_id: "!room:example".to_owned(),
                            activity_id: "event:$event:example".to_owned(),
                            timestamp_ms: 1_700_000_000_000,
                            highlight: false,
                            relation: None,
                            kind: RoomActivityKind::Message(RoomMessageActivity {
                                event_id: Some("$event:example".to_owned()),
                                transaction_id: None,
                                sender: user_profile("@alice:example"),
                                presentation: mirx_core::RoomMessagePresentationKind::Text,
                                body: "hello".to_owned(),
                                styles: Vec::new(),
                                user_links: Vec::new(),
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
                COMMAND_ID_FIND_ACTIVE_VIEW,
                br#"{"id":"find","args":{"query":"needle"}}"#,
            )
            .unwrap(),
            ParsedCommand::FindActiveView {
                id: "find".to_owned(),
                args: FindActiveViewArgs {
                    query: "needle".to_owned(),
                },
            }
        );
        assert_eq!(
            parse_command_request(
                COMMAND_ID_DUMP_VERIFICATION_MENU_STATE,
                br#"{"id":"verification-menu","args":{}}"#,
            )
            .unwrap(),
            ParsedCommand::DumpVerificationMenuState {
                id: "verification-menu".to_owned(),
            }
        );
        assert_eq!(
            parse_command_request(COMMAND_ID_DUMP_ROOM_MENU_STATE, br#"{"id":"room-menu"}"#)
                .unwrap(),
            ParsedCommand::DumpRoomMenuState {
                id: "room-menu".to_owned()
            }
        );
        assert_eq!(
            parse_command_request(
                COMMAND_ID_DUMP_MESSAGE_MENU_STATE,
                br#"{"id":"message-menu","args":{}}"#
            )
            .unwrap(),
            ParsedCommand::DumpMessageMenuState {
                id: "message-menu".to_owned()
            }
        );
        assert_eq!(
            parse_command_request(
                COMMAND_ID_SELECT_TIMELINE_EVENT,
                br#"{"id":"select-event","args":{"event_id":"$event:example"}}"#
            )
            .unwrap(),
            ParsedCommand::SelectTimelineEvent {
                id: "select-event".to_owned(),
                args: SelectTimelineEventArgs {
                    event_id: "$event:example".to_owned()
                }
            }
        );
        assert_eq!(
            parse_command_request(
                COMMAND_ID_SELECT_MEMBER,
                br#"{"id":"select-member","args":{"user_id":"@bob:example"}}"#
            )
            .unwrap(),
            ParsedCommand::SelectMember {
                id: "select-member".to_owned(),
                args: SelectMemberArgs {
                    user_id: "@bob:example".to_owned()
                }
            }
        );
        assert_eq!(
            parse_command_request(COMMAND_ID_OPEN_MEMBER_CONTEXT_MENU, br#"{"id":"member-menu","args":{"user_id":"@bob:example","command":"query_member"}}"#).unwrap(),
            ParsedCommand::OpenMemberContextMenu { id: "member-menu".to_owned(), args: OpenMemberContextMenuArgs { user_id: "@bob:example".to_owned(), command: ContextMenuCommand::QueryMember } }
        );
        assert_eq!(
            parse_command_request(COMMAND_ID_OPEN_MEMBER_CONTEXT_MENU, br#"{"id":"member-menu","args":{"user_id":"@bob:example","command":"kick_member"}}"#).unwrap(),
            ParsedCommand::OpenMemberContextMenu { id: "member-menu".to_owned(), args: OpenMemberContextMenuArgs { user_id: "@bob:example".to_owned(), command: ContextMenuCommand::KickMember } }
        );
        assert_eq!(
            parse_command_request(
                COMMAND_ID_OPEN_MEMBER_CONTEXT_MENU,
                br#"{"id":"member-menu","args":{"user_id":"@bob:example","command":"ban_member"}}"#
            )
            .unwrap(),
            ParsedCommand::OpenMemberContextMenu {
                id: "member-menu".to_owned(),
                args: OpenMemberContextMenuArgs {
                    user_id: "@bob:example".to_owned(),
                    command: ContextMenuCommand::BanMember
                }
            }
        );
        assert!(matches!(
            parse_command_request(
                COMMAND_ID_DUMP_MEMBER_MENU_STATE,
                br#"{"id":"member-state"}"#
            )
            .unwrap(),
            ParsedCommand::DumpMemberMenuState { .. }
        ));
        assert!(matches!(
            parse_command_request(
                COMMAND_ID_QUERY_SELECTED_MEMBER_VIA_MENU,
                br#"{"id":"query-member"}"#
            )
            .unwrap(),
            ParsedCommand::QuerySelectedMemberViaMenu { .. }
        ));
        assert!(matches!(
            parse_command_request(
                COMMAND_ID_MENTION_SELECTED_MEMBER_VIA_MENU,
                br#"{"id":"mention-member"}"#
            )
            .unwrap(),
            ParsedCommand::MentionSelectedMemberViaMenu { .. }
        ));
        for (arrangement, expected) in [
            ("cascade", MdiArrangement::Cascade),
            ("tile_horizontal", MdiArrangement::TileHorizontal),
            ("tile_vertical", MdiArrangement::TileVertical),
            ("arrange_icons", MdiArrangement::ArrangeIcons),
        ] {
            assert_eq!(
                parse_command_request(
                    COMMAND_ID_ARRANGE_WINDOWS_VIA_WINDOW_MENU,
                    format!(r#"{{"id":"arrange","args":{{"arrangement":"{arrangement}"}}}}"#)
                        .as_bytes()
                )
                .unwrap(),
                ParsedCommand::ArrangeWindowsViaWindowMenu {
                    id: "arrange".to_owned(),
                    args: ArrangeWindowsArgs {
                        arrangement: expected
                    }
                }
            );
        }
        for (state, expected) in [
            ("maximized", MdiWindowState::Maximized),
            ("normal", MdiWindowState::Normal),
            ("minimized", MdiWindowState::Minimized),
        ] {
            assert_eq!(
                parse_command_request(
                    COMMAND_ID_SET_VIEW_WINDOW_STATE,
                    format!(
                        r#"{{"id":"state","args":{{"view":{{"kind":"status"}},"state":"{state}"}}}}"#
                    )
                    .as_bytes()
                )
                .unwrap(),
                ParsedCommand::SetViewWindowState {
                    id: "state".to_owned(),
                    args: SetViewWindowStateArgs {
                        view: ViewIdentity::Status,
                        state: expected
                    }
                }
            );
        }
        for (command_id, request) in [
            (
                COMMAND_ID_ARRANGE_WINDOWS_VIA_WINDOW_MENU,
                br#"{"id":"arrange","args":{"arrangement":"tile"}}"#.as_slice(),
            ),
            (
                COMMAND_ID_ARRANGE_WINDOWS_VIA_WINDOW_MENU,
                br#"{"id":"arrange","args":{}}"#.as_slice(),
            ),
            (
                COMMAND_ID_SET_VIEW_WINDOW_STATE,
                br#"{"id":"state","args":{"view":{"kind":"status"},"state":"hidden"}}"#.as_slice(),
            ),
        ] {
            let error = parse_command_request(command_id, request).unwrap_err();
            assert!(
                error.err.message.starts_with("invalid command args"),
                "unexpected error {error:?}"
            );
        }
        assert_eq!(
            parse_command_request(
                COMMAND_ID_OPEN_SELECTED_MEMBER_CONTEXT_MENU,
                br#"{"id":"selected-member-menu","args":{"command":"mention_member"}}"#
            )
            .unwrap(),
            ParsedCommand::OpenSelectedMemberContextMenu {
                id: "selected-member-menu".to_owned(),
                args: OpenSelectedMemberContextMenuArgs {
                    command: ContextMenuCommand::MentionMember
                }
            }
        );
        for (command_id, id) in [
            (COMMAND_ID_ACCEPT_INVITE_VIA_ROOM_MENU, "accept"),
            (COMMAND_ID_REJECT_INVITE_VIA_ROOM_MENU, "reject"),
            (COMMAND_ID_PART_ROOM_VIA_ROOM_MENU, "part"),
            (COMMAND_ID_REPLY_TO_SELECTED_MESSAGE_VIA_MENU, "reply"),
            (COMMAND_ID_EDIT_SELECTED_MESSAGE_VIA_MENU, "edit"),
            (COMMAND_ID_REACT_TO_SELECTED_MESSAGE_VIA_MENU, "react"),
            (COMMAND_ID_REDACT_SELECTED_MESSAGE_VIA_MENU, "redact"),
            (
                COMMAND_ID_SAVE_ATTACHMENT_VIA_MESSAGE_MENU,
                "save-attachment",
            ),
        ] {
            assert!(
                parse_command_request(
                    command_id,
                    format!(r#"{{"id":"{id}","args":{{}}}}"#).as_bytes()
                )
                .is_ok()
            );
        }
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
                br#"{"id":"restore","args":{"homeserver_url":"https://matrix.example","session":{"user_id":"@alice:example","device_id":"DEVICE","access_token":"secret-token","refresh_token":"refresh-token","auth":"Matrix"}}}"#,
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
            (
                br#"{"id":"compose-key","args":{"key":{"kind":"tab"}}}"#.as_slice(),
                ComposeKey::Tab,
            ),
            (
                br#"{"id":"compose-key","args":{"key":{"kind":"shift_tab"}}}"#.as_slice(),
                ComposeKey::ShiftTab,
            ),
            (
                br#"{"id":"compose-key","args":{"key":{"kind":"escape"}}}"#.as_slice(),
                ComposeKey::Escape,
            ),
            (
                br#"{"id":"compose-key","args":{"key":{"kind":"format","key":"bold"}}}"#.as_slice(),
                ComposeKey::Format {
                    key: MircFormatKey::Bold,
                },
            ),
            (
                br#"{"id":"compose-key","args":{"key":{"kind":"format","key":"reverse"}}}"#
                    .as_slice(),
                ComposeKey::Format {
                    key: MircFormatKey::Reverse,
                },
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
                br#"{"id":"select-nav","args":{"path":[{"text":"mirx","same_text_index":0},{"text":"rooms: 4","same_text_index":0},{"text":"Work","same_text_index":0},{"text":"Shared","same_text_index":0}]}}"#,
            )
            .unwrap(),
            ParsedCommand::SelectNavigationPath {
                id: "select-nav".to_owned(),
                args: SelectNavigationPathArgs {
                    path: vec![
                        NavigationPathSegment {
                            text: "mirx".to_owned(),
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
                br#"{"id":"append","args":{"events":[{"RoomActivityUpdate":{"room_id":"!room:example","target":{"type":"transaction","transaction_id":"txn-1"},"kind":{"type":"message_sent","event_id":"$event:example"}}}]}}"#,
            )
            .unwrap(),
            ParsedCommand::AppendSyntheticEvents {
                id: "append".to_owned(),
                args: LoadEventsArgs {
                    events: vec![Event::RoomActivityUpdate(mirx_core::RoomActivityUpdate {
                        room_id: "!room:example".to_owned(),
                        target: mirx_core::RoomActivityUpdateTarget::Transaction {
                            transaction_id: "txn-1".to_owned(),
                        },
                        kind: mirx_core::RoomActivityUpdateKind::MessageSent {
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
    fn find_active_view_response_and_ui_find_state_round_trip() {
        let response = FindActiveViewResponse {
            active: ViewIdentity::Room {
                room_id: "!room:example".to_owned(),
            },
            query: "needle".to_owned(),
            matched: true,
            match_count: 3,
            current_match_index: Some(1),
            selected_text: Some("needle in haystack".to_owned()),
            history_search: UiFindHistorySearchState::LocalOnly,
        };
        let ui_find_state = UiFindState {
            active: ViewIdentity::Status,
            query: "synced".to_owned(),
            matched: false,
            match_count: 0,
            current_match_index: None,
            selected_text: None,
            history_search: UiFindHistorySearchState::Failed {
                message: "homeserver unavailable".to_owned(),
            },
        };

        assert_eq!(
            serde_json::from_value::<FindActiveViewResponse>(
                serde_json::to_value(&response).unwrap()
            )
            .unwrap(),
            response
        );
        assert_eq!(
            serde_json::from_value::<UiFindState>(serde_json::to_value(&ui_find_state).unwrap())
                .unwrap(),
            ui_find_state
        );
    }

    #[test]
    fn ui_state_handles_active_find_null_and_populated() {
        let without_find = serde_json::from_value::<UiState>(json!({
            "navigation":{"text":"mirx","children":[]},
            "mdi":{"active":{"kind":"status"},"views":[]},
            "status_bar":{"text":"ready"},
            "switchbar":{"rows":1,"items":[]},
            "frame_layout":{"switchbar":{"x":0,"y":0,"width":1008,"height":28},"treebar":{"x":0,"y":28,"width":240,"height":600},"mdi_client":{"x":240,"y":28,"width":768,"height":600}},
            "dpi":96,
            "active_find":null,
            "rooms":[],
            "text_font":{"face":"Fixedsys","points":9,"bold":false,"line_height":15,"edit_height":25,"average_char_width":8,"fallback_faces":["Tahoma","Noto Emoji"]},
            "timestamp":{"show":true,"format":"[HH:nn]"},
            "colors":{"items":[{"item":"background","index":0,"color":"#ffffff"}],"switchbar":{"event":5,"message":4,"highlight":12}},
            "active_system_menu_items":[],
            "active_system_menu_checked":[],
            "active_system_menu_disabled":[],
            "menu_bar":[],
            "tray":{"icon":false,"hidden":false,"menu":[]},
            "refresh_counters":{"timeline_row_builds":0,"rows_wrapped":0,"treebar_rebuilds":0,"switchbar_rebuilds":0}
        }))
        .unwrap();
        assert_eq!(without_find.active_find, None);

        let with_find = serde_json::from_value::<UiState>(json!({
            "navigation":{"text":"mirx","children":[]},
            "mdi":{"active":{"kind":"room","room_id":"!room:example"},"views":[]},
            "status_bar":{"text":"ready"},
            "switchbar":{"rows":1,"items":[]},
            "frame_layout":{"switchbar":{"x":0,"y":0,"width":1008,"height":28},"treebar":{"x":0,"y":28,"width":240,"height":600},"mdi_client":{"x":240,"y":28,"width":768,"height":600}},
            "dpi":96,
            "active_find":{
                "active":{"kind":"room","room_id":"!room:example"},
                "query":"needle",
                "matched":true,
                "match_count":2,
                "current_match_index":1,
                "selected_text":"needle two",
                "history_search":{"state":"loading"}
            },
            "rooms":[],
            "text_font":{"face":"Fixedsys","points":9,"bold":false,"line_height":15,"edit_height":25,"average_char_width":8,"fallback_faces":["Tahoma","Noto Emoji"]},
            "timestamp":{"show":true,"format":"[HH:nn]"},
            "colors":{"items":[{"item":"background","index":0,"color":"#ffffff"}],"switchbar":{"event":5,"message":4,"highlight":12}},
            "active_system_menu_items":[],
            "active_system_menu_checked":[],
            "active_system_menu_disabled":[],
            "menu_bar":[],
            "tray":{"icon":false,"hidden":false,"menu":[]},
            "refresh_counters":{"timeline_row_builds":0,"rows_wrapped":0,"treebar_rebuilds":0,"switchbar_rebuilds":0}
        }))
        .unwrap();
        assert_eq!(
            with_find.active_find,
            Some(UiFindState {
                active: ViewIdentity::Room {
                    room_id: "!room:example".to_owned(),
                },
                query: "needle".to_owned(),
                matched: true,
                match_count: 2,
                current_match_index: Some(1),
                selected_text: Some("needle two".to_owned()),
                history_search: UiFindHistorySearchState::Loading,
            })
        );
    }

    #[test]
    fn find_active_view_args_reject_unknown_fields() {
        let error = parse_command_request(
            COMMAND_ID_FIND_ACTIVE_VIEW,
            br#"{"id":"find","args":{"query":"needle","unexpected":true}}"#,
        )
        .unwrap_err();
        assert_eq!(
            serde_json::to_value(error).unwrap(),
            json!({
                "id":"find",
                "ok":false,
                "err":{
                    "code":"invalid_args",
                    "message":"invalid command args: unknown field `unexpected`, expected `query`"
                }
            })
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
                topic: None,
                membership: RoomMembership::Join,
                encrypted: false,
                is_space: false,
                is_direct: false,
                direct_user_ids: Vec::new(),
                tags: Vec::new(),
                space_children: Vec::new(),
                notification_mode: Some(RoomNotificationMode::AllMessages),
                canonical_alias: None,
                alt_aliases: Vec::new(),
            }]),
            Event::VerificationRequest(VerificationRequest {
                flow_id: "flow-1".to_owned(),
                user_id: "@alice:example".to_owned(),
                direction: VerificationDirection::Incoming {
                    from_device: "ALICEDEVICE".to_owned(),
                },
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
                room_id: Some("!room:example".to_owned()),
                message: "send failed".to_owned(),
            }),
        ];

        assert_eq!(
            serde_json::to_value(&events).unwrap(),
            json!([
                "Started",
                {"SyncedRooms":[{"room_id":"!one:example","display_name":"One","topic":null,"membership":"join","encrypted":false,"is_space":false,"is_direct":false,"direct_user_ids":[],"tags":[],"space_children":[],"notification_mode":"all_messages","canonical_alias":null,"alt_aliases":[]}]},
                {"VerificationRequest":{"flow_id":"flow-1","user_id":"@alice:example","direction":{"type":"incoming","from_device":"ALICEDEVICE"}}},
                {"RoomReadState":{"room_id":"!ops:example","latest_active_event_id":"$latest:example","fully_read_event_id":"$fully-read:example","marked_unread":true,"num_unread":7,"num_notifications":3,"num_mentions":1}},
                {"Error":{"operation":"SendMessage","room_id":"!room:example","message":"send failed"}}
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
            br#"{"id":"load","args":{"events":["Started",{"SyncedRooms":[{"room_id":"!one:example","display_name":"One","topic":null,"membership":"join","encrypted":false,"is_space":false,"is_direct":false,"direct_user_ids":[],"tags":[],"space_children":[],"notification_mode":"all_messages","canonical_alias":null,"alt_aliases":[],"unexpected":true}]}]}}"#,
        )
        .unwrap_err();

        assert_eq!(
            serde_json::to_value(error).unwrap(),
            json!({
                "id":"load",
                "ok":false,
                "err":{
                    "code":"invalid_args",
                    "message":"invalid command args: unknown field `unexpected`, expected one of `room_id`, `display_name`, `topic`, `membership`, `encrypted`, `is_space`, `is_direct`, `direct_user_ids`, `tags`, `space_children`, `notification_mode`, `canonical_alias`, `alt_aliases`"
                }
            })
        );

        let error = parse_command_request(
            COMMAND_ID_LOAD_SYNTHETIC_STATE,
            br#"{"id":"load","args":{"events":["Started",{"SyncedRooms":[{"room_id":"!space:example","display_name":"Space","topic":null,"membership":"join","encrypted":false,"is_space":true,"is_direct":false,"direct_user_ids":[],"tags":[],"space_children":[{"room_id":"!one:example","suggested":true,"order":"aaa","unexpected":true}],"notification_mode":"all_messages","canonical_alias":null,"alt_aliases":[]}]}]}}"#,
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
            br#"{"id":"load","args":{"events":["Started",{"SyncedRooms":[{"room_id":"!one:example","display_name":"One","topic":null,"membership":"join","encrypted":false,"is_space":false,"is_direct":false,"direct_user_ids":[],"tags":[{"tag":"m.favourite","order":"0.25","unexpected":true}],"space_children":[],"notification_mode":"all_messages","canonical_alias":null,"alt_aliases":[]}]}]}}"#,
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
            br#"{"id":"compose-key","args":{"key":{"kind":"return","key":"bold"}}}"#.as_slice(),
            br#"{"id":"compose-key","args":{"key":{"kind":"format"}}}"#.as_slice(),
            br#"{"id":"compose-key","args":{"key":{"kind":"format","key":"blink"}}}"#.as_slice(),
            br#"{"id":"compose-key","args":{"key":{"kind":"format","key":"bold","x":1}}}"#
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
            br#"{"id":"select-nav","args":{"path":[{"text":"mirx","same_text_index":0}],"unexpected":true}}"#,
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
    fn open_dialog_parses_each_dialog_and_rejects_unknown_ones() {
        for (dialog, expected) in [
            ("options", AutomationDialog::Options),
            ("colors", AutomationDialog::Colors),
            ("select_server", AutomationDialog::SelectServer),
            ("about", AutomationDialog::About),
            ("commands_join", AutomationDialog::CommandsJoin),
            ("recover_e2ee", AutomationDialog::RecoverE2ee),
            ("setup_e2ee_recovery", AutomationDialog::SetupE2eeRecovery),
            ("reset_e2ee", AutomationDialog::ResetE2ee),
            ("close_active_view", AutomationDialog::CloseActiveView),
            ("compose_return", AutomationDialog::ComposeReturn),
        ] {
            assert_eq!(
                parse_command_request(
                    COMMAND_ID_OPEN_DIALOG,
                    format!(r#"{{"id":"open","args":{{"dialog":"{dialog}"}}}}"#).as_bytes()
                )
                .unwrap(),
                ParsedCommand::OpenDialog {
                    id: "open".to_owned(),
                    args: OpenDialogArgs { dialog: expected },
                }
            );
        }
        for request in [
            br#"{"id":"open","args":{"dialog":"no_such"}}"#.as_slice(),
            br#"{"id":"open","args":{"dialog":"select-server"}}"#.as_slice(),
            br#"{"id":"open","args":{}}"#.as_slice(),
            br#"{"id":"open"}"#.as_slice(),
            br#"{"id":"open","args":{"dialog":"about","page":"x"}}"#.as_slice(),
        ] {
            let error = parse_command_request(COMMAND_ID_OPEN_DIALOG, request).unwrap_err();
            assert_eq!(error.err.code, "invalid_args", "unexpected error {error:?}");
        }
    }

    #[test]
    fn select_options_page_parses_a_path_and_rejects_other_shapes() {
        assert_eq!(
            parse_command_request(
                COMMAND_ID_SELECT_OPTIONS_PAGE,
                br#"{"id":"page","args":{"path":"IRC/Messages"}}"#,
            )
            .unwrap(),
            ParsedCommand::SelectOptionsPage {
                id: "page".to_owned(),
                args: SelectOptionsPageArgs {
                    path: "IRC/Messages".to_owned(),
                },
            }
        );
        for request in [
            br#"{"id":"page","args":{}}"#.as_slice(),
            br#"{"id":"page"}"#.as_slice(),
            br#"{"id":"page","args":{"path":["IRC","Messages"]}}"#.as_slice(),
            br#"{"id":"page","args":{"path":"Display","unexpected":true}}"#.as_slice(),
        ] {
            let error = parse_command_request(COMMAND_ID_SELECT_OPTIONS_PAGE, request).unwrap_err();
            assert_eq!(error.err.code, "invalid_args", "unexpected error {error:?}");
        }
    }

    #[test]
    fn choose_tray_menu_item_parses_a_label_and_rejects_other_shapes() {
        assert_eq!(
            parse_command_request(
                COMMAND_ID_CHOOSE_TRAY_MENU_ITEM,
                br#"{"id":"tray","args":{"label":"Alpha"}}"#,
            )
            .unwrap(),
            ParsedCommand::ChooseTrayMenuItem {
                id: "tray".to_owned(),
                args: ChooseTrayMenuItemArgs {
                    label: "Alpha".to_owned(),
                },
            }
        );
        for request in [
            br#"{"id":"tray","args":{}}"#.as_slice(),
            br#"{"id":"tray"}"#.as_slice(),
            br#"{"id":"tray","args":{"label":1}}"#.as_slice(),
            br#"{"id":"tray","args":{"label":"Alpha","checked":true}}"#.as_slice(),
        ] {
            let error =
                parse_command_request(COMMAND_ID_CHOOSE_TRAY_MENU_ITEM, request).unwrap_err();
            assert_eq!(error.err.code, "invalid_args", "unexpected error {error:?}");
        }
    }

    #[test]
    fn crash_parses_a_message_and_rejects_other_shapes() {
        assert_eq!(
            parse_command_request(
                COMMAND_ID_CRASH,
                br#"{"id":"crash","args":{"message":"boom"}}"#,
            )
            .unwrap(),
            ParsedCommand::Crash {
                id: "crash".to_owned(),
                args: CrashArgs {
                    message: "boom".to_owned(),
                },
            }
        );
        for request in [
            br#"{"id":"crash","args":{}}"#.as_slice(),
            br#"{"id":"crash"}"#.as_slice(),
            br#"{"id":"crash","args":{"message":1}}"#.as_slice(),
            br#"{"id":"crash","args":{"message":"boom","now":true}}"#.as_slice(),
        ] {
            let error = parse_command_request(COMMAND_ID_CRASH, request).unwrap_err();
            assert_eq!(error.err.code, "invalid_args", "unexpected error {error:?}");
        }
    }

    #[test]
    fn set_navigation_expanded_names_a_view_or_a_space() {
        assert_eq!(
            parse_command_request(
                COMMAND_ID_SET_NAVIGATION_EXPANDED,
                br#"{"id":"fold","args":{"node":{"space":"!work:example"},"expanded":false}}"#,
            )
            .unwrap(),
            ParsedCommand::SetNavigationExpanded {
                id: "fold".to_owned(),
                args: SetNavigationExpandedArgs {
                    node: NavigationNodeKey::Space("!work:example".to_owned()),
                    expanded: false,
                },
            }
        );
        assert_eq!(
            parse_command_request(
                COMMAND_ID_SET_NAVIGATION_EXPANDED,
                br#"{"id":"unfold","args":{"node":{"view":{"kind":"status"}},"expanded":true}}"#,
            )
            .unwrap(),
            ParsedCommand::SetNavigationExpanded {
                id: "unfold".to_owned(),
                args: SetNavigationExpandedArgs {
                    node: NavigationNodeKey::View(ViewIdentity::Status),
                    expanded: true,
                },
            }
        );
        for request in [
            br#"{"id":"fold","args":{"node":{"space":"!work:example"}}}"#.as_slice(),
            br#"{"id":"fold","args":{"node":{},"expanded":false}}"#.as_slice(),
            br#"{"id":"fold","args":{"node":{"space":"!a:example","view":{"kind":"status"}},"expanded":false}}"#.as_slice(),
            br#"{"id":"fold","args":{"node":{"space":"!a:example"},"expanded":false,"x":1}}"#.as_slice(),
        ] {
            let error =
                parse_command_request(COMMAND_ID_SET_NAVIGATION_EXPANDED, request).unwrap_err();
            assert_eq!(error.err.code, "invalid_args", "unexpected error {error:?}");
        }
    }

    #[test]
    fn select_navigation_view_args_round_trip() {
        assert_eq!(
            parse_command_request(
                COMMAND_ID_SELECT_NAVIGATION_VIEW,
                br#"{"id":"select-nav-view","args":{"view":{"kind":"thread","room_id":"!ops:example","thread_root_event_id":"$root:example"}}}"#,
            )
            .unwrap(),
            ParsedCommand::SelectNavigationView {
                id: "select-nav-view".to_owned(),
                args: SelectViewArgs {
                    view: ViewIdentity::Thread {
                        room_id: "!ops:example".to_owned(),
                        thread_root_event_id: "$root:example".to_owned(),
                    },
                },
            }
        );
        for request in [
            br#"{"id":"select-nav-view","args":{}}"#.as_slice(),
            br#"{"id":"select-nav-view","args":{"view":{"kind":"status"},"unexpected":true}}"#
                .as_slice(),
        ] {
            let error =
                parse_command_request(COMMAND_ID_SELECT_NAVIGATION_VIEW, request).unwrap_err();
            assert_eq!(error.err.code, "invalid_args", "unexpected error {error:?}");
        }
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
                    device_display_name: "mirx-ui".to_owned(),
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
            br#"{"id":"restore","args":{"homeserver_url":"https://matrix.example","session":{"user_id":"@alice:example","device_id":"DEVICE","access_token":"secret-token","refresh_token":null,"auth":"Matrix","unexpected":true}}}"#,
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
            br#"{"id":"restore","args":{"homeserver_url":"https://matrix.example","session":{"user_id":"@alice:example","device_id":"DEVICE","access_token":"secret-token","refresh_token":null,"auth":"Matrix"},"unexpected":true}}"#,
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
            (COMMAND_ID_DUMP_ROOM_MENU_STATE, "room-menu"),
            (COMMAND_ID_DUMP_MESSAGE_MENU_STATE, "message-menu"),
            (COMMAND_ID_ACCEPT_INVITE_VIA_ROOM_MENU, "accept"),
            (COMMAND_ID_REJECT_INVITE_VIA_ROOM_MENU, "reject"),
            (COMMAND_ID_PART_ROOM_VIA_ROOM_MENU, "part"),
            (COMMAND_ID_REPLY_TO_SELECTED_MESSAGE_VIA_MENU, "reply"),
            (COMMAND_ID_REDACT_SELECTED_MESSAGE_VIA_MENU, "redact"),
            (
                COMMAND_ID_SAVE_ATTACHMENT_VIA_MESSAGE_MENU,
                "save-attachment",
            ),
        ] {
            let request = format!(r#"{{"id":"{request_id}","args":{{"unexpected":true}}}}"#);
            let error = parse_command_request(command_id, request.as_bytes())
                .expect_err("unexpected args must fail");
            assert_eq!(
                serde_json::to_value(error).unwrap()["err"]["code"],
                json!("invalid_args")
            );
        }

        for request in [
            br#"{"id":"select-event"}"#.as_slice(),
            br#"{"id":"select-event","args":{"event_id":"$event:example","unexpected":true}}"#
                .as_slice(),
        ] {
            let error = parse_command_request(COMMAND_ID_SELECT_TIMELINE_EVENT, request)
                .expect_err("invalid timeline event selection args must fail");
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
    fn setup_e2ee_recovery_round_trips_and_redacts_passphrase() {
        let command = parse_command_request(
            COMMAND_ID_SETUP_E2EE_RECOVERY,
            br#"{"id":"setup","args":{"passphrase":"sample setup passphrase","wait_for_backups_to_upload":true}}"#,
        )
        .unwrap();
        assert_eq!(
            command,
            ParsedCommand::SetupE2eeRecovery {
                id: "setup".to_owned(),
                args: SetupE2eeRecoveryArgs {
                    passphrase: Some("sample setup passphrase".to_owned()),
                    wait_for_backups_to_upload: true,
                },
            }
        );
        assert!(!format!("{command:?}").contains("sample setup passphrase"));
    }

    #[test]
    fn setup_e2ee_recovery_rejects_invalid_args_without_echoing_passphrase() {
        for request in [
            br#"{"id":"setup","args":{"passphrase":"sample setup passphrase","unexpected":true}}"#.as_slice(),
            br#"{"id":"setup","args":{"passphrase":true,"wait_for_backups_to_upload":false}}"#.as_slice(),
            br#"{"id":"setup","args":{"passphrase":"sample setup passphrase","wait_for_backups_to_upload":"yes"}}"#.as_slice(),
        ] {
            let error = parse_command_request(COMMAND_ID_SETUP_E2EE_RECOVERY, request).unwrap_err();
            assert!(!serde_json::to_string(&error).unwrap().contains("sample setup passphrase"));
        }
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
            placement: MdiWindowPlacement {
                state: MdiWindowState::Normal,
                x: 0,
                y: 0,
                width: 320,
                height: 200,
            },
            lines: vec!["reply".to_owned()],
            members: Vec::new(),
            log: None,
        };
        let mdi_view_json = json!({
            "identity": {
                "kind": "thread",
                "room_id": "!room:example",
                "thread_root_event_id": "$thread-root:example",
            },
            "title": "Thread",
            "active": true,
            "placement": {"state": "normal", "x": 0, "y": 0, "width": 320, "height": 200},
            "lines": ["reply"],
            "members": [],
            "log": null,
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
            activity: Some(ViewActivityLevel::Message),
            rect: UiRect {
                x: 120,
                y: 0,
                width: 120,
                height: 36,
            },
        };
        let switchbar_item_json = json!({
            "identity": {
                "kind": "thread",
                "room_id": "!room:example",
                "thread_root_event_id": "$thread-root:example",
            },
            "title": "Thread",
            "active": false,
            "activity": "message",
            "rect": {"x": 120, "y": 0, "width": 120, "height": 36},
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
                "placement": {"state": "normal", "x": 0, "y": 0, "width": 320, "height": 200},
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
                "activity": "message",
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
                    "protocol_version":69,
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
                    pending_core_commands: 0,
                    posted_messages_pending: false,
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
                    "pending_core_commands":0,
                    "posted_messages_pending":false,
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
                    connection: ConnectionState::Syncing,
                    reconnect_attempts: 0,
                    rooms: vec![super::DumpRoom {
                        room_id: "!room:example".to_owned(),
                        display_name: "Room".to_owned(),
                        topic: None,
                        membership: RoomMembership::Join,
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
                    last_e2ee_recovery_setup_result: None,
                    last_verification: None,
                    submitted_membership_commands: Vec::new(),
                    submitted_member_moderation_commands: Vec::new(),
                    submitted_power_level_commands: Vec::new(),
                    submitted_profile_commands: Vec::new(),
                    submitted_user_commands: Vec::new(),
                    submitted_direct_room_commands: Vec::new(),
                    submitted_room_topic_commands: Vec::new(),
                    submitted_reaction_commands: Vec::new(),
                    submitted_edit_commands: Vec::new(),
                    submitted_reply_commands: Vec::new(),
                    submitted_message_commands: Vec::new(),
                    submitted_file_commands: Vec::new(),
                    submitted_notification_commands: Vec::new(),
                    submitted_redaction_commands: Vec::new(),
                    submitted_history_commands: Vec::new(),
                    submitted_verification_actions: Vec::new(),
                    submitted_verification_requests: Vec::new(),
                    identity_warnings: Vec::new(),
                    submitted_identity_reset_commands: Vec::new(),
                    submitted_room_key_transfers: Vec::new(),
                    submitted_session_commands: Vec::new(),
                    submitted_recovery_commands: Vec::new(),
                    submitted_setup_recovery_commands: Vec::new(),
                    submitted_room_list_requests: Vec::new(),
                    opened_links: Vec::new(),
                    shown_tips: Vec::new(),
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
                    "connection":"syncing", "reconnect_attempts":0,
                    "rooms":[{"room_id":"!room:example","display_name":"Room","topic":null,"membership":"join","encrypted":true,"is_space":false,"is_direct":true,"direct_user_ids":["@alice:example"],"tags":[{"tag":"m.favourite","order":"0.25"}],"space_children":[]}],
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
                        },
                        "session":"verified"
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
                            text: "Status".to_owned(),
                            same_text_index: 0,
                        },
                        NavigationPathSegment {
                            text: "Work".to_owned(),
                            same_text_index: 0,
                        },
                        NavigationPathSegment {
                            text: "Shared".to_owned(),
                            same_text_index: 1,
                        },
                    ],
                    view: Some(ViewIdentity::Room {
                        room_id: "!shared:example".to_owned(),
                    }),
                },
            ))
            .unwrap(),
            json!({
                "id":"selected-nav",
                "ok":true,
                "json":{
                    "path":[
                        {"text":"Status","same_text_index":0},
                        {"text":"Work","same_text_index":0},
                        {"text":"Shared","same_text_index":1}
                    ],
                    "view":{"kind":"room","room_id":"!shared:example"}
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
                            highlight: false,
                            relation: Some(DumpRoomActivityRelation::Thread {
                                thread_root_event_id: "$thread-root:example".to_owned(),
                                in_reply_to_event_id: Some("$reply:example".to_owned()),
                                is_falling_back: false,
                            }),
                            reactions: Vec::new(),
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
                            highlight: false,
                            relation: None,
                            reactions: Vec::new(),
                            kind: DumpRoomActivityKind::Image(Box::new(DumpRoomImageActivity {
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
                                    thumbnail_source: None,
                                }),
                                media_state: None,
                            })),
                        },
                        DumpRoomActivity {
                            room_id: "!room:example".to_owned(),
                            activity_id: "state:$state-1".to_owned(),
                            timestamp_ms: 1_700_000_060_000,
                            highlight: false,
                            relation: None,
                            reactions: Vec::new(),
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
                            "highlight":false,
                            "relation":{
                                "type":"thread",
                                "thread_root_event_id":"$thread-root:example",
                                "in_reply_to_event_id":"$reply:example",
                                "is_falling_back":false
                            },
                            "reactions":[],
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
                            "highlight":false,
                            "relation":null,
                            "reactions":[],
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
                                    "is_animated":false,
                                    "thumbnail_source":null
                                },
                                "media_state":null
                            }
                        },
                        {
                            "room_id":"!room:example",
                            "activity_id":"state:$state-1",
                            "timestamp_ms":1700000060000u64,
                            "highlight":false,
                            "relation":null,
                            "reactions":[],
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
                        topic: None,
                        membership: RoomMembership::Join,
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
                "json":{"rooms":[{"room_id":"!room:example","display_name":"Room","topic":null,"membership":"join","encrypted":true,"is_space":false,"is_direct":true,"direct_user_ids":["@alice:example"],"tags":[{"tag":"m.favourite","order":"0.25"}],"space_children":[]}]}
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
                        text: "Status".to_owned(),
                        view: Some(ViewIdentity::Status),
                        space_id: None,
                        expanded: Some(true),
                        activity: None,
                        children: vec![NavigationNode {
                            text: "Work".to_owned(),
                            view: None,
                            space_id: Some("!work:example".to_owned()),
                            expanded: Some(false),
                            activity: Some(ViewActivityLevel::Message),
                            children: vec![NavigationNode {
                                text: "Room".to_owned(),
                                view: Some(ViewIdentity::Room {
                                    room_id: "!room:example".to_owned(),
                                }),
                                space_id: None,
                                expanded: None,
                                activity: Some(ViewActivityLevel::Message),
                                children: vec![],
                            }],
                        }],
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
                                placement: MdiWindowPlacement {
                                    state: MdiWindowState::Minimized,
                                    x: 0,
                                    y: 0,
                                    width: 320,
                                    height: 200,
                                },
                                lines: vec!["status: synced".to_owned()],
                                members: Vec::new(),
                                log: None,
                            },
                            MdiView {
                                identity: ViewIdentity::Room {
                                    room_id: "!room:example".to_owned(),
                                },
                                title: "Room".to_owned(),
                                active: true,
                                placement: MdiWindowPlacement {
                                    state: MdiWindowState::Maximized,
                                    x: 0,
                                    y: 0,
                                    width: 320,
                                    height: 200,
                                },
                                lines: vec!["hello".to_owned()],
                                members: vec![RoomMemberView {
                                    user_id: "@alice:example".to_owned(),
                                    display_name: Some("Alice".to_owned()),
                                    label: "Alice".to_owned(),
                                    rank: RoomMemberRank::Moderator,
                                    can_kick: true,
                                    can_ban: false,
                                }],
                                log: None,
                            },
                            MdiView {
                                identity: ViewIdentity::Thread {
                                    room_id: "!room:example".to_owned(),
                                    thread_root_event_id: "$thread-root:example".to_owned(),
                                },
                                title: "Thread".to_owned(),
                                active: false,
                                placement: MdiWindowPlacement {
                                    state: MdiWindowState::Normal,
                                    x: 0,
                                    y: 0,
                                    width: 320,
                                    height: 200,
                                },
                                lines: vec!["reply".to_owned()],
                                members: Vec::new(),
                                log: None,
                            },
                        ],
                    },
                    status_bar: StatusBarState {
                        text: "synced 1 room(s)".to_owned(),
                    },
                    switchbar: SwitchbarState {
                        rows: 1,
                        items: vec![
                            SwitchbarItem {
                                identity: ViewIdentity::Status,
                                title: "Status".to_owned(),
                                active: false,
                                activity: None,
                                rect: UiRect {
                                    x: 0,
                                    y: 0,
                                    width: 120,
                                    height: 36,
                                },
                            },
                            SwitchbarItem {
                                identity: ViewIdentity::Room {
                                    room_id: "!room:example".to_owned(),
                                },
                                title: "Room".to_owned(),
                                active: true,
                                activity: Some(ViewActivityLevel::Message),
                                rect: UiRect {
                                    x: 120,
                                    y: 0,
                                    width: 120,
                                    height: 36,
                                },
                            },
                            SwitchbarItem {
                                identity: ViewIdentity::Thread {
                                    room_id: "!room:example".to_owned(),
                                    thread_root_event_id: "$thread-root:example".to_owned(),
                                },
                                title: "Thread".to_owned(),
                                active: false,
                                activity: Some(ViewActivityLevel::Message),
                                rect: UiRect {
                                    x: 240,
                                    y: 0,
                                    width: 120,
                                    height: 36,
                                },
                            },
                        ],
                    },
                    frame_layout: UiFrameLayout {
                        switchbar: Some(UiRect {
                            x: 0,
                            y: 0,
                            width: 1008,
                            height: 28,
                        }),
                        treebar: None,
                        mdi_client: UiRect {
                            x: 0,
                            y: 28,
                            width: 1008,
                            height: 600,
                        },
                    },
                    dpi: 96,
                    active_find: Some(UiFindState {
                        active: ViewIdentity::Room {
                            room_id: "!room:example".to_owned(),
                        },
                        query: "hello".to_owned(),
                        matched: true,
                        match_count: 1,
                        current_match_index: Some(0),
                        selected_text: Some("hello".to_owned()),
                        history_search: UiFindHistorySearchState::LocalOnly,
                    }),
                    rooms: vec![UiRoomState {
                        room_id: "!room:example".to_owned(),
                        display_name: "Room".to_owned(),
                        membership: RoomMembership::Join,
                        encrypted: true,
                        is_space: false,
                        is_direct: true,
                        direct_user_ids: vec!["@alice:example".to_owned()],
                        tags: vec![super::DumpRoomTag {
                            tag: "m.favourite".to_owned(),
                            order: Some("0.25".to_owned()),
                        }],
                        space_children: Vec::new(),
                        activity: Some(ViewActivityLevel::Message),
                        read_target_event_id: Some("$event:example".to_owned()),
                        read_state: Some(UiRoomReadState {
                            latest_active_event_id: Some("$latest:example".to_owned()),
                            fully_read_event_id: Some("$fully-read:example".to_owned()),
                            marked_unread: true,
                            num_unread: 7,
                            num_notifications: 3,
                            num_mentions: 1,
                        }),
                        history_backfill: UiRoomHistoryBackfillState::Idle {
                            next: HistoryFrom::Latest,
                        },
                        timeline_row_builds: 4,
                        buffer_lines: 2,
                    }],
                    text_font: super::UiTextFont {
                        face: "Fixedsys".to_owned(),
                        points: 9,
                        bold: false,
                        line_height: 15,
                        edit_height: 25,
                        average_char_width: 8,
                        fallback_faces: vec!["Tahoma".to_owned(), "Noto Emoji".to_owned()],
                    },
                    timestamp: super::UiTimestamp {
                        show: true,
                        format: "[HH:nn]".to_owned(),
                    },
                    colors: super::UiColors {
                        items: vec![super::UiColorItem {
                            item: "join".to_owned(),
                            index: 3,
                            color: "#009300".to_owned(),
                        }],
                        switchbar: super::UiActivityColors {
                            event: 5,
                            message: 4,
                            highlight: 12,
                        },
                    },
                    active_system_menu_items: vec!["&Restore".to_owned(), "&Font...".to_owned()],
                    active_system_menu_checked: vec![],
                    active_system_menu_disabled: vec!["&Restore".to_owned()],
                    menu_bar: vec![super::UiMenu {
                        label: "&Help".to_owned(),
                        items: vec![super::UiMenuItem {
                            label: "&Help Topics".to_owned(),
                            enabled: true,
                            checked: false,
                        }],
                    }],
                    compose_target: Some(super::UiComposeTarget {
                        kind: super::UiComposeTargetKind::Reply,
                        event_id: "$question".to_owned(),
                    }),
                    room_list: Some(super::UiRoomList {
                        columns: vec!["Room".to_owned(), "Users".to_owned()],
                        rows: vec![vec!["#rust:example".to_owned(), "42".to_owned()]],
                        selected: Some(0),
                        column_click_points: vec![super::UiPoint { x: 10, y: 8 }],
                        visible_row_click_points: vec![super::UiPoint { x: 12, y: 24 }],
                        top_row: 0,
                    }),
                    options_dialog: Some(super::UiOptionsDialog {
                        tree: vec!["Connect".to_owned(), "Connect/Options".to_owned()],
                        selected: "Connect/Options".to_owned(),
                        connect: super::UiConnectOptions {
                            startup: true,
                            reconnect: false,
                            retry: true,
                            retry_times: "99".to_owned(),
                            retry_delay: "".to_owned(),
                            retry_fields_enabled: true,
                        },
                        visible_controls: vec![6101],
                        highlight_items: vec!["deploy".to_owned()],
                        highlight_controls_enabled: true,
                        highlight_delete_enabled: false,
                        activity_colors: super::UiActivityColors {
                            event: 5,
                            message: 4,
                            highlight: 12,
                        },
                        logging: super::UiLoggingOptions {
                            channels: true,
                            queries: false,
                            strip_codes: false,
                            timestamp: true,
                        },
                        tray: super::UiTrayOptions {
                            always: true,
                            startup: false,
                            minimize: true,
                            single_click: false,
                        },
                        tips: super::UiTipsOptions {
                            highlights: true,
                            queries: false,
                            channels: true,
                        },
                    }),
                    colors_dialog: Some(super::UiColorsDialog {
                        selected: "normal".to_owned(),
                        items: vec![super::UiColorItem {
                            item: "normal".to_owned(),
                            index: 1,
                            color: "#000000".to_owned(),
                        }],
                        palette_cells: vec![super::UiRect {
                            x: 0,
                            y: 0,
                            width: 20,
                            height: 20,
                        }],
                        preview_items: vec![super::UiColorsPreviewItem {
                            item: "join".to_owned(),
                            click_point: super::UiPoint { x: 80, y: 11 },
                        }],
                    }),
                    color_index: Some(super::UiColorIndex {
                        part: super::UiColorCodePart::Background,
                        rect: super::UiRect {
                            x: 10,
                            y: 500,
                            width: 180,
                            height: 44,
                        },
                        cells: vec![super::UiRect {
                            x: 2,
                            y: 2,
                            width: 20,
                            height: 18,
                        }],
                    }),
                    active_timeline: Some(super::UiActiveTimeline {
                        top_line: 2,
                        visible_lines: vec![super::UiTimelineLine {
                            text: "    > <Alice> are we deploying?".to_owned(),
                            kind: super::UiTimelineLineKind::ReplyQuote,
                            color: "#7f7f7f".to_owned(),
                            styles: Vec::new(),
                            event_id: Some("$answer".to_owned()),
                            click_point: super::UiPoint { x: 8, y: 9 },
                            text_left: 8,
                            links: vec![super::UiTimelineLink {
                                start: 4,
                                end: 9,
                                url: "#a:b.c".to_owned(),
                                user_id: None,
                            }],
                        }],
                    }),
                    tray: super::UiTray {
                        icon: true,
                        hidden: false,
                        menu: vec![super::UiTrayMenuItem {
                            label: "Status".to_owned(),
                            checked: false,
                        }],
                    },
                    ignored_users: Some(vec!["@spam:example".to_owned()]),
                    refresh_counters: super::UiRefreshCounters {
                        timeline_row_builds: 5,
                        rows_wrapped: 6,
                        treebar_rebuilds: 7,
                        switchbar_rebuilds: 8,
                    },
                },
            ))
            .unwrap(),
            json!({
                "id":"ui",
                "ok":true,
                "json":{
                    "navigation":{
                        "text":"Status",
                        "view":{"kind":"status"},
                        "space_id":null,
                        "expanded":true,
                        "activity":null,
                        "children":[
                            {"text":"Work","view":null,"space_id":"!work:example","expanded":false,"activity":"message","children":[
                                {"text":"Room","view":{"kind":"room","room_id":"!room:example"},"space_id":null,"expanded":null,"activity":"message","children":[]}
                            ]}
                        ]
                    },
                    "mdi":{
                        "active":{"kind":"room","room_id":"!room:example"},
                        "views":[
                            {
                                "identity":{"kind":"status"},
                                "title":"Status",
                                "active":false,
                                "placement":{"state":"minimized","x":0,"y":0,"width":320,"height":200},
                                "lines":["status: synced"],
                                "members":[],
                                "log":null
                            },
                            {
                                "identity":{"kind":"room","room_id":"!room:example"},
                                "title":"Room",
                                "active":true,
                                "placement":{"state":"maximized","x":0,"y":0,"width":320,"height":200},
                                "lines":["hello"],
                                "members":[
                                    {
                                        "user_id":"@alice:example",
                                        "display_name":"Alice",
                                        "label":"Alice",
                                        "rank":"moderator",
                                        "can_kick":true,
                                        "can_ban":false
                                    }
                                ],
                                "log":null
                            },
                            {
                                "identity":{"kind":"thread","room_id":"!room:example","thread_root_event_id":"$thread-root:example"},
                                "title":"Thread",
                                "active":false,
                                "placement":{"state":"normal","x":0,"y":0,"width":320,"height":200},
                                "lines":["reply"],
                                "members":[],
                                "log":null
                            }
                        ]
                    },
                    "status_bar":{
                        "text":"synced 1 room(s)"
                    },
                    "switchbar":{
                        "rows":1,
                        "items":[
                            {
                                "identity":{"kind":"status"},
                                "title":"Status",
                                "active":false,
                                "activity":null,
                                "rect":{"x":0,"y":0,"width":120,"height":36}
                            },
                            {
                                "identity":{"kind":"room","room_id":"!room:example"},
                                "title":"Room",
                                "active":true,
                                "activity":"message",
                                "rect":{"x":120,"y":0,"width":120,"height":36}
                            },
                            {
                                "identity":{"kind":"thread","room_id":"!room:example","thread_root_event_id":"$thread-root:example"},
                                "title":"Thread",
                                "active":false,
                                "activity":"message",
                                "rect":{"x":240,"y":0,"width":120,"height":36}
                            }
                        ]
                    },
                    "frame_layout":{
                        "switchbar":{"x":0,"y":0,"width":1008,"height":28},
                        "treebar":null,
                        "mdi_client":{"x":0,"y":28,"width":1008,"height":600}
                    },
                    "dpi":96,
                    "active_find":{
                        "active":{"kind":"room","room_id":"!room:example"},
                        "query":"hello",
                        "matched":true,
                        "match_count":1,
                        "current_match_index":0,
                        "selected_text":"hello",
                        "history_search":{"state":"local_only"}
                    },
                    "rooms":[
                        {
                            "room_id":"!room:example",
                            "display_name":"Room",
                            "membership":"join",
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
                            "activity":"message",
                            "read_target_event_id":"$event:example",
                            "read_state":{
                                "latest_active_event_id":"$latest:example",
                                "fully_read_event_id":"$fully-read:example",
                                "marked_unread":true,
                                "num_unread":7,
                                "num_notifications":3,
                                "num_mentions":1
                            },
                            "history_backfill":{"state":"idle","next":{"type":"latest"}},
                            "timeline_row_builds":4,
                            "buffer_lines":2
                        }
                    ],
                    "text_font":{
                        "face":"Fixedsys",
                        "points":9,
                        "bold":false,
                        "line_height":15,
                        "edit_height":25,
                        "average_char_width":8,
                        "fallback_faces":["Tahoma","Noto Emoji"]
                    },
                    "timestamp":{"show":true,"format":"[HH:nn]"},
                    "colors":{
                        "items":[{"item":"join","index":3,"color":"#009300"}],
                        "switchbar":{"event":5,"message":4,"highlight":12}
                    },
                    "active_system_menu_items":["&Restore","&Font..."],
                    "active_system_menu_checked":[],
                    "active_system_menu_disabled":["&Restore"],
                    "menu_bar":[{"label":"&Help","items":[{"label":"&Help Topics","enabled":true,"checked":false}]}],
                    "compose_target":{"kind":"reply","event_id":"$question"},
                    "room_list":{
                        "columns":["Room","Users"],
                        "rows":[["#rust:example","42"]],
                        "selected":0,
                        "column_click_points":[{"x":10,"y":8}],
                        "visible_row_click_points":[{"x":12,"y":24}],
                        "top_row":0
                    },
                    "options_dialog":{
                        "tree":["Connect","Connect/Options"],
                        "selected":"Connect/Options",
                        "connect":{
                            "startup":true,
                            "reconnect":false,
                            "retry":true,
                            "retry_times":"99",
                            "retry_delay":"",
                            "retry_fields_enabled":true
                        },
                        "visible_controls":[6101],
                        "highlight_items":["deploy"],
                        "highlight_controls_enabled":true,
                        "highlight_delete_enabled":false,
                        "activity_colors":{"event":5,"message":4,"highlight":12},
                        "logging":{"channels":true,"queries":false,"strip_codes":false,"timestamp":true},
                        "tray":{"always":true,"startup":false,"minimize":true,"single_click":false},
                        "tips":{"highlights":true,"queries":false,"channels":true}
                    },
                    "colors_dialog":{
                        "selected":"normal",
                        "items":[{"item":"normal","index":1,"color":"#000000"}],
                        "palette_cells":[{"x":0,"y":0,"width":20,"height":20}],
                        "preview_items":[{"item":"join","click_point":{"x":80,"y":11}}]
                    },
                    "color_index":{
                        "part":"background",
                        "rect":{"x":10,"y":500,"width":180,"height":44},
                        "cells":[{"x":2,"y":2,"width":20,"height":18}]
                    },
                    "active_timeline":{
                        "top_line":2,
                        "visible_lines":[{
                            "text":"    > <Alice> are we deploying?",
                            "kind":"reply_quote",
                            "color":"#7f7f7f",
                            "styles":[],
                            "event_id":"$answer",
                            "click_point":{"x":8,"y":9},
                            "text_left":8,
                            "links":[{"start":4,"end":9,"url":"#a:b.c","user_id":null}]
                        }]
                    },
                    "tray":{"icon":true,"hidden":false,"menu":[{"label":"Status","checked":false}]},
                    "ignored_users":["@spam:example"],
                    "refresh_counters":{
                        "timeline_row_builds":5,
                        "rows_wrapped":6,
                        "treebar_rebuilds":7,
                        "switchbar_rebuilds":8
                    }
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
            connection: ConnectionState::Syncing,
            reconnect_attempts: 0,
            rooms: Vec::new(),
            last_activity: None,
            e2ee_status: Some(sample_e2ee_status()),
            last_e2ee_recovery_setup_result: None,
            last_verification: None,
            submitted_membership_commands: Vec::new(),
            submitted_member_moderation_commands: Vec::new(),
            submitted_power_level_commands: Vec::new(),
            submitted_profile_commands: Vec::new(),
            submitted_user_commands: Vec::new(),
            submitted_direct_room_commands: Vec::new(),
            submitted_room_topic_commands: Vec::new(),
            submitted_reaction_commands: Vec::new(),
            submitted_edit_commands: Vec::new(),
            submitted_reply_commands: Vec::new(),
            submitted_message_commands: Vec::new(),
            submitted_file_commands: Vec::new(),
            submitted_notification_commands: Vec::new(),
            submitted_redaction_commands: Vec::new(),
            submitted_history_commands: Vec::new(),
            submitted_verification_actions: Vec::new(),
            submitted_verification_requests: Vec::new(),
            identity_warnings: Vec::new(),
            submitted_identity_reset_commands: Vec::new(),
            submitted_room_key_transfers: Vec::new(),
            submitted_session_commands: Vec::new(),
            submitted_recovery_commands: Vec::new(),
            submitted_setup_recovery_commands: Vec::new(),
            submitted_room_list_requests: Vec::new(),
            opened_links: Vec::new(),
            shown_tips: Vec::new(),
            last_error: None,
            selected_room: None,
        };
        let json = serde_json::to_value(&model).unwrap();
        assert_eq!(serde_json::from_value::<DumpModel>(json).unwrap(), model);

        for invalid_json in [
            json!({
                "status":"ready",
                "lifecycle_state":"ready",
                "connection":"syncing", "reconnect_attempts":0,
                "rooms":[],
                "last_activity":null,
                "e2ee_status":{
                    "recovery":"incomplete",
                    "backup":{"state":"enabled","enabled_locally":true,"exists_on_server":true},
                    "cross_signing":{"has_master":true,"has_self_signing":true,"has_user_signing":false,"complete":false},
                    "session":"verified",
                    "unexpected":true
                },
                "last_verification":null,
                "last_error":null,
                "selected_room":null
            }),
            json!({
                "status":"ready",
                "lifecycle_state":"ready",
                "connection":"syncing", "reconnect_attempts":0,
                "rooms":[],
                "last_activity":null,
                "e2ee_status":{
                    "recovery":"incomplete",
                    "backup":{"state":"enabled","enabled_locally":true,"exists_on_server":true,"unexpected":true},
                    "cross_signing":{"has_master":true,"has_self_signing":true,"has_user_signing":false,"complete":false},
                    "session":"verified"
                },
                "last_verification":null,
                "last_error":null,
                "selected_room":null
            }),
            json!({
                "status":"ready",
                "lifecycle_state":"ready",
                "connection":"syncing", "reconnect_attempts":0,
                "rooms":[],
                "last_activity":null,
                "e2ee_status":{
                    "recovery":"incomplete",
                    "backup":{"state":"enabled","enabled_locally":true,"exists_on_server":true},
                    "cross_signing":{"has_master":true,"has_self_signing":true,"has_user_signing":false,"complete":false,"unexpected":true},
                    "session":"verified"
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
    fn dump_model_round_trips_setup_recovery_data_and_rejects_unknown_nested_fields() {
        let model = DumpModel {
            status: "ready".to_owned(),
            lifecycle_state: LifecycleState::Ready,
            connection: ConnectionState::Syncing,
            reconnect_attempts: 0,
            rooms: Vec::new(),
            last_activity: None,
            e2ee_status: None,
            last_e2ee_recovery_setup_result: Some(DumpE2eeRecoverySetupResult {
                recovery_key_present: true,
            }),
            last_verification: None,
            submitted_membership_commands: Vec::new(),
            submitted_member_moderation_commands: Vec::new(),
            submitted_power_level_commands: Vec::new(),
            submitted_profile_commands: Vec::new(),
            submitted_user_commands: Vec::new(),
            submitted_direct_room_commands: Vec::new(),
            submitted_room_topic_commands: Vec::new(),
            submitted_reaction_commands: Vec::new(),
            submitted_edit_commands: Vec::new(),
            submitted_reply_commands: Vec::new(),
            submitted_message_commands: Vec::new(),
            submitted_file_commands: Vec::new(),
            submitted_notification_commands: Vec::new(),
            submitted_redaction_commands: Vec::new(),
            submitted_history_commands: Vec::new(),
            submitted_verification_actions: Vec::new(),
            submitted_verification_requests: Vec::new(),
            identity_warnings: Vec::new(),
            submitted_identity_reset_commands: Vec::new(),
            submitted_room_key_transfers: Vec::new(),
            submitted_session_commands: Vec::new(),
            submitted_recovery_commands: Vec::new(),
            submitted_setup_recovery_commands: vec![DumpSubmittedSetupRecoveryCommand {
                passphrase_redacted: true,
                wait_for_backups_to_upload: true,
            }],
            submitted_room_list_requests: Vec::new(),
            opened_links: Vec::new(),
            shown_tips: Vec::new(),
            last_error: None,
            selected_room: None,
        };
        let json = serde_json::to_value(&model).unwrap();
        assert_eq!(serde_json::from_value::<DumpModel>(json).unwrap(), model);

        for invalid_json in [
            json!({
                "status":"ready", "lifecycle_state":"ready", "connection":"syncing", "reconnect_attempts":0, "rooms":[], "last_activity":null,
                "e2ee_status":null,
                "last_e2ee_recovery_setup_result":{"recovery_key_present":true,"unexpected":true},
                "last_verification":null, "last_error":null, "selected_room":null,
            }),
            json!({
                "status":"ready", "lifecycle_state":"ready", "connection":"syncing", "reconnect_attempts":0, "rooms":[], "last_activity":null,
                "e2ee_status":null, "last_e2ee_recovery_setup_result":null, "last_verification":null,
                "submitted_setup_recovery_commands":[{"passphrase_redacted":true,"wait_for_backups_to_upload":true,"unexpected":true}],
                "last_error":null, "selected_room":null,
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
            connection: ConnectionState::Syncing,
            reconnect_attempts: 0,
            rooms: Vec::new(),
            last_activity: None,
            e2ee_status: None,
            last_e2ee_recovery_setup_result: None,
            last_verification: None,
            submitted_membership_commands: Vec::new(),
            submitted_member_moderation_commands: Vec::new(),
            submitted_power_level_commands: Vec::new(),
            submitted_profile_commands: Vec::new(),
            submitted_user_commands: Vec::new(),
            submitted_direct_room_commands: Vec::new(),
            submitted_room_topic_commands: vec![DumpSubmittedRoomTopicCommand {
                room_id: "!ops:example".to_owned(),
                topic: "incident review".to_owned(),
            }],
            submitted_reaction_commands: Vec::new(),
            submitted_edit_commands: Vec::new(),
            submitted_reply_commands: Vec::new(),
            submitted_message_commands: Vec::new(),
            submitted_file_commands: Vec::new(),
            submitted_notification_commands: Vec::new(),
            submitted_redaction_commands: Vec::new(),
            submitted_history_commands: Vec::new(),
            submitted_verification_actions: Vec::new(),
            submitted_verification_requests: Vec::new(),
            identity_warnings: Vec::new(),
            submitted_identity_reset_commands: Vec::new(),
            submitted_room_key_transfers: Vec::new(),
            submitted_session_commands: Vec::new(),
            submitted_recovery_commands: Vec::new(),
            submitted_setup_recovery_commands: Vec::new(),
            submitted_room_list_requests: Vec::new(),
            opened_links: Vec::new(),
            shown_tips: Vec::new(),
            last_error: None,
            selected_room: Some("!ops:example".to_owned()),
        };
        let json = serde_json::to_value(&model).unwrap();
        assert_eq!(serde_json::from_value::<DumpModel>(json).unwrap(), model);

        let error = serde_json::from_value::<DumpModel>(json!({
            "status":"ready",
            "lifecycle_state":"ready",
            "connection":"syncing", "reconnect_attempts":0,
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
            "submitted_reaction_commands":[],
            "submitted_redaction_commands":[],
            "last_error":null,
            "selected_room":"!ops:example"
        }))
        .unwrap_err();

        assert!(error.to_string().contains("unknown field `unexpected`"));
    }

    #[test]
    fn dump_model_round_trips_submitted_edit_commands_and_rejects_unknown_nested_fields() {
        let model = DumpModel {
            status: "ready".to_owned(),
            lifecycle_state: LifecycleState::Ready,
            connection: ConnectionState::Syncing,
            reconnect_attempts: 0,
            rooms: Vec::new(),
            last_activity: None,
            e2ee_status: None,
            last_e2ee_recovery_setup_result: None,
            last_verification: None,
            submitted_membership_commands: Vec::new(),
            submitted_member_moderation_commands: Vec::new(),
            submitted_power_level_commands: Vec::new(),
            submitted_profile_commands: Vec::new(),
            submitted_user_commands: Vec::new(),
            submitted_direct_room_commands: Vec::new(),
            submitted_room_topic_commands: Vec::new(),
            submitted_reaction_commands: Vec::new(),
            submitted_edit_commands: vec![DumpSubmittedEditCommand {
                room_id: "!ops:example".to_owned(),
                target_event_id: "$target:example".to_owned(),
                body: "replacement text".to_owned(),
                styles: Vec::new(),
            }],
            submitted_reply_commands: Vec::new(),
            submitted_message_commands: Vec::new(),
            submitted_file_commands: Vec::new(),
            submitted_notification_commands: Vec::new(),
            submitted_redaction_commands: Vec::new(),
            submitted_history_commands: Vec::new(),
            submitted_verification_actions: Vec::new(),
            submitted_verification_requests: Vec::new(),
            identity_warnings: Vec::new(),
            submitted_identity_reset_commands: Vec::new(),
            submitted_room_key_transfers: Vec::new(),
            submitted_session_commands: Vec::new(),
            submitted_recovery_commands: Vec::new(),
            submitted_setup_recovery_commands: Vec::new(),
            submitted_room_list_requests: Vec::new(),
            opened_links: Vec::new(),
            shown_tips: Vec::new(),
            last_error: None,
            selected_room: Some("!ops:example".to_owned()),
        };
        let json = serde_json::to_value(&model).unwrap();
        assert_eq!(
            json,
            json!({
                "status":"ready",
                "lifecycle_state":"ready",
                "connection":"syncing", "reconnect_attempts":0,
                "rooms":[],
                "last_activity":null,
                "e2ee_status":null,
                "last_verification":null,
                "submitted_edit_commands":[
                    {
                        "room_id":"!ops:example",
                        "target_event_id":"$target:example",
                        "body":"replacement text",
                        "styles":[]
                    }
                ],
                "last_error":null,
                "selected_room":"!ops:example"
            })
        );
        assert_eq!(serde_json::from_value::<DumpModel>(json).unwrap(), model);

        let error = serde_json::from_value::<DumpModel>(json!({
            "status":"ready",
            "lifecycle_state":"ready",
            "connection":"syncing", "reconnect_attempts":0,
            "rooms":[],
            "last_activity":null,
            "e2ee_status":null,
            "last_verification":null,
            "submitted_membership_commands":[],
            "submitted_direct_room_commands":[],
            "submitted_room_topic_commands":[],
            "submitted_reaction_commands":[],
            "submitted_edit_commands":[
                {
                    "room_id":"!ops:example",
                    "target_event_id":"$target:example",
                    "body":"replacement text",
                    "styles":[],
                    "unexpected":true
                }
            ],
            "submitted_redaction_commands":[],
            "last_error":null,
            "selected_room":"!ops:example"
        }))
        .unwrap_err();

        assert!(error.to_string().contains("unknown field `unexpected`"));
    }

    #[test]
    fn dump_model_round_trips_submitted_reply_commands_and_rejects_unknown_nested_fields() {
        let model = DumpModel {
            status: "ready".to_owned(),
            lifecycle_state: LifecycleState::Ready,
            connection: ConnectionState::Syncing,
            reconnect_attempts: 0,
            rooms: Vec::new(),
            last_activity: None,
            e2ee_status: None,
            last_e2ee_recovery_setup_result: None,
            last_verification: None,
            submitted_membership_commands: Vec::new(),
            submitted_member_moderation_commands: Vec::new(),
            submitted_power_level_commands: Vec::new(),
            submitted_profile_commands: Vec::new(),
            submitted_user_commands: Vec::new(),
            submitted_direct_room_commands: Vec::new(),
            submitted_room_topic_commands: Vec::new(),
            submitted_reaction_commands: Vec::new(),
            submitted_edit_commands: Vec::new(),
            submitted_reply_commands: vec![DumpSubmittedReplyCommand {
                room_id: "!ops:example".to_owned(),
                target_event_id: "$target:example".to_owned(),
                sender_user_id: "@alice:example".to_owned(),
                body: "reply text".to_owned(),
                styles: Vec::new(),
                thread: None,
            }],
            submitted_message_commands: Vec::new(),
            submitted_file_commands: Vec::new(),
            submitted_notification_commands: Vec::new(),
            submitted_redaction_commands: Vec::new(),
            submitted_history_commands: Vec::new(),
            submitted_verification_actions: Vec::new(),
            submitted_verification_requests: Vec::new(),
            identity_warnings: Vec::new(),
            submitted_identity_reset_commands: Vec::new(),
            submitted_room_key_transfers: Vec::new(),
            submitted_session_commands: Vec::new(),
            submitted_recovery_commands: Vec::new(),
            submitted_setup_recovery_commands: Vec::new(),
            submitted_room_list_requests: Vec::new(),
            opened_links: Vec::new(),
            shown_tips: Vec::new(),
            last_error: None,
            selected_room: Some("!ops:example".to_owned()),
        };
        let json = serde_json::to_value(&model).unwrap();
        assert_eq!(
            json,
            json!({
                "status":"ready",
                "lifecycle_state":"ready",
                "connection":"syncing", "reconnect_attempts":0,
                "rooms":[],
                "last_activity":null,
                "e2ee_status":null,
                "last_verification":null,
                "submitted_reply_commands":[
                    {
                        "room_id":"!ops:example",
                        "target_event_id":"$target:example",
                        "sender_user_id":"@alice:example",
                        "body":"reply text",
                        "styles":[]
                    }
                ],
                "last_error":null,
                "selected_room":"!ops:example"
            })
        );
        assert_eq!(serde_json::from_value::<DumpModel>(json).unwrap(), model);

        let error = serde_json::from_value::<DumpModel>(json!({
            "status":"ready",
            "lifecycle_state":"ready",
            "connection":"syncing", "reconnect_attempts":0,
            "rooms":[],
            "last_activity":null,
            "e2ee_status":null,
            "last_verification":null,
            "submitted_membership_commands":[],
            "submitted_direct_room_commands":[],
            "submitted_room_topic_commands":[],
            "submitted_reaction_commands":[],
            "submitted_edit_commands":[],
            "submitted_reply_commands":[
                {
                    "room_id":"!ops:example",
                    "target_event_id":"$target:example",
                    "sender_user_id":"@alice:example",
                    "body":"reply text",
                    "styles":[],
                    "unexpected":true
                }
            ],
            "submitted_redaction_commands":[],
            "last_error":null,
            "selected_room":"!ops:example"
        }))
        .unwrap_err();

        assert!(error.to_string().contains("unknown field `unexpected`"));
    }

    #[test]
    fn dump_model_round_trips_threaded_submitted_reply_commands_and_rejects_unknown_thread_fields()
    {
        let model = DumpModel {
            status: "ready".to_owned(),
            lifecycle_state: LifecycleState::Ready,
            connection: ConnectionState::Syncing,
            reconnect_attempts: 0,
            rooms: Vec::new(),
            last_activity: None,
            e2ee_status: None,
            last_e2ee_recovery_setup_result: None,
            last_verification: None,
            submitted_membership_commands: Vec::new(),
            submitted_member_moderation_commands: Vec::new(),
            submitted_power_level_commands: Vec::new(),
            submitted_profile_commands: Vec::new(),
            submitted_user_commands: Vec::new(),
            submitted_direct_room_commands: Vec::new(),
            submitted_room_topic_commands: Vec::new(),
            submitted_reaction_commands: Vec::new(),
            submitted_edit_commands: Vec::new(),
            submitted_reply_commands: vec![DumpSubmittedReplyCommand {
                room_id: "!ops:example".to_owned(),
                target_event_id: "$target:example".to_owned(),
                sender_user_id: "@alice:example".to_owned(),
                body: "reply text".to_owned(),
                styles: Vec::new(),
                thread: Some(DumpSubmittedReplyThreadTarget {
                    thread_root_event_id: "$thread-root:example".to_owned(),
                    latest_event_id: "$latest:example".to_owned(),
                }),
            }],
            submitted_message_commands: Vec::new(),
            submitted_file_commands: Vec::new(),
            submitted_notification_commands: Vec::new(),
            submitted_redaction_commands: Vec::new(),
            submitted_history_commands: Vec::new(),
            submitted_verification_actions: Vec::new(),
            submitted_verification_requests: Vec::new(),
            identity_warnings: Vec::new(),
            submitted_identity_reset_commands: Vec::new(),
            submitted_room_key_transfers: Vec::new(),
            submitted_session_commands: Vec::new(),
            submitted_recovery_commands: Vec::new(),
            submitted_setup_recovery_commands: Vec::new(),
            submitted_room_list_requests: Vec::new(),
            opened_links: Vec::new(),
            shown_tips: Vec::new(),
            last_error: None,
            selected_room: Some("!ops:example".to_owned()),
        };
        let json = serde_json::to_value(&model).unwrap();
        assert_eq!(
            json,
            json!({
                "status":"ready",
                "lifecycle_state":"ready",
                "connection":"syncing", "reconnect_attempts":0,
                "rooms":[],
                "last_activity":null,
                "e2ee_status":null,
                "last_verification":null,
                "submitted_reply_commands":[
                    {
                        "room_id":"!ops:example",
                        "target_event_id":"$target:example",
                        "sender_user_id":"@alice:example",
                        "body":"reply text",
                        "styles":[],
                        "thread":{
                            "thread_root_event_id":"$thread-root:example",
                            "latest_event_id":"$latest:example"
                        }
                    }
                ],
                "last_error":null,
                "selected_room":"!ops:example"
            })
        );
        assert_eq!(serde_json::from_value::<DumpModel>(json).unwrap(), model);

        let error = serde_json::from_value::<DumpModel>(json!({
            "status":"ready",
            "lifecycle_state":"ready",
            "connection":"syncing", "reconnect_attempts":0,
            "rooms":[],
            "last_activity":null,
            "e2ee_status":null,
            "last_verification":null,
            "submitted_membership_commands":[],
            "submitted_direct_room_commands":[],
            "submitted_room_topic_commands":[],
            "submitted_reaction_commands":[],
            "submitted_edit_commands":[],
            "submitted_reply_commands":[
                {
                    "room_id":"!ops:example",
                    "target_event_id":"$target:example",
                    "sender_user_id":"@alice:example",
                    "body":"reply text",
                    "styles":[],
                    "thread":{
                        "thread_root_event_id":"$thread-root:example",
                        "latest_event_id":"$latest:example",
                        "unexpected":true
                    }
                }
            ],
            "submitted_redaction_commands":[],
            "last_error":null,
            "selected_room":"!ops:example"
        }))
        .unwrap_err();

        assert!(error.to_string().contains("unknown field `unexpected`"));
    }

    #[test]
    fn dump_model_round_trips_submitted_reaction_commands_and_rejects_unknown_nested_fields() {
        let model = DumpModel {
            status: "ready".to_owned(),
            lifecycle_state: LifecycleState::Ready,
            connection: ConnectionState::Syncing,
            reconnect_attempts: 0,
            rooms: Vec::new(),
            last_activity: None,
            e2ee_status: None,
            last_e2ee_recovery_setup_result: None,
            last_verification: None,
            submitted_membership_commands: Vec::new(),
            submitted_member_moderation_commands: Vec::new(),
            submitted_power_level_commands: Vec::new(),
            submitted_profile_commands: Vec::new(),
            submitted_user_commands: Vec::new(),
            submitted_direct_room_commands: Vec::new(),
            submitted_room_topic_commands: Vec::new(),
            submitted_reaction_commands: vec![DumpSubmittedReactionCommand {
                room_id: "!ops:example".to_owned(),
                target_event_id: "$target:example".to_owned(),
                key: "+1".to_owned(),
            }],
            submitted_edit_commands: Vec::new(),
            submitted_reply_commands: Vec::new(),
            submitted_message_commands: Vec::new(),
            submitted_file_commands: Vec::new(),
            submitted_notification_commands: Vec::new(),
            submitted_redaction_commands: vec![DumpSubmittedRedactionCommand {
                room_id: "!ops:example".to_owned(),
                event_id: "$reaction:example".to_owned(),
            }],
            submitted_history_commands: Vec::new(),
            submitted_verification_actions: Vec::new(),
            submitted_verification_requests: Vec::new(),
            identity_warnings: Vec::new(),
            submitted_identity_reset_commands: Vec::new(),
            submitted_room_key_transfers: Vec::new(),
            submitted_session_commands: Vec::new(),
            submitted_recovery_commands: Vec::new(),
            submitted_setup_recovery_commands: Vec::new(),
            submitted_room_list_requests: Vec::new(),
            opened_links: Vec::new(),
            shown_tips: Vec::new(),
            last_error: None,
            selected_room: Some("!ops:example".to_owned()),
        };
        let json = serde_json::to_value(&model).unwrap();
        assert_eq!(
            json,
            json!({
                "status":"ready",
                "lifecycle_state":"ready",
                "connection":"syncing", "reconnect_attempts":0,
                "rooms":[],
                "last_activity":null,
                "e2ee_status":null,
                "last_verification":null,
                "submitted_reaction_commands":[
                    {
                        "room_id":"!ops:example",
                        "target_event_id":"$target:example",
                        "key":"+1"
                    }
                ],
                "submitted_redaction_commands":[
                    {
                        "room_id":"!ops:example",
                        "event_id":"$reaction:example"
                    }
                ],
                "last_error":null,
                "selected_room":"!ops:example"
            })
        );
        assert_eq!(serde_json::from_value::<DumpModel>(json).unwrap(), model);

        let reaction_error = serde_json::from_value::<DumpModel>(json!({
            "status":"ready",
            "lifecycle_state":"ready",
            "connection":"syncing", "reconnect_attempts":0,
            "rooms":[],
            "last_activity":null,
            "e2ee_status":null,
            "last_verification":null,
            "submitted_membership_commands":[],
            "submitted_direct_room_commands":[],
            "submitted_room_topic_commands":[],
            "submitted_reaction_commands":[
                {
                    "room_id":"!ops:example",
                    "target_event_id":"$target:example",
                    "key":"+1",
                    "unexpected":true
                }
            ],
            "submitted_redaction_commands":[],
            "last_error":null,
            "selected_room":"!ops:example"
        }))
        .unwrap_err();
        assert!(
            reaction_error
                .to_string()
                .contains("unknown field `unexpected`")
        );

        let redaction_error = serde_json::from_value::<DumpModel>(json!({
            "status":"ready",
            "lifecycle_state":"ready",
            "connection":"syncing", "reconnect_attempts":0,
            "rooms":[],
            "last_activity":null,
            "e2ee_status":null,
            "last_verification":null,
            "submitted_membership_commands":[],
            "submitted_direct_room_commands":[],
            "submitted_room_topic_commands":[],
            "submitted_reaction_commands":[],
            "submitted_redaction_commands":[
                {
                    "room_id":"!ops:example",
                    "event_id":"$reaction:example",
                    "unexpected":true
                }
            ],
            "last_error":null,
            "selected_room":"!ops:example"
        }))
        .unwrap_err();
        assert!(
            redaction_error
                .to_string()
                .contains("unknown field `unexpected`")
        );
    }

    #[test]
    fn dump_model_round_trips_submitted_history_commands() {
        let model = DumpModel {
            status: "ready".to_owned(),
            lifecycle_state: LifecycleState::Ready,
            connection: ConnectionState::Syncing,
            reconnect_attempts: 0,
            rooms: Vec::new(),
            last_activity: None,
            e2ee_status: None,
            last_e2ee_recovery_setup_result: None,
            last_verification: None,
            submitted_membership_commands: Vec::new(),
            submitted_member_moderation_commands: Vec::new(),
            submitted_power_level_commands: Vec::new(),
            submitted_profile_commands: Vec::new(),
            submitted_user_commands: Vec::new(),
            submitted_direct_room_commands: Vec::new(),
            submitted_room_topic_commands: Vec::new(),
            submitted_reaction_commands: Vec::new(),
            submitted_edit_commands: Vec::new(),
            submitted_reply_commands: Vec::new(),
            submitted_message_commands: Vec::new(),
            submitted_file_commands: Vec::new(),
            submitted_notification_commands: Vec::new(),
            submitted_redaction_commands: Vec::new(),
            submitted_history_commands: vec![
                DumpSubmittedHistoryCommand {
                    room_id: "!room:example".to_owned(),
                    from: HistoryFrom::Latest,
                    limit: 50,
                },
                DumpSubmittedHistoryCommand {
                    room_id: "!room:example".to_owned(),
                    from: HistoryFrom::Token {
                        token: "token".to_owned(),
                    },
                    limit: 50,
                },
            ],
            submitted_verification_actions: Vec::new(),
            submitted_verification_requests: Vec::new(),
            identity_warnings: Vec::new(),
            submitted_identity_reset_commands: Vec::new(),
            submitted_room_key_transfers: Vec::new(),
            submitted_session_commands: Vec::new(),
            submitted_recovery_commands: Vec::new(),
            submitted_setup_recovery_commands: Vec::new(),
            submitted_room_list_requests: Vec::new(),
            opened_links: Vec::new(),
            shown_tips: Vec::new(),
            last_error: None,
            selected_room: Some("!room:example".to_owned()),
        };
        let json = serde_json::to_value(&model).unwrap();
        assert_eq!(
            json,
            json!({
                "status":"ready",
                "lifecycle_state":"ready",
                "connection":"syncing", "reconnect_attempts":0,
                "rooms":[],
                "last_activity":null,
                "e2ee_status":null,
                "last_verification":null,
                "submitted_history_commands":[
                    {
                        "room_id":"!room:example",
                        "from":{"type":"latest"},
                        "limit":50
                    },
                    {
                        "room_id":"!room:example",
                        "from":{"type":"token","token":"token"},
                        "limit":50
                    }
                ],
                "last_error":null,
                "selected_room":"!room:example"
            })
        );
        assert_eq!(serde_json::from_value::<DumpModel>(json).unwrap(), model);
    }

    #[test]
    fn dump_model_submitted_history_commands_default_to_empty_when_omitted() {
        let model = serde_json::from_value::<DumpModel>(json!({
            "status":"ready",
            "lifecycle_state":"ready",
            "connection":"syncing", "reconnect_attempts":0,
            "rooms":[],
            "last_activity":null,
            "e2ee_status":null,
            "last_verification":null,
            "last_error":null,
            "selected_room":null
        }))
        .unwrap();

        assert_eq!(
            model,
            DumpModel {
                status: "ready".to_owned(),
                lifecycle_state: LifecycleState::Ready,
                connection: ConnectionState::Syncing,
                reconnect_attempts: 0,
                rooms: Vec::new(),
                last_activity: None,
                e2ee_status: None,
                last_e2ee_recovery_setup_result: None,
                last_verification: None,
                submitted_membership_commands: Vec::new(),
                submitted_member_moderation_commands: Vec::new(),
                submitted_power_level_commands: Vec::new(),
                submitted_profile_commands: Vec::new(),
                submitted_user_commands: Vec::new(),
                submitted_direct_room_commands: Vec::new(),
                submitted_room_topic_commands: Vec::new(),
                submitted_reaction_commands: Vec::new(),
                submitted_edit_commands: Vec::new(),
                submitted_reply_commands: Vec::new(),
                submitted_message_commands: Vec::new(),
                submitted_file_commands: Vec::new(),
                submitted_notification_commands: Vec::new(),
                submitted_redaction_commands: Vec::new(),
                submitted_history_commands: Vec::new(),
                submitted_verification_actions: Vec::new(),
                submitted_verification_requests: Vec::new(),
                identity_warnings: Vec::new(),
                submitted_identity_reset_commands: Vec::new(),
                submitted_room_key_transfers: Vec::new(),
                submitted_session_commands: Vec::new(),
                submitted_recovery_commands: Vec::new(),
                submitted_setup_recovery_commands: Vec::new(),
                submitted_room_list_requests: Vec::new(),
                opened_links: Vec::new(),
                shown_tips: Vec::new(),
                last_error: None,
                selected_room: None,
            }
        );
    }

    #[test]
    fn dump_model_submitted_history_commands_reject_unknown_nested_fields() {
        let error = serde_json::from_value::<DumpModel>(json!({
            "status":"ready",
            "lifecycle_state":"ready",
            "connection":"syncing", "reconnect_attempts":0,
            "rooms":[],
            "last_activity":null,
            "e2ee_status":null,
            "last_verification":null,
            "submitted_membership_commands":[],
            "submitted_direct_room_commands":[],
            "submitted_room_topic_commands":[],
            "submitted_reaction_commands":[],
            "submitted_history_commands":[
                {
                    "room_id":"!room:example",
                    "from":{"type":"token","token":"token"},
                    "limit":50,
                    "unexpected":true
                }
            ],
            "last_error":null,
            "selected_room":null
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
                topic: Some("incident review".to_owned()),
                membership: RoomMembership::Join,
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
                notification_mode: Some(RoomNotificationMode::AllMessages),
                canonical_alias: None,
                alt_aliases: Vec::new(),
            }),
            super::DumpRoom {
                room_id: "!room:example".to_owned(),
                display_name: "Room".to_owned(),
                topic: Some("incident review".to_owned()),
                membership: RoomMembership::Join,
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
                "navigation":{"text":"mirx","children":[]},
                "mdi":{"active":{"kind":"status"},"views":[]},
                "status_bar":{"text":"ready"},
                "switchbar":{"items":[]},
                "active_find":null,
                "unexpected":true
            }))
            .is_err()
        );
        assert!(
            serde_json::from_value::<super::UiState>(json!({
                "navigation":{"text":"mirx","children":[]},
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
                "membership":"join",
                "encrypted":true,
                "is_space":false,
                "is_direct":false,
                "direct_user_ids":[],
                "tags":[],
                "space_children":[],
                "activity":null,
                "read_target_event_id":null,
                "read_state":null,
                "history_backfill":{"state":"idle","next":{"type":"latest"}},
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
        let plain_activity = RoomActivity {
            room_id: "!plain:example".to_owned(),
            activity_id: "event:$plain:example".to_owned(),
            timestamp_ms: 1_700_000_000_100,
            highlight: false,
            relation: None,
            kind: RoomActivityKind::Message(RoomMessageActivity {
                event_id: Some("$plain:example".to_owned()),
                transaction_id: None,
                sender: user_profile("@plain:example"),
                presentation: mirx_core::RoomMessagePresentationKind::Text,
                body: "plain".to_owned(),
                styles: Vec::new(),
                user_links: Vec::new(),
                send_state: RoomMessageSendState::Sent,
            }),
        };
        assert_eq!(
            super::dump_room_activity_from_activity(&plain_activity),
            DumpRoomActivity {
                room_id: "!plain:example".to_owned(),
                activity_id: "event:$plain:example".to_owned(),
                timestamp_ms: 1_700_000_000_100,
                highlight: false,
                relation: None,
                reactions: Vec::new(),
                kind: DumpRoomActivityKind::Message(DumpRoomMessageActivity {
                    event_id: Some("$plain:example".to_owned()),
                    transaction_id: None,
                    sender: "@plain:example".to_owned(),
                    presentation: RoomMessagePresentationKind::Text,
                    body: "plain".to_owned(),
                    send_state: DumpRoomMessageSendState::Sent,
                }),
            }
        );
        let activity = RoomActivity {
            room_id: "!room:example".to_owned(),
            activity_id: "event:$event:example".to_owned(),
            timestamp_ms: 1_700_000_000_000,
            highlight: false,
            relation: Some(RoomActivityRelation::Thread {
                thread_root_event_id: "$thread-root:example".to_owned(),
                in_reply_to_event_id: Some("$reply:example".to_owned()),
                is_falling_back: true,
            }),
            kind: RoomActivityKind::Message(RoomMessageActivity {
                event_id: Some("$event:example".to_owned()),
                transaction_id: Some("txn-1".to_owned()),
                sender: user_profile("@alice:example"),
                presentation: mirx_core::RoomMessagePresentationKind::Text,
                body: "hello".to_owned(),
                styles: Vec::new(),
                user_links: Vec::new(),
                send_state: RoomMessageSendState::Sent,
            }),
        };
        assert_eq!(
            super::dump_room_activity_from_activity(&activity),
            DumpRoomActivity {
                room_id: "!room:example".to_owned(),
                activity_id: "event:$event:example".to_owned(),
                timestamp_ms: 1_700_000_000_000,
                highlight: false,
                relation: Some(DumpRoomActivityRelation::Thread {
                    thread_root_event_id: "$thread-root:example".to_owned(),
                    in_reply_to_event_id: Some("$reply:example".to_owned()),
                    is_falling_back: true,
                }),
                reactions: Vec::new(),
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
        let reaction_dump = DumpRoomActivity {
            room_id: "!room:example".to_owned(),
            activity_id: "event:$event:example".to_owned(),
            timestamp_ms: 1_700_000_000_000,
            highlight: false,
            relation: None,
            reactions: vec![
                DumpRoomReactionSummary {
                    key: "fire".to_owned(),
                    count: 2,
                    senders: vec!["@alice:example".to_owned(), "@bob:example".to_owned()],
                    reaction_event_ids: vec![
                        "$reaction-2:example".to_owned(),
                        "$reaction-3:example".to_owned(),
                    ],
                },
                DumpRoomReactionSummary {
                    key: "+1".to_owned(),
                    count: 1,
                    senders: vec!["@carol:example".to_owned()],
                    reaction_event_ids: vec!["$reaction-1:example".to_owned()],
                },
            ],
            kind: DumpRoomActivityKind::Message(DumpRoomMessageActivity {
                event_id: Some("$event:example".to_owned()),
                transaction_id: Some("txn-1".to_owned()),
                sender: "@alice:example".to_owned(),
                presentation: RoomMessagePresentationKind::Text,
                body: "hello".to_owned(),
                send_state: DumpRoomMessageSendState::Sent,
            }),
        };
        let reaction_dump_json = json!({
            "room_id":"!room:example",
            "activity_id":"event:$event:example",
            "timestamp_ms":1700000000000u64,
            "highlight":false,
            "relation":null,
            "reactions":[
                {
                    "key":"fire",
                    "count":2u64,
                    "senders":["@alice:example","@bob:example"],
                    "reaction_event_ids":["$reaction-2:example","$reaction-3:example"]
                },
                {
                    "key":"+1",
                    "count":1u64,
                    "senders":["@carol:example"],
                    "reaction_event_ids":["$reaction-1:example"]
                }
            ],
            "kind":{
                "type":"message",
                "event_id":"$event:example",
                "transaction_id":"txn-1",
                "sender":"@alice:example",
                "presentation":"text",
                "body":"hello",
                "send_state":{"state":"sent"}
            }
        });
        assert_eq!(
            serde_json::to_value(&reaction_dump)
                .expect("serialize dump room activity with reactions"),
            reaction_dump_json.clone()
        );
        assert_eq!(
            serde_json::from_value::<DumpRoomActivity>(reaction_dump_json)
                .expect("deserialize dump room activity with reactions"),
            reaction_dump
        );
        assert!(
            serde_json::from_value::<DumpRoomReactionSummary>(json!({
                "key":"+1",
                "count":1u64,
                "senders":["@alice:example"],
                "reaction_event_ids":["$reaction-1:example"],
                "unexpected":true
            }))
            .is_err()
        );
        let unsupported_activity = RoomActivity {
            room_id: "!room:example".to_owned(),
            activity_id: "event:$custom1:example.org".to_owned(),
            timestamp_ms: 1_710_000_000_000,
            highlight: false,
            relation: None,
            kind: RoomActivityKind::UnsupportedMessage(UnsupportedRoomMessageActivity {
                event_id: "$custom1:example.org".to_owned(),
                sender: user_profile("@alice:example.org"),
                msgtype: "com.example.custom".to_owned(),
                body: "clip.mp4".to_owned(),
            }),
        };
        assert_eq!(
            super::dump_room_activity_from_activity(&unsupported_activity),
            DumpRoomActivity {
                room_id: "!room:example".to_owned(),
                activity_id: "event:$custom1:example.org".to_owned(),
                timestamp_ms: 1_710_000_000_000,
                highlight: false,
                relation: None,
                reactions: Vec::new(),
                kind: DumpRoomActivityKind::UnsupportedMessage(
                    DumpUnsupportedRoomMessageActivity {
                        event_id: "$custom1:example.org".to_owned(),
                        sender: "@alice:example.org".to_owned(),
                        msgtype: "com.example.custom".to_owned(),
                        body: "clip.mp4".to_owned(),
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
                "activity_id":"event:$custom1:example.org",
                "timestamp_ms":1710000000000u64,
                "highlight":false,
                "relation":null,
                "reactions":[],
                "kind":{
                    "type":"unsupported_message",
                    "event_id":"$custom1:example.org",
                    "sender":"@alice:example.org",
                    "msgtype":"com.example.custom",
                    "body":"clip.mp4"
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
            highlight: false,
            relation: None,
            kind: RoomActivityKind::Image(Box::new(RoomImageActivity {
                event_id: "$image1:example.org".to_owned(),
                sender: user_profile("@alice:example.org"),
                body: "diagram.png".to_owned(),
                filename: Some("diagram.png".to_owned()),
                source: sample_encrypted_room_media_source("mxc://example.org/diagram"),
                info: Some(RoomImageInfo {
                    mimetype: Some("image/png".to_owned()),
                    size: Some(12_345),
                    width: Some(640),
                    height: Some(480),
                    is_animated: Some(false),
                    thumbnail_source: Some(sample_encrypted_room_media_source(
                        "mxc://example.org/diagram-thumb",
                    )),
                }),
            })),
        };
        assert_eq!(
            super::dump_room_activity_from_activity(&image_activity),
            DumpRoomActivity {
                room_id: "!room:example".to_owned(),
                activity_id: "event:$image1:example.org".to_owned(),
                timestamp_ms: 1_710_000_010_000,
                highlight: false,
                relation: None,
                reactions: Vec::new(),
                kind: DumpRoomActivityKind::Image(Box::new(DumpRoomImageActivity {
                    event_id: "$image1:example.org".to_owned(),
                    sender: "@alice:example.org".to_owned(),
                    body: "diagram.png".to_owned(),
                    filename: Some("diagram.png".to_owned()),
                    source: sample_dump_encrypted_room_media_source("mxc://example.org/diagram"),
                    info: Some(DumpRoomImageInfo {
                        mimetype: Some("image/png".to_owned()),
                        size: Some(12_345),
                        width: Some(640),
                        height: Some(480),
                        is_animated: Some(false),
                        thumbnail_source: Some(sample_dump_encrypted_room_media_source(
                            "mxc://example.org/diagram-thumb",
                        )),
                    }),
                    media_state: None,
                })),
            }
        );
        assert_eq!(
            serde_json::to_value(super::dump_room_activity_from_activity(&image_activity))
                .expect("serialize image dump room activity"),
            json!({
                "room_id":"!room:example",
                "activity_id":"event:$image1:example.org",
                "timestamp_ms":1710000010000u64,
                "highlight":false,
                "relation":null,
                "reactions":[],
                "kind":{
                    "type":"image",
                    "event_id":"$image1:example.org",
                    "sender":"@alice:example.org",
                    "body":"diagram.png",
                    "filename":"diagram.png",
                    "source":{
                        "type":"encrypted",
                        "url":"mxc://example.org/diagram",
                        "key":{
                            "kty":"oct",
                            "key_ops":["encrypt","decrypt"],
                            "alg":"A256CTR",
                            "k":"base64url-key",
                            "ext":true
                        },
                        "iv":"base64-iv",
                        "hashes":{
                            "sha256":"base64-sha256"
                        },
                        "v":"v2"
                    },
                    "info":{
                        "mimetype":"image/png",
                        "size":12345u64,
                        "width":640u64,
                        "height":480u64,
                        "is_animated":false,
                        "thumbnail_source":{
                            "type":"encrypted",
                            "url":"mxc://example.org/diagram-thumb",
                            "key":{
                                "kty":"oct",
                                "key_ops":["encrypt","decrypt"],
                                "alg":"A256CTR",
                                "k":"base64url-key",
                                "ext":true
                            },
                            "iv":"base64-iv",
                            "hashes":{
                                "sha256":"base64-sha256"
                            },
                            "v":"v2"
                        }
                    },
                    "media_state":null
                }
            })
        );
        assert_eq!(
            super::dump_room_message_activity_line_from_activity(&image_activity),
            None
        );
        let file_activity = RoomActivity {
            room_id: "!room:example".to_owned(),
            activity_id: "event:$file1:example.org".to_owned(),
            timestamp_ms: 1_710_000_015_000,
            highlight: false,
            relation: None,
            kind: RoomActivityKind::File(Box::new(RoomFileActivity {
                event_id: "$file1:example.org".to_owned(),
                sender: user_profile("@alice:example.org"),
                body: "Quarterly report".to_owned(),
                filename: Some("report.pdf".to_owned()),
                source: sample_encrypted_room_media_source("mxc://example.org/report"),
                info: Some(RoomFileInfo {
                    mimetype: Some("application/pdf".to_owned()),
                    size: Some(45_678),
                }),
            })),
        };
        assert_eq!(
            super::dump_room_activity_from_activity(&file_activity),
            DumpRoomActivity {
                room_id: "!room:example".to_owned(),
                activity_id: "event:$file1:example.org".to_owned(),
                timestamp_ms: 1_710_000_015_000,
                highlight: false,
                relation: None,
                reactions: Vec::new(),
                kind: DumpRoomActivityKind::File(Box::new(DumpRoomFileActivity {
                    event_id: "$file1:example.org".to_owned(),
                    sender: "@alice:example.org".to_owned(),
                    body: "Quarterly report".to_owned(),
                    filename: Some("report.pdf".to_owned()),
                    source: sample_dump_encrypted_room_media_source("mxc://example.org/report"),
                    info: Some(DumpRoomFileInfo {
                        mimetype: Some("application/pdf".to_owned()),
                        size: Some(45_678),
                    }),
                })),
            }
        );
        assert_eq!(
            serde_json::to_value(super::dump_room_activity_from_activity(&file_activity))
                .expect("serialize file dump room activity"),
            json!({
                "room_id":"!room:example",
                "activity_id":"event:$file1:example.org",
                "timestamp_ms":1710000015000u64,
                "highlight":false,
                "relation":null,
                "reactions":[],
                "kind":{
                    "type":"file",
                    "event_id":"$file1:example.org",
                    "sender":"@alice:example.org",
                    "body":"Quarterly report",
                    "filename":"report.pdf",
                    "source":{
                        "type":"encrypted",
                        "url":"mxc://example.org/report",
                        "key":{
                            "kty":"oct",
                            "key_ops":["encrypt","decrypt"],
                            "alg":"A256CTR",
                            "k":"base64url-key",
                            "ext":true
                        },
                        "iv":"base64-iv",
                        "hashes":{
                            "sha256":"base64-sha256"
                        },
                        "v":"v2"
                    },
                    "info":{
                        "mimetype":"application/pdf",
                        "size":45678u64
                    }
                }
            })
        );
        let audio_activity = RoomActivity {
            room_id: "!room:example".to_owned(),
            activity_id: "event:$audio2:example.org".to_owned(),
            timestamp_ms: 1_710_000_016_000,
            highlight: false,
            relation: None,
            kind: RoomActivityKind::Audio(Box::new(RoomAudioActivity {
                event_id: "$audio2:example.org".to_owned(),
                sender: user_profile("@alice:example.org"),
                body: "Voice note".to_owned(),
                filename: Some("voice-note.ogg".to_owned()),
                source: sample_encrypted_room_media_source("mxc://example.org/audio"),
                info: Some(RoomAudioInfo {
                    mimetype: Some("audio/ogg".to_owned()),
                    size: Some(23_456),
                    duration_ms: Some(12_000),
                }),
            })),
        };
        assert_eq!(
            super::dump_room_activity_from_activity(&audio_activity),
            DumpRoomActivity {
                room_id: "!room:example".to_owned(),
                activity_id: "event:$audio2:example.org".to_owned(),
                timestamp_ms: 1_710_000_016_000,
                highlight: false,
                relation: None,
                reactions: Vec::new(),
                kind: DumpRoomActivityKind::Audio(Box::new(DumpRoomAudioActivity {
                    event_id: "$audio2:example.org".to_owned(),
                    sender: "@alice:example.org".to_owned(),
                    body: "Voice note".to_owned(),
                    filename: Some("voice-note.ogg".to_owned()),
                    source: sample_dump_encrypted_room_media_source("mxc://example.org/audio"),
                    info: Some(DumpRoomAudioInfo {
                        mimetype: Some("audio/ogg".to_owned()),
                        size: Some(23_456),
                        duration_ms: Some(12_000),
                    }),
                })),
            }
        );
        assert_eq!(
            serde_json::to_value(super::dump_room_activity_from_activity(&audio_activity))
                .expect("serialize audio dump room activity"),
            json!({
                "room_id":"!room:example",
                "activity_id":"event:$audio2:example.org",
                "timestamp_ms":1710000016000u64,
                "highlight":false,
                "relation":null,
                "reactions":[],
                "kind":{
                    "type":"audio",
                    "event_id":"$audio2:example.org",
                    "sender":"@alice:example.org",
                    "body":"Voice note",
                    "filename":"voice-note.ogg",
                    "source":{
                        "type":"encrypted",
                        "url":"mxc://example.org/audio",
                        "key":{
                            "kty":"oct",
                            "key_ops":["encrypt","decrypt"],
                            "alg":"A256CTR",
                            "k":"base64url-key",
                            "ext":true
                        },
                        "iv":"base64-iv",
                        "hashes":{
                            "sha256":"base64-sha256"
                        },
                        "v":"v2"
                    },
                    "info":{
                        "mimetype":"audio/ogg",
                        "size":23456u64,
                        "duration_ms":12000u64
                    }
                }
            })
        );
        let video_activity = RoomActivity {
            room_id: "!room:example".to_owned(),
            activity_id: "event:$video1:example.org".to_owned(),
            timestamp_ms: 1_710_000_018_000,
            highlight: false,
            relation: None,
            kind: RoomActivityKind::Video(Box::new(RoomVideoActivity {
                event_id: "$video1:example.org".to_owned(),
                sender: user_profile("@alice:example.org"),
                body: "clip.mp4".to_owned(),
                filename: Some("clip.mp4".to_owned()),
                source: RoomMediaSource::Plain {
                    url: "mxc://example.org/video".to_owned(),
                },
                info: Some(RoomVideoInfo {
                    mimetype: Some("video/mp4".to_owned()),
                    size: Some(34_567),
                    duration_ms: Some(12_000),
                    width: Some(1920),
                    height: Some(1080),
                    thumbnail_source: Some(RoomMediaSource::Plain {
                        url: "mxc://example.org/video-thumbnail".to_owned(),
                    }),
                }),
            })),
        };
        assert_eq!(
            super::dump_room_activity_from_activity(&video_activity),
            DumpRoomActivity {
                room_id: "!room:example".to_owned(),
                activity_id: "event:$video1:example.org".to_owned(),
                timestamp_ms: 1_710_000_018_000,
                highlight: false,
                relation: None,
                reactions: Vec::new(),
                kind: DumpRoomActivityKind::Video(Box::new(DumpRoomVideoActivity {
                    event_id: "$video1:example.org".to_owned(),
                    sender: "@alice:example.org".to_owned(),
                    body: "clip.mp4".to_owned(),
                    filename: Some("clip.mp4".to_owned()),
                    source: DumpRoomMediaSource::Plain {
                        url: "mxc://example.org/video".to_owned(),
                    },
                    info: Some(DumpRoomVideoInfo {
                        mimetype: Some("video/mp4".to_owned()),
                        size: Some(34_567),
                        duration_ms: Some(12_000),
                        width: Some(1920),
                        height: Some(1080),
                        thumbnail_source: Some(DumpRoomMediaSource::Plain {
                            url: "mxc://example.org/video-thumbnail".to_owned(),
                        }),
                    }),
                    media_state: None,
                })),
            }
        );
        assert_eq!(
            serde_json::to_value(super::dump_room_activity_from_activity(&video_activity))
                .expect("serialize video dump room activity"),
            json!({
                "room_id":"!room:example",
                "activity_id":"event:$video1:example.org",
                "timestamp_ms":1710000018000u64,
                "highlight":false,
                "relation":null,
                "reactions":[],
                "kind":{
                    "type":"video",
                    "event_id":"$video1:example.org",
                    "sender":"@alice:example.org",
                    "body":"clip.mp4",
                    "filename":"clip.mp4",
                    "source":{"type":"plain","url":"mxc://example.org/video"},
                    "info":{
                        "mimetype":"video/mp4",
                        "size":34567u64,
                        "duration_ms":12000u64,
                        "width":1920u64,
                        "height":1080u64,
                        "thumbnail_source":{"type":"plain","url":"mxc://example.org/video-thumbnail"}
                    },
                    "media_state":null
                }
            })
        );
        let redacted_activity = RoomActivity {
            room_id: "!room:example".to_owned(),
            activity_id: "event:$redacted:example.org".to_owned(),
            timestamp_ms: 1_710_000_020_000,
            highlight: false,
            relation: Some(RoomActivityRelation::Thread {
                thread_root_event_id: "$redact-thread-root".to_owned(),
                in_reply_to_event_id: Some("$redact-thread-root".to_owned()),
                is_falling_back: false,
            }),
            kind: RoomActivityKind::Redacted(RoomRedactedActivity {
                event_id: "$redacted:example.org".to_owned(),
            }),
        };
        assert_eq!(
            super::dump_room_activity_from_activity(&redacted_activity),
            DumpRoomActivity {
                room_id: "!room:example".to_owned(),
                activity_id: "event:$redacted:example.org".to_owned(),
                timestamp_ms: 1_710_000_020_000,
                highlight: false,
                relation: Some(DumpRoomActivityRelation::Thread {
                    thread_root_event_id: "$redact-thread-root".to_owned(),
                    in_reply_to_event_id: Some("$redact-thread-root".to_owned()),
                    is_falling_back: false,
                }),
                reactions: Vec::new(),
                kind: DumpRoomActivityKind::Redacted(DumpRoomRedactedActivity {
                    event_id: "$redacted:example.org".to_owned(),
                }),
            }
        );
        let redacted_json =
            serde_json::to_value(super::dump_room_activity_from_activity(&redacted_activity))
                .expect("serialize redacted dump room activity");
        assert_eq!(
            redacted_json,
            json!({
                "room_id":"!room:example",
                "activity_id":"event:$redacted:example.org",
                "timestamp_ms":1710000020000u64,
                "highlight":false,
                "relation":{
                    "type":"thread",
                    "thread_root_event_id":"$redact-thread-root",
                    "in_reply_to_event_id":"$redact-thread-root",
                    "is_falling_back":false
                },
                "reactions":[],
                "kind":{
                    "type":"redacted",
                    "event_id":"$redacted:example.org"
                }
            })
        );
        let redacted_json_text =
            serde_json::to_string(&redacted_json).expect("serialize redacted dump room activity");
        for leaked_field in [
            "redaction text payload should disappear",
            "redaction edited replacement should disappear",
            "redaction-image-body.png",
            "redaction-image-file.png",
            "mxc://example.org/redaction-image-source",
            "m.video",
            "redaction unsupported payload should disappear",
            "redaction topic should disappear",
            "state_key",
            "presentation",
            "body",
            "filename",
            "source",
            "media_state",
            "msgtype",
            "content",
        ] {
            assert!(
                !redacted_json_text.contains(leaked_field),
                "redacted automation dump unexpectedly leaked {leaked_field:?}: {redacted_json_text}",
            );
        }
        assert_eq!(
            super::dump_room_message_activity_line_from_activity(&redacted_activity),
            None
        );
        assert!(
            serde_json::from_value::<DumpRoomActivity>(json!({
                "room_id":"!room:example",
                "activity_id":"event:$event:example",
                "timestamp_ms":1700000000000u64,
                "highlight":false,
                "reactions":[],
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
            serde_json::from_value::<DumpRoomVideoActivity>(json!({
                "event_id":"$video1:example.org",
                "sender":"@alice:example.org",
                "body":"clip.mp4",
                "filename":"clip.mp4",
                "source":{"type":"plain","url":"mxc://example.org/video"},
                "info":null,
                "unexpected":true
            }))
            .is_err()
        );
        assert!(
            serde_json::from_value::<DumpRoomPreviewMediaState>(json!({
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
            serde_json::from_value::<DumpRoomVideoInfo>(json!({
                "mimetype":"video/mp4",
                "size":34567u64,
                "duration_ms":12000u64,
                "width":1920u64,
                "height":1080u64,
                "thumbnail_source":null,
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
                "highlight":false,
                "reactions":[],
                "kind":{
                    "type":"state_notice",
                    "event_id":"$state-1",
                    "sender":"@alice:example",
                    "state_key":"@alice:example",
                    "content":{
                        "type":"member",
                        "change":{"type":"joined"},
                        "target_display_name":"alice",
                        "reason":null
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
                "highlight":false,
                "reactions":[],
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
        assert!(
            serde_json::from_value::<DumpRoomActivity>(json!({
                "room_id":"!ops:example",
                "activity_id":"event:$redacted-1",
                "timestamp_ms":1700000000000u64,
                "highlight":false,
                "reactions":[],
                "kind":{
                    "type":"redacted",
                    "event_id":"$redacted-1",
                    "unexpected":true
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
                room_id: Some("!room:example".to_owned()),
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
                room_id: Some("!room:example".to_owned()),
                message: "failed".to_owned(),
            }),
            super::DumpError {
                operation: "mark_room_read".to_owned(),
                message: "failed".to_owned(),
            }
        );
        assert_eq!(
            super::operation_code(Operation::BackfillRoomHistory),
            "backfill_room_history"
        );
    }
}
