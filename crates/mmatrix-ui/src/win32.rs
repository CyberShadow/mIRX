#![allow(unsafe_op_in_unsafe_fn)]

use std::borrow::Cow;
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::ffi::{OsStr, c_void};
use std::fs;
use std::io::ErrorKind;
use std::mem::{size_of, zeroed};
use std::os::windows::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::ptr::{copy_nonoverlapping, null, null_mut};
use std::sync::atomic::{AtomicI32, Ordering};
use std::sync::{Arc, Mutex, mpsc::RecvTimeoutError};
use std::thread;
use std::time::Duration;

use mmatrix_core::{
    AcceptRoomInviteRequest, BackfillRoomHistoryRequest, Command, CommandSender, CoreConfig,
    CoreErrorEvent, CoreHandle, CreateDirectRoomRequest, E2eeBackupState, E2eeCrossSigningStatus,
    E2eeRecoveryState, E2eeStatus, EditMessageRequest, Event, FetchRoomImageMediaRequest,
    JoinRoomRequest, LeaveRoomRequest, LoginRequest, MarkRoomReadRequest, Operation,
    ROOM_HISTORY_BACKFILL_DEFAULT_LIMIT, RecoverE2eeRequest, RedactEventRequest,
    RejectRoomInviteRequest, RestoreSessionRequest, RoomActivity, RoomActivityKind,
    RoomActivityRelation, RoomActivityUpdate, RoomActivityUpdateKind, RoomActivityUpdateTarget,
    RoomAudioInfo, RoomFileInfo, RoomHistoryBackfill, RoomHistoryBackfillEvent, RoomImageInfo,
    RoomImageMedia, RoomImageMediaFormat, RoomImageMediaKind, RoomMediaSource, RoomMemberSummary,
    RoomMembers, RoomMembership, RoomMessageActivity, RoomMessagePresentationKind,
    RoomMessageSendState, RoomReadState, RoomRedactedActivity, RoomStateNoticeContent, RoomSummary,
    RoomTyping, SendMessageReplyTarget, SendMessageRequest, SendMessageThreadTarget,
    SendReactionRequest, SetRoomTopicRequest, StoredSession, VerificationEmoji, VerificationFlowId,
    VerificationRequest, VerificationState, VerificationStateEvent, start,
};
use serde::Serialize;
use serde_json::{Value, json};
use tokio::sync::mpsc::{UnboundedReceiver, unbounded_channel};
use windows_sys::Win32::Foundation::{
    CloseHandle, GetLastError, GlobalFree, HANDLE, HWND, INVALID_HANDLE_VALUE, LPARAM, LRESULT,
    RECT, SIZE, WPARAM,
};
use windows_sys::Win32::Graphics::Gdi::{
    BeginPaint, COLOR_WINDOW, CreateSolidBrush, DT_END_ELLIPSIS, DT_NOPREFIX, DT_SINGLELINE,
    DeleteObject, DrawTextW, EndPaint, FillRect, FrameRect, GetDC, GetTextExtentPoint32W, HBRUSH,
    InvalidateRect, PAINTSTRUCT, ReleaseDC, SelectObject, SetBkMode, SetTextColor, TRANSPARENT,
    TextOutW,
};
use windows_sys::Win32::Graphics::GdiPlus::{
    GdipCreateFromHDC, GdipDeleteGraphics, GdipDisposeImage, GdipDrawImageRectI,
    GdipGetImageHeight, GdipGetImageWidth, GdipLoadImageFromStream, GdiplusShutdown,
    GdiplusStartup, GdiplusStartupInput, GpGraphics, GpImage,
};
use windows_sys::Win32::System::Com::StructuredStorage::CreateStreamOnHGlobal;
use windows_sys::Win32::System::DataExchange::COPYDATASTRUCT;
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::System::Memory::{
    CreateFileMappingW, FILE_MAP_ALL_ACCESS, GMEM_MOVEABLE, GlobalAlloc, GlobalLock, GlobalUnlock,
    MEMORY_MAPPED_VIEW_ADDRESS, MapViewOfFile, PAGE_READWRITE, UnmapViewOfFile,
};
use windows_sys::Win32::System::Threading::GetCurrentProcessId;
use windows_sys::Win32::UI::Controls::{
    BTNS_CHECK, BTNS_SHOWTEXT, EM_GETSEL, EM_SCROLLCARET, EM_SETSEL, I_IMAGENONE, ICC_BAR_CLASSES,
    ICC_TREEVIEW_CLASSES, INITCOMMONCONTROLSEX, InitCommonControlsEx, NMHDR, SB_SETTEXTW,
    SBARS_SIZEGRIP, STATUSCLASSNAMEW, SetScrollInfo, TB_ADDBUTTONSW, TB_AUTOSIZE, TB_BUTTONCOUNT,
    TB_BUTTONSTRUCTSIZE, TB_DELETEBUTTON, TB_SETBUTTONSIZE, TB_SETEXTENDEDSTYLE, TB_SETSTATE,
    TBBUTTON, TBSTATE_CHECKED, TBSTATE_ENABLED, TBSTATE_MARKED, TBSTYLE_EX_MIXEDBUTTONS,
    TBSTYLE_FLAT, TOOLBARCLASSNAMEW, TVE_EXPAND, TVGN_CARET, TVGN_CHILD, TVGN_NEXT, TVGN_PARENT,
    TVGN_ROOT, TVI_LAST, TVI_ROOT, TVIF_STATE, TVIF_TEXT, TVINSERTSTRUCTW, TVINSERTSTRUCTW_0,
    TVIS_BOLD, TVITEMEXW, TVM_DELETEITEM, TVM_EXPAND, TVM_GETITEMW, TVM_GETNEXTITEM,
    TVM_INSERTITEMW, TVM_SELECTITEM, TVM_SETITEMW, TVN_SELCHANGEDW,
};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{EnableWindow, SetFocus};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, BS_DEFPUSHBUTTON, BS_PUSHBUTTON, CLIENTCREATESTRUCT, CREATESTRUCTW, CW_USEDEFAULT,
    CallWindowProcW, CreateMenu, CreatePopupMenu, CreateWindowExW, DefFrameProcW, DefMDIChildProcW,
    DefWindowProcW, DestroyWindow, DispatchMessageW, DrawMenuBar, ES_AUTOHSCROLL, ES_AUTOVSCROLL,
    ES_MULTILINE, ES_PASSWORD, ES_READONLY, ES_WANTRETURN, GWL_STYLE, GWLP_USERDATA, GWLP_WNDPROC,
    GetClientRect, GetMessageW, GetParent, GetScrollInfo, GetWindowLongPtrW, HMENU, HideCaret,
    IDC_ARROW, IsWindow, KillTimer, LB_ADDSTRING, LB_RESETCONTENT, LBS_NOINTEGRALHEIGHT,
    LoadCursorW, MB_ICONERROR, MB_OK, MDICREATESTRUCTW, MF_POPUP, MF_STRING, MSG, MessageBoxW,
    MoveWindow, PostMessageW, PostQuitMessage, RegisterClassExW, SB_BOTTOM, SB_LINEDOWN, SB_LINEUP,
    SB_PAGEDOWN, SB_PAGEUP, SB_THUMBPOSITION, SB_THUMBTRACK, SB_TOP, SB_VERT, SCROLLINFO, SIF_ALL,
    SIF_PAGE, SIF_POS, SIF_RANGE, SIF_TRACKPOS, SW_HIDE, SW_SHOW, SendMessageW,
    SetForegroundWindow, SetMenu, SetTimer, SetWindowLongPtrW, ShowWindow, TranslateMDISysAccel,
    TranslateMessage, WINDOW_EX_STYLE, WM_APP, WM_CHAR, WM_CLOSE, WM_COMMAND, WM_COPYDATA,
    WM_CREATE, WM_DESTROY, WM_GETFONT, WM_GETTEXT, WM_GETTEXTLENGTH, WM_KEYDOWN, WM_MDIACTIVATE,
    WM_MDICREATE, WM_MDIDESTROY, WM_MDIGETACTIVE, WM_MDIMAXIMIZE, WM_NCCREATE, WM_NCDESTROY,
    WM_NOTIFY, WM_PAINT, WM_PASTE, WM_SETTEXT, WM_SIZE, WM_TIMER, WM_VSCROLL, WNDCLASSEXW, WNDPROC,
    WS_BORDER, WS_CAPTION, WS_CHILD, WS_CLIPCHILDREN, WS_EX_CLIENTEDGE, WS_OVERLAPPED,
    WS_OVERLAPPEDWINDOW, WS_SYSMENU, WS_TABSTOP, WS_VISIBLE, WS_VSCROLL,
};

use crate::{
    AppCommand, FrameSize, HELP, RunOptions,
    automation::{
        AcceptedResponse, COMMAND_ID_DUMP_COMPOSE_TEXT, COMMAND_ID_DUMP_ROOM_ACTIVITY,
        COMMAND_ID_LOAD_SYNTHETIC_STATE, COMMAND_ID_PING, COMMAND_ID_WAIT_IDLE, ComposeKey,
        ComposeTextResponse, DumpMembershipCommandAction, DumpModel, DumpRoomActivity,
        DumpRoomActivityKind, DumpRoomActivityResponse, DumpRoomImageMediaState,
        DumpRoomMessageActivityLine, DumpRoomReactionSummary, DumpSubmittedDirectRoomCommand,
        DumpSubmittedEditCommand, DumpSubmittedHistoryCommand, DumpSubmittedMembershipCommand,
        DumpSubmittedReactionCommand, DumpSubmittedRedactionCommand, DumpSubmittedReplyCommand,
        DumpSubmittedReplyThreadTarget, DumpSubmittedRoomTopicCommand,
        DumpSubmittedVerificationAction, DumpVerificationActionKind, FailureEnvelope,
        FindActiveViewResponse, GetVersionResponse, LifecycleState, ListRoomsResponse,
        LoadEventsResponse, MdiState, MdiView, NavigationNode, NavigationPathSegment,
        ParsedCommand, PongResponse, ResponseEnvelope, RoomMemberView, SelectRoomResponse,
        SelectViewResponse, SelectedNavigationPathResponse, SendTextResponse, StatusBarState,
        SwitchbarItem, SwitchbarState, UiFindState, UiRoomHistoryBackfillState, UiRoomState,
        UiState, VerificationActionArgs, ViewIdentity, WaitIdleResponse, dump_error_from_core,
        dump_room_activity_from_activity, dump_room_from_summary,
        dump_room_message_send_state_from_core, dump_ui_room_read_state_from_core,
        dump_verification_from_state_event, failure_response, parse_command_request,
        success_response,
    },
};

const FRAME_CLASS_NAME: &str = "mmatrixWin32Frame";
const MDI_CHILD_CLASS_NAME: &str = "mmatrixWin32MdiChild";
const ROOM_CONTENT_CLASS_NAME: &str = "mmatrixWin32RoomContent";
const TIMELINE_CLASS_NAME: &str = "mmatrixWin32Timeline";
const LOGIN_DIALOG_CLASS_NAME: &str = "mmatrixWin32LoginDialog";
const RECOVER_E2EE_DIALOG_CLASS_NAME: &str = "mmatrixWin32RecoverE2eeDialog";
const FRAME_TITLE: &str = "mmatrix";
const DEFAULT_FRAME_SIZE: FrameSize = FrameSize {
    width: 1024,
    height: 768,
};
const TREE_CLASS_NAME: &str = "SysTreeView32";
const MDI_CLASS_NAME: &str = "MDICLIENT";
const EDIT_CLASS_NAME: &str = "EDIT";
const STATIC_CLASS_NAME: &str = "STATIC";
const BUTTON_CLASS_NAME: &str = "BUTTON";
const TREE_ID: u16 = 1001;
const MDI_ID: u16 = 1002;
const EDIT_ID: u16 = 1003;
const STATUS_BAR_ID: u16 = 1004;
const SWITCHBAR_ID: u16 = 1005;
const VIEW_CONTENT_ID: u16 = 2001;
const MENU_ID_LOGIN: usize = 40001;
const MENU_ID_RESTORE_SAVED_SESSION: usize = 40002;
const MENU_ID_START_SYNC: usize = 40003;
const MENU_ID_LOGOUT: usize = 40004;
const MENU_ID_RECOVER_E2EE: usize = 40005;
const SWITCHBAR_COMMAND_FIRST: usize = 41001;
const SWITCHBAR_COMMAND_LAST: usize = 41999;
const LOGIN_DIALOG_HOME_SERVER_ID: u16 = 5001;
const LOGIN_DIALOG_USERNAME_ID: u16 = 5002;
const LOGIN_DIALOG_PASSWORD_ID: u16 = 5003;
const LOGIN_DIALOG_DEVICE_ID: u16 = 5004;
const LOGIN_DIALOG_OK_ID: usize = 5005;
const LOGIN_DIALOG_CANCEL_ID: usize = 5006;
const RECOVER_E2EE_DIALOG_SECRET_ID: u16 = 5007;
const RECOVER_E2EE_DIALOG_OK_ID: usize = 5008;
const RECOVER_E2EE_DIALOG_CANCEL_ID: usize = 5009;
const WM_CORE_EVENT: u32 = WM_APP + 1;
const WM_SELF_TEST: u32 = WM_APP + 2;
const WM_TIMELINE_GET_TOP_LINE: u32 = WM_APP + 100;
const AUTO_CLOSE_TIMER_ID: usize = 1;
const AUTO_CLOSE_DELAY_MS: u32 = 250;
const AUTOMATION_RESPONSE_CAPACITY: usize = 4 * 1024 * 1024;
const TREE_WIDTH: i32 = 240;
const EDIT_HEIGHT: i32 = 28;
const SWITCHBAR_HEIGHT: i32 = 28;
const STATUS_BAR_HEIGHT: i32 = 22;
const ROOM_MEMBER_LIST_WIDTH: i32 = 180;
const ROOM_MEMBER_LIST_GAP: i32 = 4;
const ROOM_TIMELINE_MIN_WIDTH: i32 = 240;
const TIMELINE_ROW_HEIGHT: i32 = 20;
const TIMELINE_LEFT_PADDING: i32 = 8;
const TIMELINE_RIGHT_PADDING: i32 = 8;
const TIMELINE_COLUMN_GAP: i32 = 8;
const TIMELINE_TIMESTAMP_COLUMN_WIDTH: i32 = 56;
const TIMELINE_SENDER_COLUMN_WIDTH: i32 = 184;
const TIMELINE_VERTICAL_PADDING: i32 = 6;
const TIMELINE_IMAGE_THUMBNAIL_MAX_WIDTH: i32 = 160;
const TIMELINE_IMAGE_THUMBNAIL_MAX_HEIGHT: i32 = 120;
const TIMELINE_IMAGE_MEDIA_SLOT_LINES: usize = 7;
const TIMELINE_IMAGE_LEFT_INDENT: i32 = 24;
const TIMELINE_FIND_HIGHLIGHT_COLOR: u32 = 0x00f0f8d8;
const PROFILE_SUBDIR: &str = "mmatrix\\ui-shell";
const SESSION_FILE_NAME: &str = "session.json";
const SAVED_SESSION_VERSION: u32 = 1;
const PROTECTED_SESSION_MAGIC: &[u8] = b"mmatrix-ui-session:v1\0";
const SESSION_DPAPI_ENTROPY: &[u8] = b"mmatrix-ui/session/v1";
const STORE_PASSPHRASE_FILE_NAME: &str = "store-passphrase.dpapi";
const PROTECTED_STORE_PASSPHRASE_MAGIC: &[u8] = b"mmatrix-ui-store-secret:v1\0";
const STORE_PASSPHRASE_DPAPI_ENTROPY: &[u8] = b"mmatrix-ui/store-passphrase/v1";
const DEFAULT_DEVICE_DISPLAY_NAME: &str = "mmatrix-ui";
const VK_RETURN_KEY: WPARAM = 13;
const VK_UP_KEY: WPARAM = 38;
const VK_DOWN_KEY: WPARAM = 40;
const VK_DELETE_KEY: WPARAM = 46;

#[derive(Default)]
struct Controls {
    tree: HWND,
    mdi_client: HWND,
    edit: HWND,
    status_bar: HWND,
    switchbar: HWND,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
enum ViewId {
    Status,
    Room(String),
    Thread {
        room_id: String,
        thread_root_event_id: String,
    },
}

impl ViewId {
    fn room_id(&self) -> Option<&str> {
        match self {
            Self::Status => None,
            Self::Room(room_id) => Some(room_id.as_str()),
            Self::Thread { room_id, .. } => Some(room_id.as_str()),
        }
    }

    fn thread_key(&self) -> Option<(&str, &str)> {
        match self {
            Self::Thread {
                room_id,
                thread_root_event_id,
            } => Some((room_id.as_str(), thread_root_event_id.as_str())),
            _ => None,
        }
    }
}

struct MdiViewState {
    child: HWND,
    content: HWND,
}

struct MdiChildCreateState {
    view_id: ViewId,
}

struct RoomContentState {
    timeline: HWND,
    members: HWND,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
enum TimelineRowIdentity {
    ActivityPrimary {
        activity_id: String,
    },
    ActivityReactionSummary {
        activity_id: String,
    },
    Synthetic {
        label: TimelineSyntheticRowLabel,
        logical_text: String,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum TimelineSyntheticRowLabel {
    Header,
    Topic,
    Typing,
    Backfill,
    Context,
    Plain,
}

impl TimelineRowIdentity {
    fn activity_primary(activity_id: &str) -> Self {
        Self::ActivityPrimary {
            activity_id: activity_id.to_owned(),
        }
    }

    fn activity_reaction_summary(activity_id: &str) -> Self {
        Self::ActivityReactionSummary {
            activity_id: activity_id.to_owned(),
        }
    }

    fn synthetic(label: TimelineSyntheticRowLabel, logical_text: &str) -> Self {
        Self::Synthetic {
            label,
            logical_text: logical_text.to_owned(),
        }
    }

    fn default_for_row(kind: &TimelineRowKind, logical_text: &str) -> Self {
        let label = match kind {
            TimelineRowKind::Header => TimelineSyntheticRowLabel::Header,
            TimelineRowKind::Topic => TimelineSyntheticRowLabel::Topic,
            TimelineRowKind::Typing => TimelineSyntheticRowLabel::Typing,
            TimelineRowKind::Plain => TimelineSyntheticRowLabel::Plain,
            _ => TimelineSyntheticRowLabel::Context,
        };
        Self::synthetic(label, logical_text)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct TimelineRow {
    identity: TimelineRowIdentity,
    kind: TimelineRowKind,
    logical_text: String,
    visual: TimelineRowVisual,
    image_state: Option<TimelineImageState>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum TimelineRowVisual {
    FullWidth {
        text: String,
    },
    Log {
        timestamp: String,
        sender_prefix: String,
        body: String,
    },
}

impl TimelineRow {
    fn full_width(kind: TimelineRowKind, text: String) -> Self {
        Self {
            identity: TimelineRowIdentity::default_for_row(&kind, &text),
            kind,
            logical_text: text.clone(),
            visual: TimelineRowVisual::FullWidth { text },
            image_state: None,
        }
    }

    fn log(
        kind: TimelineRowKind,
        logical_text: String,
        timestamp: String,
        sender_prefix: String,
        body: String,
    ) -> Self {
        Self {
            identity: TimelineRowIdentity::default_for_row(&kind, &logical_text),
            kind,
            logical_text,
            visual: TimelineRowVisual::Log {
                timestamp,
                sender_prefix,
                body,
            },
            image_state: None,
        }
    }

    fn with_identity(mut self, identity: TimelineRowIdentity) -> Self {
        self.identity = identity;
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum TimelineRowKind {
    Header,
    Topic,
    Typing,
    ReactionSummary,
    Message {
        presentation: RoomMessagePresentationKind,
        send_state: TimelineMessageSendState,
    },
    Image,
    File,
    Audio,
    Unsupported,
    StateNotice,
    Plain,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TimelineMessageSendState {
    Pending,
    Sent,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum TimelineImageState {
    Pending,
    Loaded {
        key: String,
        width: u32,
        height: u32,
    },
    Failed {
        message: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct TimelinePaintLine {
    kind: TimelineRowKind,
    visual: TimelinePaintLineVisual,
    image_state: Option<TimelineImageState>,
    image_slot_index: Option<usize>,
    source_row_index: Option<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum TimelinePaintLineVisual {
    FullWidth(String),
    Log {
        timestamp: Option<String>,
        sender_prefix: Option<String>,
        body: String,
    },
    ImageSlot,
}

struct TimelineState {
    source_rows: Vec<TimelineRow>,
    rendered_text: String,
    lines: Vec<String>,
    paint_lines: Vec<TimelinePaintLine>,
    selected_find_source_row_index: Option<usize>,
    top_line: usize,
    line_height: i32,
    vertical_padding: i32,
}

impl Default for TimelineState {
    fn default() -> Self {
        Self {
            source_rows: Vec::new(),
            rendered_text: String::new(),
            lines: Vec::new(),
            paint_lines: Vec::new(),
            selected_find_source_row_index: None,
            top_line: 0,
            line_height: TIMELINE_ROW_HEIGHT,
            vertical_padding: TIMELINE_VERTICAL_PADDING,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct TimelineFindMatch {
    row_identity: TimelineRowIdentity,
    source_row_index: usize,
    logical_text: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct StatusFindMatch {
    line_index: usize,
    utf16_start: usize,
    utf16_end: usize,
    line_text: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum FindMatch {
    Timeline(TimelineFindMatch),
    Status(StatusFindMatch),
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ViewFindState {
    raw_query: String,
    folded_query: String,
    matches: Vec<FindMatch>,
    selected_match_index: Option<usize>,
    feedback: String,
}

#[cfg_attr(not(test), allow(dead_code))]
#[derive(Debug, Clone, PartialEq, Eq)]
struct ActiveViewFindOutcome {
    active_view: ViewId,
    raw_query: String,
    matched: bool,
    match_count: usize,
    current_match_index: Option<usize>,
    selected_text: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum RoomHistoryBackfillStatus {
    Idle,
    Loading {
        from: Option<String>,
    },
    Exhausted,
    Failed {
        from: Option<String>,
        message: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct RoomHistoryBackfillState {
    next_token: Option<String>,
    status: RoomHistoryBackfillStatus,
}

impl Default for RoomHistoryBackfillState {
    fn default() -> Self {
        Self {
            next_token: None,
            status: RoomHistoryBackfillStatus::Idle,
        }
    }
}

#[derive(Default)]
struct UiModel {
    status: String,
    lifecycle_state: LifecycleState,
    rooms: Vec<RoomSummary>,
    room_read_states: BTreeMap<String, RoomReadState>,
    selected_room: Option<String>,
    room_views: BTreeMap<String, RoomViewModel>,
    last_activity: Option<LastMessageActivity>,
    last_e2ee_status: Option<E2eeStatus>,
    last_verification: Option<VerificationStateEvent>,
    last_error: Option<CoreErrorEvent>,
}

#[derive(Default, Clone)]
struct RoomViewModel {
    topic: Option<String>,
    activities: Vec<RoomActivity>,
    reactions_by_target: BTreeMap<String, BTreeMap<String, RoomReactionEntry>>,
    threads: BTreeMap<String, ThreadViewModel>,
    image_media: BTreeMap<String, RoomImageMediaState>,
    typing_user_ids: Vec<String>,
    members: Vec<RoomMemberSummary>,
    history: RoomHistoryBackfillState,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct TimelineBackfillAnchor {
    room_id: String,
    source_row: TimelineRow,
    source_row_index: usize,
    source_row_count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ThreadViewModel {
    label: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum RoomImageMediaState {
    Pending {
        event_id: String,
        format: RoomImageMediaFormat,
    },
    Loaded {
        event_id: String,
        format: RoomImageMediaFormat,
        image_key: String,
        width: u32,
        height: u32,
    },
    Failed {
        event_id: String,
        format: RoomImageMediaFormat,
        message: String,
    },
}

struct GdiplusSession {
    token: usize,
}

impl GdiplusSession {
    fn startup() -> Result<Self, String> {
        let input = GdiplusStartupInput {
            GdiplusVersion: 1,
            DebugEventCallback: 0,
            SuppressBackgroundThread: 0,
            SuppressExternalCodecs: 0,
        };
        let mut token = 0usize;
        let status = unsafe { GdiplusStartup(&mut token, &input, null_mut()) };
        if status != 0 {
            return Err(format_gdiplus_status("GdiplusStartup", status));
        }
        Ok(Self { token })
    }
}

impl Drop for GdiplusSession {
    fn drop(&mut self) {
        unsafe { GdiplusShutdown(self.token) };
    }
}

struct GdiplusImage {
    raw: *mut GpImage,
    width: u32,
    height: u32,
}

impl Drop for GdiplusImage {
    fn drop(&mut self) {
        unsafe {
            GdipDisposeImage(self.raw);
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct LastMessageActivity {
    room_id: String,
    event_id: Option<String>,
    transaction_id: Option<String>,
    sender: String,
    timestamp_ms: u64,
    presentation: RoomMessagePresentationKind,
    body: String,
    send_state: RoomMessageSendState,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct MessageReplacementFields {
    presentation: RoomMessagePresentationKind,
    body: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct RoomReactionEntry {
    reaction_event_id: String,
    sender: String,
    timestamp_ms: u64,
    key: String,
}

impl RoomViewModel {
    fn clear_visible_entries(&mut self) {
        self.activities.retain(|activity| {
            matches!(
                &activity.kind,
                RoomActivityKind::Message(RoomMessageActivity {
                    send_state: RoomMessageSendState::Pending,
                    ..
                })
            )
        });
        self.reactions_by_target.clear();
        self.retain_image_media_for_visible_activities();
        self.rebuild_threads();
    }

    fn apply_activity(&mut self, activity: RoomActivity) {
        match &activity.kind {
            RoomActivityKind::Message(message) => {
                let transaction_id = message.transaction_id.clone();
                let event_id = message.event_id.clone();
                match &message.send_state {
                    RoomMessageSendState::Pending => {
                        let transaction_id = transaction_id
                            .as_deref()
                            .expect("pending room activity requires a transaction id");
                        self.upsert_message_activity(activity, Some(transaction_id), None);
                    }
                    RoomMessageSendState::Sent => {
                        let event_id = event_id
                            .as_deref()
                            .expect("sent room activity requires an event id");
                        self.upsert_message_activity(
                            activity,
                            transaction_id.as_deref(),
                            Some(event_id),
                        );
                    }
                    RoomMessageSendState::Failed { .. } => {
                        self.upsert_message_activity(
                            activity,
                            transaction_id.as_deref(),
                            event_id.as_deref(),
                        );
                    }
                }
            }
            RoomActivityKind::Image(_)
            | RoomActivityKind::File(_)
            | RoomActivityKind::Audio(_)
            | RoomActivityKind::UnsupportedMessage(_) => self.activities.push(activity),
            RoomActivityKind::StateNotice(notice) => {
                if let RoomStateNoticeContent::Topic { topic } = &notice.content {
                    self.topic = Some(topic.clone());
                }
                self.activities.push(activity);
            }
            RoomActivityKind::Redacted(redacted) => {
                self.remove_reactions_for_target(&redacted.event_id);
                self.activities.push(activity);
            }
        }
        self.rebuild_threads();
    }

    fn apply_history_backfill(&mut self, room_id: &str, backfill: RoomHistoryBackfill) {
        assert_eq!(
            backfill.room_id, room_id,
            "room history backfill must target the mutated room view"
        );

        let mut prepended_count = 0usize;
        let mut changed = false;
        for event in backfill.events {
            match event {
                RoomHistoryBackfillEvent::Activity(activity) => {
                    assert_eq!(
                        activity.room_id, room_id,
                        "room history activity must target the mutated room view"
                    );
                    if room_activity_event_id(&activity).is_some_and(|event_id| {
                        self.find_activity_index_by_event_id(event_id).is_some()
                    }) {
                        continue;
                    }

                    self.activities.insert(prepended_count, *activity);
                    prepended_count += 1;
                    changed = true;
                }
                RoomHistoryBackfillEvent::ActivityUpdate(update) => {
                    assert_eq!(
                        update.room_id, room_id,
                        "room history activity update must target the mutated room view"
                    );
                    if self.validate_update(&update).is_err() {
                        continue;
                    }

                    let applied = match (&update.target, &update.kind) {
                        (
                            RoomActivityUpdateTarget::Event { event_id },
                            RoomActivityUpdateKind::MessageReplaced {
                                presentation, body, ..
                            },
                        ) => self.replace_message(
                            event_id,
                            MessageReplacementFields {
                                presentation: *presentation,
                                body: body.clone(),
                            },
                        ),
                        (
                            RoomActivityUpdateTarget::Event { event_id },
                            RoomActivityUpdateKind::ActivityRedacted { .. },
                        ) => self.redact_activity(event_id),
                        (
                            RoomActivityUpdateTarget::Event { event_id },
                            RoomActivityUpdateKind::ReactionAdded {
                                reaction_event_id,
                                sender,
                                timestamp_ms,
                                key,
                            },
                        ) => self.add_reaction(
                            event_id,
                            reaction_event_id,
                            sender,
                            *timestamp_ms,
                            key,
                        ),
                        (
                            RoomActivityUpdateTarget::Event { event_id },
                            RoomActivityUpdateKind::ReactionRemoved {
                                reaction_event_id,
                                key,
                                ..
                            },
                        ) => self.remove_reaction(event_id, reaction_event_id, key),
                        _ => false,
                    };
                    changed |= applied;
                }
            }
        }

        if changed {
            self.rebuild_threads();
            self.retain_image_media_for_visible_activities();
        }
    }

    fn replace_typing(&mut self, typing: RoomTyping) {
        self.typing_user_ids = typing.user_ids;
    }

    fn replace_members(&mut self, members: RoomMembers) {
        self.members = members.members;
    }

    fn projected_image_state(
        &self,
        activity_id: &str,
        event_id: &str,
        format: &RoomImageMediaFormat,
    ) -> TimelineImageState {
        match self.image_media.get(activity_id) {
            Some(RoomImageMediaState::Pending {
                event_id: stored_event_id,
                format: stored_format,
            }) if stored_event_id == event_id && stored_format == format => {
                TimelineImageState::Pending
            }
            Some(RoomImageMediaState::Loaded {
                event_id: stored_event_id,
                format: stored_format,
                image_key,
                width,
                height,
            }) if stored_event_id == event_id && stored_format == format => {
                TimelineImageState::Loaded {
                    key: image_key.clone(),
                    width: *width,
                    height: *height,
                }
            }
            Some(RoomImageMediaState::Failed {
                event_id: stored_event_id,
                format: stored_format,
                message,
            }) if stored_event_id == event_id && stored_format == format => {
                TimelineImageState::Failed {
                    message: message.clone(),
                }
            }
            _ => TimelineImageState::Pending,
        }
    }

    fn should_request_image_media(
        &self,
        activity_id: &str,
        event_id: &str,
        format: &RoomImageMediaFormat,
    ) -> bool {
        match self.image_media.get(activity_id) {
            Some(RoomImageMediaState::Pending {
                event_id: stored_event_id,
                format: stored_format,
            })
            | Some(RoomImageMediaState::Loaded {
                event_id: stored_event_id,
                format: stored_format,
                ..
            })
            | Some(RoomImageMediaState::Failed {
                event_id: stored_event_id,
                format: stored_format,
                ..
            }) => stored_event_id != event_id || stored_format != format,
            None => true,
        }
    }

    fn mark_image_media_pending(
        &mut self,
        activity_id: String,
        event_id: String,
        format: RoomImageMediaFormat,
    ) -> Option<RoomImageMediaState> {
        self.image_media.insert(
            activity_id,
            RoomImageMediaState::Pending { event_id, format },
        )
    }

    fn remove_image_media(&mut self, activity_id: &str) -> Option<RoomImageMediaState> {
        self.image_media.remove(activity_id)
    }

    fn insert_image_media(
        &mut self,
        activity_id: String,
        image_media: RoomImageMediaState,
    ) -> Option<RoomImageMediaState> {
        self.image_media.insert(activity_id, image_media)
    }

    fn upsert_message_activity(
        &mut self,
        activity: RoomActivity,
        transaction_id: Option<&str>,
        event_id: Option<&str>,
    ) {
        if let Some(index) = self.find_message_activity_index(transaction_id, event_id) {
            self.activities[index] = activity;
        } else {
            self.activities.push(activity);
        }
    }

    fn mark_message_sent(&mut self, update: RoomActivityUpdate) {
        let RoomActivityUpdate { target, kind, .. } = update;
        let RoomActivityUpdateTarget::Transaction { transaction_id } = target else {
            panic!("message sent update requires a transaction target");
        };
        let RoomActivityUpdateKind::MessageSent { event_id } = kind else {
            panic!("message sent update requires a sent activity update");
        };
        let transaction_index = self
            .find_pending_message_activity_index(transaction_id.as_str())
            .expect("outgoing sent requires an existing queued timeline row");
        let duplicate_event_index = self
            .find_message_activity_index(None, Some(event_id.as_str()))
            .filter(|event_index| *event_index != transaction_index);
        let duplicate_event =
            duplicate_event_index.map(|event_index| self.activities[event_index].clone());
        let duplicate_event = duplicate_event.and_then(|activity| {
            let RoomActivityKind::Message(message) = activity.kind else {
                return None;
            };
            Some((
                activity.activity_id,
                activity.timestamp_ms,
                activity.relation,
                message.sender,
                message.presentation,
                message.body,
            ))
        });
        let new_activity_id = duplicate_event
            .as_ref()
            .map(|(activity_id, _, _, _, _, _)| activity_id.clone())
            .unwrap_or_else(|| format!("event:{event_id}"));
        let synced_timestamp_ms = duplicate_event
            .as_ref()
            .map(|(_, timestamp_ms, _, _, _, _)| *timestamp_ms);
        let synced_relation = duplicate_event
            .as_ref()
            .and_then(|(_, _, relation, _, _, _)| relation.clone());
        let synced_sender = duplicate_event
            .as_ref()
            .map(|(_, _, _, sender, _, _)| sender.clone());
        let synced_presentation = duplicate_event
            .as_ref()
            .map(|(_, _, _, _, presentation, _)| *presentation);
        let synced_body = duplicate_event
            .as_ref()
            .map(|(_, _, _, _, _, body)| body.clone());

        let activity = self
            .activities
            .get_mut(transaction_index)
            .expect("transaction row index must resolve");
        activity.activity_id = new_activity_id;
        if let Some(synced_timestamp_ms) = synced_timestamp_ms {
            activity.timestamp_ms = synced_timestamp_ms;
        }
        if let Some(synced_relation) = synced_relation {
            activity.relation = Some(synced_relation);
        }
        let RoomActivityKind::Message(message) = &mut activity.kind else {
            panic!("outgoing sent requires a message room activity");
        };

        message.event_id = Some(event_id.clone());
        if let Some(synced_sender) = synced_sender {
            message.sender = synced_sender;
        }
        if let Some(synced_presentation) = synced_presentation {
            message.presentation = synced_presentation;
        }
        if let Some(synced_body) = synced_body {
            message.body = synced_body;
        }
        message.send_state = RoomMessageSendState::Sent;

        if let Some(duplicate_event_index) = duplicate_event_index {
            self.activities.remove(duplicate_event_index);
        }
        self.rebuild_threads();
    }

    fn mark_message_failed(&mut self, update: RoomActivityUpdate) {
        let RoomActivityUpdate { target, kind, .. } = update;
        let RoomActivityUpdateTarget::Transaction { transaction_id } = target else {
            panic!("message failed update requires a transaction target");
        };
        let RoomActivityUpdateKind::MessageFailed {
            message,
            recoverable,
        } = kind
        else {
            panic!("message failed update requires a failed activity update");
        };
        let activity = self
            .find_pending_message_activity_mut(transaction_id.as_str())
            .expect("outgoing failed requires an existing queued timeline row");
        let RoomActivityKind::Message(room_message) = &mut activity.kind else {
            panic!("outgoing failed requires a message room activity");
        };

        room_message.send_state = RoomMessageSendState::Failed {
            message,
            recoverable,
        };
        self.rebuild_threads();
    }

    fn replace_message(
        &mut self,
        target_event_id: &str,
        replacement: MessageReplacementFields,
    ) -> bool {
        let Some(activity) = self.activities.iter_mut().find(|activity| {
            matches!(
                &activity.kind,
                RoomActivityKind::Message(RoomMessageActivity {
                    event_id: Some(event_id),
                    ..
                }) if event_id == target_event_id
            )
        }) else {
            return false;
        };
        let RoomActivityKind::Message(message) = &mut activity.kind else {
            panic!("message replacement requires a message room activity");
        };
        message.presentation = replacement.presentation;
        message.body = replacement.body;
        self.rebuild_threads();
        true
    }

    fn redact_activity(&mut self, target_event_id: &str) -> bool {
        let Some(index) = self.find_activity_index_by_event_id(target_event_id) else {
            return false;
        };
        let activity_id = self.activities[index].activity_id.clone();
        self.remove_reactions_for_target(target_event_id);
        self.activities[index].kind = RoomActivityKind::Redacted(RoomRedactedActivity {
            event_id: target_event_id.to_owned(),
        });
        self.remove_image_media(&activity_id);
        self.rebuild_threads();
        true
    }

    fn add_reaction(
        &mut self,
        target_event_id: &str,
        reaction_event_id: &str,
        sender: &str,
        timestamp_ms: u64,
        key: &str,
    ) -> bool {
        let Some(index) = self.find_activity_index_by_event_id(target_event_id) else {
            return false;
        };
        if matches!(self.activities[index].kind, RoomActivityKind::Redacted(_)) {
            return false;
        }
        self.reactions_by_target
            .entry(target_event_id.to_owned())
            .or_default()
            .insert(
                reaction_event_id.to_owned(),
                RoomReactionEntry {
                    reaction_event_id: reaction_event_id.to_owned(),
                    sender: sender.to_owned(),
                    timestamp_ms,
                    key: key.to_owned(),
                },
            );
        true
    }

    fn remove_reaction(
        &mut self,
        target_event_id: &str,
        reaction_event_id: &str,
        key: &str,
    ) -> bool {
        let Some(reactions) = self.reactions_by_target.get_mut(target_event_id) else {
            return false;
        };
        let matches_key = reactions
            .get(reaction_event_id)
            .is_some_and(|entry| entry.key == key);
        if !matches_key {
            return false;
        }
        reactions.remove(reaction_event_id);
        if reactions.is_empty() {
            self.reactions_by_target.remove(target_event_id);
        }
        true
    }

    fn remove_reactions_for_target(&mut self, target_event_id: &str) {
        self.reactions_by_target.remove(target_event_id);
    }

    fn reaction_summaries_for_event(&self, target_event_id: &str) -> Vec<DumpRoomReactionSummary> {
        let Some(reactions) = self.reactions_by_target.get(target_event_id) else {
            return Vec::new();
        };

        let mut grouped = BTreeMap::<String, Vec<&RoomReactionEntry>>::new();
        for entry in reactions.values() {
            grouped.entry(entry.key.clone()).or_default().push(entry);
        }

        grouped
            .into_iter()
            .map(|(key, mut entries)| {
                entries.sort_by(|left, right| left.reaction_event_id.cmp(&right.reaction_event_id));
                DumpRoomReactionSummary {
                    key,
                    count: entries.len() as u64,
                    senders: entries.iter().map(|entry| entry.sender.clone()).collect(),
                    reaction_event_ids: entries
                        .iter()
                        .map(|entry| entry.reaction_event_id.clone())
                        .collect(),
                }
            })
            .collect()
    }

    fn find_message_activity_index(
        &self,
        transaction_id: Option<&str>,
        event_id: Option<&str>,
    ) -> Option<usize> {
        if let Some(transaction_id) = transaction_id
            && let Some(index) = self.activities.iter().position(|activity| {
                matches!(
                    &activity.kind,
                    RoomActivityKind::Message(RoomMessageActivity {
                        transaction_id: Some(existing_transaction_id),
                        ..
                    }) if existing_transaction_id == transaction_id
                )
            })
        {
            return Some(index);
        }

        if let Some(event_id) = event_id
            && let Some(index) = self.activities.iter().position(|activity| {
                matches!(
                    &activity.kind,
                    RoomActivityKind::Message(RoomMessageActivity {
                        event_id: Some(existing_event_id),
                        ..
                    }) if existing_event_id == event_id
                )
            })
        {
            return Some(index);
        }

        None
    }

    fn find_activity_index_by_event_id(&self, event_id: &str) -> Option<usize> {
        self.activities
            .iter()
            .position(|activity| room_activity_event_id(activity) == Some(event_id))
    }

    fn find_pending_message_activity_index(&self, transaction_id: &str) -> Option<usize> {
        self.activities.iter().position(|activity| {
            matches!(
                &activity.kind,
                RoomActivityKind::Message(RoomMessageActivity {
                    transaction_id: Some(existing_transaction_id),
                    send_state: RoomMessageSendState::Pending,
                    ..
                }) if existing_transaction_id == transaction_id
            )
        })
    }

    fn find_pending_message_activity_mut(
        &mut self,
        transaction_id: &str,
    ) -> Option<&mut RoomActivity> {
        let index = self.find_pending_message_activity_index(transaction_id)?;
        self.activities.get_mut(index)
    }

    fn rebuild_threads(&mut self) {
        let root_labels = self
            .activities
            .iter()
            .filter_map(|activity| {
                Some((
                    room_activity_event_id(activity)?.to_owned(),
                    room_activity_thread_root_label(activity)?,
                ))
            })
            .collect::<BTreeMap<_, _>>();

        self.threads = self
            .activities
            .iter()
            .filter_map(|activity| room_activity_thread_root_event_id(activity))
            .map(|thread_root_event_id| {
                let label = root_labels
                    .get(thread_root_event_id)
                    .cloned()
                    .unwrap_or_else(|| thread_root_event_id.to_owned());
                (thread_root_event_id.to_owned(), ThreadViewModel { label })
            })
            .collect();
    }

    fn validate_update(&self, update: &RoomActivityUpdate) -> Result<(), String> {
        match (&update.target, &update.kind) {
            (
                RoomActivityUpdateTarget::Transaction { transaction_id },
                RoomActivityUpdateKind::MessageSent { .. },
            ) => self
                .find_pending_message_activity_index(transaction_id)
                .map(|_| ())
                .ok_or_else(|| "outgoing sent requires an existing queued timeline row".to_owned()),
            (
                RoomActivityUpdateTarget::Transaction { transaction_id },
                RoomActivityUpdateKind::MessageFailed { .. },
            ) => self
                .find_pending_message_activity_index(transaction_id)
                .map(|_| ())
                .ok_or_else(|| {
                    "outgoing failed requires an existing queued timeline row".to_owned()
                }),
            (
                RoomActivityUpdateTarget::Event { event_id },
                RoomActivityUpdateKind::MessageReplaced { .. },
            ) => self
                .find_message_activity_index(None, Some(event_id))
                .map(|_| ())
                .ok_or_else(|| "message edit requires an existing visible target event".to_owned()),
            (
                RoomActivityUpdateTarget::Event { event_id },
                RoomActivityUpdateKind::ActivityRedacted { .. },
            ) => self
                .find_activity_index_by_event_id(event_id)
                .map(|_| ())
                .ok_or_else(|| {
                    "activity redaction requires an existing visible target event".to_owned()
                }),
            (
                RoomActivityUpdateTarget::Event { event_id },
                RoomActivityUpdateKind::ReactionAdded { .. },
            ) => {
                let Some(index) = self.find_activity_index_by_event_id(event_id) else {
                    return Err("reaction add requires an existing visible target event".to_owned());
                };
                if matches!(self.activities[index].kind, RoomActivityKind::Redacted(_)) {
                    return Err("reaction add requires an existing visible target event".to_owned());
                }
                Ok(())
            }
            (
                RoomActivityUpdateTarget::Event { event_id },
                RoomActivityUpdateKind::ReactionRemoved {
                    reaction_event_id,
                    key,
                    ..
                },
            ) => self
                .reactions_by_target
                .get(event_id)
                .and_then(|reactions| reactions.get(reaction_event_id))
                .filter(|entry| entry.key == *key)
                .map(|_| ())
                .ok_or_else(|| {
                    "reaction removal requires a matching stored reaction event".to_owned()
                }),
            (
                RoomActivityUpdateTarget::Event { .. },
                RoomActivityUpdateKind::MessageSent { .. },
            ) => Err("message sent update requires a transaction target".to_owned()),
            (
                RoomActivityUpdateTarget::Event { .. },
                RoomActivityUpdateKind::MessageFailed { .. },
            ) => Err("message failed update requires a transaction target".to_owned()),
            (
                RoomActivityUpdateTarget::Transaction { .. },
                RoomActivityUpdateKind::MessageReplaced { .. },
            ) => Err("message edit requires an event target".to_owned()),
            (
                RoomActivityUpdateTarget::Transaction { .. },
                RoomActivityUpdateKind::ActivityRedacted { .. },
            ) => Err("activity redacted update requires an event target".to_owned()),
            (
                RoomActivityUpdateTarget::Transaction { .. },
                RoomActivityUpdateKind::ReactionAdded { .. },
            ) => Err("reaction add requires an event target".to_owned()),
            (
                RoomActivityUpdateTarget::Transaction { .. },
                RoomActivityUpdateKind::ReactionRemoved { .. },
            ) => Err("reaction removal requires an event target".to_owned()),
        }
    }

    fn retain_image_media_for_visible_activities(&mut self) {
        self.image_media.retain(|activity_id, _| {
            self.activities
                .iter()
                .any(|activity| activity.activity_id == *activity_id)
        });
    }
}

fn validate_synthetic_room_activity_events(
    room_views: &BTreeMap<String, RoomViewModel>,
    events: &[Event],
) -> Result<(), String> {
    let mut room_views = room_views.clone();

    for event in events {
        match event {
            Event::RoomActivity(activity) => room_views
                .entry(activity.room_id.clone())
                .or_default()
                .apply_activity(activity.as_ref().clone()),
            Event::RoomActivityUpdate(update) => {
                let room_view = room_views.entry(update.room_id.clone()).or_default();
                room_view.validate_update(update)?;
                match (&update.target, &update.kind) {
                    (
                        RoomActivityUpdateTarget::Transaction { .. },
                        RoomActivityUpdateKind::MessageSent { .. },
                    ) => room_view.mark_message_sent(update.clone()),
                    (
                        RoomActivityUpdateTarget::Transaction { .. },
                        RoomActivityUpdateKind::MessageFailed { .. },
                    ) => room_view.mark_message_failed(update.clone()),
                    (
                        RoomActivityUpdateTarget::Event { event_id },
                        RoomActivityUpdateKind::MessageReplaced {
                            presentation, body, ..
                        },
                    ) => {
                        let replaced = room_view.replace_message(
                            event_id,
                            MessageReplacementFields {
                                presentation: *presentation,
                                body: body.clone(),
                            },
                        );
                        assert!(replaced, "validated synthetic message edit must resolve");
                    }
                    (
                        RoomActivityUpdateTarget::Transaction { .. },
                        RoomActivityUpdateKind::MessageReplaced { .. },
                    ) => panic!("message edit requires an event target"),
                    (
                        RoomActivityUpdateTarget::Event { event_id },
                        RoomActivityUpdateKind::ActivityRedacted { .. },
                    ) => {
                        let redacted = room_view.redact_activity(event_id);
                        assert!(
                            redacted,
                            "validated synthetic activity redaction must resolve"
                        );
                    }
                    (
                        RoomActivityUpdateTarget::Event { event_id },
                        RoomActivityUpdateKind::ReactionAdded {
                            reaction_event_id,
                            sender,
                            timestamp_ms,
                            key,
                        },
                    ) => {
                        let added = room_view.add_reaction(
                            event_id,
                            reaction_event_id,
                            sender,
                            *timestamp_ms,
                            key,
                        );
                        assert!(added, "validated synthetic reaction add must resolve");
                    }
                    (
                        RoomActivityUpdateTarget::Event { event_id },
                        RoomActivityUpdateKind::ReactionRemoved {
                            reaction_event_id,
                            key,
                            ..
                        },
                    ) => {
                        let removed = room_view.remove_reaction(event_id, reaction_event_id, key);
                        assert!(removed, "validated synthetic reaction removal must resolve");
                    }
                    (
                        RoomActivityUpdateTarget::Event { .. },
                        RoomActivityUpdateKind::MessageSent { .. },
                    ) => panic!("message sent update requires a transaction target"),
                    (
                        RoomActivityUpdateTarget::Event { .. },
                        RoomActivityUpdateKind::MessageFailed { .. },
                    ) => panic!("message failed update requires a transaction target"),
                    (
                        RoomActivityUpdateTarget::Transaction { .. },
                        RoomActivityUpdateKind::ActivityRedacted { .. },
                    ) => panic!("activity redacted update requires an event target"),
                    (
                        RoomActivityUpdateTarget::Transaction { .. },
                        RoomActivityUpdateKind::ReactionAdded { .. },
                    ) => panic!("reaction add requires an event target"),
                    (
                        RoomActivityUpdateTarget::Transaction { .. },
                        RoomActivityUpdateKind::ReactionRemoved { .. },
                    ) => panic!("reaction removal requires an event target"),
                }
            }
            _ => {}
        }
    }

    Ok(())
}

fn initialize_gdiplus_state() -> (Option<GdiplusSession>, Option<String>) {
    match GdiplusSession::startup() {
        Ok(session) => (Some(session), None),
        Err(message) => (None, Some(message)),
    }
}

fn timeline_thumbnail_format() -> RoomImageMediaFormat {
    RoomImageMediaFormat::Thumbnail {
        width: TIMELINE_IMAGE_THUMBNAIL_MAX_WIDTH as u32,
        height: TIMELINE_IMAGE_THUMBNAIL_MAX_HEIGHT as u32,
    }
}

fn room_image_media_storage_key(
    room_id: &str,
    activity_id: &str,
    event_id: &str,
    format: &RoomImageMediaFormat,
) -> String {
    match format {
        RoomImageMediaFormat::Thumbnail { width, height } => {
            format!("{room_id}|{activity_id}|{event_id}|thumbnail:{width}x{height}")
        }
        RoomImageMediaFormat::File => {
            format!("{room_id}|{activity_id}|{event_id}|file")
        }
    }
}

fn format_gdiplus_status(operation: &str, status: i32) -> String {
    format!("{operation} failed with GDI+ status {status}")
}

unsafe fn release_com_interface(ptr: *mut c_void) {
    if ptr.is_null() {
        return;
    }
    let vtable = *(ptr as *mut *mut windows_sys::core::IUnknown_Vtbl);
    ((*vtable).Release)(ptr);
}

fn decode_image_bytes(bytes: &[u8]) -> Result<GdiplusImage, String> {
    unsafe {
        let hglobal = GlobalAlloc(GMEM_MOVEABLE, bytes.len());
        if hglobal.is_null() {
            return Err(last_os_error("GlobalAlloc"));
        }

        let mut stream = null_mut();
        let copy_result = (|| -> Result<GdiplusImage, String> {
            let locked = GlobalLock(hglobal);
            if locked.is_null() {
                return Err(last_os_error("GlobalLock"));
            }
            copy_nonoverlapping(bytes.as_ptr(), locked.cast::<u8>(), bytes.len());
            GlobalUnlock(hglobal);

            let hr = CreateStreamOnHGlobal(hglobal, 1, &mut stream);
            if hr < 0 {
                return Err(format!(
                    "CreateStreamOnHGlobal failed with HRESULT 0x{:08x}",
                    hr as u32
                ));
            }

            let decode_result = (|| -> Result<GdiplusImage, String> {
                let mut image = null_mut::<GpImage>();
                let status = GdipLoadImageFromStream(stream, &mut image);
                if status != 0 {
                    return Err(format_gdiplus_status("GdipLoadImageFromStream", status));
                }

                let mut width = 0u32;
                let status = GdipGetImageWidth(image, &mut width);
                if status != 0 {
                    GdipDisposeImage(image);
                    return Err(format_gdiplus_status("GdipGetImageWidth", status));
                }

                let mut height = 0u32;
                let status = GdipGetImageHeight(image, &mut height);
                if status != 0 {
                    GdipDisposeImage(image);
                    return Err(format_gdiplus_status("GdipGetImageHeight", status));
                }

                if width == 0 || height == 0 {
                    GdipDisposeImage(image);
                    return Err("decoded image reported zero width or height".to_owned());
                }

                Ok(GdiplusImage {
                    raw: image,
                    width,
                    height,
                })
            })();

            release_com_interface(stream);
            decode_result
        })();

        if copy_result.is_err() && stream.is_null() {
            GlobalFree(hglobal);
        }
        copy_result
    }
}

fn last_activity_from_room_activity(activity: &RoomActivity) -> Option<LastMessageActivity> {
    match &activity.kind {
        RoomActivityKind::Message(message) => Some(LastMessageActivity {
            room_id: activity.room_id.clone(),
            event_id: message.event_id.clone(),
            transaction_id: message.transaction_id.clone(),
            sender: message.sender.clone(),
            timestamp_ms: activity.timestamp_ms,
            presentation: message.presentation,
            body: message.body.clone(),
            send_state: message.send_state.clone(),
        }),
        RoomActivityKind::UnsupportedMessage(message) => Some(LastMessageActivity {
            room_id: activity.room_id.clone(),
            event_id: Some(message.event_id.clone()),
            transaction_id: None,
            sender: message.sender.clone(),
            timestamp_ms: activity.timestamp_ms,
            presentation: RoomMessagePresentationKind::Text,
            body: format!("[unsupported {}] {}", message.msgtype, message.body),
            send_state: RoomMessageSendState::Sent,
        }),
        RoomActivityKind::Image(message) => Some(LastMessageActivity {
            room_id: activity.room_id.clone(),
            event_id: Some(message.event_id.clone()),
            transaction_id: None,
            sender: message.sender.clone(),
            timestamp_ms: activity.timestamp_ms,
            presentation: RoomMessagePresentationKind::Text,
            body: format!(
                "[image] {}",
                message.filename.as_deref().unwrap_or(message.body.as_str())
            ),
            send_state: RoomMessageSendState::Sent,
        }),
        RoomActivityKind::File(message) => Some(LastMessageActivity {
            room_id: activity.room_id.clone(),
            event_id: Some(message.event_id.clone()),
            transaction_id: None,
            sender: message.sender.clone(),
            timestamp_ms: activity.timestamp_ms,
            presentation: RoomMessagePresentationKind::Text,
            body: format!(
                "[file] {}",
                message.filename.as_deref().unwrap_or(message.body.as_str())
            ),
            send_state: RoomMessageSendState::Sent,
        }),
        RoomActivityKind::Audio(message) => Some(LastMessageActivity {
            room_id: activity.room_id.clone(),
            event_id: Some(message.event_id.clone()),
            transaction_id: None,
            sender: message.sender.clone(),
            timestamp_ms: activity.timestamp_ms,
            presentation: RoomMessagePresentationKind::Text,
            body: format!(
                "[audio] {}",
                message.filename.as_deref().unwrap_or(message.body.as_str())
            ),
            send_state: RoomMessageSendState::Sent,
        }),
        RoomActivityKind::StateNotice(_) => None,
        RoomActivityKind::Redacted(_) => None,
    }
}

fn room_activity_marks_inactive_room(activity: &RoomActivity) -> bool {
    match &activity.kind {
        RoomActivityKind::Message(RoomMessageActivity { send_state, .. }) => {
            !matches!(send_state, &RoomMessageSendState::Pending)
        }
        RoomActivityKind::Image(_) => true,
        RoomActivityKind::File(_) => true,
        RoomActivityKind::Audio(_) => true,
        RoomActivityKind::UnsupportedMessage(_) => true,
        RoomActivityKind::StateNotice(_) => true,
        RoomActivityKind::Redacted(_) => true,
    }
}

fn room_activity_event_id(activity: &RoomActivity) -> Option<&str> {
    match &activity.kind {
        RoomActivityKind::Message(RoomMessageActivity {
            event_id: Some(event_id),
            ..
        }) => Some(event_id.as_str()),
        RoomActivityKind::Image(message) => Some(message.event_id.as_str()),
        RoomActivityKind::File(message) => Some(message.event_id.as_str()),
        RoomActivityKind::Audio(message) => Some(message.event_id.as_str()),
        RoomActivityKind::UnsupportedMessage(message) => Some(message.event_id.as_str()),
        RoomActivityKind::StateNotice(notice) => Some(notice.event_id.as_str()),
        RoomActivityKind::Redacted(activity) => Some(activity.event_id.as_str()),
        _ => None,
    }
}

fn room_activity_thread_root_event_id(activity: &RoomActivity) -> Option<&str> {
    match &activity.relation {
        Some(RoomActivityRelation::Thread {
            thread_root_event_id,
            ..
        }) => Some(thread_root_event_id.as_str()),
        _ => None,
    }
}

fn room_activity_thread_root_label(activity: &RoomActivity) -> Option<String> {
    match &activity.kind {
        RoomActivityKind::Message(message) => Some(message.body.clone()),
        RoomActivityKind::Image(message) => Some(
            message
                .filename
                .as_deref()
                .unwrap_or(message.body.as_str())
                .to_owned(),
        ),
        RoomActivityKind::File(message) => Some(
            message
                .filename
                .as_deref()
                .unwrap_or(message.body.as_str())
                .to_owned(),
        ),
        RoomActivityKind::Audio(message) => Some(
            message
                .filename
                .as_deref()
                .unwrap_or(message.body.as_str())
                .to_owned(),
        ),
        RoomActivityKind::UnsupportedMessage(message) => Some(message.body.clone()),
        RoomActivityKind::StateNotice(_) => None,
        RoomActivityKind::Redacted(_) => Some("Message redacted".to_owned()),
    }
}

fn room_activity_belongs_to_thread(activity: &RoomActivity, thread_root_event_id: &str) -> bool {
    room_activity_event_id(activity) == Some(thread_root_event_id)
        || room_activity_thread_root_event_id(activity) == Some(thread_root_event_id)
}

fn room_activity_view_id(room_id: &str, relation: Option<&RoomActivityRelation>) -> ViewId {
    match relation {
        Some(RoomActivityRelation::Thread {
            thread_root_event_id,
            ..
        }) => ViewId::Thread {
            room_id: room_id.to_owned(),
            thread_root_event_id: thread_root_event_id.clone(),
        },
        _ => ViewId::Room(room_id.to_owned()),
    }
}

fn room_activity_target_view_id(activity: &RoomActivity) -> ViewId {
    room_activity_view_id(&activity.room_id, activity.relation.as_ref())
}

fn room_activity_update_marks_inactive_room(update: &RoomActivityUpdate) -> bool {
    matches!(
        (&update.target, &update.kind),
        (
            RoomActivityUpdateTarget::Transaction { .. },
            RoomActivityUpdateKind::MessageFailed { .. }
        )
    )
}

fn failed_room_activity_update_view_id(state: &WindowState, update: &RoomActivityUpdate) -> ViewId {
    let (
        RoomActivityUpdateTarget::Transaction { transaction_id },
        RoomActivityUpdateKind::MessageFailed { .. },
    ) = (&update.target, &update.kind)
    else {
        panic!("failed room activity marker target requires a failed activity update");
    };
    let room_view = state
        .model
        .room_views
        .get(&update.room_id)
        .expect("outgoing failed requires an existing queued timeline row");
    let pending_index = room_view
        .find_pending_message_activity_index(transaction_id)
        .expect("outgoing failed requires an existing queued timeline row");
    let pending_activity = room_view
        .activities
        .get(pending_index)
        .expect("pending room activity index must resolve");
    room_activity_target_view_id(pending_activity)
}

fn read_target_for_room_activity(activity: &RoomActivity) -> Option<&str> {
    match &activity.kind {
        RoomActivityKind::Message(RoomMessageActivity { send_state, .. })
            if !matches!(send_state, RoomMessageSendState::Pending) =>
        {
            room_activity_event_id(activity)
        }
        RoomActivityKind::Image(_)
        | RoomActivityKind::File(_)
        | RoomActivityKind::Audio(_)
        | RoomActivityKind::UnsupportedMessage(_)
        | RoomActivityKind::StateNotice(_)
        | RoomActivityKind::Redacted(_) => room_activity_event_id(activity),
        _ => None,
    }
}

fn room_image_media_loaded_key(image_media: &RoomImageMediaState) -> Option<&str> {
    match image_media {
        RoomImageMediaState::Loaded { image_key, .. } => Some(image_key.as_str()),
        _ => None,
    }
}

fn prune_decoded_images(state: &mut WindowState) {
    let referenced_keys = state
        .model
        .room_views
        .values()
        .flat_map(|room_view| room_view.image_media.values())
        .filter_map(room_image_media_loaded_key)
        .map(str::to_owned)
        .collect::<BTreeSet<_>>();
    state
        .decoded_images
        .retain(|image_key, _| referenced_keys.contains(image_key));
}

fn room_image_activity_matches(
    room_view: &RoomViewModel,
    activity_id: &str,
    event_id: &str,
) -> bool {
    room_view.activities.iter().any(|activity| {
        activity.activity_id == activity_id
            && matches!(
                &activity.kind,
                RoomActivityKind::Image(message) if message.event_id == event_id
            )
    })
}

fn decoded_image_from_loaded_media(
    state: &mut WindowState,
    room_id: &str,
    activity_id: &str,
    event_id: &str,
    format: &RoomImageMediaFormat,
    bytes: &[u8],
) -> Result<RoomImageMediaState, String> {
    let Some(_session) = state.gdiplus_session.as_ref() else {
        return Err(state
            .gdiplus_startup_error
            .clone()
            .unwrap_or_else(|| "GDI+ startup did not complete".to_owned()));
    };
    let image = decode_image_bytes(bytes)?;
    let key = room_image_media_storage_key(room_id, activity_id, event_id, format);
    let width = image.width;
    let height = image.height;
    state.decoded_images.insert(key.clone(), image);
    Ok(RoomImageMediaState::Loaded {
        event_id: event_id.to_owned(),
        format: format.clone(),
        image_key: key,
        width,
        height,
    })
}

fn apply_room_image_media_event(state: &mut WindowState, media: RoomImageMedia) {
    let RoomImageMedia {
        room_id,
        activity_id,
        event_id,
        kind,
    } = media;
    let format = match &kind {
        RoomImageMediaKind::Loaded { format, .. } | RoomImageMediaKind::Failed { format, .. } => {
            format
        }
    };
    if format != &timeline_thumbnail_format() {
        return;
    }
    let Some(room_view) = state.model.room_views.get(&room_id) else {
        return;
    };
    if !room_image_activity_matches(room_view, &activity_id, &event_id) {
        return;
    }

    let previous = {
        state
            .model
            .room_views
            .get_mut(&room_id)
            .expect("room image media updates require an existing room view")
            .remove_image_media(&activity_id)
    };
    if let Some(image_key) = previous
        .as_ref()
        .and_then(room_image_media_loaded_key)
        .map(str::to_owned)
    {
        state.decoded_images.remove(&image_key);
    }

    let next_state = match kind {
        RoomImageMediaKind::Failed { format, message } => RoomImageMediaState::Failed {
            event_id,
            format,
            message,
        },
        RoomImageMediaKind::Loaded { format, bytes } => {
            match decoded_image_from_loaded_media(
                state,
                &room_id,
                &activity_id,
                &event_id,
                &format,
                &bytes,
            ) {
                Ok(image_media) => image_media,
                Err(message) => RoomImageMediaState::Failed {
                    event_id,
                    format,
                    message,
                },
            }
        }
    };

    state.model.status = format!("image media update from {room_id}");
    state
        .model
        .room_views
        .get_mut(&room_id)
        .expect("room image media updates require an existing room view")
        .insert_image_media(activity_id, next_state);
}

fn apply_room_activity_event(state: &mut WindowState, activity: RoomActivity) {
    let clears_last_activity = matches!(activity.kind, RoomActivityKind::Redacted(_));
    let target_view_id = room_activity_target_view_id(&activity);
    if room_activity_marks_inactive_room(&activity)
        && state.active_view != target_view_id
        && sendable_room_by_id(&state.model.rooms, &activity.room_id).is_some()
    {
        state.room_activity.insert(target_view_id.clone());
        if !state.synthetic_load_in_progress
            && let Some(event_id) = read_target_for_room_activity(&activity)
        {
            state
                .room_activity_read_targets
                .insert(target_view_id, event_id.to_owned());
        }
    }
    state.model.status = format!("timeline update from {}", activity.room_id);
    let room_id = activity.room_id.clone();
    let last_activity = last_activity_from_room_activity(&activity);
    state
        .model
        .room_views
        .entry(room_id)
        .or_default()
        .apply_activity(activity);
    if let Some(last_activity) = last_activity {
        state.model.last_activity = Some(last_activity);
    } else if clears_last_activity {
        state.model.last_activity = None;
    }
}

fn apply_room_activity_update_event(state: &mut WindowState, update: RoomActivityUpdate) {
    let target_view_id = room_activity_update_marks_inactive_room(&update)
        .then(|| failed_room_activity_update_view_id(state, &update));
    if let Some(target_view_id) = target_view_id
        && state.active_view != target_view_id
        && sendable_room_by_id(&state.model.rooms, &update.room_id).is_some()
    {
        state.room_activity.insert(target_view_id);
    }
    state.model.status = format!("timeline update from {}", update.room_id);
    let room_id = update.room_id.clone();
    let refresh_last_activity = state
        .model
        .last_activity
        .as_ref()
        .is_some_and(|last_activity| match (&update.target, &update.kind) {
            (
                RoomActivityUpdateTarget::Transaction { transaction_id },
                RoomActivityUpdateKind::MessageSent { event_id },
            ) => {
                last_activity.room_id == room_id
                    && (last_activity.transaction_id.as_deref() == Some(transaction_id.as_str())
                        || last_activity.event_id.as_deref() == Some(event_id.as_str()))
            }
            (
                RoomActivityUpdateTarget::Transaction { transaction_id },
                RoomActivityUpdateKind::MessageFailed { .. },
            ) => {
                last_activity.room_id == room_id
                    && last_activity.transaction_id.as_deref() == Some(transaction_id.as_str())
            }
            (
                RoomActivityUpdateTarget::Event { event_id },
                RoomActivityUpdateKind::MessageReplaced { .. },
            ) => {
                last_activity.room_id == room_id
                    && last_activity.event_id.as_deref() == Some(event_id.as_str())
            }
            (
                RoomActivityUpdateTarget::Event { event_id },
                RoomActivityUpdateKind::ActivityRedacted { .. },
            ) => {
                last_activity.room_id == room_id
                    && last_activity.event_id.as_deref() == Some(event_id.as_str())
            }
            (
                RoomActivityUpdateTarget::Event { event_id },
                RoomActivityUpdateKind::ReactionAdded { .. },
            )
            | (
                RoomActivityUpdateTarget::Event { event_id },
                RoomActivityUpdateKind::ReactionRemoved { .. },
            ) => {
                last_activity.room_id == room_id
                    && last_activity.event_id.as_deref() == Some(event_id.as_str())
            }
            _ => false,
        });
    match (&update.target, &update.kind) {
        (
            RoomActivityUpdateTarget::Transaction { .. },
            RoomActivityUpdateKind::MessageSent { .. },
        ) => state
            .model
            .room_views
            .entry(room_id.clone())
            .or_default()
            .mark_message_sent(update.clone()),
        (
            RoomActivityUpdateTarget::Transaction { .. },
            RoomActivityUpdateKind::MessageFailed { .. },
        ) => state
            .model
            .room_views
            .entry(room_id.clone())
            .or_default()
            .mark_message_failed(update.clone()),
        (
            RoomActivityUpdateTarget::Event { event_id },
            RoomActivityUpdateKind::MessageReplaced {
                presentation, body, ..
            },
        ) => {
            let _ = state
                .model
                .room_views
                .entry(room_id.clone())
                .or_default()
                .replace_message(
                    event_id,
                    MessageReplacementFields {
                        presentation: *presentation,
                        body: body.clone(),
                    },
                );
        }
        (
            RoomActivityUpdateTarget::Event { event_id },
            RoomActivityUpdateKind::ActivityRedacted { .. },
        ) => {
            let redacted = state
                .model
                .room_views
                .entry(room_id.clone())
                .or_default()
                .redact_activity(event_id);
            if redacted {
                prune_decoded_images(state);
            }
        }
        (
            RoomActivityUpdateTarget::Event { event_id },
            RoomActivityUpdateKind::ReactionAdded {
                reaction_event_id,
                sender,
                timestamp_ms,
                key,
            },
        ) => {
            let _ = state
                .model
                .room_views
                .entry(room_id.clone())
                .or_default()
                .add_reaction(event_id, reaction_event_id, sender, *timestamp_ms, key);
        }
        (
            RoomActivityUpdateTarget::Event { event_id },
            RoomActivityUpdateKind::ReactionRemoved {
                reaction_event_id,
                key,
                ..
            },
        ) => {
            let _ = state
                .model
                .room_views
                .entry(room_id.clone())
                .or_default()
                .remove_reaction(event_id, reaction_event_id, key);
        }
        (
            RoomActivityUpdateTarget::Transaction { .. },
            RoomActivityUpdateKind::MessageReplaced { .. },
        ) => panic!("message edit requires an event target"),
        (
            RoomActivityUpdateTarget::Transaction { .. },
            RoomActivityUpdateKind::ActivityRedacted { .. },
        ) => panic!("activity redacted update requires an event target"),
        (
            RoomActivityUpdateTarget::Transaction { .. },
            RoomActivityUpdateKind::ReactionAdded { .. },
        ) => panic!("reaction add requires an event target"),
        (
            RoomActivityUpdateTarget::Transaction { .. },
            RoomActivityUpdateKind::ReactionRemoved { .. },
        ) => panic!("reaction removal requires an event target"),
        (RoomActivityUpdateTarget::Event { .. }, RoomActivityUpdateKind::MessageSent { .. }) => {
            panic!("message sent update requires a transaction target")
        }
        (RoomActivityUpdateTarget::Event { .. }, RoomActivityUpdateKind::MessageFailed { .. }) => {
            panic!("message failed update requires a transaction target")
        }
    }
    if refresh_last_activity
        && let Some(updated_activity) =
            state
                .model
                .room_views
                .get(&room_id)
                .and_then(|room_view| match &update.target {
                    RoomActivityUpdateTarget::Transaction { transaction_id } => room_view
                        .find_message_activity_index(Some(transaction_id.as_str()), None)
                        .and_then(|index| room_view.activities.get(index)),
                    RoomActivityUpdateTarget::Event { event_id } => room_view
                        .find_activity_index_by_event_id(event_id.as_str())
                        .and_then(|index| room_view.activities.get(index)),
                })
    {
        state.model.last_activity = last_activity_from_room_activity(updated_activity);
    }
}

fn apply_room_history_backfill_event(state: &mut WindowState, backfill: RoomHistoryBackfill) {
    let room_id = backfill.room_id.clone();
    state.model.status = format!("timeline update from {room_id}");
    let end = backfill.end.clone();
    let room_view = state.model.room_views.entry(room_id.clone()).or_default();
    room_view.apply_history_backfill(&room_id, backfill);
    room_view.history.next_token = end.clone();
    room_view.history.status = match end {
        Some(_) => RoomHistoryBackfillStatus::Idle,
        None => RoomHistoryBackfillStatus::Exhausted,
    };
    prune_decoded_images(state);
}

fn apply_backfill_room_history_error(state: &mut WindowState, message: String) {
    let Some(room_id) = loading_room_history_room_id(&state.model.room_views) else {
        return;
    };
    let room_view = state
        .model
        .room_views
        .get_mut(&room_id)
        .expect("loading room history state must resolve to an existing room view");
    let RoomHistoryBackfillStatus::Loading { from } = &room_view.history.status else {
        panic!("loading room history room must stay loading until backfill completion");
    };
    let from = from.clone();
    room_view.history.status = RoomHistoryBackfillStatus::Failed {
        from: from.clone(),
        message,
    };
}

fn apply_room_read_state_event(state: &mut WindowState, read_state: RoomReadState) {
    let room_id = read_state.room_id.clone();
    if sendable_room_by_id(&state.model.rooms, &room_id).is_none() {
        return;
    }
    state
        .model
        .room_read_states
        .insert(room_id.clone(), read_state.clone());

    let latest_active_event_id = read_state.latest_active_event_id;
    let fully_read_event_id = read_state.fully_read_event_id;
    let marked_unread = read_state.marked_unread;
    let num_unread = read_state.num_unread;
    let num_notifications = read_state.num_notifications;
    let num_mentions = read_state.num_mentions;

    let has_unread_state =
        marked_unread || num_unread > 0 || num_notifications > 0 || num_mentions > 0;
    let clear_read_target = !has_unread_state
        || state
            .room_activity_read_targets
            .get(&ViewId::Room(room_id.clone()))
            .is_some_and(|target| {
                Some(target.as_str()) == latest_active_event_id.as_deref()
                    || Some(target.as_str()) == fully_read_event_id.as_deref()
            });
    if clear_read_target {
        state
            .room_activity_read_targets
            .remove(&ViewId::Room(room_id.clone()));
    }

    let room_view_id = ViewId::Room(room_id.clone());
    let room_is_active = state.active_view == room_view_id;
    if has_unread_state && !room_is_active {
        state.room_activity.insert(room_view_id);
    } else {
        state.room_activity.remove(&room_view_id);
    }
}

fn apply_room_typing_event(state: &mut WindowState, typing: RoomTyping) {
    state
        .model
        .room_views
        .entry(typing.room_id.clone())
        .or_default()
        .replace_typing(typing);
}

fn apply_room_members_event(state: &mut WindowState, members: RoomMembers) {
    state
        .model
        .room_views
        .entry(members.room_id.clone())
        .or_default()
        .replace_members(members);
}

fn room_member_view_rows(members: &[RoomMemberSummary]) -> Vec<RoomMemberView> {
    let mut display_name_counts = BTreeMap::new();
    for member in members {
        if let Some(display_name) = member
            .display_name
            .as_deref()
            .filter(|name| !name.is_empty())
        {
            *display_name_counts
                .entry(display_name.to_owned())
                .or_insert(0usize) += 1;
        }
    }

    members
        .iter()
        .map(|member| {
            let label = match member.display_name.as_deref() {
                Some(display_name) if !display_name.is_empty() => {
                    if display_name_counts
                        .get(display_name)
                        .copied()
                        .unwrap_or_default()
                        > 1
                    {
                        format!("{display_name} ({})", member.user_id)
                    } else {
                        display_name.to_owned()
                    }
                }
                _ => member.user_id.clone(),
            };
            RoomMemberView {
                user_id: member.user_id.clone(),
                display_name: member.display_name.clone(),
                label,
            }
        })
        .collect()
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct SavedSession {
    version: u32,
    homeserver_url: String,
    session: StoredSession,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct LoginForm {
    homeserver_url: String,
    username: String,
    password: String,
    device_display_name: String,
}

struct LoginDialogState {
    result: Arc<Mutex<Option<LoginForm>>>,
    homeserver_label: HWND,
    homeserver_url_edit: HWND,
    username_label: HWND,
    username_edit: HWND,
    password_label: HWND,
    password_edit: HWND,
    device_display_name_label: HWND,
    device_display_name_edit: HWND,
    ok_button: HWND,
    cancel_button: HWND,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct RecoverE2eeForm {
    recovery_secret: String,
}

struct RecoverE2eeDialogState {
    result: Arc<Mutex<Option<RecoverE2eeForm>>>,
    recovery_secret_label: HWND,
    recovery_secret_edit: HWND,
    ok_button: HWND,
    cancel_button: HWND,
}

struct AutomationState {
    response_name: String,
    response_mapping: HANDLE,
    response_view: *mut u8,
    response_capacity: usize,
}

#[derive(Default, Debug, Clone, PartialEq, Eq)]
struct ComposeHistory {
    entries: Vec<String>,
    navigation: Option<ComposeHistoryNavigation>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ComposeHistoryNavigation {
    index: usize,
    draft: String,
}

impl ComposeHistory {
    fn record_submission(&mut self, body: String) {
        self.entries.push(body);
        self.navigation = None;
    }

    fn reset_navigation(&mut self) {
        self.navigation = None;
    }

    fn previous(&mut self, current_text: String) -> Option<&str> {
        if self.entries.is_empty() {
            return None;
        }

        match self.navigation.as_mut() {
            Some(navigation) => {
                if navigation.index > 0 {
                    navigation.index -= 1;
                }
            }
            None => {
                self.navigation = Some(ComposeHistoryNavigation {
                    index: self.entries.len() - 1,
                    draft: current_text,
                });
            }
        }

        let index = self
            .navigation
            .as_ref()
            .expect("compose history navigation must exist after previous")
            .index;
        Some(&self.entries[index])
    }

    fn next(&mut self) -> Option<String> {
        let navigation = self.navigation.as_mut()?;
        if navigation.index + 1 < self.entries.len() {
            navigation.index += 1;
            return Some(self.entries[navigation.index].clone());
        }

        let draft = navigation.draft.clone();
        self.navigation = None;
        Some(draft)
    }
}

enum ComposeHistoryDirection {
    Previous,
    Next,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum ComposeSubmission<'a> {
    Message {
        presentation: RoomMessagePresentationKind,
        body: Cow<'a, str>,
    },
    LocalCommand(LocalSlashCommand<'a>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum LocalSlashCommand<'a> {
    Status,
    Find {
        query: String,
    },
    Window {
        target: &'a str,
    },
    Join {
        target: String,
    },
    Query {
        user_id: String,
    },
    Accept {
        target: Option<String>,
    },
    Reject {
        target: Option<String>,
    },
    Part {
        target: Option<String>,
    },
    React {
        target_event_id: String,
        key: String,
    },
    Reply {
        target_event_id: String,
        body: String,
    },
    Edit {
        target_event_id: String,
        body: String,
    },
    Redact {
        event_id: String,
    },
    Unreact {
        reaction_event_id: String,
    },
    Topic {
        topic: String,
    },
    Clear,
    E2ee(E2eeSlashCommand),
    Verify {
        action: VerificationAction,
    },
    Invalid {
        feedback: String,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum E2eeSlashCommand {
    ShowStatus,
    Refresh,
    RecoverDialog,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WindowTargetError {
    Unknown,
    Ambiguous,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum QueryTargetResolution {
    None,
    Single(String),
    Ambiguous,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum VerificationAction {
    Accept,
    Cancel,
    Confirm,
    Reject,
}

struct WindowState {
    controls: Controls,
    event_queue: Arc<Mutex<VecDeque<Event>>>,
    command_sender: Option<CommandSender>,
    inert_command_receiver: Option<UnboundedReceiver<Command>>,
    automation: Option<AutomationState>,
    render_state_mode: bool,
    exit_code: Arc<AtomicI32>,
    event_generation: u64,
    last_idle_probe_generation: Option<u64>,
    synthetic_load_in_progress: bool,
    pending_login_homeserver_url: Option<String>,
    current_session_homeserver_url: Option<String>,
    tree_items: BTreeMap<isize, ViewId>,
    primary_tree_items: BTreeMap<ViewId, isize>,
    views: BTreeMap<ViewId, MdiViewState>,
    hwnd_views: BTreeMap<isize, ViewId>,
    active_view: ViewId,
    find_states: BTreeMap<ViewId, ViewFindState>,
    switchbar_views: Vec<ViewId>,
    room_activity: BTreeSet<ViewId>,
    room_activity_read_targets: BTreeMap<ViewId, String>,
    compose_history: ComposeHistory,
    edit_window_proc: WNDPROC,
    model: UiModel,
    decoded_images: BTreeMap<String, GdiplusImage>,
    gdiplus_session: Option<GdiplusSession>,
    gdiplus_startup_error: Option<String>,
    recorded_commands: Vec<Command>,
}

impl Drop for WindowState {
    fn drop(&mut self) {
        self.decoded_images.clear();
    }
}

struct CoreBridge {
    command_sender: CommandSender,
    join_handle: thread::JoinHandle<()>,
}

pub(crate) fn run(command: AppCommand) -> ExitCode {
    match run_inner(command) {
        Ok(code) => code,
        Err(message) => {
            eprintln!("{message}");
            ExitCode::from(1)
        }
    }
}

fn run_inner(command: AppCommand) -> Result<ExitCode, String> {
    if matches!(command, AppCommand::Help) {
        println!("{HELP}");
        return Ok(ExitCode::SUCCESS);
    }

    let render_state_events = match &command {
        AppCommand::Run(options) => load_render_state(options)?,
        AppCommand::SelfTest | AppCommand::AutoClose | AppCommand::Help => None,
    };
    let render_state_mode = match &command {
        AppCommand::Run(options) => options.render_state.is_some(),
        AppCommand::SelfTest | AppCommand::AutoClose | AppCommand::Help => false,
    };
    let automation_enabled = automation_enabled_for_command(&command);

    let instance = unsafe { GetModuleHandleW(null()) };
    if instance.is_null() {
        return Err(last_os_error("GetModuleHandleW"));
    }

    init_common_controls()?;
    register_frame_class(instance)?;
    register_mdi_child_class(instance)?;
    register_room_content_class(instance)?;
    register_timeline_class(instance)?;
    register_login_dialog_class(instance)?;
    register_recover_e2ee_dialog_class(instance)?;

    let exit_code = Arc::new(AtomicI32::new(0));
    let event_queue = Arc::new(Mutex::new(VecDeque::new()));
    let (gdiplus_session, gdiplus_startup_error) = initialize_gdiplus_state();

    let state = Box::new(WindowState {
        controls: Controls::default(),
        event_queue: Arc::clone(&event_queue),
        command_sender: None,
        inert_command_receiver: None,
        automation: None,
        render_state_mode,
        exit_code: Arc::clone(&exit_code),
        event_generation: 0,
        last_idle_probe_generation: None,
        synthetic_load_in_progress: false,
        pending_login_homeserver_url: None,
        current_session_homeserver_url: None,
        tree_items: BTreeMap::new(),
        primary_tree_items: BTreeMap::new(),
        views: BTreeMap::new(),
        hwnd_views: BTreeMap::new(),
        active_view: ViewId::Status,
        find_states: BTreeMap::new(),
        switchbar_views: Vec::new(),
        room_activity: BTreeSet::new(),
        room_activity_read_targets: BTreeMap::new(),
        compose_history: ComposeHistory::default(),
        edit_window_proc: None,
        model: initial_model(),
        decoded_images: BTreeMap::new(),
        gdiplus_session,
        gdiplus_startup_error,
        recorded_commands: Vec::new(),
    });

    let hwnd = create_main_window(instance, state, frame_size_for_command(&command))?;

    let startup = (|| -> Result<Option<CoreBridge>, String> {
        if automation_enabled {
            unsafe { enable_automation(hwnd)? };
        }

        let bridge = if should_start_core_bridge(&command) {
            let bridge = start_core_bridge(hwnd, Arc::clone(&event_queue))?;
            unsafe {
                state_mut(hwnd).command_sender = Some(bridge.command_sender.clone());
                if session_path()?.is_file() {
                    submit_restore_saved_session(state_mut(hwnd))?;
                    refresh_status_bar(hwnd)?;
                }
            }
            Some(bridge)
        } else {
            unsafe {
                if should_install_inert_command_sender(&command) {
                    install_inert_command_sender(state_mut(hwnd));
                }
            }
            None
        };

        unsafe {
            ShowWindow(hwnd, SW_SHOW);
            if automation_enabled {
                SetForegroundWindow(hwnd);
            }
            if let Some(events) = render_state_events {
                apply_event_load(hwnd, events)?;
            }
        }

        Ok(bridge)
    })();

    let bridge = match startup {
        Ok(bridge) => bridge,
        Err(message) => {
            unsafe {
                DestroyWindow(hwnd);
            }
            return Err(message);
        }
    };

    unsafe {
        match command {
            AppCommand::SelfTest => {
                PostMessageW(hwnd, WM_SELF_TEST, 0, 0);
            }
            AppCommand::AutoClose => {
                SetTimer(hwnd, AUTO_CLOSE_TIMER_ID, AUTO_CLOSE_DELAY_MS, None);
            }
            AppCommand::Run(_) | AppCommand::Help => {}
        }
    }

    message_loop();

    if let Some(bridge) = bridge {
        let _ = bridge.join_handle.join();
    }

    Ok(ExitCode::from(
        exit_code.load(Ordering::Relaxed).clamp(0, 255) as u8,
    ))
}

fn start_core_bridge(hwnd: HWND, queue: Arc<Mutex<VecDeque<Event>>>) -> Result<CoreBridge, String> {
    let profile_dir = profile_dir()?;
    let store_passphrase = load_or_create_store_passphrase(&profile_dir)?;
    let handle = start(CoreConfig {
        profile_dir,
        store_passphrase,
    })
    .map_err(|err| format!("failed to start mmatrix-core: {err}"))?;

    let command_sender = handle.command_sender().clone();
    let hwnd_value = hwnd as isize;
    let join_handle = thread::Builder::new()
        .name("mmatrix-ui-core-bridge".to_owned())
        .spawn(move || bridge_core_events(hwnd_value as HWND, queue, handle))
        .map_err(|err| format!("failed to spawn core bridge thread: {err}"))?;

    Ok(CoreBridge {
        command_sender,
        join_handle,
    })
}

fn automation_enabled_for_command(command: &AppCommand) -> bool {
    match command {
        AppCommand::Run(options) => options.automation || options.render_state.is_some(),
        AppCommand::SelfTest => true,
        AppCommand::AutoClose | AppCommand::Help => false,
    }
}

fn should_start_core_bridge(command: &AppCommand) -> bool {
    match command {
        AppCommand::Run(options) => options.render_state.is_none(),
        AppCommand::AutoClose => true,
        AppCommand::SelfTest | AppCommand::Help => false,
    }
}

fn should_install_inert_command_sender(command: &AppCommand) -> bool {
    match command {
        AppCommand::Run(options) => options.automation && options.render_state.is_some(),
        AppCommand::SelfTest | AppCommand::AutoClose | AppCommand::Help => false,
    }
}

fn install_inert_command_sender(state: &mut WindowState) {
    let (sender, receiver) = unbounded_channel();
    state.command_sender = Some(CommandSender::new(sender));
    state.inert_command_receiver = Some(receiver);
}

fn frame_size_for_command(command: &AppCommand) -> FrameSize {
    match command {
        AppCommand::Run(options) => options.frame_size.unwrap_or(DEFAULT_FRAME_SIZE),
        AppCommand::SelfTest | AppCommand::AutoClose | AppCommand::Help => DEFAULT_FRAME_SIZE,
    }
}

fn initial_model() -> UiModel {
    UiModel {
        status: "offline".to_owned(),
        lifecycle_state: LifecycleState::Offline,
        ..UiModel::default()
    }
}

fn load_render_state(options: &RunOptions) -> Result<Option<Vec<Event>>, String> {
    let Some(path) = &options.render_state else {
        return Ok(None);
    };

    let file_bytes = fs::read(path)
        .map_err(|err| format!("failed to read render-state file {}: {err}", path.display()))?;
    serde_json::from_slice(&file_bytes)
        .map(Some)
        .map_err(|err| {
            format!(
                "failed to parse render-state file {} as JSON event list: {err}",
                path.display()
            )
        })
}

fn bridge_core_events(hwnd: HWND, queue: Arc<Mutex<VecDeque<Event>>>, handle: CoreHandle) {
    loop {
        match handle.events().recv_timeout(Duration::from_millis(100)) {
            Ok(event) => {
                let should_stop = matches!(event, Event::Stopped);
                queue
                    .lock()
                    .expect("core event queue poisoned")
                    .push_back(event);
                unsafe {
                    PostMessageW(hwnd, WM_CORE_EVENT, 0, 0);
                }
                if should_stop {
                    break;
                }
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => break,
        }
    }

    let _ = handle.join();
}

fn profile_dir() -> Result<PathBuf, String> {
    let local_app_data =
        std::env::var_os("LOCALAPPDATA").ok_or_else(|| "LOCALAPPDATA is not set".to_owned())?;
    Ok(PathBuf::from(local_app_data).join(PROFILE_SUBDIR))
}

fn session_path() -> Result<PathBuf, String> {
    Ok(profile_dir()?.join(SESSION_FILE_NAME))
}

fn load_saved_session() -> Result<SavedSession, String> {
    let path = session_path()?;
    let bytes = fs::read(&path)
        .map_err(|err| format!("failed to read saved session {}: {err}", path.display()))?;
    let protected = bytes.strip_prefix(PROTECTED_SESSION_MAGIC).ok_or_else(|| {
        format!(
            "saved session {} is not in protected format",
            path.display()
        )
    })?;
    let payload = crate::dpapi::unprotect(protected, SESSION_DPAPI_ENTROPY).map_err(|err| {
        format!(
            "failed to unprotect saved session {}: {err}",
            path.display()
        )
    })?;
    let saved = serde_json::from_slice::<SavedSession>(&payload)
        .map_err(|err| format!("failed to parse saved session {}: {err}", path.display()))?;
    if saved.version != SAVED_SESSION_VERSION {
        return Err(format!(
            "unsupported saved session version {} in {}",
            saved.version,
            path.display()
        ));
    }
    Ok(saved)
}

fn save_session(homeserver_url: &str, session: &StoredSession) -> Result<(), String> {
    let path = session_path()?;
    let Some(profile_dir) = path.parent() else {
        return Err(format!(
            "saved session path {} has no parent directory",
            path.display()
        ));
    };
    fs::create_dir_all(profile_dir).map_err(|err| {
        format!(
            "failed to create saved session directory {}: {err}",
            profile_dir.display()
        )
    })?;

    let temp_path = path.with_extension("json.tmp");
    let payload = SavedSession {
        version: SAVED_SESSION_VERSION,
        homeserver_url: homeserver_url.to_owned(),
        session: session.clone(),
    };
    let serialized = serde_json::to_vec(&payload).map_err(|err| {
        format!(
            "failed to serialize saved session {}: {err}",
            path.display()
        )
    })?;
    let protected =
        crate::dpapi::protect(&serialized, "mmatrix saved session", SESSION_DPAPI_ENTROPY)
            .map_err(|err| format!("failed to protect saved session {}: {err}", path.display()))?;
    let mut bytes = Vec::with_capacity(PROTECTED_SESSION_MAGIC.len() + protected.len());
    bytes.extend_from_slice(PROTECTED_SESSION_MAGIC);
    bytes.extend_from_slice(&protected);
    fs::write(&temp_path, bytes).map_err(|err| {
        format!(
            "failed to write saved session {}: {err}",
            temp_path.display()
        )
    })?;
    match fs::remove_file(&path) {
        Ok(()) => {}
        Err(err) if err.kind() == ErrorKind::NotFound => {}
        Err(err) => {
            return Err(format!(
                "failed to replace saved session {}: {err}",
                path.display()
            ));
        }
    }
    fs::rename(&temp_path, &path)
        .map_err(|err| format!("failed to install saved session {}: {err}", path.display()))?;
    Ok(())
}

fn delete_saved_session() -> Result<(), String> {
    let path = session_path()?;
    match fs::remove_file(&path) {
        Ok(()) => Ok(()),
        Err(err) if err.kind() == ErrorKind::NotFound => Ok(()),
        Err(err) => Err(format!(
            "failed to delete saved session {}: {err}",
            path.display()
        )),
    }
}

fn load_or_create_store_passphrase(profile_dir: &Path) -> Result<String, String> {
    let path = profile_dir.join(STORE_PASSPHRASE_FILE_NAME);
    if path.is_file() {
        let bytes = fs::read(&path).map_err(|err| {
            format!(
                "failed to read protected store passphrase {}: {err}",
                path.display()
            )
        })?;
        let protected = bytes
            .strip_prefix(PROTECTED_STORE_PASSPHRASE_MAGIC)
            .ok_or_else(|| {
                format!(
                    "protected store passphrase {} is missing the expected marker",
                    path.display()
                )
            })?;
        let payload =
            crate::dpapi::unprotect(protected, STORE_PASSPHRASE_DPAPI_ENTROPY).map_err(|err| {
                format!(
                    "failed to unprotect store passphrase {}: {err}",
                    path.display()
                )
            })?;
        let passphrase = String::from_utf8(payload).map_err(|err| {
            format!(
                "protected store passphrase {} is not valid UTF-8: {err}",
                path.display()
            )
        })?;
        if passphrase.len() != 64
            || !passphrase
                .bytes()
                .all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'))
        {
            return Err(format!(
                "protected store passphrase {} must be 64 lowercase hex characters",
                path.display()
            ));
        }
        return Ok(passphrase);
    }

    let matrix_store_path = profile_dir.join("matrix-store");
    if matrix_store_path.exists() {
        return Err(format!(
            "Matrix store {} exists but protected passphrase {} is missing",
            matrix_store_path.display(),
            path.display()
        ));
    }

    create_store_passphrase(profile_dir, &path)
}

fn create_store_passphrase(profile_dir: &Path, path: &Path) -> Result<String, String> {
    fs::create_dir_all(profile_dir).map_err(|err| {
        format!(
            "failed to create profile directory {}: {err}",
            profile_dir.display()
        )
    })?;

    let passphrase = hex_string(&crate::dpapi::random_bytes::<32>()?);
    let protected = crate::dpapi::protect(
        passphrase.as_bytes(),
        "mmatrix store passphrase",
        STORE_PASSPHRASE_DPAPI_ENTROPY,
    )
    .map_err(|err| {
        format!(
            "failed to protect store passphrase {}: {err}",
            path.display()
        )
    })?;
    let temp_path = path.with_extension("dpapi.tmp");
    let mut bytes = Vec::with_capacity(PROTECTED_STORE_PASSPHRASE_MAGIC.len() + protected.len());
    bytes.extend_from_slice(PROTECTED_STORE_PASSPHRASE_MAGIC);
    bytes.extend_from_slice(&protected);
    fs::write(&temp_path, bytes).map_err(|err| {
        format!(
            "failed to write protected store passphrase {}: {err}",
            temp_path.display()
        )
    })?;
    fs::rename(&temp_path, path).map_err(|err| {
        format!(
            "failed to install protected store passphrase {}: {err}",
            path.display()
        )
    })?;
    Ok(passphrase)
}

fn hex_string(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(char::from(b"0123456789abcdef"[(byte >> 4) as usize]));
        out.push(char::from(b"0123456789abcdef"[(byte & 0x0f) as usize]));
    }
    out
}

fn init_common_controls() -> Result<(), String> {
    let controls = INITCOMMONCONTROLSEX {
        dwSize: size_of::<INITCOMMONCONTROLSEX>() as u32,
        dwICC: ICC_TREEVIEW_CLASSES | ICC_BAR_CLASSES,
    };
    let success = unsafe { InitCommonControlsEx(&controls) };
    if success == 0 {
        return Err(last_os_error("InitCommonControlsEx"));
    }
    Ok(())
}

fn register_frame_class(instance: *mut c_void) -> Result<(), String> {
    let class_name = wide_null(FRAME_CLASS_NAME);
    let window_class = WNDCLASSEXW {
        cbSize: size_of::<WNDCLASSEXW>() as u32,
        style: 0,
        lpfnWndProc: Some(window_proc),
        cbClsExtra: 0,
        cbWndExtra: 0,
        hInstance: instance,
        hIcon: null_mut(),
        hCursor: unsafe { LoadCursorW(null_mut(), IDC_ARROW) },
        hbrBackground: (COLOR_WINDOW as isize + 1) as HBRUSH,
        lpszMenuName: null(),
        lpszClassName: class_name.as_ptr(),
        hIconSm: null_mut(),
    };

    let atom = unsafe { RegisterClassExW(&window_class) };
    if atom == 0 {
        let error = unsafe { GetLastError() };
        if error != 1410 {
            return Err(last_os_error("RegisterClassExW"));
        }
    }

    Ok(())
}

fn register_mdi_child_class(instance: *mut c_void) -> Result<(), String> {
    let class_name = wide_null(MDI_CHILD_CLASS_NAME);
    let window_class = WNDCLASSEXW {
        cbSize: size_of::<WNDCLASSEXW>() as u32,
        style: 0,
        lpfnWndProc: Some(mdi_child_window_proc),
        cbClsExtra: 0,
        cbWndExtra: 0,
        hInstance: instance,
        hIcon: null_mut(),
        hCursor: unsafe { LoadCursorW(null_mut(), IDC_ARROW) },
        hbrBackground: (COLOR_WINDOW as isize + 1) as HBRUSH,
        lpszMenuName: null(),
        lpszClassName: class_name.as_ptr(),
        hIconSm: null_mut(),
    };

    let atom = unsafe { RegisterClassExW(&window_class) };
    if atom == 0 {
        let error = unsafe { GetLastError() };
        if error != 1410 {
            return Err(last_os_error("RegisterClassExW MDI child"));
        }
    }

    Ok(())
}

fn register_room_content_class(instance: *mut c_void) -> Result<(), String> {
    let class_name = wide_null(ROOM_CONTENT_CLASS_NAME);
    let window_class = WNDCLASSEXW {
        cbSize: size_of::<WNDCLASSEXW>() as u32,
        style: 0,
        lpfnWndProc: Some(room_content_window_proc),
        cbClsExtra: 0,
        cbWndExtra: 0,
        hInstance: instance,
        hIcon: null_mut(),
        hCursor: unsafe { LoadCursorW(null_mut(), IDC_ARROW) },
        hbrBackground: (COLOR_WINDOW as isize + 1) as HBRUSH,
        lpszMenuName: null(),
        lpszClassName: class_name.as_ptr(),
        hIconSm: null_mut(),
    };

    let atom = unsafe { RegisterClassExW(&window_class) };
    if atom == 0 {
        let error = unsafe { GetLastError() };
        if error != 1410 {
            return Err(last_os_error("RegisterClassExW room content"));
        }
    }

    Ok(())
}

fn register_timeline_class(instance: *mut c_void) -> Result<(), String> {
    let class_name = wide_null(TIMELINE_CLASS_NAME);
    let window_class = WNDCLASSEXW {
        cbSize: size_of::<WNDCLASSEXW>() as u32,
        style: 0,
        lpfnWndProc: Some(timeline_window_proc),
        cbClsExtra: 0,
        cbWndExtra: 0,
        hInstance: instance,
        hIcon: null_mut(),
        hCursor: unsafe { LoadCursorW(null_mut(), IDC_ARROW) },
        hbrBackground: (COLOR_WINDOW as isize + 1) as HBRUSH,
        lpszMenuName: null(),
        lpszClassName: class_name.as_ptr(),
        hIconSm: null_mut(),
    };

    let atom = unsafe { RegisterClassExW(&window_class) };
    if atom == 0 {
        let error = unsafe { GetLastError() };
        if error != 1410 {
            return Err(last_os_error("RegisterClassExW timeline"));
        }
    }

    Ok(())
}

fn register_login_dialog_class(instance: *mut c_void) -> Result<(), String> {
    let class_name = wide_null(LOGIN_DIALOG_CLASS_NAME);
    let window_class = WNDCLASSEXW {
        cbSize: size_of::<WNDCLASSEXW>() as u32,
        style: 0,
        lpfnWndProc: Some(login_dialog_window_proc),
        cbClsExtra: 0,
        cbWndExtra: 0,
        hInstance: instance,
        hIcon: null_mut(),
        hCursor: unsafe { LoadCursorW(null_mut(), IDC_ARROW) },
        hbrBackground: (COLOR_WINDOW as isize + 1) as HBRUSH,
        lpszMenuName: null(),
        lpszClassName: class_name.as_ptr(),
        hIconSm: null_mut(),
    };

    let atom = unsafe { RegisterClassExW(&window_class) };
    if atom == 0 {
        let error = unsafe { GetLastError() };
        if error != 1410 {
            return Err(last_os_error("RegisterClassExW login dialog"));
        }
    }

    Ok(())
}

fn register_recover_e2ee_dialog_class(instance: *mut c_void) -> Result<(), String> {
    let class_name = wide_null(RECOVER_E2EE_DIALOG_CLASS_NAME);
    let window_class = WNDCLASSEXW {
        cbSize: size_of::<WNDCLASSEXW>() as u32,
        style: 0,
        lpfnWndProc: Some(recover_e2ee_dialog_window_proc),
        cbClsExtra: 0,
        cbWndExtra: 0,
        hInstance: instance,
        hIcon: null_mut(),
        hCursor: unsafe { LoadCursorW(null_mut(), IDC_ARROW) },
        hbrBackground: (COLOR_WINDOW as isize + 1) as HBRUSH,
        lpszMenuName: null(),
        lpszClassName: class_name.as_ptr(),
        hIconSm: null_mut(),
    };

    let atom = unsafe { RegisterClassExW(&window_class) };
    if atom == 0 {
        let error = unsafe { GetLastError() };
        if error != 1410 {
            return Err(last_os_error("RegisterClassExW recover e2ee dialog"));
        }
    }

    Ok(())
}

fn create_main_window(
    instance: *mut c_void,
    state: Box<WindowState>,
    frame_size: FrameSize,
) -> Result<HWND, String> {
    let class_name = wide_null(FRAME_CLASS_NAME);
    let title = wide_null(FRAME_TITLE);
    let state_ptr = Box::into_raw(state);

    let hwnd = unsafe {
        CreateWindowExW(
            0 as WINDOW_EX_STYLE,
            class_name.as_ptr(),
            title.as_ptr(),
            WS_OVERLAPPEDWINDOW | WS_VISIBLE | WS_CLIPCHILDREN,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            frame_size.width,
            frame_size.height,
            null_mut(),
            null_mut(),
            instance,
            state_ptr.cast(),
        )
    };

    if hwnd.is_null() {
        unsafe {
            drop(Box::from_raw(state_ptr));
        }
        return Err(last_os_error("CreateWindowExW"));
    }

    Ok(hwnd)
}

unsafe extern "system" fn window_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match message {
        WM_NCCREATE => {
            let create = &*(lparam as *const CREATESTRUCTW);
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, create.lpCreateParams as isize);
            1
        }
        WM_CREATE => match create_controls(hwnd) {
            Ok(()) => {
                if let Err(message) = sync_views_and_tree(hwnd) {
                    state_mut(hwnd).exit_code.store(1, Ordering::Relaxed);
                    eprintln!("{message}");
                    -1
                } else {
                    0
                }
            }
            Err(message) => {
                state_mut(hwnd).exit_code.store(1, Ordering::Relaxed);
                eprintln!("{message}");
                -1
            }
        },
        WM_SIZE => {
            layout_controls(hwnd);
            0
        }
        WM_COMMAND => {
            if handle_switchbar_command(hwnd, wparam) || handle_menu_command(hwnd, wparam) {
                0
            } else {
                default_window_proc(hwnd, message, wparam, lparam)
            }
        }
        WM_CORE_EVENT => {
            handle_core_events(hwnd);
            0
        }
        WM_COPYDATA => handle_copydata(hwnd, lparam),
        WM_NOTIFY => {
            if handle_tree_selection(hwnd, lparam) {
                0
            } else {
                default_window_proc(hwnd, message, wparam, lparam)
            }
        }
        WM_SELF_TEST => {
            if let Err(message) = run_self_test(hwnd) {
                state_mut(hwnd).exit_code.store(1, Ordering::Relaxed);
                eprintln!("{message}");
            }
            DestroyWindow(hwnd);
            0
        }
        WM_TIMER if wparam == AUTO_CLOSE_TIMER_ID => {
            KillTimer(hwnd, AUTO_CLOSE_TIMER_ID);
            DestroyWindow(hwnd);
            0
        }
        WM_DESTROY => {
            if let Some(sender) = state_mut(hwnd).command_sender.as_ref() {
                let _ = sender.send(Command::Shutdown);
            }
            PostQuitMessage(0);
            0
        }
        WM_NCDESTROY => {
            let state_ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut WindowState;
            if !state_ptr.is_null() {
                destroy_automation_state(&mut *state_ptr);
                SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
                drop(Box::from_raw(state_ptr));
            }
            0
        }
        _ => default_window_proc(hwnd, message, wparam, lparam),
    }
}

unsafe extern "system" fn mdi_child_window_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match message {
        WM_CREATE => {
            match create_mdi_child_content(hwnd, &mdi_child_create_state(lparam).view_id) {
                Ok(content) => {
                    SetWindowLongPtrW(hwnd, GWLP_USERDATA, content as isize);
                    0
                }
                Err(message) => {
                    eprintln!("{message}");
                    -1
                }
            }
        }
        WM_SIZE => {
            let content = mdi_child_content(hwnd);
            if !content.is_null() {
                let mut rect = RECT::default();
                GetClientRect(hwnd, &mut rect);
                MoveWindow(
                    content,
                    0,
                    0,
                    rect.right - rect.left,
                    rect.bottom - rect.top,
                    1,
                );
            }
            0
        }
        WM_MDIACTIVATE => {
            if lparam as HWND == hwnd
                && let Err(message) = handle_native_mdi_child_activation(hwnd)
            {
                eprintln!("{message}");
            }
            DefMDIChildProcW(hwnd, message, wparam, lparam)
        }
        WM_NCDESTROY => {
            unregister_destroyed_view(hwnd);
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
            DefMDIChildProcW(hwnd, message, wparam, lparam)
        }
        _ => DefMDIChildProcW(hwnd, message, wparam, lparam),
    }
}

unsafe extern "system" fn timeline_window_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match message {
        WM_CREATE => {
            let state = Box::new(TimelineState::default());
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, Box::into_raw(state) as isize);
            DefWindowProcW(hwnd, message, wparam, lparam)
        }
        WM_SETTEXT => set_timeline_text(hwnd, lparam as *const u16),
        WM_GETTEXTLENGTH => timeline_text_len(hwnd),
        WM_GETTEXT => timeline_get_text(hwnd, wparam, lparam),
        WM_TIMELINE_GET_TOP_LINE => timeline_state_ref(hwnd).top_line as LRESULT,
        WM_SIZE => {
            rebuild_timeline_paint_lines(hwnd);
            clamp_timeline_top_line(hwnd);
            update_timeline_scrollbar(hwnd);
            InvalidateRect(hwnd, null(), 1);
            0
        }
        WM_VSCROLL => scroll_timeline(hwnd, wparam),
        WM_PAINT => {
            paint_timeline(hwnd);
            0
        }
        WM_NCDESTROY => {
            let state_ptr = timeline_state_ptr(hwnd);
            if !state_ptr.is_null() {
                SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
                drop(Box::from_raw(state_ptr));
            }
            DefWindowProcW(hwnd, message, wparam, lparam)
        }
        _ => DefWindowProcW(hwnd, message, wparam, lparam),
    }
}

unsafe extern "system" fn room_content_window_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match message {
        WM_CREATE => match create_room_content_state(hwnd) {
            Ok(state) => {
                SetWindowLongPtrW(hwnd, GWLP_USERDATA, Box::into_raw(state) as isize);
                layout_room_content(hwnd);
                0
            }
            Err(message) => {
                eprintln!("{message}");
                -1
            }
        },
        WM_SIZE => {
            layout_room_content(hwnd);
            0
        }
        WM_NCDESTROY => {
            let state_ptr = room_content_state_ptr(hwnd);
            if !state_ptr.is_null() {
                SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
                drop(Box::from_raw(state_ptr));
            }
            DefWindowProcW(hwnd, message, wparam, lparam)
        }
        _ => DefWindowProcW(hwnd, message, wparam, lparam),
    }
}

unsafe fn mdi_child_create_state(lparam: LPARAM) -> &'static MdiChildCreateState {
    let create = &*(lparam as *const CREATESTRUCTW);
    let mdi_create = &*(create.lpCreateParams as *const MDICREATESTRUCTW);
    let state_ptr = mdi_create.lParam as *const MdiChildCreateState;
    assert!(
        !state_ptr.is_null(),
        "MDI child creation state must be present during WM_CREATE"
    );
    &*state_ptr
}

unsafe fn room_content_state_ptr(hwnd: HWND) -> *mut RoomContentState {
    GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut RoomContentState
}

unsafe fn room_content_state_ref(hwnd: HWND) -> &'static RoomContentState {
    let state_ptr = room_content_state_ptr(hwnd);
    assert!(
        !state_ptr.is_null(),
        "room content state must exist while the control is alive"
    );
    &*state_ptr
}

unsafe fn create_room_content_state(hwnd: HWND) -> Result<Box<RoomContentState>, String> {
    let instance = GetModuleHandleW(null());
    let timeline = CreateWindowExW(
        WS_EX_CLIENTEDGE,
        wide_null(TIMELINE_CLASS_NAME).as_ptr(),
        wide_null("").as_ptr(),
        WS_CHILD | WS_VISIBLE | WS_VSCROLL,
        0,
        0,
        0,
        0,
        hwnd,
        null_mut(),
        instance,
        null_mut(),
    );
    if timeline.is_null() {
        return Err(last_os_error("CreateWindowExW room timeline"));
    }

    let members = CreateWindowExW(
        WS_EX_CLIENTEDGE,
        wide_null("LISTBOX").as_ptr(),
        wide_null("").as_ptr(),
        WS_CHILD | WS_VISIBLE | WS_VSCROLL | LBS_NOINTEGRALHEIGHT as u32,
        0,
        0,
        0,
        0,
        hwnd,
        null_mut(),
        instance,
        null_mut(),
    );
    if members.is_null() {
        return Err(last_os_error("CreateWindowExW room member list"));
    }

    Ok(Box::new(RoomContentState { timeline, members }))
}

unsafe fn layout_room_content(hwnd: HWND) {
    let state_ptr = room_content_state_ptr(hwnd);
    if state_ptr.is_null() {
        return;
    }

    let state = &*state_ptr;
    let mut rect = RECT::default();
    GetClientRect(hwnd, &mut rect);
    let width = rect.right - rect.left;
    let height = rect.bottom - rect.top;
    let show_members =
        width >= ROOM_TIMELINE_MIN_WIDTH + ROOM_MEMBER_LIST_GAP + ROOM_MEMBER_LIST_WIDTH;

    if show_members {
        let timeline_width = width - ROOM_MEMBER_LIST_WIDTH - ROOM_MEMBER_LIST_GAP;
        MoveWindow(state.timeline, 0, 0, timeline_width, height, 1);
        MoveWindow(
            state.members,
            timeline_width + ROOM_MEMBER_LIST_GAP,
            0,
            ROOM_MEMBER_LIST_WIDTH,
            height,
            1,
        );
        ShowWindow(state.members, SW_SHOW);
    } else {
        MoveWindow(state.timeline, 0, 0, width, height, 1);
        MoveWindow(state.members, 0, 0, 0, 0, 1);
        ShowWindow(state.members, SW_HIDE);
    }
}

unsafe fn timeline_state_ptr(hwnd: HWND) -> *mut TimelineState {
    GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut TimelineState
}

unsafe fn timeline_state_ref(hwnd: HWND) -> &'static TimelineState {
    let state_ptr = timeline_state_ptr(hwnd);
    assert!(
        !state_ptr.is_null(),
        "timeline state must exist while the control is alive"
    );
    &*state_ptr
}

unsafe fn timeline_state_mut(hwnd: HWND) -> &'static mut TimelineState {
    let state_ptr = timeline_state_ptr(hwnd);
    assert!(
        !state_ptr.is_null(),
        "timeline state must exist while the control is alive"
    );
    &mut *state_ptr
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct TimelineColumnLayout {
    timestamp_left: i32,
    sender_left: i32,
    body_left: i32,
    body_right: i32,
}

fn timeline_column_layout(client_width: i32) -> TimelineColumnLayout {
    let timestamp_left = TIMELINE_LEFT_PADDING;
    let sender_left = timestamp_left + TIMELINE_TIMESTAMP_COLUMN_WIDTH + TIMELINE_COLUMN_GAP;
    let body_left = sender_left + TIMELINE_SENDER_COLUMN_WIDTH + TIMELINE_COLUMN_GAP;
    let body_right = (client_width - TIMELINE_RIGHT_PADDING).max(body_left + 1);
    TimelineColumnLayout {
        timestamp_left,
        sender_left,
        body_left,
        body_right,
    }
}

fn timeline_body_column_width(client_width: i32) -> i32 {
    let layout = timeline_column_layout(client_width);
    layout.body_right - layout.body_left
}

fn plain_timeline_paint_lines(lines: &[String]) -> Vec<TimelinePaintLine> {
    lines
        .iter()
        .cloned()
        .map(|text| TimelinePaintLine {
            kind: TimelineRowKind::Plain,
            visual: TimelinePaintLineVisual::FullWidth(text),
            image_state: None,
            image_slot_index: None,
            source_row_index: None,
        })
        .collect()
}

fn trim_timeline_visual_segment(text: &str) -> String {
    text.trim_end_matches(|c: char| c.is_whitespace())
        .to_owned()
}

fn wrap_timeline_visual_line<Measure>(line: &str, max_width: i32, measure: &Measure) -> Vec<String>
where
    Measure: Fn(&str) -> i32,
{
    assert!(max_width > 0, "timeline body column width must be positive");

    if line.is_empty() {
        return vec![String::new()];
    }

    let mut wrapped = Vec::new();
    let mut remaining = line;
    while !remaining.is_empty() {
        if measure(remaining) <= max_width {
            wrapped.push(trim_timeline_visual_segment(remaining));
            break;
        }

        let mut last_fit_end = 0;
        let mut last_whitespace_break = None;
        let mut first_char_end = None;
        for (index, ch) in remaining.char_indices() {
            let next = index + ch.len_utf8();
            if first_char_end.is_none() {
                first_char_end = Some(next);
            }
            if measure(&remaining[..next]) <= max_width {
                last_fit_end = next;
                if ch.is_whitespace() && index > 0 {
                    last_whitespace_break = Some(index);
                }
                continue;
            }
            break;
        }

        let break_index = last_whitespace_break
            .or_else(|| (last_fit_end > 0).then_some(last_fit_end))
            .or(first_char_end)
            .expect("a non-empty timeline body line must have at least one char boundary");
        wrapped.push(trim_timeline_visual_segment(&remaining[..break_index]));
        remaining = &remaining[break_index..];
    }

    wrapped
}

fn wrap_timeline_visual_body<Measure>(body: &str, max_width: i32, measure: &Measure) -> Vec<String>
where
    Measure: Fn(&str) -> i32,
{
    split_rendered_lines(body)
        .into_iter()
        .flat_map(|line| wrap_timeline_visual_line(&line, max_width, measure))
        .collect()
}

fn timeline_full_width_paint_lines(kind: &TimelineRowKind, text: &str) -> Vec<TimelinePaintLine> {
    split_rendered_lines(text)
        .into_iter()
        .map(|line| TimelinePaintLine {
            kind: kind.clone(),
            visual: TimelinePaintLineVisual::FullWidth(line),
            image_state: None,
            image_slot_index: None,
            source_row_index: None,
        })
        .collect()
}

fn timeline_row_paint_lines_with_measure<Measure>(
    rows: &[TimelineRow],
    client_width: i32,
    measure: &Measure,
) -> Vec<TimelinePaintLine>
where
    Measure: Fn(&str) -> i32,
{
    let body_width = timeline_body_column_width(client_width);
    let mut paint_lines = Vec::new();
    for (source_row_index, row) in rows.iter().enumerate() {
        match &row.visual {
            TimelineRowVisual::FullWidth { text } => {
                let mut row_paint_lines = timeline_full_width_paint_lines(&row.kind, text);
                for line in &mut row_paint_lines {
                    line.source_row_index = Some(source_row_index);
                }
                paint_lines.extend(row_paint_lines);
            }
            TimelineRowVisual::Log {
                timestamp,
                sender_prefix,
                body,
            } => {
                let mut first_visual_line = true;
                for wrapped_body_line in wrap_timeline_visual_body(body, body_width, measure) {
                    paint_lines.push(TimelinePaintLine {
                        kind: row.kind.clone(),
                        visual: TimelinePaintLineVisual::Log {
                            timestamp: first_visual_line.then(|| timestamp.clone()),
                            sender_prefix: first_visual_line.then(|| sender_prefix.clone()),
                            body: wrapped_body_line,
                        },
                        image_state: None,
                        image_slot_index: None,
                        source_row_index: Some(source_row_index),
                    });
                    first_visual_line = false;
                }
            }
        }
        if let Some(image_state) = &row.image_state {
            for image_slot_index in 0..TIMELINE_IMAGE_MEDIA_SLOT_LINES {
                paint_lines.push(TimelinePaintLine {
                    kind: row.kind.clone(),
                    visual: TimelinePaintLineVisual::ImageSlot,
                    image_state: Some(image_state.clone()),
                    image_slot_index: Some(image_slot_index),
                    source_row_index: Some(source_row_index),
                });
            }
        }
    }
    paint_lines
}

unsafe fn timeline_measure_text_width(
    hdc: windows_sys::Win32::Graphics::Gdi::HDC,
    text: &str,
) -> i32 {
    if text.is_empty() {
        return 0;
    }

    let wide = wide_null(text);
    let mut size = SIZE { cx: 0, cy: 0 };
    let result = GetTextExtentPoint32W(hdc, wide.as_ptr(), wide.len() as i32 - 1, &mut size);
    assert!(
        result != 0,
        "GetTextExtentPoint32W must succeed for timeline text"
    );
    size.cx
}

unsafe fn timeline_measurement_hdc(
    hwnd: HWND,
) -> (
    windows_sys::Win32::Graphics::Gdi::HDC,
    *mut c_void,
    *mut c_void,
) {
    let hdc = GetDC(hwnd);
    assert!(
        !hdc.is_null(),
        "GetDC must succeed for timeline measurement"
    );
    let font = SendMessageW(hwnd, WM_GETFONT, 0, 0) as *mut c_void;
    let previous = if font.is_null() {
        null_mut()
    } else {
        SelectObject(hdc, font)
    };
    (hdc, font, previous)
}

unsafe fn timeline_row_paint_lines(
    hwnd: HWND,
    rows: &[TimelineRow],
    client_width: i32,
) -> Vec<TimelinePaintLine> {
    let (hdc, font, previous) = timeline_measurement_hdc(hwnd);
    let paint_lines = timeline_row_paint_lines_with_measure(rows, client_width, &|text| unsafe {
        timeline_measure_text_width(hdc, text)
    });
    if !font.is_null() && !previous.is_null() {
        SelectObject(hdc, previous);
    }
    ReleaseDC(hwnd, hdc);
    paint_lines
}

unsafe fn timeline_client_width(hwnd: HWND) -> i32 {
    let mut rect = RECT::default();
    GetClientRect(hwnd, &mut rect);
    rect.right - rect.left
}

unsafe fn rebuild_timeline_paint_lines(hwnd: HWND) {
    let client_width = timeline_client_width(hwnd);
    let (source_rows, lines) = {
        let state = timeline_state_ref(hwnd);
        (state.source_rows.clone(), state.lines.clone())
    };
    let paint_lines = if source_rows.is_empty() {
        plain_timeline_paint_lines(&lines)
    } else {
        timeline_row_paint_lines(hwnd, &source_rows, client_width)
    };
    timeline_state_mut(hwnd).paint_lines = paint_lines;
}

unsafe fn apply_timeline_content(
    hwnd: HWND,
    source_rows: Vec<TimelineRow>,
    rendered_text: String,
    lines: Vec<String>,
    paint_lines: Vec<TimelinePaintLine>,
) {
    let state = timeline_state_mut(hwnd);
    state.source_rows = source_rows;
    state.rendered_text = rendered_text;
    state.lines = lines;
    state.paint_lines = paint_lines;
    clamp_timeline_top_line(hwnd);
    update_timeline_scrollbar(hwnd);
    InvalidateRect(hwnd, null(), 1);
}

unsafe fn set_timeline_text(hwnd: HWND, text_ptr: *const u16) -> LRESULT {
    let (rendered_text, lines) = normalize_timeline_text(&wide_ptr_to_string(text_ptr));
    let paint_lines = plain_timeline_paint_lines(&lines);
    apply_timeline_content(hwnd, Vec::new(), rendered_text, lines, paint_lines);
    1
}

unsafe fn set_timeline_rows(hwnd: HWND, rows: &[TimelineRow]) {
    let (rendered_text, lines) =
        normalize_timeline_text(&render_view_text(&project_timeline_rows(rows)));
    let paint_lines = timeline_row_paint_lines(hwnd, rows, timeline_client_width(hwnd));
    debug_assert_eq!(
        project_timeline_rows(rows),
        rows.iter()
            .map(|row| row.logical_text.clone())
            .collect::<Vec<_>>()
    );
    apply_timeline_content(hwnd, rows.to_vec(), rendered_text, lines, paint_lines);
}

unsafe fn timeline_source_row_paint_line_count(hwnd: HWND, row: &TimelineRow) -> usize {
    timeline_row_paint_lines(hwnd, std::slice::from_ref(row), timeline_client_width(hwnd)).len()
}

unsafe fn timeline_source_row_index_for_top_line(
    hwnd: HWND,
    rows: &[TimelineRow],
    top_line: usize,
) -> Option<usize> {
    let mut next_top_line = 0usize;
    for (index, row) in rows.iter().enumerate() {
        let line_count = timeline_source_row_paint_line_count(hwnd, row);
        if top_line < next_top_line + line_count {
            return Some(index);
        }
        next_top_line += line_count;
    }

    (!rows.is_empty()).then_some(rows.len() - 1)
}

unsafe fn timeline_top_line_for_source_row_index(
    hwnd: HWND,
    rows: &[TimelineRow],
    source_row_index: usize,
) -> usize {
    rows.iter()
        .take(source_row_index)
        .map(|row| timeline_source_row_paint_line_count(hwnd, row))
        .sum()
}

unsafe fn set_room_member_list(hwnd: HWND, rows: &[RoomMemberView]) {
    SendMessageW(hwnd, LB_RESETCONTENT, 0, 0);
    for row in rows {
        let label = wide_null(&row.label);
        SendMessageW(hwnd, LB_ADDSTRING, 0, label.as_ptr() as LPARAM);
    }
}

unsafe fn timeline_text_len(hwnd: HWND) -> LRESULT {
    timeline_state_ref(hwnd)
        .rendered_text
        .encode_utf16()
        .count() as LRESULT
}

unsafe fn timeline_get_text(hwnd: HWND, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if wparam == 0 || lparam == 0 {
        return 0;
    }

    let state = timeline_state_ref(hwnd);
    let capacity = wparam;
    let buffer = lparam as *mut u16;
    let encoded = state.rendered_text.encode_utf16().collect::<Vec<_>>();
    let copied = encoded.len().min(capacity.saturating_sub(1));
    copy_nonoverlapping(encoded.as_ptr(), buffer, copied);
    *buffer.add(copied) = 0;
    copied as LRESULT
}

unsafe fn timeline_visible_rows(hwnd: HWND) -> usize {
    let state = timeline_state_ref(hwnd);
    let mut rect = RECT::default();
    GetClientRect(hwnd, &mut rect);
    let client_height = rect.bottom - rect.top;
    let usable_height = (client_height - state.vertical_padding * 2).max(state.line_height);
    (usable_height / state.line_height).max(1) as usize
}

fn timeline_max_top_line(line_count: usize, visible_rows: usize) -> usize {
    line_count.saturating_sub(visible_rows)
}

fn clamped_timeline_top_line(top_line: usize, line_count: usize, visible_rows: usize) -> usize {
    top_line.min(timeline_max_top_line(line_count, visible_rows))
}

unsafe fn clamp_timeline_top_line(hwnd: HWND) {
    let visible_rows = timeline_visible_rows(hwnd);
    let state = timeline_state_mut(hwnd);
    state.top_line =
        clamped_timeline_top_line(state.top_line, state.paint_lines.len(), visible_rows);
}

unsafe fn update_timeline_scrollbar(hwnd: HWND) {
    let visible_rows = timeline_visible_rows(hwnd);
    let state = timeline_state_ref(hwnd);
    let expected_max = state.paint_lines.len().saturating_sub(1) as i32;
    let expected_page = visible_rows.max(1).min(state.paint_lines.len().max(1)) as u32;
    let scroll_info = SCROLLINFO {
        cbSize: size_of::<SCROLLINFO>() as u32,
        fMask: SIF_RANGE | SIF_PAGE | SIF_POS,
        nMin: 0,
        nMax: expected_max,
        nPage: expected_page,
        nPos: state.top_line as i32,
        nTrackPos: 0,
    };
    let set_result = SetScrollInfo(hwnd, SB_VERT, &scroll_info, 1);

    let mut actual = SCROLLINFO {
        cbSize: size_of::<SCROLLINFO>() as u32,
        fMask: SIF_ALL,
        nMin: 0,
        nMax: 0,
        nPage: 0,
        nPos: 0,
        nTrackPos: 0,
    };
    let get_success = GetScrollInfo(hwnd, SB_VERT, &mut actual);
    if get_success == 0
        || actual.nMax != expected_max
        || actual.nPage != expected_page
        || actual.nPos != state.top_line as i32
    {
        let mut rect = RECT::default();
        GetClientRect(hwnd, &mut rect);
        let style = GetWindowLongPtrW(hwnd, GWL_STYLE) as u32;
        let error = if get_success == 0 { GetLastError() } else { 0 };
        eprintln!(
            "timeline scrollbar mismatch: hwnd={:#x} style=0x{style:x} rect={}x{} lines={} visible_rows={} expected_max={} expected_page={} expected_pos={} set_result={} get_success={} actual_max={} actual_page={} actual_pos={} last_error={}",
            hwnd as usize,
            rect.right - rect.left,
            rect.bottom - rect.top,
            state.paint_lines.len(),
            visible_rows,
            expected_max,
            expected_page,
            state.top_line,
            set_result,
            get_success,
            actual.nMax,
            actual.nPage,
            actual.nPos,
            error
        );
    }
}

unsafe fn capture_room_history_backfill_anchor(
    state: &WindowState,
    room_id: &str,
) -> Option<TimelineBackfillAnchor> {
    let ViewId::Room(active_room_id) = &state.active_view else {
        return None;
    };
    if active_room_id != room_id {
        return None;
    }

    let view = state.views.get(&state.active_view)?;
    let timeline = room_content_timeline(view.content);
    let timeline_state = timeline_state_ref(timeline);
    let source_row_index = timeline_source_row_index_for_top_line(
        timeline,
        &timeline_state.source_rows,
        timeline_state.top_line,
    )?;
    Some(TimelineBackfillAnchor {
        room_id: room_id.to_owned(),
        source_row: timeline_state.source_rows.get(source_row_index)?.clone(),
        source_row_index,
        source_row_count: timeline_state.source_rows.len(),
    })
}

unsafe fn restore_room_history_backfill_anchor(hwnd: HWND, anchor: TimelineBackfillAnchor) {
    let state = state_ref(hwnd);
    let ViewId::Room(active_room_id) = &state.active_view else {
        return;
    };
    if *active_room_id != anchor.room_id {
        return;
    }

    let Some(view) = state.views.get(&state.active_view) else {
        return;
    };
    let timeline = room_content_timeline(view.content);
    let timeline_state = timeline_state_ref(timeline);
    if timeline_state.source_rows.is_empty() {
        return;
    }

    let source_row_index = timeline_state
        .source_rows
        .iter()
        .position(|row| row == &anchor.source_row)
        .unwrap_or_else(|| {
            anchor
                .source_row_index
                .saturating_add(
                    timeline_state
                        .source_rows
                        .len()
                        .saturating_sub(anchor.source_row_count),
                )
                .min(timeline_state.source_rows.len().saturating_sub(1))
        });
    let top_line = timeline_top_line_for_source_row_index(
        timeline,
        &timeline_state.source_rows,
        source_row_index,
    );
    let visible_rows = timeline_visible_rows(timeline);
    let max_top_line = timeline_max_top_line(timeline_state.paint_lines.len(), visible_rows);
    timeline_state_mut(timeline).top_line = top_line.min(max_top_line);
    update_timeline_scrollbar(timeline);
    InvalidateRect(timeline, null(), 1);
}

unsafe fn scroll_timeline(hwnd: HWND, wparam: WPARAM) -> LRESULT {
    let command = (wparam & 0xffff) as i32;
    let visible_rows = timeline_visible_rows(hwnd);
    let max_top_line = {
        let state = timeline_state_ref(hwnd);
        timeline_max_top_line(state.paint_lines.len(), visible_rows)
    };
    let current = timeline_state_ref(hwnd).top_line;

    let next = match command {
        SB_TOP => 0,
        SB_BOTTOM => max_top_line,
        SB_LINEUP => current.saturating_sub(1),
        SB_LINEDOWN => current.saturating_add(1),
        SB_PAGEUP => current.saturating_sub(visible_rows),
        SB_PAGEDOWN => current.saturating_add(visible_rows),
        SB_THUMBPOSITION | SB_THUMBTRACK => {
            let mut scroll_info = SCROLLINFO {
                cbSize: size_of::<SCROLLINFO>() as u32,
                fMask: SIF_TRACKPOS,
                nMin: 0,
                nMax: 0,
                nPage: 0,
                nPos: 0,
                nTrackPos: 0,
            };
            GetScrollInfo(hwnd, SB_VERT, &mut scroll_info);
            scroll_info.nTrackPos.max(0) as usize
        }
        _ => current,
    }
    .min(max_top_line);

    if next != current {
        timeline_state_mut(hwnd).top_line = next;
        update_timeline_scrollbar(hwnd);
        InvalidateRect(hwnd, null(), 1);
    }

    if matches!(
        command,
        SB_TOP | SB_PAGEUP | SB_THUMBPOSITION | SB_THUMBTRACK
    ) && next == 0
        && let Err(message) = submit_backfill_room_history(hwnd)
    {
        eprintln!("{message}");
    }

    0
}

unsafe fn paint_timeline(hwnd: HWND) {
    let mut paint = PAINTSTRUCT::default();
    let hdc = BeginPaint(hwnd, &mut paint);
    FillRect(hdc, &paint.rcPaint, (COLOR_WINDOW as isize + 1) as HBRUSH);
    SetBkMode(hdc, TRANSPARENT as i32);
    let mut client_rect = RECT::default();
    GetClientRect(hwnd, &mut client_rect);
    let layout = timeline_column_layout(client_rect.right - client_rect.left);
    let font = SendMessageW(hwnd, WM_GETFONT, 0, 0) as *mut c_void;
    let previous = if font.is_null() {
        null_mut()
    } else {
        SelectObject(hdc, font)
    };
    let find_highlight_brush = CreateSolidBrush(TIMELINE_FIND_HIGHLIGHT_COLOR);

    let state = timeline_state_ref(hwnd);
    let visible_rows = timeline_visible_rows(hwnd);
    for (row, line) in state
        .paint_lines
        .iter()
        .skip(state.top_line)
        .take(visible_rows)
        .enumerate()
    {
        let line_y = state.vertical_padding + row as i32 * state.line_height;
        if state.selected_find_source_row_index.is_some()
            && line.source_row_index == state.selected_find_source_row_index
        {
            let rect = RECT {
                left: 0,
                top: line_y,
                right: client_rect.right,
                bottom: line_y + state.line_height,
            };
            FillRect(hdc, &rect, find_highlight_brush);
        }
        if let Some(image_slot_index) = line.image_slot_index {
            if let Some(image_state) = line.image_state.as_ref() {
                paint_timeline_image_slot(hwnd, hdc, line_y, image_slot_index, image_state);
            }
            continue;
        }

        SetTextColor(hdc, timeline_row_color(&line.kind));
        match &line.visual {
            TimelinePaintLineVisual::FullWidth(text) => {
                let wide = wide_null(text);
                let mut rect = RECT {
                    left: TIMELINE_LEFT_PADDING,
                    top: line_y,
                    right: (client_rect.right - TIMELINE_RIGHT_PADDING)
                        .max(TIMELINE_LEFT_PADDING + 1),
                    bottom: line_y + state.line_height,
                };
                DrawTextW(
                    hdc,
                    wide.as_ptr(),
                    wide.len() as i32 - 1,
                    &mut rect,
                    DT_SINGLELINE | DT_NOPREFIX,
                );
            }
            TimelinePaintLineVisual::Log {
                timestamp,
                sender_prefix,
                body,
            } => {
                if let Some(timestamp) = timestamp {
                    let wide = wide_null(timestamp);
                    let mut rect = RECT {
                        left: layout.timestamp_left,
                        top: line_y,
                        right: layout.timestamp_left + TIMELINE_TIMESTAMP_COLUMN_WIDTH,
                        bottom: line_y + state.line_height,
                    };
                    DrawTextW(
                        hdc,
                        wide.as_ptr(),
                        wide.len() as i32 - 1,
                        &mut rect,
                        DT_SINGLELINE | DT_NOPREFIX | DT_END_ELLIPSIS,
                    );
                }
                if let Some(sender_prefix) = sender_prefix {
                    let wide = wide_null(sender_prefix);
                    let mut rect = RECT {
                        left: layout.sender_left,
                        top: line_y,
                        right: layout.sender_left + TIMELINE_SENDER_COLUMN_WIDTH,
                        bottom: line_y + state.line_height,
                    };
                    DrawTextW(
                        hdc,
                        wide.as_ptr(),
                        wide.len() as i32 - 1,
                        &mut rect,
                        DT_SINGLELINE | DT_NOPREFIX | DT_END_ELLIPSIS,
                    );
                }
                let wide = wide_null(body);
                let mut rect = RECT {
                    left: layout.body_left,
                    top: line_y,
                    right: layout.body_right,
                    bottom: line_y + state.line_height,
                };
                DrawTextW(
                    hdc,
                    wide.as_ptr(),
                    wide.len() as i32 - 1,
                    &mut rect,
                    DT_SINGLELINE | DT_NOPREFIX,
                );
            }
            TimelinePaintLineVisual::ImageSlot => {}
        }
    }

    DeleteObject(find_highlight_brush as *mut c_void);
    if !font.is_null() && !previous.is_null() {
        SelectObject(hdc, previous);
    }

    EndPaint(hwnd, &paint);
}

const fn rgb(r: u8, g: u8, b: u8) -> u32 {
    r as u32 | ((g as u32) << 8) | ((b as u32) << 16)
}

fn timeline_row_color(kind: &TimelineRowKind) -> u32 {
    match kind {
        TimelineRowKind::Header | TimelineRowKind::Plain => rgb(0x00, 0x00, 0x00),
        TimelineRowKind::Topic => rgb(0x4A, 0x42, 0x2A),
        TimelineRowKind::Typing => rgb(0x68, 0x68, 0x68),
        TimelineRowKind::ReactionSummary => rgb(0x6B, 0x6B, 0x6B),
        TimelineRowKind::Message {
            send_state: TimelineMessageSendState::Pending,
            ..
        } => rgb(0x4E, 0x5D, 0x69),
        TimelineRowKind::Message {
            send_state: TimelineMessageSendState::Failed,
            ..
        } => rgb(0x86, 0x22, 0x22),
        TimelineRowKind::Message {
            presentation: RoomMessagePresentationKind::Notice,
            send_state: TimelineMessageSendState::Sent,
        } => rgb(0x63, 0x54, 0x2A),
        TimelineRowKind::Message {
            presentation: RoomMessagePresentationKind::Emote,
            send_state: TimelineMessageSendState::Sent,
        } => rgb(0x24, 0x5E, 0x7A),
        TimelineRowKind::Message { .. } => rgb(0x00, 0x00, 0x00),
        TimelineRowKind::Image | TimelineRowKind::File | TimelineRowKind::Audio => {
            rgb(0x1F, 0x64, 0x5A)
        }
        TimelineRowKind::Unsupported => rgb(0x7A, 0x46, 0x16),
        TimelineRowKind::StateNotice => rgb(0x3D, 0x67, 0x35),
    }
}

fn timeline_image_slot_rect(line_y: i32, image_slot_index: usize) -> RECT {
    let slot_height = TIMELINE_IMAGE_MEDIA_SLOT_LINES as i32 * TIMELINE_ROW_HEIGHT;
    let top = line_y + (slot_height - TIMELINE_IMAGE_THUMBNAIL_MAX_HEIGHT) / 2
        - image_slot_index as i32 * TIMELINE_ROW_HEIGHT;
    let left = TIMELINE_LEFT_PADDING + TIMELINE_IMAGE_LEFT_INDENT;
    RECT {
        left,
        top,
        right: left + TIMELINE_IMAGE_THUMBNAIL_MAX_WIDTH,
        bottom: top + TIMELINE_IMAGE_THUMBNAIL_MAX_HEIGHT,
    }
}

unsafe fn paint_timeline_image_slot(
    hwnd: HWND,
    hdc: windows_sys::Win32::Graphics::Gdi::HDC,
    line_y: i32,
    image_slot_index: usize,
    image_state: &TimelineImageState,
) {
    let rect = timeline_image_slot_rect(line_y, image_slot_index);
    match image_state {
        TimelineImageState::Pending => {
            paint_timeline_image_placeholder(hdc, &rect, "loading image...");
        }
        TimelineImageState::Loaded { key, width, height } => {
            paint_timeline_loaded_image(hwnd, hdc, &rect, key, *width, *height);
        }
        TimelineImageState::Failed { .. } => {
            paint_timeline_image_placeholder(hdc, &rect, "image unavailable");
        }
    }
}

unsafe fn paint_timeline_image_placeholder(
    hdc: windows_sys::Win32::Graphics::Gdi::HDC,
    rect: &RECT,
    message: &str,
) {
    let border = CreateSolidBrush(rgb(0x7A, 0x7A, 0x7A));
    if !border.is_null() {
        FrameRect(hdc, rect, border);
        DeleteObject(border as _);
    }
    let text = wide_null(message);
    SetTextColor(hdc, rgb(0x5E, 0x5E, 0x5E));
    TextOutW(
        hdc,
        rect.left + 8,
        rect.top + 8,
        text.as_ptr(),
        text.len() as i32 - 1,
    );
}

unsafe fn paint_timeline_loaded_image(
    hwnd: HWND,
    hdc: windows_sys::Win32::Graphics::Gdi::HDC,
    rect: &RECT,
    key: &str,
    width: u32,
    height: u32,
) {
    let window_state = timeline_window_state(hwnd);
    let Some(image) = window_state.decoded_images.get(key) else {
        paint_timeline_image_placeholder(hdc, rect, "image unavailable");
        return;
    };
    let (draw_width, draw_height) = scaled_thumbnail_dimensions(
        width,
        height,
        TIMELINE_IMAGE_THUMBNAIL_MAX_WIDTH,
        TIMELINE_IMAGE_THUMBNAIL_MAX_HEIGHT,
    );
    let draw_x = rect.left + (TIMELINE_IMAGE_THUMBNAIL_MAX_WIDTH - draw_width) / 2;
    let draw_y = rect.top + (TIMELINE_IMAGE_THUMBNAIL_MAX_HEIGHT - draw_height) / 2;

    let border = CreateSolidBrush(rgb(0x7A, 0x7A, 0x7A));
    if !border.is_null() {
        FrameRect(hdc, rect, border);
        DeleteObject(border as _);
    }

    let mut graphics = null_mut::<GpGraphics>();
    if GdipCreateFromHDC(hdc, &mut graphics) != 0 {
        paint_timeline_image_placeholder(hdc, rect, "image unavailable");
        return;
    }
    let status = GdipDrawImageRectI(graphics, image.raw, draw_x, draw_y, draw_width, draw_height);
    GdipDeleteGraphics(graphics);
    if status != 0 {
        paint_timeline_image_placeholder(hdc, rect, "image unavailable");
    }
}

unsafe fn timeline_window_state(hwnd: HWND) -> &'static WindowState {
    let room_content = GetParent(hwnd);
    assert!(
        !room_content.is_null(),
        "timeline control must have a room content parent"
    );
    let mdi_child = GetParent(room_content);
    assert!(
        !mdi_child.is_null(),
        "room content must have an mdi child parent"
    );
    let mdi_client = GetParent(mdi_child);
    assert!(
        !mdi_client.is_null(),
        "mdi child must have an mdi client parent"
    );
    let frame = GetParent(mdi_client);
    assert!(!frame.is_null(), "mdi client must have a frame parent");
    state_ref(frame)
}

unsafe fn timeline_window_state_mut(hwnd: HWND) -> &'static mut WindowState {
    let room_content = GetParent(hwnd);
    assert!(
        !room_content.is_null(),
        "timeline control must have a room content parent"
    );
    let mdi_child = GetParent(room_content);
    assert!(
        !mdi_child.is_null(),
        "room content must have an mdi child parent"
    );
    let mdi_client = GetParent(mdi_child);
    assert!(
        !mdi_client.is_null(),
        "mdi child must have an mdi client parent"
    );
    let frame = GetParent(mdi_client);
    assert!(!frame.is_null(), "mdi client must have a frame parent");
    state_mut(frame)
}

fn scaled_thumbnail_dimensions(
    width: u32,
    height: u32,
    max_width: i32,
    max_height: i32,
) -> (i32, i32) {
    let width = width as f64;
    let height = height as f64;
    let scale = f64::min(
        1.0,
        f64::min(max_width as f64 / width, max_height as f64 / height),
    );
    let draw_width = (width * scale).round() as i32;
    let draw_height = (height * scale).round() as i32;
    (draw_width.max(1), draw_height.max(1))
}

unsafe extern "system" fn login_dialog_window_proc(
    hwnd: HWND,
    message: u32,
    _wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match message {
        WM_NCCREATE => {
            let create = &*(lparam as *const CREATESTRUCTW);
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, create.lpCreateParams as isize);
            1
        }
        WM_CREATE => match create_login_dialog_controls(hwnd) {
            Ok(()) => 0,
            Err(message) => {
                eprintln!("{message}");
                -1
            }
        },
        WM_COMMAND => {
            let command_id = _wparam & 0xffff;
            match command_id {
                LOGIN_DIALOG_OK_ID => {
                    let Some(state) = login_dialog_state(hwnd) else {
                        return 0;
                    };
                    let form = LoginForm {
                        homeserver_url: read_window_text(state.homeserver_url_edit),
                        username: read_window_text(state.username_edit),
                        password: read_window_text(state.password_edit),
                        device_display_name: read_window_text(state.device_display_name_edit),
                    };
                    *state
                        .result
                        .lock()
                        .expect("login dialog result mutex poisoned") = Some(form);
                    DestroyWindow(hwnd);
                    0
                }
                LOGIN_DIALOG_CANCEL_ID => {
                    DestroyWindow(hwnd);
                    0
                }
                _ => DefWindowProcW(hwnd, message, _wparam, lparam),
            }
        }
        WM_CLOSE => {
            DestroyWindow(hwnd);
            0
        }
        WM_SIZE => {
            layout_login_dialog(hwnd);
            0
        }
        WM_NCDESTROY => {
            let state_ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut LoginDialogState;
            if !state_ptr.is_null() {
                SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
                drop(Box::from_raw(state_ptr));
            }
            0
        }
        _ => DefWindowProcW(hwnd, message, _wparam, lparam),
    }
}

unsafe extern "system" fn recover_e2ee_dialog_window_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match message {
        WM_NCCREATE => {
            let create = &*(lparam as *const CREATESTRUCTW);
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, create.lpCreateParams as isize);
            1
        }
        WM_CREATE => match create_recover_e2ee_dialog_controls(hwnd) {
            Ok(()) => 0,
            Err(message) => {
                eprintln!("{message}");
                -1
            }
        },
        WM_COMMAND => {
            let command_id = wparam & 0xffff;
            match command_id {
                RECOVER_E2EE_DIALOG_OK_ID => {
                    let Some(state) = recover_e2ee_dialog_state(hwnd) else {
                        return 0;
                    };
                    let recovery_secret = read_window_text(state.recovery_secret_edit);
                    if recovery_secret.is_empty() {
                        show_error_message(
                            hwnd,
                            "Recover E2EE",
                            "recovery key or passphrase must not be empty",
                        );
                        return 0;
                    }
                    clear_recover_e2ee_dialog_secret(hwnd);
                    *state
                        .result
                        .lock()
                        .expect("recover e2ee dialog result mutex poisoned") =
                        Some(RecoverE2eeForm { recovery_secret });
                    DestroyWindow(hwnd);
                    0
                }
                RECOVER_E2EE_DIALOG_CANCEL_ID => {
                    clear_recover_e2ee_dialog_secret(hwnd);
                    DestroyWindow(hwnd);
                    0
                }
                _ => DefWindowProcW(hwnd, message, wparam, lparam),
            }
        }
        WM_CLOSE => {
            clear_recover_e2ee_dialog_secret(hwnd);
            DestroyWindow(hwnd);
            0
        }
        WM_SIZE => {
            layout_recover_e2ee_dialog(hwnd);
            0
        }
        WM_NCDESTROY => {
            let state_ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut RecoverE2eeDialogState;
            if !state_ptr.is_null() {
                SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
                drop(Box::from_raw(state_ptr));
            }
            0
        }
        _ => DefWindowProcW(hwnd, message, wparam, lparam),
    }
}

unsafe fn create_controls(hwnd: HWND) -> Result<(), String> {
    let instance = GetModuleHandleW(null());
    let state = state_mut(hwnd);

    create_account_menu(hwnd)?;

    state.controls.switchbar = CreateWindowExW(
        0,
        TOOLBARCLASSNAMEW,
        null(),
        WS_CHILD | WS_VISIBLE | TBSTYLE_FLAT,
        0,
        0,
        0,
        0,
        hwnd,
        SWITCHBAR_ID as usize as HMENU,
        instance,
        null_mut(),
    );
    if state.controls.switchbar.is_null() {
        return Err(last_os_error("CreateWindowExW switchbar"));
    }
    SendMessageW(
        state.controls.switchbar,
        TB_BUTTONSTRUCTSIZE,
        size_of::<TBBUTTON>(),
        0,
    );
    SendMessageW(
        state.controls.switchbar,
        TB_SETEXTENDEDSTYLE,
        0,
        TBSTYLE_EX_MIXEDBUTTONS as LPARAM,
    );
    SendMessageW(
        state.controls.switchbar,
        TB_SETBUTTONSIZE,
        0,
        pack_size_lparam(160, SWITCHBAR_HEIGHT),
    );

    state.controls.tree = create_child_window(
        hwnd,
        TREE_CLASS_NAME,
        "",
        WS_CHILD | WS_VISIBLE | WS_BORDER,
        0,
        TREE_ID,
        instance,
        null_mut(),
    )?;

    let mut mdi_client: CLIENTCREATESTRUCT = zeroed();
    mdi_client.hWindowMenu = null_mut();
    mdi_client.idFirstChild = 50000;
    state.controls.mdi_client = create_child_window(
        hwnd,
        MDI_CLASS_NAME,
        "",
        WS_CHILD | WS_VISIBLE | WS_BORDER,
        0,
        MDI_ID,
        instance,
        (&mut mdi_client as *mut CLIENTCREATESTRUCT).cast(),
    )?;

    state.controls.edit = create_child_window(
        hwnd,
        EDIT_CLASS_NAME,
        "",
        WS_CHILD | WS_VISIBLE | WS_BORDER | ES_AUTOHSCROLL as u32,
        WS_EX_CLIENTEDGE,
        EDIT_ID,
        instance,
        null_mut(),
    )?;

    let previous_proc = SetWindowLongPtrW(
        state.controls.edit,
        GWLP_WNDPROC,
        edit_window_proc as *const () as isize,
    );
    if previous_proc == 0 {
        return Err(last_os_error("SetWindowLongPtrW edit subclass"));
    }
    state.edit_window_proc = Some(std::mem::transmute::<
        isize,
        unsafe extern "system" fn(HWND, u32, WPARAM, LPARAM) -> LRESULT,
    >(previous_proc));

    state.controls.status_bar = CreateWindowExW(
        0,
        STATUSCLASSNAMEW,
        null(),
        WS_CHILD | WS_VISIBLE | SBARS_SIZEGRIP,
        0,
        0,
        0,
        0,
        hwnd,
        STATUS_BAR_ID as usize as HMENU,
        instance,
        null_mut(),
    );
    if state.controls.status_bar.is_null() {
        return Err(last_os_error("CreateWindowExW status bar"));
    }

    Ok(())
}

unsafe fn create_account_menu(hwnd: HWND) -> Result<(), String> {
    let account_menu = CreatePopupMenu();
    if account_menu.is_null() {
        return Err(last_os_error("CreatePopupMenu"));
    }
    append_menu_text(account_menu, MF_STRING, MENU_ID_LOGIN, "Login")?;
    append_menu_text(
        account_menu,
        MF_STRING,
        MENU_ID_RESTORE_SAVED_SESSION,
        "Restore Saved Session",
    )?;
    append_menu_text(account_menu, MF_STRING, MENU_ID_START_SYNC, "Start Sync")?;
    append_menu_text(
        account_menu,
        MF_STRING,
        MENU_ID_RECOVER_E2EE,
        "Recover E2EE...",
    )?;
    append_menu_text(account_menu, MF_STRING, MENU_ID_LOGOUT, "Logout")?;

    let frame_menu = CreateMenu();
    if frame_menu.is_null() {
        return Err(last_os_error("CreateMenu"));
    }
    append_menu_text(frame_menu, MF_POPUP, account_menu as usize, "Account")?;

    if SetMenu(hwnd, frame_menu) == 0 {
        return Err(last_os_error("SetMenu"));
    }
    if DrawMenuBar(hwnd) == 0 {
        return Err(last_os_error("DrawMenuBar"));
    }
    Ok(())
}

unsafe fn append_menu_text(
    menu: HMENU,
    flags: u32,
    item_id: usize,
    text: &str,
) -> Result<(), String> {
    let wide = wide_null(text);
    if AppendMenuW(menu, flags, item_id, wide.as_ptr()) == 0 {
        return Err(last_os_error("AppendMenuW"));
    }
    Ok(())
}

unsafe fn handle_menu_command(hwnd: HWND, wparam: WPARAM) -> bool {
    let command_id = wparam & 0xffff;
    let result = match command_id {
        MENU_ID_LOGIN => show_login_menu_dialog(hwnd).map(|handled| handled.then_some(())),
        MENU_ID_RESTORE_SAVED_SESSION => submit_restore_saved_session(state_mut(hwnd))
            .and_then(|_| refresh_status_bar(hwnd))
            .map(|_| Some(())),
        MENU_ID_START_SYNC => submit_start_sync(state_mut(hwnd)).map(|_| Some(())),
        MENU_ID_RECOVER_E2EE => {
            show_recover_e2ee_menu_dialog(hwnd).map(|handled| handled.then_some(()))
        }
        MENU_ID_LOGOUT => submit_logout(state_mut(hwnd)).map(|_| Some(())),
        _ => return false,
    };

    if let Err(message) = result {
        show_error_message(hwnd, "Account", &message);
    }

    true
}

unsafe fn handle_switchbar_command(hwnd: HWND, wparam: WPARAM) -> bool {
    let command_id = wparam & 0xffff;
    if !(SWITCHBAR_COMMAND_FIRST..=SWITCHBAR_COMMAND_LAST).contains(&command_id) {
        return false;
    }

    let index = command_id - SWITCHBAR_COMMAND_FIRST;
    let Some(view_id) = state_ref(hwnd).switchbar_views.get(index).cloned() else {
        return false;
    };

    if let Err(message) = activate_view(hwnd, &view_id, None) {
        show_error_message(hwnd, "Switchbar", &message);
    }

    true
}

unsafe fn show_login_menu_dialog(hwnd: HWND) -> Result<bool, String> {
    let Some(form) = show_login_dialog(hwnd)? else {
        return Ok(false);
    };
    submit_login(
        state_mut(hwnd),
        form.homeserver_url,
        form.username,
        form.password,
        form.device_display_name,
    )?;
    refresh_status_bar(hwnd)?;
    Ok(true)
}

unsafe fn show_recover_e2ee_menu_dialog(hwnd: HWND) -> Result<bool, String> {
    let Some(form) = show_recover_e2ee_dialog(hwnd)? else {
        return Ok(false);
    };
    submit_recover_e2ee(state_mut(hwnd), form.recovery_secret)?;
    refresh_status_bar(hwnd)?;
    Ok(true)
}

#[allow(clippy::too_many_arguments)]
unsafe fn create_child_window(
    parent: HWND,
    class_name: &str,
    title: &str,
    style: u32,
    ex_style: u32,
    control_id: u16,
    instance: *mut c_void,
    parameter: *mut c_void,
) -> Result<HWND, String> {
    let class_name = wide_null(class_name);
    let title = wide_null(title);
    let hwnd = CreateWindowExW(
        ex_style,
        class_name.as_ptr(),
        title.as_ptr(),
        style,
        0,
        0,
        0,
        0,
        parent,
        control_id as usize as HMENU,
        instance,
        parameter,
    );

    if hwnd.is_null() {
        return Err(last_os_error("CreateWindowExW child"));
    }

    Ok(hwnd)
}

const fn pack_size_lparam(width: i32, height: i32) -> LPARAM {
    (((height as u32) << 16) | (width as u32 & 0xffff)) as LPARAM
}

unsafe fn create_login_dialog_controls(hwnd: HWND) -> Result<(), String> {
    let instance = GetModuleHandleW(null());
    let state = login_dialog_state_mut(hwnd).expect("login dialog state must exist");

    state.homeserver_label = create_login_dialog_control(
        hwnd,
        STATIC_CLASS_NAME,
        "Homeserver URL",
        WS_CHILD | WS_VISIBLE,
        0,
        0,
        instance,
    )?;
    state.homeserver_url_edit = create_login_dialog_control(
        hwnd,
        EDIT_CLASS_NAME,
        "",
        WS_CHILD | WS_VISIBLE | WS_BORDER | WS_TABSTOP | ES_AUTOHSCROLL as u32,
        WS_EX_CLIENTEDGE,
        LOGIN_DIALOG_HOME_SERVER_ID,
        instance,
    )?;

    state.username_label = create_login_dialog_control(
        hwnd,
        STATIC_CLASS_NAME,
        "Username",
        WS_CHILD | WS_VISIBLE,
        0,
        0,
        instance,
    )?;
    state.username_edit = create_login_dialog_control(
        hwnd,
        EDIT_CLASS_NAME,
        "",
        WS_CHILD | WS_VISIBLE | WS_BORDER | WS_TABSTOP | ES_AUTOHSCROLL as u32,
        WS_EX_CLIENTEDGE,
        LOGIN_DIALOG_USERNAME_ID,
        instance,
    )?;

    state.password_label = create_login_dialog_control(
        hwnd,
        STATIC_CLASS_NAME,
        "Password",
        WS_CHILD | WS_VISIBLE,
        0,
        0,
        instance,
    )?;
    state.password_edit = create_login_dialog_control(
        hwnd,
        EDIT_CLASS_NAME,
        "",
        WS_CHILD | WS_VISIBLE | WS_BORDER | WS_TABSTOP | ES_AUTOHSCROLL as u32 | ES_PASSWORD as u32,
        WS_EX_CLIENTEDGE,
        LOGIN_DIALOG_PASSWORD_ID,
        instance,
    )?;

    state.device_display_name_label = create_login_dialog_control(
        hwnd,
        STATIC_CLASS_NAME,
        "Device Display Name",
        WS_CHILD | WS_VISIBLE,
        0,
        0,
        instance,
    )?;
    state.device_display_name_edit = create_login_dialog_control(
        hwnd,
        EDIT_CLASS_NAME,
        "",
        WS_CHILD | WS_VISIBLE | WS_BORDER | WS_TABSTOP | ES_AUTOHSCROLL as u32,
        WS_EX_CLIENTEDGE,
        LOGIN_DIALOG_DEVICE_ID,
        instance,
    )?;

    state.ok_button = create_login_dialog_control(
        hwnd,
        BUTTON_CLASS_NAME,
        "OK",
        WS_CHILD | WS_VISIBLE | WS_TABSTOP | BS_DEFPUSHBUTTON as u32,
        0,
        LOGIN_DIALOG_OK_ID as u16,
        instance,
    )?;
    state.cancel_button = create_login_dialog_control(
        hwnd,
        BUTTON_CLASS_NAME,
        "Cancel",
        WS_CHILD | WS_VISIBLE | WS_TABSTOP | BS_PUSHBUTTON as u32,
        0,
        LOGIN_DIALOG_CANCEL_ID as u16,
        instance,
    )?;

    layout_login_dialog(hwnd);
    Ok(())
}

unsafe fn create_recover_e2ee_dialog_controls(hwnd: HWND) -> Result<(), String> {
    let instance = GetModuleHandleW(null());
    let state = recover_e2ee_dialog_state_mut(hwnd).expect("recover e2ee dialog state must exist");

    state.recovery_secret_label = create_login_dialog_control(
        hwnd,
        STATIC_CLASS_NAME,
        "Recovery key or passphrase",
        WS_CHILD | WS_VISIBLE,
        0,
        0,
        instance,
    )?;
    state.recovery_secret_edit = create_login_dialog_control(
        hwnd,
        EDIT_CLASS_NAME,
        "",
        WS_CHILD | WS_VISIBLE | WS_BORDER | WS_TABSTOP | ES_AUTOHSCROLL as u32 | ES_PASSWORD as u32,
        WS_EX_CLIENTEDGE,
        RECOVER_E2EE_DIALOG_SECRET_ID,
        instance,
    )?;
    state.ok_button = create_login_dialog_control(
        hwnd,
        BUTTON_CLASS_NAME,
        "OK",
        WS_CHILD | WS_VISIBLE | WS_TABSTOP | BS_DEFPUSHBUTTON as u32,
        0,
        RECOVER_E2EE_DIALOG_OK_ID as u16,
        instance,
    )?;
    state.cancel_button = create_login_dialog_control(
        hwnd,
        BUTTON_CLASS_NAME,
        "Cancel",
        WS_CHILD | WS_VISIBLE | WS_TABSTOP | BS_PUSHBUTTON as u32,
        0,
        RECOVER_E2EE_DIALOG_CANCEL_ID as u16,
        instance,
    )?;

    layout_recover_e2ee_dialog(hwnd);
    Ok(())
}

unsafe fn create_login_dialog_control(
    parent: HWND,
    class_name: &str,
    title: &str,
    style: u32,
    ex_style: u32,
    control_id: u16,
    instance: *mut c_void,
) -> Result<HWND, String> {
    create_child_window(
        parent,
        class_name,
        title,
        style,
        ex_style,
        control_id,
        instance,
        null_mut(),
    )
}

unsafe fn layout_login_dialog(hwnd: HWND) {
    let mut rect = RECT::default();
    GetClientRect(hwnd, &mut rect);
    let width = rect.right - rect.left;
    let label_x = 16;
    let input_x = 16;
    let input_width = (width - 32).max(0);
    let label_width = input_width;
    let label_height = 18;
    let input_height = 24;
    let spacing = 8;
    let block_height = label_height + spacing + input_height + 12;
    let button_width = 96;
    let button_height = 28;

    let Some(state) = login_dialog_state(hwnd) else {
        return;
    };
    let labels = [
        state.homeserver_label,
        state.username_label,
        state.password_label,
        state.device_display_name_label,
    ];
    for (index, hwnd_control) in labels.iter().enumerate() {
        let y = 16 + (index as i32 * block_height);
        MoveWindow(*hwnd_control, label_x, y, label_width, label_height, 1);
    }
    MoveWindow(
        state.homeserver_url_edit,
        input_x,
        16 + label_height + spacing,
        input_width,
        input_height,
        1,
    );
    MoveWindow(
        state.username_edit,
        input_x,
        16 + block_height + label_height + spacing,
        input_width,
        input_height,
        1,
    );
    MoveWindow(
        state.password_edit,
        input_x,
        16 + (block_height * 2) + label_height + spacing,
        input_width,
        input_height,
        1,
    );
    MoveWindow(
        state.device_display_name_edit,
        input_x,
        16 + (block_height * 3) + label_height + spacing,
        input_width,
        input_height,
        1,
    );

    let button_y = 16 + (block_height * 4);
    let cancel_x = width - 16 - button_width;
    let ok_x = cancel_x - 12 - button_width;
    MoveWindow(
        state.ok_button,
        ok_x,
        button_y,
        button_width,
        button_height,
        1,
    );
    MoveWindow(
        state.cancel_button,
        cancel_x,
        button_y,
        button_width,
        button_height,
        1,
    );
}

unsafe fn show_login_dialog(owner: HWND) -> Result<Option<LoginForm>, String> {
    let instance = GetModuleHandleW(null());
    let result = Arc::new(Mutex::new(None));
    let dialog_state = Box::new(LoginDialogState {
        result: Arc::clone(&result),
        homeserver_label: null_mut(),
        homeserver_url_edit: null_mut(),
        username_label: null_mut(),
        username_edit: null_mut(),
        password_label: null_mut(),
        password_edit: null_mut(),
        device_display_name_label: null_mut(),
        device_display_name_edit: null_mut(),
        ok_button: null_mut(),
        cancel_button: null_mut(),
    });
    let dialog_state_ptr = Box::into_raw(dialog_state);
    let class_name = wide_null(LOGIN_DIALOG_CLASS_NAME);
    let title = wide_null("Login");
    let dialog = CreateWindowExW(
        0,
        class_name.as_ptr(),
        title.as_ptr(),
        WS_OVERLAPPED | WS_CAPTION | WS_SYSMENU | WS_VISIBLE,
        CW_USEDEFAULT,
        CW_USEDEFAULT,
        420,
        320,
        owner,
        null_mut(),
        instance,
        dialog_state_ptr.cast(),
    );
    if dialog.is_null() {
        drop(Box::from_raw(dialog_state_ptr));
        return Err(last_os_error("CreateWindowExW login dialog"));
    }

    EnableWindow(owner, 0);
    ShowWindow(dialog, SW_SHOW);

    let mut message = MSG::default();
    while IsWindow(dialog) != 0 {
        let get_result = GetMessageW(&mut message, null_mut(), 0, 0);
        if get_result <= 0 {
            break;
        }
        let frame = frame_window_for(message.hwnd);
        if !frame.is_null() {
            let mdi_client = state_ref(frame).controls.mdi_client;
            if !mdi_client.is_null() && TranslateMDISysAccel(mdi_client, &message) != 0 {
                continue;
            }
        }
        TranslateMessage(&message);
        DispatchMessageW(&message);
    }

    EnableWindow(owner, 1);
    Ok(result
        .lock()
        .expect("login dialog result mutex poisoned")
        .clone())
}

unsafe fn layout_recover_e2ee_dialog(hwnd: HWND) {
    let mut rect = RECT::default();
    GetClientRect(hwnd, &mut rect);
    let width = rect.right - rect.left;
    let label_x = 16;
    let input_x = 16;
    let input_width = (width - 32).max(0);
    let label_width = input_width;
    let label_height = 18;
    let input_height = 24;
    let spacing = 8;
    let block_height = label_height + spacing + input_height + 12;
    let button_width = 96;
    let button_height = 28;

    let Some(state) = recover_e2ee_dialog_state(hwnd) else {
        return;
    };

    MoveWindow(
        state.recovery_secret_label,
        label_x,
        16,
        label_width,
        label_height,
        1,
    );
    MoveWindow(
        state.recovery_secret_edit,
        input_x,
        16 + label_height + spacing,
        input_width,
        input_height,
        1,
    );

    let button_y = 16 + block_height;
    let cancel_x = width - 16 - button_width;
    let ok_x = cancel_x - 12 - button_width;
    MoveWindow(
        state.ok_button,
        ok_x,
        button_y,
        button_width,
        button_height,
        1,
    );
    MoveWindow(
        state.cancel_button,
        cancel_x,
        button_y,
        button_width,
        button_height,
        1,
    );
}

unsafe fn show_recover_e2ee_dialog(owner: HWND) -> Result<Option<RecoverE2eeForm>, String> {
    let instance = GetModuleHandleW(null());
    let result = Arc::new(Mutex::new(None));
    let dialog_state = Box::new(RecoverE2eeDialogState {
        result: Arc::clone(&result),
        recovery_secret_label: null_mut(),
        recovery_secret_edit: null_mut(),
        ok_button: null_mut(),
        cancel_button: null_mut(),
    });
    let dialog_state_ptr = Box::into_raw(dialog_state);
    let class_name = wide_null(RECOVER_E2EE_DIALOG_CLASS_NAME);
    let title = wide_null("Recover E2EE");
    let dialog = CreateWindowExW(
        0,
        class_name.as_ptr(),
        title.as_ptr(),
        WS_OVERLAPPED | WS_CAPTION | WS_SYSMENU | WS_VISIBLE,
        CW_USEDEFAULT,
        CW_USEDEFAULT,
        420,
        170,
        owner,
        null_mut(),
        instance,
        dialog_state_ptr.cast(),
    );
    if dialog.is_null() {
        drop(Box::from_raw(dialog_state_ptr));
        return Err(last_os_error("CreateWindowExW recover e2ee dialog"));
    }

    EnableWindow(owner, 0);
    ShowWindow(dialog, SW_SHOW);

    let mut message = MSG::default();
    while IsWindow(dialog) != 0 {
        let get_result = GetMessageW(&mut message, null_mut(), 0, 0);
        if get_result <= 0 {
            break;
        }
        let frame = frame_window_for(message.hwnd);
        if !frame.is_null() {
            let mdi_client = state_ref(frame).controls.mdi_client;
            if !mdi_client.is_null() && TranslateMDISysAccel(mdi_client, &message) != 0 {
                continue;
            }
        }
        TranslateMessage(&message);
        DispatchMessageW(&message);
    }

    EnableWindow(owner, 1);
    Ok(result
        .lock()
        .expect("recover e2ee dialog result mutex poisoned")
        .clone())
}

unsafe fn login_dialog_state(hwnd: HWND) -> Option<&'static LoginDialogState> {
    let state = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *const LoginDialogState;
    (!state.is_null()).then_some(&*state)
}

unsafe fn login_dialog_state_mut(hwnd: HWND) -> Option<&'static mut LoginDialogState> {
    let state = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut LoginDialogState;
    (!state.is_null()).then_some(&mut *state)
}

unsafe fn recover_e2ee_dialog_state(hwnd: HWND) -> Option<&'static RecoverE2eeDialogState> {
    let state = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *const RecoverE2eeDialogState;
    (!state.is_null()).then_some(&*state)
}

unsafe fn recover_e2ee_dialog_state_mut(hwnd: HWND) -> Option<&'static mut RecoverE2eeDialogState> {
    let state = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut RecoverE2eeDialogState;
    (!state.is_null()).then_some(&mut *state)
}

unsafe fn clear_recover_e2ee_dialog_secret(hwnd: HWND) {
    if let Some(state) = recover_e2ee_dialog_state(hwnd) {
        set_window_text(state.recovery_secret_edit, "");
    }
}

unsafe fn layout_controls(hwnd: HWND) {
    let state = state_mut(hwnd);
    if state.controls.tree.is_null()
        || state.controls.mdi_client.is_null()
        || state.controls.edit.is_null()
        || state.controls.status_bar.is_null()
        || state.controls.switchbar.is_null()
    {
        return;
    }

    let mut rect = RECT::default();
    GetClientRect(hwnd, &mut rect);

    let width = rect.right - rect.left;
    let height = rect.bottom - rect.top;
    let tree_width = TREE_WIDTH.min(width);
    let switchbar_height = SWITCHBAR_HEIGHT.min(height);
    let status_bar_height = STATUS_BAR_HEIGHT.min((height - switchbar_height).max(0));
    let remaining_height = (height - switchbar_height - status_bar_height).max(0);
    let edit_height = EDIT_HEIGHT.min(remaining_height);
    let mdi_width = (width - tree_width).max(0);
    let mdi_height = (height - switchbar_height - edit_height - status_bar_height).max(0);
    let compose_y = switchbar_height + mdi_height;
    let status_bar_y = compose_y + edit_height;

    MoveWindow(state.controls.switchbar, 0, 0, width, switchbar_height, 1);
    MoveWindow(
        state.controls.tree,
        0,
        switchbar_height,
        tree_width,
        mdi_height,
        1,
    );
    MoveWindow(
        state.controls.mdi_client,
        tree_width,
        switchbar_height,
        mdi_width,
        mdi_height,
        1,
    );
    MoveWindow(state.controls.edit, 0, compose_y, width, edit_height, 1);
    MoveWindow(
        state.controls.status_bar,
        0,
        status_bar_y,
        width,
        status_bar_height,
        1,
    );
}

unsafe fn handle_core_events(hwnd: HWND) {
    if let Err(message) = drain_core_events_and_refresh(hwnd) {
        state_mut(hwnd).exit_code.store(1, Ordering::Relaxed);
        eprintln!("{message}");
        DestroyWindow(hwnd);
    }
}

fn apply_core_event(state: &mut WindowState, event: Event) -> Result<(), String> {
    match event {
        Event::Started => {
            if matches!(
                state.model.lifecycle_state,
                LifecycleState::Offline | LifecycleState::CoreStarted
            ) {
                state.model.status = "core started".to_owned();
                state.model.lifecycle_state = LifecycleState::CoreStarted;
            }
        }
        Event::LoggedIn { session } => {
            if let Some(homeserver_url) = state.pending_login_homeserver_url.clone() {
                state.current_session_homeserver_url = Some(homeserver_url.clone());
                save_session(&homeserver_url, &session)?;
                state.pending_login_homeserver_url = None;
            } else if !state.synthetic_load_in_progress {
                return Err("received LoggedIn event without a pending homeserver URL".to_owned());
            }
            state.model.status = "authenticated".to_owned();
            state.model.lifecycle_state = LifecycleState::Authenticated;
        }
        Event::SessionUpdated { session } => {
            let homeserver_url = state
                .current_session_homeserver_url
                .as_ref()
                .expect("session updates require an active homeserver URL");
            save_session(homeserver_url, &session)?;
        }
        Event::RestoredSession => {
            state.model.status = "restored saved session".to_owned();
            state.model.lifecycle_state = LifecycleState::Authenticated;
        }
        Event::SyncedRooms(rooms) => {
            state.model.status = format!("synced {} room(s)", rooms.len());
            state.model.lifecycle_state = LifecycleState::Ready;
            if state
                .model
                .selected_room
                .as_deref()
                .is_some_and(|room_id| navigable_room_by_id(&rooms, room_id).is_none())
            {
                state.model.selected_room = None;
            }
            state.room_activity.retain(|view_id| {
                view_id_is_valid_for_rooms(&rooms, &state.model.room_views, view_id)
            });
            let tracked_room_activity = state.room_activity.clone();
            state
                .room_activity_read_targets
                .retain(|view_id, _| tracked_room_activity.contains(view_id));
            for (room_id, room_view) in &mut state.model.room_views {
                if sendable_room_by_id(&rooms, room_id).is_none() {
                    room_view.typing_user_ids.clear();
                    room_view.members.clear();
                }
            }
            state
                .model
                .room_read_states
                .retain(|room_id, _| sendable_room_by_id(&rooms, room_id).is_some());
            state.model.rooms = rooms;
        }
        Event::RoomActivity(activity) => apply_room_activity_event(state, *activity),
        Event::RoomActivityUpdate(update) => apply_room_activity_update_event(state, update),
        Event::RoomHistoryBackfill(backfill) => apply_room_history_backfill_event(state, backfill),
        Event::RoomImageMedia(media) => apply_room_image_media_event(state, media),
        Event::RoomTyping(typing) => apply_room_typing_event(state, typing),
        Event::RoomMembers(members) => apply_room_members_event(state, members),
        Event::RoomReadState(read_state) => apply_room_read_state_event(state, read_state),
        Event::E2eeStatus(status) => {
            state.model.last_e2ee_status = Some(status);
            state.model.status = "e2ee status updated".to_owned();
        }
        Event::E2eeRecoverySucceeded(_) => {
            state.model.status = "e2ee recovery complete".to_owned();
        }
        Event::VerificationRequest(request) => {
            let event = verification_state_event_from_request(request);
            state.model.status = verification_status_message(&event);
            state.model.last_verification = Some(event);
        }
        Event::VerificationState(event) => {
            state.model.status = verification_status_message(&event);
            state.model.last_verification = Some(event);
        }
        Event::LoggedOut => {
            delete_saved_session()?;
            state.pending_login_homeserver_url = None;
            state.current_session_homeserver_url = None;
            state.model.status = "logged out".to_owned();
            state.model.lifecycle_state = LifecycleState::LoggedOut;
            state.model.rooms.clear();
            state.model.selected_room = None;
            state.model.room_views.clear();
            state.model.last_activity = None;
            state.model.last_e2ee_status = None;
            state.model.last_verification = None;
            state.model.last_error = None;
            state.model.room_read_states.clear();
            state.room_activity.clear();
            state.room_activity_read_targets.clear();
            state.find_states.clear();
            state.decoded_images.clear();
        }
        Event::Stopped => {
            state.model.status = "core stopped".to_owned();
            state.model.lifecycle_state = LifecycleState::Stopped;
        }
        Event::SyncStarted => {
            state.model.status = "continuous sync started".to_owned();
            state.model.lifecycle_state = LifecycleState::Syncing;
        }
        Event::Error(error) => {
            if error.operation == Operation::BackfillRoomHistory {
                apply_backfill_room_history_error(state, error.message.clone());
            }
            if !preserves_submission_status(&state.model.status, error.operation) {
                state.model.status = format!("error during {}", operation_name(error.operation));
            }
            state.model.lifecycle_state = LifecycleState::Error;
            state.model.last_error = Some(error);
        }
    }
    Ok(())
}

fn preserves_submission_status(status: &str, operation: Operation) -> bool {
    match operation {
        Operation::JoinRoom => status.starts_with("join submitted: "),
        Operation::AcceptRoomInvite => status.starts_with("accept submitted: "),
        Operation::CreateDirectRoom => status.starts_with("query submitted: "),
        Operation::LeaveRoom => status.starts_with("part submitted: "),
        Operation::RejectRoomInvite => status.starts_with("reject submitted: "),
        Operation::SetRoomTopic => status.starts_with("topic change submitted for "),
        _ => false,
    }
}

fn timeline_message_send_state(send_state: &RoomMessageSendState) -> TimelineMessageSendState {
    match send_state {
        RoomMessageSendState::Pending => TimelineMessageSendState::Pending,
        RoomMessageSendState::Sent => TimelineMessageSendState::Sent,
        RoomMessageSendState::Failed { .. } => TimelineMessageSendState::Failed,
    }
}

fn append_room_label_markers(label: &mut String, room: &RoomSummary) {
    if is_invited_room(room) {
        label.push_str(" [invite]");
    }
    if room.is_direct {
        if room.direct_user_ids.is_empty() {
            label.push_str(" [query]");
        } else {
            label.push_str(" [query: ");
            label.push_str(&room.direct_user_ids.join(", "));
            label.push(']');
        }
    }
    if room.encrypted {
        label.push_str(" [e2ee]");
    }
}

fn append_room_read_state_tokens(label: &mut String, read_state: Option<&RoomReadState>) {
    let Some(read_state) = read_state else {
        return;
    };

    let mut tokens = Vec::new();
    if read_state.num_mentions > 0 {
        tokens.push(format!("@{}", read_state.num_mentions));
    }
    if read_state.num_notifications > 0 {
        tokens.push(format!("!{}", read_state.num_notifications));
    }
    if read_state.num_unread > 0 {
        tokens.push(format!("*{}", read_state.num_unread));
    }
    if read_state.marked_unread {
        tokens.push("M".to_owned());
    }
    if tokens.is_empty() {
        return;
    }

    label.push_str(" [");
    label.push_str(&tokens.join(" "));
    label.push(']');
}

fn build_room_timeline_header(room: &RoomSummary) -> TimelineRow {
    let mut header = format!("Room: {}", room.display_name);
    append_room_label_markers(&mut header, room);
    header.push_str(&format!(" ({})", room.room_id));
    TimelineRow::full_width(TimelineRowKind::Header, header.clone()).with_identity(
        TimelineRowIdentity::synthetic(TimelineSyntheticRowLabel::Header, &header),
    )
}

fn build_invite_timeline_rows(room: &RoomSummary) -> Vec<TimelineRow> {
    let invite_header = format!("Invitation to {}", room.display_name);
    vec![
        TimelineRow::full_width(TimelineRowKind::Header, invite_header.clone()).with_identity(
            TimelineRowIdentity::synthetic(TimelineSyntheticRowLabel::Header, &invite_header),
        ),
        TimelineRow::full_width(
            TimelineRowKind::Plain,
            "Use /accept or /reject to respond.".to_owned(),
        )
        .with_identity(TimelineRowIdentity::synthetic(
            TimelineSyntheticRowLabel::Context,
            "Use /accept or /reject to respond.",
        )),
    ]
}

fn push_room_context_rows(
    rows: &mut Vec<TimelineRow>,
    room: &RoomSummary,
    room_view: Option<&RoomViewModel>,
) {
    rows.push(build_room_timeline_header(room));

    if let Some(topic) = room_view.and_then(|view| view.topic.as_deref()) {
        let logical_text = format!("Topic: {topic}");
        rows.push(
            TimelineRow::full_width(TimelineRowKind::Topic, logical_text.clone()).with_identity(
                TimelineRowIdentity::synthetic(TimelineSyntheticRowLabel::Topic, &logical_text),
            ),
        );
    }

    if let Some(room_view) = room_view
        && !room_view.typing_user_ids.is_empty()
    {
        let users = room_view.typing_user_ids.join(", ");
        let verb = if room_view.typing_user_ids.len() == 1 {
            "is"
        } else {
            "are"
        };
        let logical_text = format!("* {users} {verb} typing");
        rows.push(
            TimelineRow::full_width(TimelineRowKind::Typing, logical_text.clone()).with_identity(
                TimelineRowIdentity::synthetic(TimelineSyntheticRowLabel::Typing, &logical_text),
            ),
        );
    }
}

fn push_room_history_backfill_row(rows: &mut Vec<TimelineRow>, history: &RoomHistoryBackfillState) {
    let text = match &history.status {
        RoomHistoryBackfillStatus::Idle => return,
        RoomHistoryBackfillStatus::Loading { .. } => "* Loading older history...".to_owned(),
        RoomHistoryBackfillStatus::Failed { message, .. } => {
            format!("* Unable to load older history: {message}. Scroll to top to retry.")
        }
        RoomHistoryBackfillStatus::Exhausted => "* No older history available.".to_owned(),
    };
    rows.push(
        TimelineRow::full_width(TimelineRowKind::Plain, text.clone()).with_identity(
            TimelineRowIdentity::synthetic(TimelineSyntheticRowLabel::Backfill, &text),
        ),
    );
}

fn push_room_activity_row(
    rows: &mut Vec<TimelineRow>,
    room_view: Option<&RoomViewModel>,
    activity: &RoomActivity,
) {
    match &activity.kind {
        RoomActivityKind::Message(message) => {
            let timestamp = format!("[{}]", format_timestamp_hhmm_utc(activity.timestamp_ms));
            let suffix = match &message.send_state {
                RoomMessageSendState::Pending => " (sending)",
                RoomMessageSendState::Sent => "",
                RoomMessageSendState::Failed { .. } => " (failed)",
            };
            let (logical_text, sender_prefix, body) = match message.presentation {
                RoomMessagePresentationKind::Text => (
                    format!("{timestamp} {}: {}{suffix}", message.sender, message.body),
                    format!("{}:", message.sender),
                    format!("{}{}", message.body, suffix),
                ),
                RoomMessagePresentationKind::Notice => (
                    format!("{timestamp} -{}- {}{suffix}", message.sender, message.body),
                    format!("-{}-", message.sender),
                    format!("{}{}", message.body, suffix),
                ),
                RoomMessagePresentationKind::Emote => (
                    format!("* {timestamp} {} {}{suffix}", message.sender, message.body),
                    format!("* {}", message.sender),
                    format!("{}{}", message.body, suffix),
                ),
            };
            rows.push(
                TimelineRow::log(
                    TimelineRowKind::Message {
                        presentation: message.presentation,
                        send_state: timeline_message_send_state(&message.send_state),
                    },
                    logical_text,
                    timestamp,
                    sender_prefix,
                    body,
                )
                .with_identity(TimelineRowIdentity::activity_primary(&activity.activity_id)),
            );
        }
        RoomActivityKind::Image(message) => {
            let display_name = message.filename.as_deref().unwrap_or(message.body.as_str());
            let metadata_suffix = format_room_image_metadata_suffix(message.info.as_ref());
            let format = timeline_thumbnail_format();
            let image_state = room_view.map(|view| {
                view.projected_image_state(&activity.activity_id, &message.event_id, &format)
            });
            let timestamp = format!("[{}]", format_timestamp_hhmm_utc(activity.timestamp_ms));
            let body = format!("sent image: {display_name}{metadata_suffix}");
            let mut row = TimelineRow::log(
                TimelineRowKind::Image,
                format!("* {timestamp} {} {body}", message.sender),
                timestamp,
                format!("* {}", message.sender),
                body,
            )
            .with_identity(TimelineRowIdentity::activity_primary(&activity.activity_id));
            row.image_state = image_state;
            rows.push(row);
        }
        RoomActivityKind::File(message) => {
            let display_name = message.filename.as_deref().unwrap_or(message.body.as_str());
            let metadata_suffix = format_room_file_metadata_suffix(message.info.as_ref());
            let timestamp = format!("[{}]", format_timestamp_hhmm_utc(activity.timestamp_ms));
            let body = format!("sent file: {display_name}{metadata_suffix}");
            rows.push(
                TimelineRow::log(
                    TimelineRowKind::File,
                    format!("* {timestamp} {} {body}", message.sender),
                    timestamp,
                    format!("* {}", message.sender),
                    body,
                )
                .with_identity(TimelineRowIdentity::activity_primary(&activity.activity_id)),
            );
        }
        RoomActivityKind::Audio(message) => {
            let display_name = message.filename.as_deref().unwrap_or(message.body.as_str());
            let metadata_suffix = format_room_audio_metadata_suffix(message.info.as_ref());
            let timestamp = format!("[{}]", format_timestamp_hhmm_utc(activity.timestamp_ms));
            let body = format!("sent audio: {display_name}{metadata_suffix}");
            rows.push(
                TimelineRow::log(
                    TimelineRowKind::Audio,
                    format!("* {timestamp} {} {body}", message.sender),
                    timestamp,
                    format!("* {}", message.sender),
                    body,
                )
                .with_identity(TimelineRowIdentity::activity_primary(&activity.activity_id)),
            );
        }
        RoomActivityKind::UnsupportedMessage(message) => {
            let timestamp = format!("[{}]", format_timestamp_hhmm_utc(activity.timestamp_ms));
            let body = format!("sent unsupported {}: {}", message.msgtype, message.body);
            rows.push(
                TimelineRow::log(
                    TimelineRowKind::Unsupported,
                    format!("* {timestamp} {} {body}", message.sender),
                    timestamp,
                    format!("* {}", message.sender),
                    body,
                )
                .with_identity(TimelineRowIdentity::activity_primary(&activity.activity_id)),
            );
        }
        RoomActivityKind::StateNotice(notice) => match &notice.content {
            RoomStateNoticeContent::Topic { topic } => {
                let timestamp = format!("[{}]", format_timestamp_hhmm_utc(activity.timestamp_ms));
                let body = format!("{} set topic: {topic}", notice.sender);
                rows.push(
                    TimelineRow::log(
                        TimelineRowKind::StateNotice,
                        format!("* {timestamp} {body}"),
                        timestamp,
                        "*".to_owned(),
                        body,
                    )
                    .with_identity(TimelineRowIdentity::activity_primary(&activity.activity_id)),
                );
            }
            RoomStateNoticeContent::Member {
                membership,
                displayname,
            } => {
                let timestamp = format!("[{}]", format_timestamp_hhmm_utc(activity.timestamp_ms));
                let body = match membership.as_str() {
                    "join" => {
                        let member = displayname.as_deref().unwrap_or(notice.state_key.as_str());
                        format!("{member} joined")
                    }
                    "leave" => {
                        let member = displayname.as_deref().unwrap_or(notice.state_key.as_str());
                        format!("{member} left")
                    }
                    membership => format!("{} membership: {membership}", notice.state_key),
                };
                rows.push(
                    TimelineRow::log(
                        TimelineRowKind::StateNotice,
                        format!("* {timestamp} {body}"),
                        timestamp,
                        "*".to_owned(),
                        body,
                    )
                    .with_identity(TimelineRowIdentity::activity_primary(&activity.activity_id)),
                );
            }
        },
        RoomActivityKind::Redacted(_) => {
            let timestamp = format!("[{}]", format_timestamp_hhmm_utc(activity.timestamp_ms));
            let body = "message redacted".to_owned();
            rows.push(
                TimelineRow::log(
                    TimelineRowKind::StateNotice,
                    format!("* {timestamp} {body}"),
                    timestamp,
                    "*".to_owned(),
                    body,
                )
                .with_identity(TimelineRowIdentity::activity_primary(&activity.activity_id)),
            );
        }
    }

    let Some(room_view) = room_view else {
        return;
    };
    let Some(event_id) = room_activity_event_id(activity) else {
        return;
    };
    if matches!(activity.kind, RoomActivityKind::Redacted(_)) {
        return;
    }
    let summaries = room_view.reaction_summaries_for_event(event_id);
    if summaries.is_empty() {
        return;
    }
    let summary_text = summaries
        .iter()
        .map(|summary| format!("{} x{}", summary.key, summary.count))
        .collect::<Vec<_>>()
        .join("  ");
    let logical_text = format!("    reactions: {summary_text}");
    rows.push(
        TimelineRow::full_width(TimelineRowKind::ReactionSummary, logical_text).with_identity(
            TimelineRowIdentity::activity_reaction_summary(&activity.activity_id),
        ),
    );
}

fn build_room_timeline_rows(model: &UiModel, room_id: &str) -> Vec<TimelineRow> {
    let room = room_by_id(&model.rooms, room_id).expect("room view must target an existing room");
    if is_invited_room(room) {
        return build_invite_timeline_rows(room);
    }
    let mut rows = Vec::new();
    let room_view = model.room_views.get(room_id);
    push_room_context_rows(&mut rows, room, room_view);

    if let Some(room_view) = room_view {
        push_room_history_backfill_row(&mut rows, &room_view.history);
        for activity in &room_view.activities {
            push_room_activity_row(&mut rows, Some(room_view), activity);
        }
    }

    rows
}

fn build_thread_timeline_rows(
    model: &UiModel,
    room_id: &str,
    thread_root_event_id: &str,
) -> Vec<TimelineRow> {
    let room = room_by_id(&model.rooms, room_id).expect("thread view must target an existing room");
    let room_view = model
        .room_views
        .get(room_id)
        .expect("thread view must target an existing room view");
    let mut rows = Vec::new();
    push_room_context_rows(&mut rows, room, Some(room_view));
    for activity in &room_view.activities {
        if room_activity_belongs_to_thread(activity, thread_root_event_id) {
            push_room_activity_row(&mut rows, Some(room_view), activity);
        }
    }
    rows
}

fn project_timeline_rows(rows: &[TimelineRow]) -> Vec<String> {
    rows.iter().map(|row| row.logical_text.clone()).collect()
}

#[cfg_attr(not(test), allow(dead_code))]
fn render_room_lines(model: &UiModel, room_id: &str) -> Vec<String> {
    project_timeline_rows(&build_room_timeline_rows(model, room_id))
}

fn format_room_image_metadata_suffix(info: Option<&RoomImageInfo>) -> String {
    let Some(info) = info else {
        return String::new();
    };

    let mut parts = Vec::new();
    if let Some(mimetype) = info.mimetype.as_deref() {
        parts.push(mimetype.to_owned());
    }
    if let (Some(width), Some(height)) = (info.width, info.height) {
        parts.push(format!("{width}x{height}"));
    }
    if let Some(size) = info.size {
        parts.push(format!("{size} bytes"));
    }

    if parts.is_empty() {
        String::new()
    } else {
        format!(" ({})", parts.join(", "))
    }
}

fn format_room_file_metadata_suffix(info: Option<&RoomFileInfo>) -> String {
    let Some(info) = info else {
        return String::new();
    };

    let mut parts = Vec::new();
    if let Some(mimetype) = info.mimetype.as_deref() {
        parts.push(mimetype.to_owned());
    }
    if let Some(size) = info.size {
        parts.push(format!("{size} bytes"));
    }

    if parts.is_empty() {
        String::new()
    } else {
        format!(" ({})", parts.join(", "))
    }
}

fn format_room_audio_metadata_suffix(info: Option<&RoomAudioInfo>) -> String {
    let Some(info) = info else {
        return String::new();
    };

    let mut parts = Vec::new();
    if let Some(mimetype) = info.mimetype.as_deref() {
        parts.push(mimetype.to_owned());
    }
    if let Some(size) = info.size {
        parts.push(format!("{size} bytes"));
    }
    if let Some(duration_ms) = info.duration_ms {
        parts.push(format!("{duration_ms} ms"));
    }

    if parts.is_empty() {
        String::new()
    } else {
        format!(" ({})", parts.join(", "))
    }
}

fn format_timestamp_hhmm_utc(timestamp_ms: u64) -> String {
    const MINUTES_PER_DAY: u64 = 24 * 60;

    let total_minutes = timestamp_ms / 1000 / 60;
    let minutes_of_day = total_minutes % MINUTES_PER_DAY;
    let hours = minutes_of_day / 60;
    let minutes = minutes_of_day % 60;
    format!("{hours:02}:{minutes:02}")
}

fn render_status_lines(model: &UiModel) -> Vec<String> {
    let mut lines = vec![format!("status: {}", model.status)];

    if let Some(event) = &model.last_verification {
        lines.push(String::new());
        lines.push("verification".to_owned());
        lines.push(format!("state: {}", verification_state_label(&event.state)));
        lines.push(format!("flow_id: {}", event.flow_id));
        lines.push(format!("from_user: {}", event.user_id));
        match &event.state {
            VerificationState::SasReady { emojis, decimals } => {
                lines.push(format!("emoji: {}", format_verification_emojis(emojis)));
                lines.push(format!(
                    "decimals: {}-{}-{}",
                    decimals.first, decimals.second, decimals.third
                ));
            }
            VerificationState::Done => {
                lines.push("result: verified".to_owned());
            }
            VerificationState::Cancelled { reason, code, .. } => {
                lines.push(format!("reason: {reason}"));
                lines.push(format!("code: {code}"));
            }
            VerificationState::Requested
            | VerificationState::Accepted
            | VerificationState::Ready
            | VerificationState::Confirmed => {}
        }
        if let Some(actions) = verification_actions_text(&event.state) {
            lines.push(format!("actions: {actions}"));
        }
    }

    if let Some(status) = &model.last_e2ee_status {
        lines.push(String::new());
        lines.push("e2ee".to_owned());
        lines.push(format!(
            "recovery: {}",
            e2ee_recovery_state_label(status.recovery)
        ));
        lines.push(format!(
            "backup: {}",
            e2ee_backup_state_label(status.backup.state)
        ));
        lines.push(format!(
            "backup_enabled_local: {}",
            if status.backup.enabled_locally {
                "true"
            } else {
                "false"
            }
        ));
        lines.push(format!(
            "backup_exists_on_server: {}",
            e2ee_backup_exists_label(status.backup.exists_on_server)
        ));
        lines.push(format!(
            "cross_signing: {}",
            e2ee_cross_signing_label(status.cross_signing.as_ref())
        ));
        lines.push("actions: Account > Recover E2EE..., /e2ee refresh".to_owned());
    }

    if let Some(error) = &model.last_error {
        lines.push(String::new());
        lines.push("error".to_owned());
        lines.push(format!("operation: {}", operation_name(error.operation)));
        lines.push(format!("message: {}", error.message));
    }

    lines
}

fn frame_status_text(state: &WindowState) -> String {
    let model = &state.model;
    let mut text = match &state.active_view {
        ViewId::Room(room_id) => room_by_id(&model.rooms, room_id)
            .filter(|room| is_invited_room(room))
            .map(|room| format!("invited: {} ({})", room.display_name, room.room_id))
            .unwrap_or_else(|| model.status.clone()),
        _ => model.status.clone(),
    };

    if let Some(event) = &model.last_verification {
        text.push_str(" | verification: ");
        text.push_str(verification_state_label(&event.state));
        text.push(' ');
        text.push_str(&event.flow_id);
        text.push_str(" from ");
        text.push_str(&event.user_id);
    }

    if let Some(status) = &model.last_e2ee_status {
        text.push_str(" | e2ee: recovery ");
        text.push_str(e2ee_recovery_state_label(status.recovery));
        text.push_str(", backup ");
        text.push_str(e2ee_backup_state_label(status.backup.state));
    }

    if let Some(error) = &model.last_error {
        text.push_str(" | error: ");
        text.push_str(operation_name(error.operation));
        text.push_str(": ");
        text.push_str(&error.message);
    }

    if let Some(feedback) = active_view_find_feedback(state) {
        text.push_str(" | ");
        text.push_str(feedback);
    }

    text
}

unsafe fn refresh_status_bar(hwnd: HWND) -> Result<(), String> {
    let status_bar = state_ref(hwnd).controls.status_bar;
    if status_bar.is_null() {
        return Err("status bar control was not created".to_owned());
    }

    let text = wide_null(&frame_status_text(state_ref(hwnd)));
    if SendMessageW(status_bar, SB_SETTEXTW, 0, text.as_ptr() as LPARAM) == 0 {
        return Err("failed to update status bar text".to_owned());
    }

    Ok(())
}

fn render_view_text(lines: &[String]) -> String {
    lines.join("\r\n")
}

fn fold_find_query(query: &str) -> String {
    query.chars().map(|ch| ch.to_ascii_lowercase()).collect()
}

fn ascii_case_insensitive_match_range(
    haystack: &str,
    folded_query: &str,
) -> Option<(usize, usize)> {
    if folded_query.is_empty() {
        return Some((0, 0));
    }

    let expected = folded_query.chars().collect::<Vec<_>>();
    for (start, _) in haystack.char_indices() {
        let mut next_end = start;
        let mut matched = true;
        let mut actual = haystack[start..].chars();
        for query_char in &expected {
            let Some(actual_char) = actual.next() else {
                matched = false;
                break;
            };
            if actual_char.to_ascii_lowercase() != *query_char {
                matched = false;
                break;
            }
            next_end += actual_char.len_utf8();
        }
        if matched {
            return Some((start, next_end));
        }
    }

    None
}

fn ascii_case_insensitive_contains(haystack: &str, folded_query: &str) -> bool {
    ascii_case_insensitive_match_range(haystack, folded_query).is_some()
}

fn ascii_case_insensitive_utf16_range(
    haystack: &str,
    folded_query: &str,
) -> Option<(usize, usize)> {
    let (start, end) = ascii_case_insensitive_match_range(haystack, folded_query)?;
    let utf16_start = haystack[..start].encode_utf16().count();
    let utf16_end = utf16_start + haystack[start..end].encode_utf16().count();
    Some((utf16_start, utf16_end))
}

fn compute_timeline_find_matches(rows: &[TimelineRow], folded_query: &str) -> Vec<FindMatch> {
    rows.iter()
        .enumerate()
        .filter(|(_, row)| ascii_case_insensitive_contains(&row.logical_text, folded_query))
        .map(|(source_row_index, row)| {
            FindMatch::Timeline(TimelineFindMatch {
                row_identity: row.identity.clone(),
                source_row_index,
                logical_text: row.logical_text.clone(),
            })
        })
        .collect()
}

fn compute_status_find_matches(lines: &[String], folded_query: &str) -> Vec<FindMatch> {
    let mut matches = Vec::new();
    let mut document_utf16_offset = 0usize;
    for (line_index, line_text) in lines.iter().enumerate() {
        if let Some((utf16_start, utf16_end)) =
            ascii_case_insensitive_utf16_range(line_text, folded_query)
        {
            matches.push(FindMatch::Status(StatusFindMatch {
                line_index,
                utf16_start: document_utf16_offset + utf16_start,
                utf16_end: document_utf16_offset + utf16_end,
                line_text: line_text.clone(),
            }));
        }
        document_utf16_offset += line_text.encode_utf16().count();
        if line_index + 1 < lines.len() {
            document_utf16_offset += "\r\n".encode_utf16().count();
        }
    }
    matches
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FindSelectionStart {
    TimelineSourceRow(usize),
    StatusUtf16(usize),
}

enum FindSelectionMode<'a> {
    Execute {
        folded_query: &'a str,
        previous_state: Option<&'a ViewFindState>,
        start: FindSelectionStart,
    },
    Recompute {
        previous_state: &'a ViewFindState,
    },
}

fn find_match_start(match_item: &FindMatch) -> FindSelectionStart {
    match match_item {
        FindMatch::Timeline(item) => FindSelectionStart::TimelineSourceRow(item.source_row_index),
        FindMatch::Status(item) => FindSelectionStart::StatusUtf16(item.utf16_start),
    }
}

fn preserved_find_match_index(matches: &[FindMatch], previous_match: &FindMatch) -> Option<usize> {
    match previous_match {
        FindMatch::Timeline(previous) => matches.iter().position(|match_item| {
            matches!(
                match_item,
                FindMatch::Timeline(candidate) if candidate.row_identity == previous.row_identity
            )
        }),
        FindMatch::Status(previous) => matches.iter().position(|match_item| {
            matches!(
                match_item,
                FindMatch::Status(candidate)
                    if candidate.line_text == previous.line_text
                        && candidate.utf16_start == previous.utf16_start
                        && candidate.utf16_end == previous.utf16_end
            )
        }),
    }
}

fn choose_find_match(matches: &[FindMatch], mode: FindSelectionMode<'_>) -> Option<usize> {
    if matches.is_empty() {
        return None;
    }

    match mode {
        FindSelectionMode::Execute {
            folded_query,
            previous_state,
            start,
        } => {
            if let Some(previous_state) = previous_state
                && previous_state.folded_query == folded_query
                && let Some(previous_index) = previous_state.selected_match_index
                && previous_index < previous_state.matches.len()
            {
                return Some((previous_index + 1) % matches.len());
            }

            matches
                .iter()
                .position(|match_item| match (find_match_start(match_item), start) {
                    (
                        FindSelectionStart::TimelineSourceRow(candidate),
                        FindSelectionStart::TimelineSourceRow(start),
                    ) => candidate >= start,
                    (
                        FindSelectionStart::StatusUtf16(candidate),
                        FindSelectionStart::StatusUtf16(start),
                    ) => candidate >= start,
                    _ => false,
                })
                .or(Some(0))
        }
        FindSelectionMode::Recompute { previous_state } => previous_state
            .selected_match_index
            .and_then(|previous_index| previous_state.matches.get(previous_index))
            .and_then(|previous_match| preserved_find_match_index(matches, previous_match))
            .or(Some(0)),
    }
}

fn find_feedback(
    raw_query: &str,
    matches: &[FindMatch],
    selected_match_index: Option<usize>,
) -> String {
    if matches.is_empty() {
        return format!("find: no matches for \"{raw_query}\"");
    }

    let selected_match_index =
        selected_match_index.expect("non-empty find match lists must keep a selected match");
    format!(
        "find: {}/{} \"{}\"",
        selected_match_index + 1,
        matches.len(),
        raw_query
    )
}

fn find_match_selected_text(match_item: &FindMatch) -> String {
    match match_item {
        FindMatch::Timeline(item) => item.logical_text.clone(),
        FindMatch::Status(item) => item.line_text.clone(),
    }
}

fn build_view_find_state(
    raw_query: &str,
    folded_query: &str,
    matches: Vec<FindMatch>,
    selected_match_index: Option<usize>,
) -> ViewFindState {
    let feedback = find_feedback(raw_query, &matches, selected_match_index);
    ViewFindState {
        raw_query: raw_query.to_owned(),
        folded_query: folded_query.to_owned(),
        matches,
        selected_match_index,
        feedback,
    }
}

fn active_view_find_feedback(state: &WindowState) -> Option<&str> {
    state
        .find_states
        .get(&state.active_view)
        .map(|find_state| find_state.feedback.as_str())
}

#[cfg_attr(not(test), allow(dead_code))]
fn view_find_outcome(view_id: &ViewId, find_state: &ViewFindState) -> ActiveViewFindOutcome {
    let selected_text = find_state
        .selected_match_index
        .and_then(|index| find_state.matches.get(index))
        .map(find_match_selected_text);
    ActiveViewFindOutcome {
        active_view: view_id.clone(),
        raw_query: find_state.raw_query.clone(),
        matched: !find_state.matches.is_empty(),
        match_count: find_state.matches.len(),
        current_match_index: find_state.selected_match_index,
        selected_text,
    }
}

fn dump_find_state(view_id: &ViewId, find_state: &ViewFindState) -> UiFindState {
    let outcome = view_find_outcome(view_id, find_state);
    UiFindState {
        active: view_identity_from_view_id(&outcome.active_view),
        query: outcome.raw_query,
        matched: outcome.matched,
        match_count: outcome.match_count,
        current_match_index: outcome.current_match_index,
        selected_text: outcome.selected_text,
    }
}

fn find_active_view_response(outcome: ActiveViewFindOutcome) -> FindActiveViewResponse {
    FindActiveViewResponse {
        active: view_identity_from_view_id(&outcome.active_view),
        query: outcome.raw_query,
        matched: outcome.matched,
        match_count: outcome.match_count,
        current_match_index: outcome.current_match_index,
        selected_text: outcome.selected_text,
    }
}

enum ViewFindContent<'a> {
    Timeline(&'a [TimelineRow]),
    Status(&'a [String]),
}

fn recompute_find_for_view(
    state: &mut WindowState,
    view_id: &ViewId,
    content: ViewFindContent<'_>,
) {
    let Some(previous_state) = state.find_states.get(view_id).cloned() else {
        return;
    };

    let matches = match content {
        ViewFindContent::Timeline(rows) => {
            compute_timeline_find_matches(rows, &previous_state.folded_query)
        }
        ViewFindContent::Status(lines) => {
            compute_status_find_matches(lines, &previous_state.folded_query)
        }
    };
    let selected_match_index = choose_find_match(
        &matches,
        FindSelectionMode::Recompute {
            previous_state: &previous_state,
        },
    );
    let next_state = build_view_find_state(
        &previous_state.raw_query,
        &previous_state.folded_query,
        matches,
        selected_match_index,
    );
    state.find_states.insert(view_id.clone(), next_state);
}

#[cfg_attr(not(test), allow(dead_code))]
unsafe fn status_edit_selection_start(hwnd: HWND) -> usize {
    let mut start = 0u32;
    let mut end = 0u32;
    SendMessageW(
        hwnd,
        EM_GETSEL,
        (&mut start as *mut u32) as WPARAM,
        (&mut end as *mut u32) as LPARAM,
    );
    start.min(end) as usize
}

unsafe fn clear_find_projection_for_timeline(hwnd: HWND) {
    if timeline_state_ref(hwnd)
        .selected_find_source_row_index
        .is_none()
    {
        return;
    }

    timeline_state_mut(hwnd).selected_find_source_row_index = None;
    InvalidateRect(hwnd, null(), 1);
}

unsafe fn apply_find_to_active_view(hwnd: HWND, view_id: &ViewId) -> Result<(), String> {
    let active_view = state_ref(hwnd).active_view.clone();
    let view = state_ref(hwnd)
        .views
        .get(view_id)
        .ok_or_else(|| "view state was not tracked".to_owned())?;

    if active_view != *view_id {
        if matches!(view_id, ViewId::Room(_) | ViewId::Thread { .. }) {
            clear_find_projection_for_timeline(room_content_timeline(view.content));
        }
        return Ok(());
    }

    let selected_match = state_ref(hwnd)
        .find_states
        .get(view_id)
        .and_then(|find_state| {
            find_state
                .selected_match_index
                .and_then(|index| find_state.matches.get(index))
        })
        .cloned();

    match view_id {
        ViewId::Status => {
            let edit = view.content;
            match selected_match {
                Some(FindMatch::Status(item)) => {
                    SendMessageW(edit, EM_SETSEL, item.utf16_start, item.utf16_end as LPARAM);
                }
                Some(FindMatch::Timeline(_)) => {
                    unreachable!("status views must only store status matches")
                }
                None => {
                    SendMessageW(edit, EM_SETSEL, 0, 0);
                }
            }
        }
        ViewId::Room(_) | ViewId::Thread { .. } => {
            let timeline = room_content_timeline(view.content);
            let selected_source_row_index = match selected_match {
                Some(FindMatch::Timeline(item)) => Some(item.source_row_index),
                Some(FindMatch::Status(_)) => {
                    unreachable!("timeline views must only store timeline matches")
                }
                None => None,
            };
            if timeline_state_ref(timeline).selected_find_source_row_index
                != selected_source_row_index
            {
                timeline_state_mut(timeline).selected_find_source_row_index =
                    selected_source_row_index;
                InvalidateRect(timeline, null(), 1);
            }
        }
    }

    Ok(())
}

#[cfg_attr(not(test), allow(dead_code))]
unsafe fn scroll_timeline_to_source_row_index(hwnd: HWND, source_row_index: usize) {
    let top_line = {
        let timeline_state = timeline_state_ref(hwnd);
        timeline_top_line_for_source_row_index(hwnd, &timeline_state.source_rows, source_row_index)
    };
    let visible_rows = timeline_visible_rows(hwnd);
    let max_top_line =
        timeline_max_top_line(timeline_state_ref(hwnd).paint_lines.len(), visible_rows);
    timeline_state_mut(hwnd).top_line = top_line.min(max_top_line);
    update_timeline_scrollbar(hwnd);
    InvalidateRect(hwnd, null(), 1);
}

#[cfg_attr(not(test), allow(dead_code))]
unsafe fn execute_active_view_find(
    hwnd: HWND,
    query: &str,
) -> Result<ActiveViewFindOutcome, String> {
    if query.trim().is_empty() {
        return Err("find query must not be empty".to_owned());
    }

    let active_view = state_ref(hwnd).active_view.clone();
    let previous_state = state_ref(hwnd).find_states.get(&active_view).cloned();
    let folded_query = fold_find_query(query);

    let next_state = match &active_view {
        ViewId::Status => {
            let view = state_ref(hwnd)
                .views
                .get(&active_view)
                .ok_or_else(|| "view state was not tracked".to_owned())?;
            let lines = render_status_lines(&state_ref(hwnd).model);
            let matches = compute_status_find_matches(&lines, &folded_query);
            let selection_start =
                FindSelectionStart::StatusUtf16(status_edit_selection_start(view.content));
            let selected_match_index = choose_find_match(
                &matches,
                FindSelectionMode::Execute {
                    folded_query: &folded_query,
                    previous_state: previous_state.as_ref(),
                    start: selection_start,
                },
            );
            build_view_find_state(query, &folded_query, matches, selected_match_index)
        }
        ViewId::Room(_) | ViewId::Thread { .. } => {
            let view = state_ref(hwnd)
                .views
                .get(&active_view)
                .ok_or_else(|| "view state was not tracked".to_owned())?;
            let timeline = room_content_timeline(view.content);
            let timeline_state = timeline_state_ref(timeline);
            let start_source_row_index = timeline_source_row_index_for_top_line(
                timeline,
                &timeline_state.source_rows,
                timeline_state.top_line,
            )
            .unwrap_or(0);
            let matches = compute_timeline_find_matches(&timeline_state.source_rows, &folded_query);
            let selected_match_index = choose_find_match(
                &matches,
                FindSelectionMode::Execute {
                    folded_query: &folded_query,
                    previous_state: previous_state.as_ref(),
                    start: FindSelectionStart::TimelineSourceRow(start_source_row_index),
                },
            );
            build_view_find_state(query, &folded_query, matches, selected_match_index)
        }
    };

    state_mut(hwnd)
        .find_states
        .insert(active_view.clone(), next_state);
    apply_find_to_active_view(hwnd, &active_view)?;

    if let Some(find_state) = state_ref(hwnd).find_states.get(&active_view)
        && let Some(selected_match_index) = find_state.selected_match_index
        && let Some(selected_match) = find_state.matches.get(selected_match_index)
    {
        match selected_match {
            FindMatch::Timeline(item) => {
                let view = state_ref(hwnd)
                    .views
                    .get(&active_view)
                    .ok_or_else(|| "view state was not tracked".to_owned())?;
                scroll_timeline_to_source_row_index(
                    room_content_timeline(view.content),
                    item.source_row_index,
                );
            }
            FindMatch::Status(_) => {
                let view = state_ref(hwnd)
                    .views
                    .get(&active_view)
                    .ok_or_else(|| "view state was not tracked".to_owned())?;
                SendMessageW(view.content, EM_SCROLLCARET, 0, 0);
            }
        }
    }

    refresh_status_bar(hwnd)?;
    Ok(view_find_outcome(
        &active_view,
        state_ref(hwnd)
            .find_states
            .get(&active_view)
            .expect("active view find state must be stored"),
    ))
}

fn resolve_active_view(
    active_view: &ViewId,
    selected_room: Option<&str>,
    model: &UiModel,
) -> ViewId {
    match active_view {
        ViewId::Status => ViewId::Status,
        ViewId::Room(room_id) if navigable_room_by_id(&model.rooms, room_id).is_some() => {
            ViewId::Room(room_id.clone())
        }
        ViewId::Thread {
            room_id,
            thread_root_event_id,
        } if projected_thread_by_id(model, room_id, thread_root_event_id).is_some() => {
            ViewId::Thread {
                room_id: room_id.clone(),
                thread_root_event_id: thread_root_event_id.clone(),
            }
        }
        ViewId::Thread { room_id, .. } if navigable_room_by_id(&model.rooms, room_id).is_some() => {
            ViewId::Room(room_id.clone())
        }
        ViewId::Room(_) | ViewId::Thread { .. } => {
            if let Some(room_id) = selected_room
                .filter(|room_id| navigable_room_by_id(&model.rooms, room_id).is_some())
            {
                ViewId::Room(room_id.to_owned())
            } else if let Some(room) = model.rooms.iter().find(|room| is_navigable_room(room)) {
                ViewId::Room(room.room_id.clone())
            } else {
                ViewId::Status
            }
        }
    }
}

fn take_stale_room_views(state: &mut WindowState) -> Vec<MdiViewState> {
    let stale_view_ids = state
        .views
        .keys()
        .filter(|view_id| !view_id_is_valid(&state.model, view_id))
        .cloned()
        .collect::<Vec<_>>();

    for view_id in &stale_view_ids {
        state.find_states.remove(view_id);
    }

    let removed = stale_view_ids
        .into_iter()
        .filter_map(|view_id| {
            let view = state.views.remove(&view_id)?;
            state.hwnd_views.remove(&(view.child as isize));
            Some(view)
        })
        .collect::<Vec<_>>();

    let resolved_active_view = resolve_active_view(
        &state.active_view,
        state.model.selected_room.as_deref(),
        &state.model,
    );
    if matches!(resolved_active_view, ViewId::Status) {
        state.active_view = ViewId::Status;
    } else {
        state.active_view = resolved_active_view;
    }

    removed
}

fn is_joined_message_room(room: &RoomSummary) -> bool {
    room.membership == RoomMembership::Join && !room.is_space
}

fn is_invited_room(room: &RoomSummary) -> bool {
    room.membership == RoomMembership::Invite && !room.is_space
}

fn is_navigable_room(room: &RoomSummary) -> bool {
    is_joined_message_room(room) || is_invited_room(room)
}

fn is_sendable_room(room: &RoomSummary) -> bool {
    is_joined_message_room(room)
}

fn is_invite_command_target(room: &RoomSummary) -> bool {
    is_invited_room(room)
}

fn room_by_id<'a>(rooms: &'a [RoomSummary], room_id: &str) -> Option<&'a RoomSummary> {
    rooms.iter().find(|room| room.room_id == room_id)
}

fn navigable_room_by_id<'a>(rooms: &'a [RoomSummary], room_id: &str) -> Option<&'a RoomSummary> {
    room_by_id(rooms, room_id).filter(|room| is_navigable_room(room))
}

fn sendable_room_by_id<'a>(rooms: &'a [RoomSummary], room_id: &str) -> Option<&'a RoomSummary> {
    room_by_id(rooms, room_id).filter(|room| is_sendable_room(room))
}

fn view_id_is_valid_for_rooms(
    rooms: &[RoomSummary],
    room_views: &BTreeMap<String, RoomViewModel>,
    view_id: &ViewId,
) -> bool {
    match view_id {
        ViewId::Status => true,
        ViewId::Room(room_id) => navigable_room_by_id(rooms, room_id).is_some(),
        ViewId::Thread {
            room_id,
            thread_root_event_id,
        } => {
            sendable_room_by_id(rooms, room_id).is_some()
                && room_views
                    .get(room_id)
                    .is_some_and(|room_view| room_view.threads.contains_key(thread_root_event_id))
        }
    }
}

fn projected_thread_by_id<'a>(
    model: &'a UiModel,
    room_id: &str,
    thread_root_event_id: &str,
) -> Option<&'a ThreadViewModel> {
    sendable_room_by_id(&model.rooms, room_id)?;
    model
        .room_views
        .get(room_id)?
        .threads
        .get(thread_root_event_id)
}

fn view_id_is_valid(model: &UiModel, view_id: &ViewId) -> bool {
    match view_id {
        ViewId::Status => true,
        ViewId::Room(room_id) => navigable_room_by_id(&model.rooms, room_id).is_some(),
        ViewId::Thread {
            room_id,
            thread_root_event_id,
        } => projected_thread_by_id(model, room_id, thread_root_event_id).is_some(),
    }
}

fn room_tree_label(room: &RoomSummary) -> String {
    let mut label = room.display_name.clone();
    append_room_label_markers(&mut label, room);
    label
}

fn room_navigation_label(model: &UiModel, room: &RoomSummary) -> String {
    let mut label = room_tree_label(room);
    append_room_read_state_tokens(&mut label, model.room_read_states.get(&room.room_id));
    label
}

fn space_tree_label(space: &RoomSummary) -> String {
    space.display_name.clone()
}

fn view_title(model: &UiModel, view_id: &ViewId) -> String {
    if let Some((room_id, thread_root_event_id)) = view_id.thread_key() {
        return format!(
            "Thread: {}",
            projected_thread_by_id(model, room_id, thread_root_event_id)
                .expect("thread view activation must target an existing projected thread")
                .label
        );
    }

    match view_id {
        ViewId::Status => "Status".to_owned(),
        ViewId::Room(room_id) => {
            let room = room_by_id(&model.rooms, room_id)
                .expect("room view activation must target an existing room");
            room_tree_label(room)
        }
        ViewId::Thread { .. } => unreachable!("thread views are handled above"),
    }
}

fn switchbar_button_text(state: &WindowState, view_id: &ViewId) -> String {
    match view_id {
        ViewId::Room(room_id) => {
            let room = room_by_id(&state.model.rooms, room_id)
                .expect("room switchbar button must target an existing room");
            room_navigation_label(&state.model, room)
        }
        ViewId::Status | ViewId::Thread { .. } => view_title(&state.model, view_id),
    }
}

fn switchbar_button_state(state: &WindowState, view_id: &ViewId) -> u8 {
    let mut button_state = TBSTATE_ENABLED as u8;
    if state.active_view == *view_id {
        button_state |= TBSTATE_CHECKED as u8;
    }
    if state.room_activity.contains(view_id) {
        button_state |= TBSTATE_MARKED as u8;
    }
    button_state
}

fn tree_item_state(state: &WindowState, view_id: &ViewId) -> u32 {
    if state.room_activity.contains(view_id) {
        TVIS_BOLD
    } else {
        0
    }
}

fn view_identity_from_view_id(view_id: &ViewId) -> ViewIdentity {
    match view_id {
        ViewId::Status => ViewIdentity::Status,
        ViewId::Room(room_id) => ViewIdentity::Room {
            room_id: room_id.clone(),
        },
        ViewId::Thread {
            room_id,
            thread_root_event_id,
        } => ViewIdentity::Thread {
            room_id: room_id.clone(),
            thread_root_event_id: thread_root_event_id.clone(),
        },
    }
}

fn view_id_from_identity(identity: &ViewIdentity) -> ViewId {
    match identity {
        ViewIdentity::Status => ViewId::Status,
        ViewIdentity::Room { room_id } => ViewId::Room(room_id.clone()),
        ViewIdentity::Thread {
            room_id,
            thread_root_event_id,
        } => ViewId::Thread {
            room_id: room_id.clone(),
            thread_root_event_id: thread_root_event_id.clone(),
        },
    }
}

unsafe fn create_mdi_child(
    frame_hwnd: HWND,
    title: &str,
    view_id: &ViewId,
) -> Result<HWND, String> {
    let mdi_client = state_ref(frame_hwnd).controls.mdi_client;
    if mdi_client.is_null() {
        return Err("MDI client control was not created".to_owned());
    }

    let instance = GetModuleHandleW(null());
    let mut class_name = wide_null(MDI_CHILD_CLASS_NAME);
    let mut title_wide = wide_null(title);
    let create_state = Box::new(MdiChildCreateState {
        view_id: view_id.clone(),
    });
    let create_state_ptr = Box::into_raw(create_state);
    let create = MDICREATESTRUCTW {
        szClass: class_name.as_mut_ptr(),
        szTitle: title_wide.as_mut_ptr(),
        hOwner: instance,
        x: CW_USEDEFAULT,
        y: CW_USEDEFAULT,
        cx: CW_USEDEFAULT,
        cy: CW_USEDEFAULT,
        style: WS_CHILD | WS_VISIBLE,
        lParam: create_state_ptr as isize,
    };
    let child = SendMessageW(
        mdi_client,
        WM_MDICREATE,
        0,
        (&create as *const MDICREATESTRUCTW) as LPARAM,
    ) as HWND;
    drop(Box::from_raw(create_state_ptr));
    if child.is_null() {
        return Err(last_os_error("WM_MDICREATE"));
    }

    Ok(child)
}

unsafe fn create_mdi_child_content(child: HWND, view_id: &ViewId) -> Result<HWND, String> {
    let instance = GetModuleHandleW(null());
    let (class_name, style, ex_style) = match view_id {
        ViewId::Status => (
            EDIT_CLASS_NAME,
            WS_CHILD
                | WS_VISIBLE
                | ES_MULTILINE as u32
                | ES_READONLY as u32
                | ES_AUTOVSCROLL as u32
                | ES_WANTRETURN as u32
                | WS_VSCROLL,
            WS_EX_CLIENTEDGE,
        ),
        ViewId::Room(_) | ViewId::Thread { .. } => {
            (ROOM_CONTENT_CLASS_NAME, WS_CHILD | WS_VISIBLE, 0)
        }
    };
    let content = CreateWindowExW(
        ex_style,
        wide_null(class_name).as_ptr(),
        wide_null("").as_ptr(),
        style,
        0,
        0,
        0,
        0,
        child,
        VIEW_CONTENT_ID as usize as HMENU,
        instance,
        null_mut(),
    );
    if content.is_null() {
        return Err(last_os_error("CreateWindowExW MDI child content"));
    }

    let mut rect = RECT::default();
    GetClientRect(child, &mut rect);
    MoveWindow(
        content,
        0,
        0,
        rect.right - rect.left,
        rect.bottom - rect.top,
        1,
    );

    Ok(content)
}

unsafe fn mdi_child_content(hwnd: HWND) -> HWND {
    GetWindowLongPtrW(hwnd, GWLP_USERDATA) as HWND
}

unsafe fn room_content_timeline(content: HWND) -> HWND {
    room_content_state_ref(content).timeline
}

unsafe fn room_content_member_list(content: HWND) -> HWND {
    room_content_state_ref(content).members
}

fn unregister_view_by_child(state: &mut WindowState, child: HWND) -> Option<ViewId> {
    let view_id = state.hwnd_views.remove(&(child as isize))?;
    let removed = state
        .views
        .remove(&view_id)
        .expect("HWND-to-view registry must match tracked views");
    state.find_states.remove(&view_id);
    assert_eq!(
        removed.child, child,
        "tracked child HWND must match registry"
    );
    if state.active_view == view_id {
        state.active_view = resolve_active_view(
            &state.active_view,
            state.model.selected_room.as_deref(),
            &state.model,
        );
    }
    Some(view_id)
}

unsafe fn unregister_destroyed_view(child: HWND) {
    let frame = frame_window_for(child);
    let state_ptr = GetWindowLongPtrW(frame, GWLP_USERDATA) as *mut WindowState;
    if state_ptr.is_null() {
        return;
    }

    let _ = unregister_view_by_child(&mut *state_ptr, child);
}

unsafe fn cached_view_is_alive(view: &MdiViewState) -> bool {
    IsWindow(view.child) != 0
        && IsWindow(view.content) != 0
        && mdi_child_content(view.child) == view.content
}

unsafe fn ensure_view(hwnd: HWND, view_id: ViewId) -> Result<HWND, String> {
    let stale_child = state_ref(hwnd)
        .views
        .get(&view_id)
        .and_then(|view| (!cached_view_is_alive(view)).then_some(view.child));
    if let Some(child) = stale_child {
        let removed = unregister_view_by_child(state_mut(hwnd), child)
            .expect("tracked view must exist before stale child cleanup");
        assert_eq!(
            removed, view_id,
            "stale child cleanup must match requested view"
        );
    } else if let Some(view) = state_ref(hwnd).views.get(&view_id) {
        return Ok(view.child);
    }

    let title = view_title(&state_ref(hwnd).model, &view_id);
    let child = create_mdi_child(hwnd, &title, &view_id)?;
    let content = mdi_child_content(child);
    if content.is_null() {
        return Err("MDI child content control was not created".to_owned());
    }

    let state = state_mut(hwnd);
    state.hwnd_views.insert(child as isize, view_id.clone());
    state.views.insert(view_id, MdiViewState { child, content });
    Ok(child)
}

unsafe fn ensure_status_view(hwnd: HWND) -> Result<HWND, String> {
    ensure_view(hwnd, ViewId::Status)
}

unsafe fn ensure_room_view(hwnd: HWND, room_id: &str) -> Result<HWND, String> {
    if navigable_room_by_id(&state_ref(hwnd).model.rooms, room_id).is_none() {
        return Err(format!("unknown room id: {room_id}"));
    }

    ensure_view(hwnd, ViewId::Room(room_id.to_owned()))
}

unsafe fn ensure_thread_view(
    hwnd: HWND,
    room_id: &str,
    thread_root_event_id: &str,
) -> Result<HWND, String> {
    if sendable_room_by_id(&state_ref(hwnd).model.rooms, room_id).is_none() {
        return Err(format!("unknown room id: {room_id}"));
    }
    if projected_thread_by_id(&state_ref(hwnd).model, room_id, thread_root_event_id).is_none() {
        return Err(format!(
            "unknown thread root event id for {room_id}: {thread_root_event_id}"
        ));
    }

    ensure_view(
        hwnd,
        ViewId::Thread {
            room_id: room_id.to_owned(),
            thread_root_event_id: thread_root_event_id.to_owned(),
        },
    )
}

unsafe fn destroy_stale_room_views(hwnd: HWND) {
    let (mdi_client, stale_views) = {
        let state = state_mut(hwnd);
        (state.controls.mdi_client, take_stale_room_views(state))
    };

    for view in stale_views {
        SendMessageW(mdi_client, WM_MDIDESTROY, view.child as WPARAM, 0);
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct RoomImageMediaDispatch {
    activity_id: String,
    event_id: String,
    source: RoomMediaSource,
    format: RoomImageMediaFormat,
}

fn timeline_image_thumbnail_source(
    message: &mmatrix_core::RoomImageActivity,
) -> Option<&RoomMediaSource> {
    if let Some(thumbnail_source) = message
        .info
        .as_ref()
        .and_then(|info| info.thumbnail_source.as_ref())
    {
        return Some(thumbnail_source);
    }

    match &message.source {
        RoomMediaSource::Plain { .. } => Some(&message.source),
        RoomMediaSource::Encrypted { .. } => None,
    }
}

fn collect_lazy_room_image_media_requests<'a>(
    room_view: &RoomViewModel,
    activities: impl IntoIterator<Item = &'a RoomActivity>,
) -> Vec<RoomImageMediaDispatch> {
    let format = timeline_thumbnail_format();
    activities
        .into_iter()
        .filter_map(|activity| {
            let RoomActivityKind::Image(message) = &activity.kind else {
                return None;
            };
            if !room_view.should_request_image_media(
                &activity.activity_id,
                &message.event_id,
                &format,
            ) {
                return None;
            }
            let source = timeline_image_thumbnail_source(message)?;
            Some(RoomImageMediaDispatch {
                activity_id: activity.activity_id.clone(),
                event_id: message.event_id.clone(),
                source: source.clone(),
                format: format.clone(),
            })
        })
        .collect()
}

fn dispatch_lazy_room_image_media_requests(
    state: &mut WindowState,
    room_id: &str,
    requests: Vec<RoomImageMediaDispatch>,
) -> Result<(), String> {
    if state.synthetic_load_in_progress || requests.is_empty() {
        return Ok(());
    }

    for request in requests {
        let should_request = state
            .model
            .room_views
            .get(room_id)
            .expect("image media dispatch requires an existing room view")
            .should_request_image_media(&request.activity_id, &request.event_id, &request.format);
        if !should_request {
            continue;
        }

        let previous = state
            .model
            .room_views
            .get_mut(room_id)
            .expect("image media dispatch requires an existing room view")
            .mark_image_media_pending(
                request.activity_id.clone(),
                request.event_id.clone(),
                request.format.clone(),
            );
        if let Some(image_key) = previous
            .as_ref()
            .and_then(room_image_media_loaded_key)
            .map(str::to_owned)
        {
            state.decoded_images.remove(&image_key);
        }

        let command = Command::FetchRoomImageMedia(FetchRoomImageMediaRequest {
            room_id: room_id.to_owned(),
            activity_id: request.activity_id.clone(),
            event_id: request.event_id.clone(),
            source: request.source.clone(),
            format: request.format.clone(),
        });
        let sender = match state.command_sender.as_ref().cloned() {
            Some(sender) => sender,
            None => continue,
        };
        if let Err(err) = sender.send(command.clone()) {
            state
                .model
                .room_views
                .get_mut(room_id)
                .expect("image media dispatch requires an existing room view")
                .remove_image_media(&request.activity_id);
            return Err(format!(
                "failed to enqueue fetch room image media command: {err}"
            ));
        }
        #[cfg(test)]
        state.recorded_commands.push(command);
    }

    Ok(())
}

unsafe fn refresh_view_content(hwnd: HWND, view_id: &ViewId) -> Result<(), String> {
    let (child, content, title) = {
        let state = state_ref(hwnd);
        let view = state
            .views
            .get(view_id)
            .ok_or_else(|| "view state was not tracked".to_owned())?;
        let title = view_title(&state.model, view_id);
        (view.child, view.content, title)
    };

    let title = wide_null(&title);
    SendMessageW(child, WM_SETTEXT, 0, title.as_ptr() as LPARAM);
    match view_id {
        ViewId::Status => {
            let lines = {
                let state = state_ref(hwnd);
                render_status_lines(&state.model)
            };
            let text = render_view_text(&lines);
            set_edit_text(content, &text);
            recompute_find_for_view(state_mut(hwnd), view_id, ViewFindContent::Status(&lines));
            if state_ref(hwnd).find_states.contains_key(view_id)
                && state_ref(hwnd).active_view == *view_id
            {
                apply_find_to_active_view(hwnd, view_id)?;
            }
        }
        ViewId::Room(room_id) => {
            let (rows, member_rows, image_requests) = {
                let state = state_ref(hwnd);
                let room = room_by_id(&state.model.rooms, room_id)
                    .expect("room view must target an existing room");
                let room_view = state.model.room_views.get(room_id);
                let rows = build_room_timeline_rows(&state.model, room_id);
                let image_requests = if is_sendable_room(room) {
                    room_view
                        .map(|room_view| {
                            collect_lazy_room_image_media_requests(
                                room_view,
                                room_view.activities.iter(),
                            )
                        })
                        .unwrap_or_default()
                } else {
                    Vec::new()
                };
                let member_rows = if is_sendable_room(room) {
                    room_member_view_rows(
                        room_view
                            .map(|room_view| room_view.members.as_slice())
                            .unwrap_or_default(),
                    )
                } else {
                    Vec::new()
                };
                (rows, member_rows, image_requests)
            };
            dispatch_lazy_room_image_media_requests(state_mut(hwnd), room_id, image_requests)?;
            let timeline = room_content_timeline(content);
            set_timeline_rows(timeline, &rows);
            recompute_find_for_view(state_mut(hwnd), view_id, ViewFindContent::Timeline(&rows));
            if state_ref(hwnd).active_view == *view_id {
                apply_find_to_active_view(hwnd, view_id)?;
            } else {
                clear_find_projection_for_timeline(timeline);
            }
            set_room_member_list(room_content_member_list(content), &member_rows);
            InvalidateRect(content, null(), 1);
            InvalidateRect(timeline, null(), 1);
            InvalidateRect(room_content_member_list(content), null(), 1);
        }
        ViewId::Thread {
            room_id,
            thread_root_event_id,
        } => {
            let (rows, member_rows, image_requests) = {
                let state = state_ref(hwnd);
                let room_view = state
                    .model
                    .room_views
                    .get(room_id)
                    .expect("thread view must target an existing room view");
                let rows = build_thread_timeline_rows(&state.model, room_id, thread_root_event_id);
                let image_requests = collect_lazy_room_image_media_requests(
                    room_view,
                    room_view.activities.iter().filter(|activity| {
                        room_activity_belongs_to_thread(activity, thread_root_event_id)
                    }),
                );
                let member_rows = room_member_view_rows(room_view.members.as_slice());
                (rows, member_rows, image_requests)
            };
            dispatch_lazy_room_image_media_requests(state_mut(hwnd), room_id, image_requests)?;
            let timeline = room_content_timeline(content);
            set_timeline_rows(timeline, &rows);
            recompute_find_for_view(state_mut(hwnd), view_id, ViewFindContent::Timeline(&rows));
            if state_ref(hwnd).active_view == *view_id {
                apply_find_to_active_view(hwnd, view_id)?;
            } else {
                clear_find_projection_for_timeline(timeline);
            }
            set_room_member_list(room_content_member_list(content), &member_rows);
            InvalidateRect(content, null(), 1);
            InvalidateRect(timeline, null(), 1);
            InvalidateRect(room_content_member_list(content), null(), 1);
        }
    }
    Ok(())
}

struct AppliedActivation {
    child: HWND,
    cleared_view_activity: bool,
    rebuild_switchbar_after_activation: bool,
}

unsafe fn apply_view_activation_state(
    hwnd: HWND,
    view_id: &ViewId,
    preferred_tree_item: Option<isize>,
) -> Result<AppliedActivation, String> {
    let mut cleared_view_activity = false;
    let rebuild_switchbar_after_activation = !state_ref(hwnd).switchbar_views.contains(view_id);
    let child = match view_id {
        ViewId::Status => {
            state_mut(hwnd).model.selected_room = None;
            ensure_status_view(hwnd)?
        }
        ViewId::Room(room_id) => {
            {
                let state = state_mut(hwnd);
                select_room(state, room_id)?;
                cleared_view_activity = handle_room_activation(state, room_id)?;
            }
            ensure_room_view(hwnd, room_id)?
        }
        ViewId::Thread {
            room_id,
            thread_root_event_id,
        } => {
            {
                let state = state_mut(hwnd);
                select_room(state, room_id)?;
                cleared_view_activity =
                    handle_thread_activation(state, room_id, thread_root_event_id)?;
            }
            ensure_thread_view(hwnd, room_id, thread_root_event_id)?
        }
    };

    {
        let state = state_mut(hwnd);
        state.active_view = view_id.clone();
    }

    refresh_tree_item_activity_state(hwnd)?;
    refresh_view_content(hwnd, view_id)?;
    select_tree_item(hwnd, view_id, preferred_tree_item);

    Ok(AppliedActivation {
        child,
        cleared_view_activity,
        rebuild_switchbar_after_activation,
    })
}

unsafe fn finish_view_activation_chrome(
    hwnd: HWND,
    applied: &AppliedActivation,
) -> Result<(), String> {
    if applied.cleared_view_activity || applied.rebuild_switchbar_after_activation {
        rebuild_switchbar(hwnd)?;
    } else {
        refresh_switchbar_state(hwnd)?;
    }
    refresh_status_bar(hwnd)?;
    let state = state_ref(hwnd);
    SetFocus(state.controls.edit);
    if state.render_state_mode {
        HideCaret(state.controls.edit);
    }
    Ok(())
}

unsafe fn activate_view(
    hwnd: HWND,
    view_id: &ViewId,
    preferred_tree_item: Option<isize>,
) -> Result<(), String> {
    let applied = apply_view_activation_state(hwnd, view_id, preferred_tree_item)?;

    let mdi_client = state_ref(hwnd).controls.mdi_client;
    SendMessageW(mdi_client, WM_MDIACTIVATE, applied.child as WPARAM, 0);
    SendMessageW(mdi_client, WM_MDIMAXIMIZE, applied.child as WPARAM, 0);
    finish_view_activation_chrome(hwnd, &applied)?;
    Ok(())
}

unsafe fn handle_native_mdi_child_activation(child: HWND) -> Result<(), String> {
    let mdi_client = GetParent(child);
    let frame = GetParent(mdi_client);
    let Some(view_id) = state_ref(frame).hwnd_views.get(&(child as isize)).cloned() else {
        return Ok(());
    };
    if state_ref(frame).active_view == view_id {
        return Ok(());
    }

    let applied = apply_view_activation_state(frame, &view_id, None)?;
    finish_view_activation_chrome(frame, &applied)
}

unsafe fn sync_views_and_tree(hwnd: HWND) -> Result<(), String> {
    ensure_status_view(hwnd)?;

    let room_ids = state_ref(hwnd)
        .model
        .rooms
        .iter()
        .filter(|room| is_navigable_room(room))
        .map(|room| room.room_id.clone())
        .collect::<Vec<_>>();
    destroy_stale_room_views(hwnd);
    for room_id in room_ids {
        ensure_room_view(hwnd, &room_id)?;
    }

    let view_ids = state_ref(hwnd).views.keys().cloned().collect::<Vec<_>>();
    for view_id in &view_ids {
        refresh_view_content(hwnd, view_id)?;
    }

    let active_view = {
        let state = state_mut(hwnd);
        let active = resolve_active_view(
            &state.active_view,
            state.model.selected_room.as_deref(),
            &state.model,
        );
        state.active_view = active.clone();
        active
    };
    refresh_tree(hwnd);
    rebuild_switchbar(hwnd)?;
    refresh_status_bar(hwnd)?;
    activate_view(hwnd, &active_view, None)
}

unsafe fn refresh_tree(hwnd: HWND) {
    let state = state_mut(hwnd);
    let tree = state.controls.tree;
    if tree.is_null() {
        return;
    }

    SendMessageW(tree, TVM_DELETEITEM, 0, TVI_ROOT as LPARAM);
    state.tree_items.clear();
    state.primary_tree_items.clear();

    let root = insert_tree_item(tree, TVI_ROOT, "mmatrix");
    let status = insert_mapped_tree_item(state, root, "status", ViewId::Status);

    let rooms = state.model.rooms.clone();
    let joined_rooms = rooms
        .iter()
        .filter(|room| is_joined_message_room(room))
        .cloned()
        .collect::<Vec<_>>();
    let invited_rooms = rooms
        .iter()
        .filter(|room| is_invited_room(room))
        .cloned()
        .collect::<Vec<_>>();
    let rooms_label = if rooms.is_empty() {
        "rooms: offline".to_owned()
    } else {
        format!("rooms: {}", joined_rooms.len())
    };
    let rooms_root = insert_tree_item(tree, root, &rooms_label);

    let joined_room_tree_source = rooms
        .iter()
        .filter(|room| room.membership == RoomMembership::Join)
        .cloned()
        .collect::<Vec<_>>();
    let flat_mode = joined_room_tree_source
        .iter()
        .all(|room| !room.is_space && room.space_children.is_empty());
    if flat_mode {
        for room in &joined_rooms {
            render_room_tree_leaf(state, rooms_root, room);
        }
    } else {
        let room_map = joined_room_tree_source
            .iter()
            .cloned()
            .map(|room| (room.room_id.clone(), room))
            .collect::<BTreeMap<_, _>>();
        let valid_space_children = joined_room_tree_source
            .iter()
            .filter(|room| room.is_space)
            .map(|room| {
                (
                    room.room_id.clone(),
                    room.space_children
                        .iter()
                        .filter(|child| room_map.contains_key(&child.room_id))
                        .map(|child| child.room_id.clone())
                        .collect::<Vec<_>>(),
                )
            })
            .collect::<BTreeMap<_, _>>();

        let child_space_ids = valid_space_children
            .values()
            .flatten()
            .filter_map(|child_id| {
                room_map
                    .get(child_id)
                    .filter(|room| room.is_space)
                    .map(|_| child_id.clone())
            })
            .collect::<BTreeSet<_>>();
        let rooted_room_ids = valid_space_children
            .values()
            .flatten()
            .filter_map(|child_id| {
                room_map
                    .get(child_id)
                    .filter(|room| !room.is_space)
                    .map(|_| child_id.clone())
            })
            .collect::<BTreeSet<_>>();

        let mut rendered_roots = joined_room_tree_source
            .iter()
            .filter(|room| room.is_space && !child_space_ids.contains(&room.room_id))
            .map(|room| room.room_id.clone())
            .collect::<Vec<_>>();
        let mut covered_space_ids = BTreeSet::new();
        for root_id in &rendered_roots {
            mark_reachable_space_ids(
                &mut covered_space_ids,
                &room_map,
                &valid_space_children,
                root_id,
            );
        }
        for room in joined_room_tree_source.iter().filter(|room| room.is_space) {
            if covered_space_ids.contains(&room.room_id) {
                continue;
            }
            rendered_roots.push(room.room_id.clone());
            mark_reachable_space_ids(
                &mut covered_space_ids,
                &room_map,
                &valid_space_children,
                &room.room_id,
            );
        }

        for root_id in rendered_roots {
            render_space_tree(
                state,
                rooms_root,
                &room_map,
                &valid_space_children,
                &root_id,
            );
        }

        let unspaced_rooms = joined_rooms
            .into_iter()
            .filter(|room| !rooted_room_ids.contains(&room.room_id))
            .collect::<Vec<_>>();
        if !unspaced_rooms.is_empty() {
            let label = format!("unspaced rooms: {}", unspaced_rooms.len());
            let unspaced_root = insert_tree_item(tree, rooms_root, &label);
            for room in &unspaced_rooms {
                render_room_tree_leaf(state, unspaced_root, room);
            }
            expand_tree_item(tree, unspaced_root);
        }
    }

    let invites_root = insert_tree_item(tree, root, &format!("invites: {}", invited_rooms.len()));
    for room in &invited_rooms {
        render_room_tree_leaf(state, invites_root, room);
    }

    expand_tree_item(tree, root);
    expand_tree_item(tree, rooms_root);
    expand_tree_item(tree, invites_root);
    debug_assert_eq!(
        status,
        *state.primary_tree_items.get(&ViewId::Status).unwrap()
    );
}

unsafe fn select_tree_item(hwnd: HWND, view_id: &ViewId, preferred_tree_item: Option<isize>) {
    let (tree, item) = {
        let state = state_ref(hwnd);
        let item = preferred_tree_item
            .filter(|item| {
                state
                    .tree_items
                    .get(item)
                    .is_some_and(|mapped_view| mapped_view == view_id)
            })
            .or_else(|| state.primary_tree_items.get(view_id).copied());
        (state.controls.tree, item)
    };
    let Some(item) = item else {
        return;
    };
    let current = SendMessageW(tree, TVM_GETNEXTITEM, TVGN_CARET as WPARAM, 0) as isize;
    if current == item {
        return;
    }

    SendMessageW(tree, TVM_SELECTITEM, TVGN_CARET as WPARAM, item as LPARAM);
}

unsafe fn insert_tree_item(tree: HWND, parent: isize, text: &str) -> isize {
    let mut text = wide_null(text);
    let item = TVITEMEXW {
        mask: TVIF_TEXT,
        pszText: text.as_mut_ptr(),
        cchTextMax: text.len() as i32,
        ..zeroed()
    };
    let insert = TVINSERTSTRUCTW {
        hParent: parent as _,
        hInsertAfter: TVI_LAST as _,
        Anonymous: TVINSERTSTRUCTW_0 { itemex: item },
    };

    SendMessageW(
        tree,
        TVM_INSERTITEMW,
        0,
        (&insert as *const TVINSERTSTRUCTW) as LPARAM,
    ) as isize
}

unsafe fn insert_tree_item_with_state(tree: HWND, parent: isize, text: &str, state: u32) -> isize {
    let mut text = wide_null(text);
    let item = TVITEMEXW {
        mask: TVIF_TEXT | TVIF_STATE,
        state,
        stateMask: TVIS_BOLD,
        pszText: text.as_mut_ptr(),
        cchTextMax: text.len() as i32,
        ..zeroed()
    };
    let insert = TVINSERTSTRUCTW {
        hParent: parent as _,
        hInsertAfter: TVI_LAST as _,
        Anonymous: TVINSERTSTRUCTW_0 { itemex: item },
    };

    SendMessageW(
        tree,
        TVM_INSERTITEMW,
        0,
        (&insert as *const TVINSERTSTRUCTW) as LPARAM,
    ) as isize
}

unsafe fn expand_tree_item(tree: HWND, item: isize) {
    SendMessageW(tree, TVM_EXPAND, TVE_EXPAND as WPARAM, item as LPARAM);
}

unsafe fn insert_mapped_tree_item(
    state: &mut WindowState,
    parent: isize,
    label: &str,
    view_id: ViewId,
) -> isize {
    let item = insert_tree_item_with_state(
        state.controls.tree,
        parent,
        label,
        tree_item_state(state, &view_id),
    );
    state.tree_items.insert(item, view_id.clone());
    state.primary_tree_items.entry(view_id).or_insert(item);
    item
}

unsafe fn render_room_tree_leaf(state: &mut WindowState, parent: isize, room: &RoomSummary) {
    let room_item = insert_mapped_tree_item(
        state,
        parent,
        &room_navigation_label(&state.model, room),
        ViewId::Room(room.room_id.clone()),
    );
    if !is_sendable_room(room) {
        return;
    }
    let threads = state
        .model
        .room_views
        .get(&room.room_id)
        .map(|room_view| {
            room_view
                .threads
                .iter()
                .map(|(thread_root_event_id, thread)| {
                    (thread_root_event_id.clone(), thread.label.clone())
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    for (thread_root_event_id, label) in threads {
        insert_mapped_tree_item(
            state,
            room_item,
            &label,
            ViewId::Thread {
                room_id: room.room_id.clone(),
                thread_root_event_id,
            },
        );
    }
    if state
        .model
        .room_views
        .get(&room.room_id)
        .is_some_and(|room_view| !room_view.threads.is_empty())
    {
        expand_tree_item(state.controls.tree, room_item);
    }
}

fn mark_reachable_space_ids(
    covered: &mut BTreeSet<String>,
    room_map: &BTreeMap<String, RoomSummary>,
    valid_space_children: &BTreeMap<String, Vec<String>>,
    root_id: &str,
) {
    let mut stack = vec![root_id.to_owned()];
    while let Some(space_id) = stack.pop() {
        if !covered.insert(space_id.clone()) {
            continue;
        }
        for child_id in valid_space_children
            .get(&space_id)
            .into_iter()
            .flatten()
            .rev()
        {
            let child = room_map
                .get(child_id)
                .expect("valid space child must exist in room map");
            if child.is_space {
                stack.push(child_id.clone());
            }
        }
    }
}

unsafe fn render_space_tree_children(
    state: &mut WindowState,
    parent: isize,
    room_map: &BTreeMap<String, RoomSummary>,
    valid_space_children: &BTreeMap<String, Vec<String>>,
    space_id: &str,
    ancestor_chain: &[String],
) {
    let mut next_ancestor_chain = ancestor_chain.to_vec();
    next_ancestor_chain.push(space_id.to_owned());

    for child_id in valid_space_children.get(space_id).into_iter().flatten() {
        let child = room_map
            .get(child_id)
            .expect("valid space child must exist in room map");
        if child.is_space {
            let child_item =
                insert_tree_item(state.controls.tree, parent, &space_tree_label(child));
            if !next_ancestor_chain
                .iter()
                .any(|ancestor| ancestor == child_id)
            {
                render_space_tree_children(
                    state,
                    child_item,
                    room_map,
                    valid_space_children,
                    child_id,
                    &next_ancestor_chain,
                );
            }
            expand_tree_item(state.controls.tree, child_item);
        } else {
            render_room_tree_leaf(state, parent, child);
        }
    }
}

unsafe fn render_space_tree(
    state: &mut WindowState,
    parent: isize,
    room_map: &BTreeMap<String, RoomSummary>,
    valid_space_children: &BTreeMap<String, Vec<String>>,
    space_id: &str,
) {
    let space = room_map
        .get(space_id)
        .expect("rendered root space must exist in room map");
    let space_item = insert_tree_item(state.controls.tree, parent, &space_tree_label(space));
    render_space_tree_children(
        state,
        space_item,
        room_map,
        valid_space_children,
        space_id,
        &[],
    );
    expand_tree_item(state.controls.tree, space_item);
}

unsafe fn enable_automation(hwnd: HWND) -> Result<(), String> {
    let response_name = automation_response_name(hwnd);
    let response_name_wide = wide_null(&response_name);
    let mapping = CreateFileMappingW(
        INVALID_HANDLE_VALUE,
        null(),
        PAGE_READWRITE,
        (AUTOMATION_RESPONSE_CAPACITY >> 32) as u32,
        (AUTOMATION_RESPONSE_CAPACITY & u32::MAX as usize) as u32,
        response_name_wide.as_ptr(),
    );
    if mapping.is_null() {
        return Err(last_os_error("CreateFileMappingW"));
    }

    let view = MapViewOfFile(
        mapping,
        FILE_MAP_ALL_ACCESS,
        0,
        0,
        AUTOMATION_RESPONSE_CAPACITY,
    );
    if view.Value.is_null() {
        CloseHandle(mapping);
        return Err(last_os_error("MapViewOfFile"));
    }

    // No ChangeWindowMessageFilterEx: the driver and any user-automation client run
    // at the same integrity level as the app (same user/session), so UIPI never
    // blocks their WM_COPYDATA. Adding the filter would only let lower-integrity
    // processes reach the automation surface.
    (view.Value as *mut u32).write_unaligned(0);
    state_mut(hwnd).automation = Some(AutomationState {
        response_name,
        response_mapping: mapping,
        response_view: view.Value.cast(),
        response_capacity: AUTOMATION_RESPONSE_CAPACITY,
    });
    Ok(())
}

fn automation_response_name(hwnd: HWND) -> String {
    format!(
        "Local\\mmatrix-ui-automation-response-{}-{:x}",
        unsafe { GetCurrentProcessId() },
        hwnd as usize
    )
}

fn destroy_automation_state(state: &mut WindowState) {
    let Some(automation) = state.automation.take() else {
        return;
    };

    unsafe {
        if !automation.response_view.is_null() {
            UnmapViewOfFile(MEMORY_MAPPED_VIEW_ADDRESS {
                Value: automation.response_view.cast(),
            });
        }
        if !automation.response_mapping.is_null() {
            CloseHandle(automation.response_mapping);
        }
    }
}

unsafe fn handle_copydata(hwnd: HWND, lparam: LPARAM) -> LRESULT {
    if state_ref(hwnd).automation.is_none() {
        return 0;
    }

    let (response_id, envelope) = if lparam == 0 {
        (
            None,
            ResponseEnvelope::<Value>::Failure(failure_response(
                None,
                "invalid_request",
                "COPYDATASTRUCT pointer was null",
            )),
        )
    } else {
        let request = &*(lparam as *const COPYDATASTRUCT);
        match copy_copydata_request(request) {
            Ok((command_id, request_bytes)) => {
                match parse_command_request(command_id, &request_bytes) {
                    Ok(command) => {
                        let response_id = Some(command_request_id(&command).to_owned());
                        (response_id, dispatch_command(hwnd, command))
                    }
                    Err(error) => {
                        let response_id = error.id.clone();
                        (response_id, ResponseEnvelope::Failure(error))
                    }
                }
            }
            Err(error) => {
                let response_id = error.id.clone();
                (response_id, ResponseEnvelope::Failure(error))
            }
        }
    };

    let response_bytes = match serialize_response_envelope(&envelope, response_id.clone()) {
        Ok(bytes) => bytes,
        Err(error) => match serialize_failure_envelope(ResponseEnvelope::<Value>::Failure(error)) {
            Some(bytes) => bytes,
            None => return 0,
        },
    };

    if write_response_bytes(hwnd, response_id.as_deref(), &response_bytes) {
        1
    } else {
        0
    }
}

fn copy_copydata_request(request: &COPYDATASTRUCT) -> Result<(u32, Vec<u8>), FailureEnvelope> {
    let command_id = u32::try_from(request.dwData).map_err(|_| {
        failure_response(
            None,
            "invalid_request",
            format!("command id {} exceeds u32 range", request.dwData),
        )
    })?;

    if request.cbData == 0 {
        return Ok((command_id, Vec::new()));
    }

    if request.lpData.is_null() {
        return Err(failure_response(
            None,
            "invalid_request",
            "COPYDATASTRUCT data pointer was null",
        ));
    }

    let request_len = request.cbData as usize;
    let request_bytes =
        unsafe { std::slice::from_raw_parts(request.lpData.cast::<u8>(), request_len) }.to_vec();
    Ok((command_id, request_bytes))
}

unsafe fn dispatch_command(hwnd: HWND, command: ParsedCommand) -> ResponseEnvelope<Value> {
    let response_id = command_request_id(&command).to_owned();
    match execute_command(hwnd, command) {
        Ok(json) => success_response(response_id, json),
        Err(error) => ResponseEnvelope::Failure(error),
    }
}

unsafe fn execute_command(hwnd: HWND, command: ParsedCommand) -> Result<Value, FailureEnvelope> {
    match command {
        ParsedCommand::Ping { id } => serialize_success_json(&id, PongResponse { pong: true }),
        ParsedCommand::GetVersion { id } => {
            serialize_success_json(&id, GetVersionResponse::current())
        }
        ParsedCommand::WaitIdle { id, args } => {
            let _ = args.timeout_ms;
            drain_core_events_and_refresh(hwnd).map_err(|message| {
                failure_response(Some(id.clone()), "state_unavailable", message)
            })?;

            let (pending_core_events, event_generation, synthetic_load_in_progress, idle) = {
                let state = state_mut(hwnd);
                let pending_core_events = state
                    .event_queue
                    .lock()
                    .expect("core event queue poisoned")
                    .len() as u64;
                let event_generation = state.event_generation;
                let synthetic_load_in_progress = state.synthetic_load_in_progress;
                let idle = pending_core_events == 0
                    && !synthetic_load_in_progress
                    && state.last_idle_probe_generation == Some(event_generation);
                state.last_idle_probe_generation = Some(event_generation);
                (
                    pending_core_events,
                    event_generation,
                    synthetic_load_in_progress,
                    idle,
                )
            };

            serialize_success_json(
                &id,
                WaitIdleResponse {
                    idle,
                    event_generation,
                    pending_core_events,
                    synthetic_load_in_progress,
                },
            )
        }
        ParsedCommand::DumpModel { id } => {
            let dump = dump_model_from_state(state_ref(hwnd));
            serialize_success_json(&id, dump)
        }
        ParsedCommand::DumpUiState { id } => {
            drain_core_events_and_refresh(hwnd).map_err(|message| {
                failure_response(Some(id.clone()), "ui_state_unavailable", message)
            })?;
            sync_views_and_tree(hwnd).map_err(|message| {
                failure_response(Some(id.clone()), "ui_state_unavailable", message)
            })?;
            let dump = dump_ui_state(hwnd).map_err(|message| {
                failure_response(Some(id.clone()), "ui_state_unavailable", message)
            })?;
            serialize_success_json(&id, dump)
        }
        ParsedCommand::DumpSelectedNavigationPath { id } => {
            let path = dump_selected_navigation_path(hwnd).map_err(|message| {
                failure_response(Some(id.clone()), "ui_state_unavailable", message)
            })?;
            serialize_success_json(&id, SelectedNavigationPathResponse { path })
        }
        ParsedCommand::DumpRoomActivity { id, args } => {
            let room_id = resolve_timeline_room_id(state_ref(hwnd), args.room_id.as_deref())
                .map_err(|message| failure_response(Some(id.clone()), "unknown_room", message))?;
            let entries = state_ref(hwnd)
                .model
                .room_views
                .get(&room_id)
                .map(|room_view| {
                    room_view
                        .activities
                        .iter()
                        .map(|activity| {
                            dump_room_activity_entry_from_room_entry(room_view, activity)
                        })
                        .collect()
                })
                .unwrap_or_default();
            serialize_success_json(&id, DumpRoomActivityResponse { room_id, entries })
        }
        ParsedCommand::ListRooms { id } => {
            let rooms = state_ref(hwnd)
                .model
                .rooms
                .iter()
                .map(dump_room_from_summary)
                .collect();
            serialize_success_json(&id, ListRoomsResponse { rooms })
        }
        ParsedCommand::SelectRoom { id, args } => {
            activate_view(hwnd, &ViewId::Room(args.room_id.clone()), None)
                .map_err(|message| failure_response(Some(id.clone()), "unknown_room", message))?;
            serialize_success_json(
                &id,
                SelectRoomResponse {
                    selected_room: args.room_id,
                },
            )
        }
        ParsedCommand::SelectView { id, args } => {
            let requested_view = view_id_from_identity(&args.view);
            activate_view(hwnd, &requested_view, None)
                .map_err(|message| failure_response(Some(id.clone()), "unknown_view", message))?;
            let active = active_view_identity(hwnd)
                .map_err(|message| failure_response(Some(id.clone()), "unknown_view", message))?;
            serialize_success_json(&id, SelectViewResponse { active })
        }
        ParsedCommand::FindActiveView { id, args } => {
            if args.query.trim().is_empty() {
                return Err(failure_response(
                    Some(id),
                    "invalid_find_query",
                    "find query must not be empty",
                ));
            }
            let response = execute_active_view_find(hwnd, &args.query)
                .map(find_active_view_response)
                .map_err(|message| failure_response(Some(id.clone()), "find_failed", message))?;
            serialize_success_json(&id, response)
        }
        ParsedCommand::SelectNavigationPath { id, args } => {
            select_navigation_path(hwnd, &args.path).map_err(|message| {
                failure_response(Some(id.clone()), "unknown_navigation", message)
            })?;
            serialize_success_json(&id, AcceptedResponse { accepted: true })
        }
        ParsedCommand::SendText { id, args } => {
            let body = args.body;
            let edit = compose_edit(hwnd)
                .map_err(|message| failure_response(Some(id.clone()), "send_failed", message))?;
            set_edit_text(edit, &body);
            let room_id =
                submit_compose_from_control(hwnd, args.room_id.as_deref()).map_err(|message| {
                    let code = if message.starts_with("unknown room id: ") {
                        "unknown_room"
                    } else {
                        "send_failed"
                    };
                    failure_response(Some(id.clone()), code, message)
                })?;
            serialize_success_json(
                &id,
                SendTextResponse {
                    accepted: true,
                    room_id,
                },
            )
        }
        ParsedCommand::DumpComposeText { id } => {
            let text = dump_compose_text(hwnd).map_err(|message| {
                failure_response(Some(id.clone()), "compose_unavailable", message)
            })?;
            serialize_success_json(&id, ComposeTextResponse { text })
        }
        ParsedCommand::SetComposeText { id, args } => {
            set_compose_text(hwnd, &args.text).map_err(|message| {
                failure_response(Some(id.clone()), "compose_unavailable", message)
            })?;
            serialize_success_json(&id, AcceptedResponse { accepted: true })
        }
        ParsedCommand::SendComposeKey { id, args } => {
            send_compose_key(hwnd, args.key).map_err(|message| {
                failure_response(Some(id.clone()), "compose_key_failed", message)
            })?;
            serialize_success_json(&id, AcceptedResponse { accepted: true })
        }
        ParsedCommand::LoadSyntheticState { id, args } => {
            validate_synthetic_room_activity_events(&BTreeMap::new(), &args.events).map_err(
                |message| failure_response(Some(id.clone()), "invalid_synthetic_state", message),
            )?;
            let (rooms, timeline_events) =
                unsafe { apply_event_load(hwnd, args.events) }.map_err(|message| {
                    failure_response(Some(id.clone()), "invalid_synthetic_state", message)
                })?;
            serialize_success_json(
                &id,
                LoadEventsResponse {
                    loaded: true,
                    rooms,
                    timeline_events,
                },
            )
        }
        ParsedCommand::AppendSyntheticEvents { id, args } => {
            let room_views = state_ref(hwnd).model.room_views.clone();
            validate_synthetic_room_activity_events(&room_views, &args.events).map_err(
                |message| failure_response(Some(id.clone()), "invalid_synthetic_state", message),
            )?;
            unsafe { apply_events_and_refresh(hwnd, args.events) }.map_err(|message| {
                failure_response(Some(id.clone()), "invalid_synthetic_state", message)
            })?;
            let rooms = state_ref(hwnd).model.rooms.len();
            let timeline_events = state_ref(hwnd)
                .model
                .room_views
                .values()
                .map(|room_view| room_view.activities.len())
                .sum();
            serialize_success_json(
                &id,
                LoadEventsResponse {
                    loaded: true,
                    rooms,
                    timeline_events,
                },
            )
        }
        ParsedCommand::Login { id, args } => {
            submit_login(
                state_mut(hwnd),
                args.homeserver_url,
                args.username,
                args.password,
                args.device_display_name,
            )
            .map_err(|message| failure_response(Some(id.clone()), "login_failed", message))?;
            refresh_status_bar(hwnd)
                .map_err(|message| failure_response(Some(id.clone()), "login_failed", message))?;
            serialize_success_json(&id, AcceptedResponse { accepted: true })
        }
        ParsedCommand::RestoreSession { id, args } => {
            submit_restore_session(state_mut(hwnd), args.homeserver_url, args.session)
                .map_err(|message| failure_response(Some(id.clone()), "restore_failed", message))?;
            refresh_status_bar(hwnd)
                .map_err(|message| failure_response(Some(id.clone()), "restore_failed", message))?;
            serialize_success_json(&id, AcceptedResponse { accepted: true })
        }
        ParsedCommand::RestoreSavedSession { id } => {
            submit_restore_saved_session(state_mut(hwnd)).map_err(|message| {
                failure_response(Some(id.clone()), "restore_saved_failed", message)
            })?;
            refresh_status_bar(hwnd).map_err(|message| {
                failure_response(Some(id.clone()), "restore_saved_failed", message)
            })?;
            serialize_success_json(&id, AcceptedResponse { accepted: true })
        }
        ParsedCommand::StartSync { id } => {
            submit_start_sync(state_mut(hwnd)).map_err(|message| {
                failure_response(Some(id.clone()), "start_sync_failed", message)
            })?;
            serialize_success_json(&id, AcceptedResponse { accepted: true })
        }
        ParsedCommand::Logout { id } => {
            submit_logout(state_mut(hwnd))
                .map_err(|message| failure_response(Some(id.clone()), "logout_failed", message))?;
            serialize_success_json(&id, AcceptedResponse { accepted: true })
        }
        ParsedCommand::RecoverE2ee { id, args } => {
            submit_recover_e2ee(state_mut(hwnd), args.recovery_secret).map_err(|message| {
                failure_response(Some(id.clone()), "recover_e2ee_failed", message)
            })?;
            refresh_status_bar(hwnd).map_err(|message| {
                failure_response(Some(id.clone()), "recover_e2ee_failed", message)
            })?;
            serialize_success_json(&id, AcceptedResponse { accepted: true })
        }
        ParsedCommand::AcceptVerification { id, args } => {
            submit_automation_verification_action(hwnd, VerificationAction::Accept, args).map_err(
                |message| failure_response(Some(id.clone()), "verification_failed", message),
            )?;
            serialize_success_json(&id, AcceptedResponse { accepted: true })
        }
        ParsedCommand::CancelVerification { id, args } => {
            submit_automation_verification_action(hwnd, VerificationAction::Cancel, args).map_err(
                |message| failure_response(Some(id.clone()), "verification_failed", message),
            )?;
            serialize_success_json(&id, AcceptedResponse { accepted: true })
        }
        ParsedCommand::ConfirmVerificationSas { id, args } => {
            submit_automation_verification_action(hwnd, VerificationAction::Confirm, args)
                .map_err(|message| {
                    failure_response(Some(id.clone()), "verification_failed", message)
                })?;
            serialize_success_json(&id, AcceptedResponse { accepted: true })
        }
        ParsedCommand::RejectVerificationSas { id, args } => {
            submit_automation_verification_action(hwnd, VerificationAction::Reject, args).map_err(
                |message| failure_response(Some(id.clone()), "verification_failed", message),
            )?;
            serialize_success_json(&id, AcceptedResponse { accepted: true })
        }
    }
}

fn command_request_id(command: &ParsedCommand) -> &str {
    match command {
        ParsedCommand::Ping { id }
        | ParsedCommand::GetVersion { id }
        | ParsedCommand::WaitIdle { id, .. }
        | ParsedCommand::DumpModel { id }
        | ParsedCommand::DumpUiState { id }
        | ParsedCommand::DumpSelectedNavigationPath { id }
        | ParsedCommand::DumpRoomActivity { id, .. }
        | ParsedCommand::ListRooms { id }
        | ParsedCommand::SelectRoom { id, .. }
        | ParsedCommand::SelectView { id, .. }
        | ParsedCommand::SendText { id, .. }
        | ParsedCommand::LoadSyntheticState { id, .. }
        | ParsedCommand::Login { id, .. }
        | ParsedCommand::RestoreSession { id, .. }
        | ParsedCommand::RestoreSavedSession { id }
        | ParsedCommand::StartSync { id }
        | ParsedCommand::Logout { id }
        | ParsedCommand::DumpComposeText { id }
        | ParsedCommand::SetComposeText { id, .. }
        | ParsedCommand::SendComposeKey { id, .. }
        | ParsedCommand::SelectNavigationPath { id, .. }
        | ParsedCommand::FindActiveView { id, .. }
        | ParsedCommand::AppendSyntheticEvents { id, .. }
        | ParsedCommand::RecoverE2ee { id, .. }
        | ParsedCommand::AcceptVerification { id, .. }
        | ParsedCommand::CancelVerification { id, .. }
        | ParsedCommand::ConfirmVerificationSas { id, .. }
        | ParsedCommand::RejectVerificationSas { id, .. } => id,
    }
}

fn serialize_success_json<T: Serialize>(id: &str, payload: T) -> Result<Value, FailureEnvelope> {
    serde_json::to_value(payload).map_err(|err| {
        failure_response(
            Some(id.to_owned()),
            "serialization_error",
            format!("failed to serialize response payload: {err}"),
        )
    })
}

fn serialize_response_envelope(
    envelope: &ResponseEnvelope<Value>,
    response_id: Option<String>,
) -> Result<Vec<u8>, FailureEnvelope> {
    serde_json::to_vec(envelope).map_err(|err| {
        failure_response(
            response_id,
            "serialization_error",
            format!("failed to serialize response envelope: {err}"),
        )
    })
}

fn serialize_failure_envelope(envelope: ResponseEnvelope<Value>) -> Option<Vec<u8>> {
    serde_json::to_vec(&envelope).ok()
}

unsafe fn write_response_bytes(
    hwnd: HWND,
    response_id: Option<&str>,
    response_bytes: &[u8],
) -> bool {
    let Some(automation) = state_ref(hwnd).automation.as_ref() else {
        return false;
    };

    if write_response_bytes_if_fits(automation, response_bytes) {
        return true;
    }

    let fallback = ResponseEnvelope::<Value>::Failure(failure_response(
        response_id.map(str::to_owned),
        "response_too_large",
        format!(
            "response exceeded shared-memory capacity {}",
            automation.response_capacity
        ),
    ));
    let Some(fallback_bytes) = serialize_failure_envelope(fallback) else {
        return false;
    };
    write_response_bytes_if_fits(automation, &fallback_bytes)
}

fn write_response_bytes_if_fits(automation: &AutomationState, response_bytes: &[u8]) -> bool {
    if automation.response_view.is_null() || automation.response_capacity < size_of::<u32>() {
        return false;
    }

    let Ok(response_len) = u32::try_from(response_bytes.len()) else {
        return false;
    };
    let Some(required_capacity) = size_of::<u32>().checked_add(response_bytes.len()) else {
        return false;
    };
    if required_capacity > automation.response_capacity {
        return false;
    }

    unsafe {
        let buffer =
            std::slice::from_raw_parts_mut(automation.response_view, automation.response_capacity);
        buffer[..size_of::<u32>()].copy_from_slice(&response_len.to_le_bytes());
        buffer[size_of::<u32>()..required_capacity].copy_from_slice(response_bytes);
    }
    true
}

fn split_rendered_lines(text: &str) -> Vec<String> {
    if text.is_empty() {
        return Vec::new();
    }

    text.split('\n')
        .map(|line| line.trim_end_matches('\r').to_owned())
        .collect()
}

fn normalize_timeline_text(text: &str) -> (String, Vec<String>) {
    let lines = split_rendered_lines(text);
    let normalized = render_view_text(&lines);
    (normalized, lines)
}

fn ordered_view_ids(state: &WindowState) -> Vec<ViewId> {
    let mut ordered = Vec::new();
    if state.views.contains_key(&ViewId::Status) {
        ordered.push(ViewId::Status);
    }

    for room in &state.model.rooms {
        if !is_joined_message_room(room) {
            continue;
        }
        let view_id = ViewId::Room(room.room_id.clone());
        if state.views.contains_key(&view_id) {
            ordered.push(view_id);
        }
        if let Some(room_view) = state.model.room_views.get(&room.room_id) {
            for thread_root_event_id in room_view.threads.keys() {
                let thread_view_id = ViewId::Thread {
                    room_id: room.room_id.clone(),
                    thread_root_event_id: thread_root_event_id.clone(),
                };
                if state.views.contains_key(&thread_view_id) {
                    ordered.push(thread_view_id);
                }
            }
        }
    }

    for room in &state.model.rooms {
        if !is_invited_room(room) {
            continue;
        }
        let view_id = ViewId::Room(room.room_id.clone());
        if state.views.contains_key(&view_id) {
            ordered.push(view_id);
        }
    }

    let mut remaining = state
        .views
        .keys()
        .filter_map(|view_id| match view_id {
            ViewId::Status => None,
            ViewId::Room(room_id)
                if navigable_room_by_id(&state.model.rooms, room_id).is_none() =>
            {
                Some(room_id.clone())
            }
            ViewId::Room(_) => None,
            ViewId::Thread { .. } => None,
        })
        .collect::<Vec<_>>();
    remaining.sort();
    ordered.extend(remaining.into_iter().map(ViewId::Room));
    ordered
}

unsafe fn rebuild_switchbar(hwnd: HWND) -> Result<(), String> {
    let switchbar = state_ref(hwnd).controls.switchbar;
    if switchbar.is_null() {
        return Err("switchbar control was not created".to_owned());
    }

    loop {
        let count = SendMessageW(switchbar, TB_BUTTONCOUNT, 0, 0);
        if count < 0 {
            return Err("failed to query switchbar button count".to_owned());
        }
        if count == 0 {
            break;
        }
        if SendMessageW(switchbar, TB_DELETEBUTTON, 0, 0) == 0 {
            return Err("failed to delete switchbar button".to_owned());
        }
    }

    let view_ids = ordered_view_ids(state_ref(hwnd));
    let button_count = view_ids.len();
    if SWITCHBAR_COMMAND_FIRST
        .checked_add(button_count.saturating_sub(1))
        .is_some_and(|command_id| command_id > SWITCHBAR_COMMAND_LAST)
    {
        return Err("switchbar command range exhausted".to_owned());
    }

    let labels = {
        let state = state_ref(hwnd);
        view_ids
            .iter()
            .map(|view_id| wide_null(&switchbar_button_text(state, view_id)))
            .collect::<Vec<_>>()
    };
    let button_states = {
        let state = state_ref(hwnd);
        view_ids
            .iter()
            .map(|view_id| switchbar_button_state(state, view_id))
            .collect::<Vec<_>>()
    };
    let buttons = view_ids
        .iter()
        .enumerate()
        .map(|(index, _)| TBBUTTON {
            iBitmap: I_IMAGENONE,
            idCommand: (SWITCHBAR_COMMAND_FIRST + index) as i32,
            fsState: button_states[index],
            fsStyle: (BTNS_CHECK | BTNS_SHOWTEXT) as u8,
            iString: labels[index].as_ptr() as isize,
            ..TBBUTTON::default()
        })
        .collect::<Vec<_>>();

    if !buttons.is_empty()
        && SendMessageW(
            switchbar,
            TB_ADDBUTTONSW,
            buttons.len(),
            buttons.as_ptr() as LPARAM,
        ) == 0
    {
        return Err("failed to add switchbar buttons".to_owned());
    }
    SendMessageW(switchbar, TB_AUTOSIZE, 0, 0);

    state_mut(hwnd).switchbar_views = view_ids;
    refresh_switchbar_state(hwnd)
}

unsafe fn refresh_switchbar_state(hwnd: HWND) -> Result<(), String> {
    let switchbar = state_ref(hwnd).controls.switchbar;
    if switchbar.is_null() {
        return Err("switchbar control was not created".to_owned());
    }

    let view_ids = state_ref(hwnd).switchbar_views.clone();
    for (index, view_id) in view_ids.iter().enumerate() {
        let command_id = SWITCHBAR_COMMAND_FIRST + index;
        let button_state = {
            let state = state_ref(hwnd);
            switchbar_button_state(state, view_id)
        };
        if SendMessageW(switchbar, TB_SETSTATE, command_id, button_state as LPARAM) == 0 {
            return Err(format!("failed to refresh switchbar button {command_id}"));
        }
    }

    Ok(())
}

unsafe fn refresh_tree_item_activity_state(hwnd: HWND) -> Result<(), String> {
    let tree = state_ref(hwnd).controls.tree;
    if tree.is_null() {
        return Err("tree control was not created".to_owned());
    }

    let updates = {
        let state = state_ref(hwnd);
        state
            .tree_items
            .iter()
            .map(|(tree_item, view_id)| (*tree_item, tree_item_state(state, view_id)))
            .collect::<Vec<_>>()
    };

    for (tree_item, item_state) in updates {
        let mut item = TVITEMEXW {
            mask: TVIF_STATE,
            hItem: tree_item as _,
            state: item_state,
            stateMask: TVIS_BOLD,
            ..zeroed()
        };
        if SendMessageW(
            tree,
            TVM_SETITEMW,
            0,
            (&mut item as *mut TVITEMEXW) as LPARAM,
        ) == 0
        {
            return Err(format!(
                "failed to refresh tree item activity state for handle 0x{tree_item:x}"
            ));
        }
    }

    Ok(())
}

unsafe fn read_tree_item_text(tree: HWND, item: isize) -> Result<String, String> {
    let mut buffer = vec![0u16; 1024];
    let mut tree_item = TVITEMEXW {
        mask: TVIF_TEXT,
        hItem: item as _,
        pszText: buffer.as_mut_ptr(),
        cchTextMax: buffer.len() as i32,
        ..zeroed()
    };

    if SendMessageW(
        tree,
        TVM_GETITEMW,
        0,
        (&mut tree_item as *mut TVITEMEXW) as LPARAM,
    ) == 0
    {
        return Err(format!(
            "failed to read tree item text for handle 0x{item:x}"
        ));
    }

    let text_len = buffer
        .iter()
        .position(|ch| *ch == 0)
        .unwrap_or(buffer.len());
    Ok(String::from_utf16_lossy(&buffer[..text_len]))
}

unsafe fn read_tree_item_state(tree: HWND, item: isize) -> Result<u32, String> {
    let mut tree_item = TVITEMEXW {
        mask: TVIF_STATE,
        hItem: item as _,
        stateMask: TVIS_BOLD,
        ..zeroed()
    };

    if SendMessageW(
        tree,
        TVM_GETITEMW,
        0,
        (&mut tree_item as *mut TVITEMEXW) as LPARAM,
    ) == 0
    {
        return Err(format!(
            "failed to read tree item state for handle 0x{item:x}"
        ));
    }

    Ok(tree_item.state)
}

unsafe fn read_navigation_node(tree: HWND, item: isize) -> Result<NavigationNode, String> {
    let text = read_tree_item_text(tree, item)?;
    let has_activity = read_tree_item_state(tree, item)? & TVIS_BOLD != 0;
    let mut children = Vec::new();
    let mut child =
        SendMessageW(tree, TVM_GETNEXTITEM, TVGN_CHILD as WPARAM, item as LPARAM) as isize;
    while child != 0 {
        children.push(read_navigation_node(tree, child)?);
        child = SendMessageW(tree, TVM_GETNEXTITEM, TVGN_NEXT as WPARAM, child as LPARAM) as isize;
    }

    Ok(NavigationNode {
        text,
        has_activity,
        children,
    })
}

unsafe fn dump_navigation_tree(hwnd: HWND) -> Result<NavigationNode, String> {
    let tree = state_ref(hwnd).controls.tree;
    if tree.is_null() {
        return Err("tree control was not created".to_owned());
    }

    let root = SendMessageW(tree, TVM_GETNEXTITEM, TVGN_ROOT as WPARAM, 0) as isize;
    if root == 0 {
        return Err("navigation tree root was missing".to_owned());
    }

    read_navigation_node(tree, root)
}

fn format_navigation_path(path: &[NavigationPathSegment]) -> String {
    path.iter()
        .map(|segment| {
            if segment.same_text_index == 0 {
                segment.text.clone()
            } else {
                format!("{} [{}]", segment.text, segment.same_text_index)
            }
        })
        .collect::<Vec<_>>()
        .join(" > ")
}

unsafe fn same_text_index_for_tree_item(tree: HWND, item: isize) -> Result<u32, String> {
    let parent =
        SendMessageW(tree, TVM_GETNEXTITEM, TVGN_PARENT as WPARAM, item as LPARAM) as isize;
    let target_text = read_tree_item_text(tree, item)?;
    let mut current = if parent == 0 {
        SendMessageW(tree, TVM_GETNEXTITEM, TVGN_ROOT as WPARAM, 0) as isize
    } else {
        SendMessageW(
            tree,
            TVM_GETNEXTITEM,
            TVGN_CHILD as WPARAM,
            parent as LPARAM,
        ) as isize
    };
    let mut same_text_index = 0u32;
    while current != 0 {
        if read_tree_item_text(tree, current)? == target_text {
            if current == item {
                return Ok(same_text_index);
            }
            same_text_index += 1;
        }
        current = SendMessageW(
            tree,
            TVM_GETNEXTITEM,
            TVGN_NEXT as WPARAM,
            current as LPARAM,
        ) as isize;
    }

    Err(format!(
        "tree item 0x{item:x} was not found among its siblings"
    ))
}

unsafe fn dump_selected_navigation_path(hwnd: HWND) -> Result<Vec<NavigationPathSegment>, String> {
    let tree = state_ref(hwnd).controls.tree;
    if tree.is_null() {
        return Err("tree control was not created".to_owned());
    }

    let mut item = SendMessageW(tree, TVM_GETNEXTITEM, TVGN_CARET as WPARAM, 0) as isize;
    if item == 0 {
        return Err("navigation tree selection was missing".to_owned());
    }

    let mut path = Vec::new();
    while item != 0 {
        path.push(NavigationPathSegment {
            text: read_tree_item_text(tree, item)?,
            same_text_index: same_text_index_for_tree_item(tree, item)?,
        });
        item = SendMessageW(tree, TVM_GETNEXTITEM, TVGN_PARENT as WPARAM, item as LPARAM) as isize;
    }
    path.reverse();
    Ok(path)
}

unsafe fn select_navigation_path(hwnd: HWND, path: &[NavigationPathSegment]) -> Result<(), String> {
    if path.is_empty() {
        return Err("navigation path must not be empty".to_owned());
    }

    let tree = state_ref(hwnd).controls.tree;
    if tree.is_null() {
        return Err("tree control was not created".to_owned());
    }

    let mut current = SendMessageW(tree, TVM_GETNEXTITEM, TVGN_ROOT as WPARAM, 0) as isize;
    if current == 0 {
        return Err("navigation tree root was missing".to_owned());
    }

    let root_text = read_tree_item_text(tree, current)?;
    let root_same_text_index = same_text_index_for_tree_item(tree, current)?;
    if root_text != path[0].text || root_same_text_index != path[0].same_text_index {
        return Err(format!(
            "unknown navigation path: {}",
            format_navigation_path(path)
        ));
    }

    for segment in &path[1..] {
        let mut child = SendMessageW(
            tree,
            TVM_GETNEXTITEM,
            TVGN_CHILD as WPARAM,
            current as LPARAM,
        ) as isize;
        let mut matched_child = None;
        let mut same_text_index = 0u32;
        while child != 0 {
            if read_tree_item_text(tree, child)? == segment.text {
                if same_text_index == segment.same_text_index {
                    matched_child = Some(child);
                    break;
                }
                same_text_index += 1;
            }
            child =
                SendMessageW(tree, TVM_GETNEXTITEM, TVGN_NEXT as WPARAM, child as LPARAM) as isize;
        }

        current = matched_child
            .ok_or_else(|| format!("unknown navigation path: {}", format_navigation_path(path)))?;
    }

    let selected = SendMessageW(tree, TVM_GETNEXTITEM, TVGN_CARET as WPARAM, 0) as isize;
    if selected != current {
        SendMessageW(
            tree,
            TVM_SELECTITEM,
            TVGN_CARET as WPARAM,
            current as LPARAM,
        );
    }
    let view_id = state_ref(hwnd).tree_items.get(&current).cloned();
    if let Some(view_id) = view_id {
        activate_view(hwnd, &view_id, Some(current))?;
    }

    Ok(())
}

unsafe fn dump_status_bar_state(hwnd: HWND) -> Result<StatusBarState, String> {
    if state_ref(hwnd).controls.status_bar.is_null() {
        return Err("status bar control was not created".to_owned());
    }

    Ok(StatusBarState {
        text: frame_status_text(state_ref(hwnd)),
    })
}

unsafe fn active_view_identity(hwnd: HWND) -> Result<ViewIdentity, String> {
    let mdi_client = state_ref(hwnd).controls.mdi_client;
    if mdi_client.is_null() {
        return Err("MDI client control was not created".to_owned());
    }

    let active_child = SendMessageW(mdi_client, WM_MDIGETACTIVE, 0, 0) as HWND;
    if !active_child.is_null() {
        let view_id = state_ref(hwnd)
            .hwnd_views
            .get(&(active_child as isize))
            .cloned()
            .ok_or_else(|| {
                format!(
                    "active MDI child 0x{:x} was not tracked",
                    active_child as usize
                )
            })?;
        return Ok(view_identity_from_view_id(&view_id));
    }

    let active_view = &state_ref(hwnd).active_view;
    if state_ref(hwnd).views.contains_key(active_view) {
        Ok(view_identity_from_view_id(active_view))
    } else {
        Err("active view was not tracked".to_owned())
    }
}

unsafe fn dump_mdi_state(hwnd: HWND) -> Result<MdiState, String> {
    let active = active_view_identity(hwnd)?;
    let view_ids = ordered_view_ids(state_ref(hwnd));
    let views = view_ids
        .into_iter()
        .map(|view_id| {
            let view = state_ref(hwnd)
                .views
                .get(&view_id)
                .ok_or_else(|| "view state was not tracked".to_owned())?;
            if !cached_view_is_alive(view) {
                return Err(format!(
                    "tracked view {:?} referenced a dead child or content control",
                    view_id
                ));
            }

            let identity = view_identity_from_view_id(&view_id);
            Ok(MdiView {
                title: read_window_text(view.child),
                active: identity == active,
                lines: match &view_id {
                    ViewId::Status => split_rendered_lines(&read_window_text(view.content)),
                    ViewId::Room(_) => {
                        split_rendered_lines(&read_window_text(room_content_timeline(view.content)))
                    }
                    ViewId::Thread { .. } => {
                        split_rendered_lines(&read_window_text(room_content_timeline(view.content)))
                    }
                },
                members: match &view_id {
                    ViewId::Status => Vec::new(),
                    ViewId::Room(room_id) => room_by_id(&state_ref(hwnd).model.rooms, room_id)
                        .filter(|room| is_sendable_room(room))
                        .map(|_| {
                            room_member_view_rows(
                                state_ref(hwnd)
                                    .model
                                    .room_views
                                    .get(room_id)
                                    .map(|room_view| room_view.members.as_slice())
                                    .unwrap_or_default(),
                            )
                        })
                        .unwrap_or_default(),
                    ViewId::Thread { room_id, .. } => room_member_view_rows(
                        state_ref(hwnd)
                            .model
                            .room_views
                            .get(room_id)
                            .map(|room_view| room_view.members.as_slice())
                            .unwrap_or_default(),
                    ),
                },
                identity,
            })
        })
        .collect::<Result<Vec<_>, String>>()?;

    Ok(MdiState { active, views })
}

unsafe fn dump_switchbar_state(hwnd: HWND) -> Result<SwitchbarState, String> {
    if state_ref(hwnd).controls.switchbar.is_null() {
        return Err("switchbar control was not created".to_owned());
    }

    let active = active_view_identity(hwnd)?;
    let state = state_ref(hwnd);
    let items = state
        .switchbar_views
        .clone()
        .into_iter()
        .map(|view_id| SwitchbarItem {
            identity: view_identity_from_view_id(&view_id),
            title: switchbar_button_text(state, &view_id),
            active: view_identity_from_view_id(&view_id) == active,
            has_activity: state.room_activity.contains(&view_id),
        })
        .collect();

    Ok(SwitchbarState { items })
}

fn dump_active_find_state(state: &WindowState) -> Option<UiFindState> {
    state
        .find_states
        .get(&state.active_view)
        .map(|find_state| dump_find_state(&state.active_view, find_state))
}

unsafe fn dump_ui_state(hwnd: HWND) -> Result<UiState, String> {
    let navigation = dump_navigation_tree(hwnd)?;
    let mdi = dump_mdi_state(hwnd)?;
    let status_bar = dump_status_bar_state(hwnd)?;
    let switchbar = dump_switchbar_state(hwnd)?;
    let state = state_ref(hwnd);

    Ok(UiState {
        navigation,
        mdi,
        status_bar,
        switchbar,
        active_find: dump_active_find_state(state),
        rooms: state
            .model
            .rooms
            .iter()
            .map(|room| UiRoomState {
                history_backfill: state
                    .model
                    .room_views
                    .get(&room.room_id)
                    .map_or(UiRoomHistoryBackfillState::Idle, |room_view| {
                        dump_ui_room_history_backfill_state(&room_view.history)
                    }),
                room_id: room.room_id.clone(),
                display_name: room.display_name.clone(),
                membership: room.membership,
                encrypted: room.encrypted,
                is_space: room.is_space,
                is_direct: room.is_direct,
                direct_user_ids: room.direct_user_ids.clone(),
                tags: room
                    .tags
                    .iter()
                    .map(|tag| crate::automation::DumpRoomTag {
                        tag: tag.tag.clone(),
                        order: tag.order.clone(),
                    })
                    .collect(),
                space_children: room
                    .space_children
                    .iter()
                    .map(|child| crate::automation::DumpSpaceChild {
                        room_id: child.room_id.clone(),
                        suggested: child.suggested,
                        order: child.order.clone(),
                    })
                    .collect(),
                has_activity: state
                    .room_activity
                    .contains(&ViewId::Room(room.room_id.clone())),
                read_target_event_id: state
                    .room_activity_read_targets
                    .get(&ViewId::Room(room.room_id.clone()))
                    .cloned(),
                read_state: state
                    .model
                    .room_read_states
                    .get(&room.room_id)
                    .map(dump_ui_room_read_state_from_core),
            })
            .collect(),
    })
}

fn dump_ui_room_history_backfill_state(
    history: &RoomHistoryBackfillState,
) -> UiRoomHistoryBackfillState {
    match &history.status {
        RoomHistoryBackfillStatus::Idle => UiRoomHistoryBackfillState::Idle,
        RoomHistoryBackfillStatus::Loading { from } => {
            UiRoomHistoryBackfillState::Loading { from: from.clone() }
        }
        RoomHistoryBackfillStatus::Failed { from, message } => UiRoomHistoryBackfillState::Failed {
            from: from.clone(),
            message: message.clone(),
        },
        RoomHistoryBackfillStatus::Exhausted => UiRoomHistoryBackfillState::Exhausted,
    }
}

unsafe fn send_automation_request(
    hwnd: HWND,
    command_id: u32,
    request_json: Value,
) -> Result<Value, String> {
    let request_bytes = serde_json::to_vec(&request_json)
        .map_err(|err| format!("failed to serialize self-test request: {err}"))?;
    let request_len = u32::try_from(request_bytes.len())
        .map_err(|_| "self-test request exceeded WM_COPYDATA size range".to_owned())?;
    let mut request = COPYDATASTRUCT {
        dwData: command_id as usize,
        cbData: request_len,
        lpData: request_bytes.as_ptr().cast_mut().cast(),
    };

    let send_result = SendMessageW(
        hwnd,
        WM_COPYDATA,
        hwnd as WPARAM,
        (&mut request as *mut COPYDATASTRUCT) as LPARAM,
    );
    if send_result == 0 {
        return Err(format!(
            "automation transport returned no response for command id {command_id}"
        ));
    }

    read_automation_response(hwnd)
}

unsafe fn read_automation_response(hwnd: HWND) -> Result<Value, String> {
    let automation = state_ref(hwnd)
        .automation
        .as_ref()
        .ok_or_else(|| "automation shared memory is not enabled".to_owned())?;
    if automation.response_view.is_null() || automation.response_capacity < size_of::<u32>() {
        return Err(format!(
            "automation shared memory {} is not mapped",
            automation.response_name
        ));
    }

    let buffer = std::slice::from_raw_parts(automation.response_view, automation.response_capacity);
    let response_len = u32::from_le_bytes(buffer[..size_of::<u32>()].try_into().unwrap()) as usize;
    let required_capacity = size_of::<u32>()
        .checked_add(response_len)
        .ok_or_else(|| "automation response length overflowed".to_owned())?;
    if required_capacity > automation.response_capacity {
        return Err(format!(
            "automation response from {} exceeded mapped capacity {}",
            automation.response_name, automation.response_capacity
        ));
    }

    serde_json::from_slice(&buffer[size_of::<u32>()..required_capacity])
        .map_err(|err| format!("failed to parse automation response JSON: {err}"))
}

unsafe fn run_self_test(hwnd: HWND) -> Result<(), String> {
    let edit = state_ref(hwnd).controls.edit;
    if edit.is_null() {
        return Err("self-test failed: edit control was not created".to_owned());
    }

    let expected_text = "self-test message";
    let expected = wide_null(expected_text);
    SendMessageW(edit, WM_SETTEXT, 0, expected.as_ptr() as LPARAM);

    let actual = read_window_text(edit);
    if actual.is_empty() {
        return Err("self-test failed: WM_GETTEXTLENGTH returned no text".to_owned());
    }
    if actual != expected_text {
        return Err(format!(
            "self-test failed: expected edit text \"{expected_text}\", got \"{actual}\""
        ));
    }

    let compose_text = send_automation_request(
        hwnd,
        COMMAND_ID_DUMP_COMPOSE_TEXT,
        json!({"id":"compose-text","args":{}}),
    )?;
    if compose_text["ok"] != json!(true) || compose_text["json"] != json!({"text": expected_text}) {
        return Err(format!(
            "self-test failed: unexpected compose-text response {compose_text}"
        ));
    }

    let ping = send_automation_request(hwnd, COMMAND_ID_PING, json!({"id":"ping","args":{}}))?;
    if ping["ok"] != json!(true) || ping["json"] != json!({"pong": true}) {
        return Err(format!("self-test failed: unexpected ping response {ping}"));
    }

    let events = vec![
        Event::Started,
        Event::SyncedRooms(vec![
            RoomSummary {
                room_id: "!one:example".to_owned(),
                display_name: "One".to_owned(),
                membership: RoomMembership::Join,
                encrypted: false,
                is_space: false,
                is_direct: false,
                direct_user_ids: Vec::new(),
                tags: Vec::new(),
                space_children: Vec::new(),
            },
            RoomSummary {
                room_id: "!two:example".to_owned(),
                display_name: "Two".to_owned(),
                membership: RoomMembership::Join,
                encrypted: true,
                is_space: false,
                is_direct: false,
                direct_user_ids: Vec::new(),
                tags: Vec::new(),
                space_children: Vec::new(),
            },
        ]),
        Event::RoomActivity(Box::new(RoomActivity {
            room_id: "!one:example".to_owned(),
            activity_id: "event:$one-first".to_owned(),
            timestamp_ms: 1_700_000_000_000,
            relation: None,
            kind: RoomActivityKind::Message(RoomMessageActivity {
                event_id: Some("$one-first".to_owned()),
                transaction_id: None,
                sender: "@alice:example".to_owned(),
                presentation: RoomMessagePresentationKind::Text,
                body: "first line".to_owned(),
                send_state: RoomMessageSendState::Sent,
            }),
        })),
        Event::RoomActivity(Box::new(RoomActivity {
            room_id: "!one:example".to_owned(),
            activity_id: "event:$one-second".to_owned(),
            timestamp_ms: 1_700_000_060_000,
            relation: None,
            kind: RoomActivityKind::Message(RoomMessageActivity {
                event_id: Some("$one-second".to_owned()),
                transaction_id: None,
                sender: "@alice:example".to_owned(),
                presentation: RoomMessagePresentationKind::Text,
                body: "second line".to_owned(),
                send_state: RoomMessageSendState::Sent,
            }),
        })),
        Event::RoomActivity(Box::new(RoomActivity {
            room_id: "!two:example".to_owned(),
            activity_id: "event:$two-third".to_owned(),
            timestamp_ms: 1_700_000_120_000,
            relation: None,
            kind: RoomActivityKind::Message(RoomMessageActivity {
                event_id: Some("$two-third".to_owned()),
                transaction_id: None,
                sender: "@bob:example".to_owned(),
                presentation: RoomMessagePresentationKind::Text,
                body: "third line".to_owned(),
                send_state: RoomMessageSendState::Sent,
            }),
        })),
    ];
    let load_response = send_automation_request(
        hwnd,
        COMMAND_ID_LOAD_SYNTHETIC_STATE,
        json!({"id":"load","args":{"events":events}}),
    )?;
    if load_response["ok"] != json!(true)
        || load_response["json"] != json!({"loaded": true, "rooms": 2, "timeline_events": 3})
    {
        return Err(format!(
            "self-test failed: unexpected synthetic-load response {load_response}"
        ));
    }

    let mut idle = false;
    for _ in 0..8 {
        let wait_idle = send_automation_request(
            hwnd,
            COMMAND_ID_WAIT_IDLE,
            json!({"id":"idle","args":{"timeout_ms":10}}),
        )?;
        if wait_idle["ok"] != json!(true) {
            return Err(format!(
                "self-test failed: wait-idle returned error {wait_idle}"
            ));
        }
        if wait_idle["json"]["idle"] == json!(true) {
            idle = true;
            break;
        }
    }
    if !idle {
        return Err("self-test failed: WAIT_IDLE never reported idle".to_owned());
    }

    let timeline = send_automation_request(
        hwnd,
        COMMAND_ID_DUMP_ROOM_ACTIVITY,
        json!({"id":"timeline","args":{"room_id":"!one:example"}}),
    )?;
    if timeline["ok"] != json!(true)
        || timeline["json"]
            != json!({
                "room_id":"!one:example",
                "entries":[
                    {
                        "room_id":"!one:example",
                        "activity_id":"event:$one-first",
                        "timestamp_ms":1700000000000u64,
                        "reactions":[],
                        "relation":null,
                        "kind":{
                            "type":"message",
                            "event_id":"$one-first",
                            "transaction_id":null,
                            "sender":"@alice:example",
                            "presentation":"text",
                            "body":"first line",
                            "send_state":{"state":"sent"}
                        }
                    },
                    {
                        "room_id":"!one:example",
                        "activity_id":"event:$one-second",
                        "timestamp_ms":1700000060000u64,
                        "reactions":[],
                        "relation":null,
                        "kind":{
                            "type":"message",
                            "event_id":"$one-second",
                            "transaction_id":null,
                            "sender":"@alice:example",
                            "presentation":"text",
                            "body":"second line",
                            "send_state":{"state":"sent"}
                        }
                    }
                ]
            })
    {
        return Err(format!(
            "self-test failed: unexpected dump-room-activity response {timeline}"
        ));
    }

    println!("{}", mmatrix_core::ui_self_test_line());
    Ok(())
}

unsafe fn drain_core_events(hwnd: HWND) -> Vec<Event> {
    let queue = &state_ref(hwnd).event_queue;
    let mut queue = queue.lock().expect("core event queue poisoned");
    queue.drain(..).collect()
}

unsafe fn apply_events_and_refresh(hwnd: HWND, events: Vec<Event>) -> Result<(), String> {
    if events.is_empty() {
        return Ok(());
    }

    let mut history_backfill_anchor = None;
    {
        let state = state_mut(hwnd);
        for event in events {
            if history_backfill_anchor.is_none()
                && let Event::RoomHistoryBackfill(backfill) = &event
            {
                history_backfill_anchor =
                    capture_room_history_backfill_anchor(state, &backfill.room_id);
            }
            apply_core_event(state, event)?;
            state.event_generation += 1;
        }
    }

    sync_views_and_tree(hwnd)?;
    if let Some(anchor) = history_backfill_anchor {
        restore_room_history_backfill_anchor(hwnd, anchor);
    }
    Ok(())
}

unsafe fn apply_synthetic_events_and_refresh(hwnd: HWND, events: Vec<Event>) -> Result<(), String> {
    state_mut(hwnd).synthetic_load_in_progress = true;
    let result = apply_events_and_refresh(hwnd, events);
    state_mut(hwnd).synthetic_load_in_progress = false;
    result
}

unsafe fn apply_event_load(hwnd: HWND, events: Vec<Event>) -> Result<(usize, usize), String> {
    {
        let state = state_mut(hwnd);
        state.pending_login_homeserver_url = None;
        state.current_session_homeserver_url = None;
        state.switchbar_views.clear();
        state.room_activity.clear();
        state.room_activity_read_targets.clear();
        state.find_states.clear();
        state.model = initial_model();
        state.decoded_images.clear();
    }

    apply_synthetic_events_and_refresh(hwnd, events)?;

    let (rooms, timeline_events, active_view) = {
        let state = state_mut(hwnd);
        let rooms = state.model.rooms.len();
        let timeline_events = state
            .model
            .room_views
            .values()
            .map(|room_view| room_view.activities.len())
            .sum();
        let selected_room = state
            .model
            .rooms
            .iter()
            .find(|room| is_navigable_room(room))
            .map(|room| room.room_id.clone());

        let active_view = match selected_room {
            Some(room_id) => {
                state.model.selected_room = Some(room_id.clone());
                ViewId::Room(room_id)
            }
            None => {
                state.model.selected_room = None;
                ViewId::Status
            }
        };
        state.active_view = active_view.clone();

        (rooms, timeline_events, active_view)
    };

    sync_views_and_tree(hwnd)?;
    activate_view(hwnd, &active_view, None)?;

    Ok((rooms, timeline_events))
}

unsafe fn drain_core_events_and_refresh(hwnd: HWND) -> Result<usize, String> {
    let events = drain_core_events(hwnd);
    let event_count = events.len();
    apply_events_and_refresh(hwnd, events).map(|()| event_count)
}

unsafe fn handle_tree_selection(hwnd: HWND, lparam: LPARAM) -> bool {
    if lparam == 0 {
        return false;
    }

    let notification = &*(lparam as *const NMHDR);
    if notification.idFrom != TREE_ID as usize || notification.code != TVN_SELCHANGEDW {
        return false;
    }

    let tree = state_ref(hwnd).controls.tree;
    let selected_item = SendMessageW(tree, TVM_GETNEXTITEM, TVGN_CARET as WPARAM, 0) as isize;
    if selected_item == 0 {
        return true;
    }

    let view_id = state_ref(hwnd).tree_items.get(&selected_item).cloned();
    if let Some(view_id) = view_id
        && let Err(message) = activate_view(hwnd, &view_id, Some(selected_item))
    {
        show_error_message(hwnd, "Treebar", &message);
    }

    true
}

fn select_room(state: &mut WindowState, room_id: &str) -> Result<(), String> {
    if navigable_room_by_id(&state.model.rooms, room_id).is_none() {
        return Err(format!("unknown room id: {room_id}"));
    }

    state.model.selected_room = Some(room_id.to_owned());
    Ok(())
}

unsafe fn submit_compose_from_edit(hwnd: HWND) -> Result<(), String> {
    let edit = compose_edit(hwnd)?;
    let input = read_window_text(edit);
    if input.is_empty() {
        return Err("message body must not be empty".to_owned());
    }

    match parse_compose_submission(&input) {
        ComposeSubmission::Message { presentation, body } => {
            submit_compose_with_history_entry(
                state_mut(hwnd),
                presentation,
                body.into_owned(),
                input,
                None,
            )?;
        }
        ComposeSubmission::LocalCommand(command) => {
            let history_entry = local_command_history_entry(&input, &command);
            execute_local_slash_command(hwnd, command)?;
            if let Some(entry) = history_entry {
                state_mut(hwnd).compose_history.record_submission(entry);
            }
        }
    }

    let empty = wide_null("");
    SendMessageW(edit, WM_SETTEXT, 0, empty.as_ptr() as LPARAM);
    Ok(())
}

unsafe fn compose_edit(hwnd: HWND) -> Result<HWND, String> {
    let edit = state_ref(hwnd).controls.edit;
    if edit.is_null() {
        return Err("compose edit control is not available".to_owned());
    }
    Ok(edit)
}

unsafe fn dump_compose_text(hwnd: HWND) -> Result<String, String> {
    let edit = compose_edit(hwnd)?;
    Ok(read_window_text(edit))
}

unsafe fn set_compose_text(hwnd: HWND, text: &str) -> Result<(), String> {
    let edit = compose_edit(hwnd)?;
    state_mut(hwnd).compose_history.reset_navigation();
    set_compose_edit_text(edit, text);
    Ok(())
}

unsafe fn send_compose_key(hwnd: HWND, key: ComposeKey) -> Result<(), String> {
    let edit = compose_edit(hwnd)?;
    let virtual_key = match key {
        ComposeKey::Return => VK_RETURN_KEY,
        ComposeKey::Up => VK_UP_KEY,
        ComposeKey::Down => VK_DOWN_KEY,
    };
    SendMessageW(edit, WM_KEYDOWN, virtual_key, 0);
    Ok(())
}

unsafe fn submit_compose_from_control(
    hwnd: HWND,
    explicit_room_id: Option<&str>,
) -> Result<String, String> {
    let edit = compose_edit(hwnd)?;
    let body = read_window_text(edit);
    let room_id = submit_compose(
        state_mut(hwnd),
        RoomMessagePresentationKind::Text,
        body,
        explicit_room_id,
    )?;
    let empty = wide_null("");
    SendMessageW(edit, WM_SETTEXT, 0, empty.as_ptr() as LPARAM);
    Ok(room_id)
}

fn parse_compose_submission(input: &str) -> ComposeSubmission<'_> {
    if !input.starts_with('/') {
        return ComposeSubmission::Message {
            presentation: RoomMessagePresentationKind::Text,
            body: Cow::Borrowed(input),
        };
    }

    if input.starts_with("//") {
        return ComposeSubmission::Message {
            presentation: RoomMessagePresentationKind::Text,
            body: Cow::Owned(input[1..].to_owned()),
        };
    }

    let command_end = input
        .bytes()
        .position(|byte| byte.is_ascii_whitespace())
        .unwrap_or(input.len());
    let command_name = &input[1..command_end];
    if command_name.eq_ignore_ascii_case("me") {
        let action = input[command_end..].trim_matches(|ch: char| ch.is_ascii_whitespace());
        if action.is_empty() {
            return ComposeSubmission::LocalCommand(LocalSlashCommand::Invalid {
                feedback: "usage: /me <action>".to_owned(),
            });
        }
        return ComposeSubmission::Message {
            presentation: RoomMessagePresentationKind::Emote,
            body: Cow::Borrowed(action),
        };
    }

    ComposeSubmission::LocalCommand(parse_slash_command(input))
}

fn parse_slash_command(input: &str) -> LocalSlashCommand<'_> {
    let command_end = input
        .bytes()
        .position(|byte| byte.is_ascii_whitespace())
        .unwrap_or(input.len());
    let command_name = &input[1..command_end];
    let argument = input[command_end..].trim_matches(|ch: char| ch.is_ascii_whitespace());

    match command_name.to_ascii_lowercase().as_str() {
        "status" if argument.is_empty() => LocalSlashCommand::Status,
        "status" => LocalSlashCommand::Invalid {
            feedback: "usage: /status".to_owned(),
        },
        "find" if !argument.is_empty() => LocalSlashCommand::Find {
            query: argument.to_owned(),
        },
        "find" => LocalSlashCommand::Invalid {
            feedback: "usage: /find <text>".to_owned(),
        },
        "window" if !argument.is_empty() => LocalSlashCommand::Window { target: argument },
        "window" => LocalSlashCommand::Invalid {
            feedback: "usage: /window <room-id-or-display-name>".to_owned(),
        },
        "join" if !argument.is_empty() => LocalSlashCommand::Join {
            target: argument.to_owned(),
        },
        "join" => LocalSlashCommand::Invalid {
            feedback: "usage: /join <room-id-or-alias>".to_owned(),
        },
        "query" if !argument.is_empty() => LocalSlashCommand::Query {
            user_id: argument.to_owned(),
        },
        "query" => LocalSlashCommand::Invalid {
            feedback: "usage: /query <mxid>".to_owned(),
        },
        "accept" if argument.is_empty() => LocalSlashCommand::Accept { target: None },
        "accept" => LocalSlashCommand::Accept {
            target: Some(argument.to_owned()),
        },
        "reject" if argument.is_empty() => LocalSlashCommand::Reject { target: None },
        "reject" => LocalSlashCommand::Reject {
            target: Some(argument.to_owned()),
        },
        "part" if argument.is_empty() => LocalSlashCommand::Part { target: None },
        "part" => LocalSlashCommand::Part {
            target: Some(argument.to_owned()),
        },
        "react" => parse_react_slash_command(argument),
        "reply" => parse_reply_slash_command(argument),
        "edit" => parse_edit_slash_command(argument),
        "redact" => parse_redact_slash_command(argument),
        "unreact" => parse_unreact_slash_command(argument),
        "topic" if !argument.is_empty() => LocalSlashCommand::Topic {
            topic: argument.to_owned(),
        },
        "topic" => LocalSlashCommand::Invalid {
            feedback: "usage: /topic <text>".to_owned(),
        },
        "clear" if argument.is_empty() => LocalSlashCommand::Clear,
        "clear" => LocalSlashCommand::Invalid {
            feedback: "usage: /clear".to_owned(),
        },
        "e2ee" => match argument.to_ascii_lowercase().as_str() {
            "" | "status" => LocalSlashCommand::E2ee(E2eeSlashCommand::ShowStatus),
            "refresh" => LocalSlashCommand::E2ee(E2eeSlashCommand::Refresh),
            "recover" => LocalSlashCommand::E2ee(E2eeSlashCommand::RecoverDialog),
            _ => LocalSlashCommand::Invalid {
                feedback: "usage: /e2ee [status|refresh|recover]".to_owned(),
            },
        },
        "verify" => match argument.to_ascii_lowercase().as_str() {
            "accept" => LocalSlashCommand::Verify {
                action: VerificationAction::Accept,
            },
            "cancel" => LocalSlashCommand::Verify {
                action: VerificationAction::Cancel,
            },
            "confirm" => LocalSlashCommand::Verify {
                action: VerificationAction::Confirm,
            },
            "reject" => LocalSlashCommand::Verify {
                action: VerificationAction::Reject,
            },
            _ => LocalSlashCommand::Invalid {
                feedback: "usage: /verify accept|cancel|confirm|reject".to_owned(),
            },
        },
        _ => LocalSlashCommand::Invalid {
            feedback: format!("unknown command: {}", &input[..command_end]),
        },
    }
}

fn parse_react_slash_command(argument: &str) -> LocalSlashCommand<'_> {
    let Some(target_end) = argument.bytes().position(|byte| byte.is_ascii_whitespace()) else {
        return LocalSlashCommand::Invalid {
            feedback: "usage: /react <event-id> <key>".to_owned(),
        };
    };
    let target_event_id = &argument[..target_end];
    let key = argument[target_end..].trim_matches(|ch: char| ch.is_ascii_whitespace());
    if target_event_id.is_empty() || key.is_empty() {
        return LocalSlashCommand::Invalid {
            feedback: "usage: /react <event-id> <key>".to_owned(),
        };
    }
    LocalSlashCommand::React {
        target_event_id: target_event_id.to_owned(),
        key: key.to_owned(),
    }
}

fn parse_edit_slash_command(argument: &str) -> LocalSlashCommand<'_> {
    let Some(target_end) = argument.bytes().position(|byte| byte.is_ascii_whitespace()) else {
        return LocalSlashCommand::Invalid {
            feedback: "usage: /edit <event-id> <replacement text>".to_owned(),
        };
    };
    let target_event_id = &argument[..target_end];
    let body = argument[target_end..].trim_matches(|ch: char| ch.is_ascii_whitespace());
    if target_event_id.is_empty() || body.is_empty() {
        return LocalSlashCommand::Invalid {
            feedback: "usage: /edit <event-id> <replacement text>".to_owned(),
        };
    }
    LocalSlashCommand::Edit {
        target_event_id: target_event_id.to_owned(),
        body: body.to_owned(),
    }
}

fn parse_reply_slash_command(argument: &str) -> LocalSlashCommand<'_> {
    let Some(target_end) = argument.bytes().position(|byte| byte.is_ascii_whitespace()) else {
        return LocalSlashCommand::Invalid {
            feedback: "usage: /reply <event-id> <message>".to_owned(),
        };
    };
    let target_event_id = &argument[..target_end];
    let body = argument[target_end..].trim_matches(|ch: char| ch.is_ascii_whitespace());
    if target_event_id.is_empty() || body.is_empty() {
        return LocalSlashCommand::Invalid {
            feedback: "usage: /reply <event-id> <message>".to_owned(),
        };
    }
    LocalSlashCommand::Reply {
        target_event_id: target_event_id.to_owned(),
        body: body.to_owned(),
    }
}

fn parse_unreact_slash_command(argument: &str) -> LocalSlashCommand<'_> {
    if argument.is_empty() || argument.bytes().any(|byte| byte.is_ascii_whitespace()) {
        return LocalSlashCommand::Invalid {
            feedback: "usage: /unreact <reaction-event-id>".to_owned(),
        };
    }
    LocalSlashCommand::Unreact {
        reaction_event_id: argument.to_owned(),
    }
}

fn parse_redact_slash_command(argument: &str) -> LocalSlashCommand<'_> {
    if argument.is_empty() || argument.bytes().any(|byte| byte.is_ascii_whitespace()) {
        return LocalSlashCommand::Invalid {
            feedback: "usage: /redact <event-id>".to_owned(),
        };
    }
    LocalSlashCommand::Redact {
        event_id: argument.to_owned(),
    }
}

fn local_command_history_entry(input: &str, command: &LocalSlashCommand<'_>) -> Option<String> {
    if matches!(command, LocalSlashCommand::Invalid { .. }) && is_e2ee_command_input(input) {
        return None;
    }

    Some(input.to_owned())
}

fn is_e2ee_command_input(input: &str) -> bool {
    if !input.starts_with('/') {
        return false;
    }

    let command_end = input
        .bytes()
        .position(|byte| byte.is_ascii_whitespace())
        .unwrap_or(input.len());
    input[1..command_end].eq_ignore_ascii_case("e2ee")
}

fn submit_compose(
    state: &mut WindowState,
    presentation: RoomMessagePresentationKind,
    body: String,
    explicit_room_id: Option<&str>,
) -> Result<String, String> {
    let history_entry = body.clone();
    submit_compose_with_history_entry(state, presentation, body, history_entry, explicit_room_id)
}

fn submit_compose_with_history_entry(
    state: &mut WindowState,
    presentation: RoomMessagePresentationKind,
    body: String,
    history_entry: String,
    explicit_room_id: Option<&str>,
) -> Result<String, String> {
    if body.is_empty() {
        return Err("message body must not be empty".to_owned());
    }

    let destination = resolve_compose_destination(state, explicit_room_id)?;

    let sender = state
        .command_sender
        .as_ref()
        .ok_or_else(|| "core command sender is not available".to_owned())?;
    let send_body = body.clone();
    let clears_active_thread_marker = explicit_room_id.is_none() && destination.thread.is_some();
    let command = Command::SendMessage(SendMessageRequest {
        room_id: destination.room_id.clone(),
        presentation,
        body: send_body,
        thread: destination.thread,
        reply: None,
    });
    sender
        .send(command.clone())
        .map_err(|err| format!("failed to enqueue send message command: {err}"))?;
    #[cfg(test)]
    state.recorded_commands.push(command);
    if clears_active_thread_marker {
        clear_view_activity_marker(state, &state.active_view.clone());
    }
    state.compose_history.record_submission(history_entry);

    Ok(destination.room_id)
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ComposeDestination {
    room_id: String,
    thread: Option<SendMessageThreadTarget>,
}

fn resolve_compose_destination(
    state: &WindowState,
    explicit_room_id: Option<&str>,
) -> Result<ComposeDestination, String> {
    if let Some(room_id) = explicit_room_id {
        if sendable_room_by_id(&state.model.rooms, room_id).is_some() {
            return Ok(ComposeDestination {
                room_id: room_id.to_owned(),
                thread: None,
            });
        }
        if let Some(room) = room_by_id(&state.model.rooms, room_id)
            && is_invited_room(room)
        {
            return Err(format!("room is not joined: {room_id}"));
        }
        return Err(format!("unknown room id: {room_id}"));
    }

    match &state.active_view {
        ViewId::Room(room_id) => {
            if sendable_room_by_id(&state.model.rooms, room_id).is_some() {
                Ok(ComposeDestination {
                    room_id: room_id.clone(),
                    thread: None,
                })
            } else {
                Err(format!("room is not joined: {room_id}"))
            }
        }
        ViewId::Thread {
            room_id,
            thread_root_event_id,
        } => {
            projected_thread_by_id(&state.model, room_id, thread_root_event_id)
                .expect("active thread compose must target an existing projected thread");
            Ok(ComposeDestination {
                room_id: room_id.clone(),
                thread: Some(SendMessageThreadTarget {
                    thread_root_event_id: thread_root_event_id.clone(),
                    latest_event_id: latest_durable_thread_event_id(
                        &state.model,
                        room_id,
                        thread_root_event_id,
                    ),
                }),
            })
        }
        ViewId::Status => Err("no room selected".to_owned()),
    }
}

fn latest_durable_thread_event_id(
    model: &UiModel,
    room_id: &str,
    thread_root_event_id: &str,
) -> String {
    let room_view = model
        .room_views
        .get(room_id)
        .expect("thread compose must target an existing room view");
    let mut latest_event_id = thread_root_event_id;

    for activity in &room_view.activities {
        if room_activity_belongs_to_thread(activity, thread_root_event_id)
            && let Some(event_id) = room_activity_event_id(activity)
            && !event_id.starts_with("txn:")
        {
            latest_event_id = event_id;
        }
    }

    latest_event_id.to_owned()
}

unsafe fn execute_local_slash_command(
    hwnd: HWND,
    command: LocalSlashCommand<'_>,
) -> Result<(), String> {
    match command {
        LocalSlashCommand::Status => {
            activate_view(hwnd, &ViewId::Status, None)?;
            set_local_command_status(hwnd, "status window active".to_owned())
        }
        LocalSlashCommand::Find { query } => match execute_active_view_find(hwnd, &query) {
            Ok(_) => Ok(()),
            Err(message) => set_local_command_status(hwnd, message),
        },
        LocalSlashCommand::Window { target } => {
            let room_id = match resolve_room_command_target(&state_ref(hwnd).model.rooms, target) {
                Ok(room_id) => room_id,
                Err(WindowTargetError::Unknown) => {
                    return set_local_command_status(
                        hwnd,
                        format!("unknown window target: {target}"),
                    );
                }
                Err(WindowTargetError::Ambiguous) => {
                    return set_local_command_status(
                        hwnd,
                        format!("ambiguous window target: {target}"),
                    );
                }
            };
            activate_view(hwnd, &ViewId::Room(room_id), None)?;
            set_local_command_status(hwnd, format!("window active: {target}"))
        }
        LocalSlashCommand::Join { target } => {
            let result = {
                let state = state_mut(hwnd);
                submit_join_slash_command(state, target)
            };
            match result {
                Ok(message) | Err(message) => set_local_command_status(hwnd, message),
            }
        }
        LocalSlashCommand::Query { user_id } => {
            match resolve_direct_room_query_target(&state_ref(hwnd).model, &user_id) {
                QueryTargetResolution::Single(room_id) => {
                    activate_view(hwnd, &ViewId::Room(room_id), None)?;
                    set_local_command_status(hwnd, format!("query opened: {user_id}"))
                }
                QueryTargetResolution::Ambiguous => {
                    set_local_command_status(hwnd, format!("ambiguous query target: {user_id}"))
                }
                QueryTargetResolution::None => {
                    let result = {
                        let state = state_mut(hwnd);
                        submit_query_slash_command(state, user_id)
                    };
                    match result {
                        Ok(message) | Err(message) => set_local_command_status(hwnd, message),
                    }
                }
            }
        }
        LocalSlashCommand::Accept { target } => {
            let result = {
                let state = state_mut(hwnd);
                submit_accept_slash_command(state, target.as_deref())
            };
            match result {
                Ok(message) | Err(message) => set_local_command_status(hwnd, message),
            }
        }
        LocalSlashCommand::Reject { target } => {
            let result = {
                let state = state_mut(hwnd);
                submit_reject_slash_command(state, target.as_deref())
            };
            match result {
                Ok(message) | Err(message) => set_local_command_status(hwnd, message),
            }
        }
        LocalSlashCommand::Part { target } => {
            let result = {
                let state = state_mut(hwnd);
                submit_part_slash_command(state, target.as_deref())
            };
            match result {
                Ok(message) | Err(message) => set_local_command_status(hwnd, message),
            }
        }
        LocalSlashCommand::React {
            target_event_id,
            key,
        } => {
            let result = {
                let state = state_mut(hwnd);
                submit_react_slash_command(state, target_event_id, key)
            };
            match result {
                Ok(message) | Err(message) => set_local_command_status(hwnd, message),
            }
        }
        LocalSlashCommand::Reply {
            target_event_id,
            body,
        } => {
            let result = {
                let state = state_mut(hwnd);
                submit_reply_slash_command(state, target_event_id, body)
            };
            match result {
                Ok(message) | Err(message) => set_local_command_status(hwnd, message),
            }
        }
        LocalSlashCommand::Edit {
            target_event_id,
            body,
        } => {
            let result = {
                let state = state_mut(hwnd);
                submit_edit_slash_command(state, target_event_id, body)
            };
            match result {
                Ok(message) | Err(message) => set_local_command_status(hwnd, message),
            }
        }
        LocalSlashCommand::Redact { event_id } => {
            let result = {
                let state = state_mut(hwnd);
                submit_redact_slash_command(state, event_id)
            };
            match result {
                Ok(message) | Err(message) => set_local_command_status(hwnd, message),
            }
        }
        LocalSlashCommand::Unreact { reaction_event_id } => {
            let result = {
                let state = state_mut(hwnd);
                submit_unreact_slash_command(state, reaction_event_id)
            };
            match result {
                Ok(message) | Err(message) => set_local_command_status(hwnd, message),
            }
        }
        LocalSlashCommand::Topic { topic } => {
            let result = {
                let state = state_mut(hwnd);
                submit_topic_slash_command(state, topic)
            };
            match result {
                Ok(message) | Err(message) => set_local_command_status(hwnd, message),
            }
        }
        LocalSlashCommand::Clear => match state_ref(hwnd).active_view.clone() {
            ViewId::Room(room_id) => {
                {
                    let state = state_mut(hwnd);
                    if let Some(view) = state.model.room_views.get_mut(&room_id) {
                        view.clear_visible_entries();
                    }
                    prune_decoded_images(state);
                }
                refresh_view_content(hwnd, &ViewId::Room(room_id.clone()))?;
                set_local_command_status(hwnd, format!("cleared window: {room_id}"))
            }
            ViewId::Thread { .. } => {
                set_local_command_status(hwnd, "nothing to clear in thread view".to_owned())
            }
            ViewId::Status => {
                set_local_command_status(hwnd, "nothing to clear in status view".to_owned())
            }
        },
        LocalSlashCommand::E2ee(command) => match command {
            E2eeSlashCommand::ShowStatus => {
                activate_view(hwnd, &ViewId::Status, None)?;
                set_local_command_status(hwnd, "e2ee status shown".to_owned())
            }
            E2eeSlashCommand::Refresh => {
                let command = Command::RefreshE2eeStatus;
                let result = {
                    let state = state_mut(hwnd);
                    let sender = match state.command_sender.as_ref() {
                        Some(sender) => sender,
                        None => {
                            return set_local_command_status(
                                hwnd,
                                "core command sender is not available".to_owned(),
                            );
                        }
                    };
                    sender.send(command.clone()).map_err(|err| {
                        format!("failed to enqueue refresh e2ee status command: {err}")
                    })?;
                    #[cfg(test)]
                    state.recorded_commands.push(command);
                    Ok::<(), String>(())
                };
                match result {
                    Ok(()) => set_local_command_status(hwnd, "e2ee refresh submitted".to_owned()),
                    Err(message) => set_local_command_status(hwnd, message),
                }
            }
            E2eeSlashCommand::RecoverDialog => match show_recover_e2ee_menu_dialog(hwnd) {
                Ok(_) => Ok(()),
                Err(message) => set_local_command_status(hwnd, message),
            },
        },
        LocalSlashCommand::Verify { action } => {
            let state = state_mut(hwnd);
            let result = match action {
                VerificationAction::Accept => submit_accept_verification(state),
                VerificationAction::Cancel => submit_cancel_verification(state),
                VerificationAction::Confirm => submit_confirm_verification_sas(state),
                VerificationAction::Reject => submit_reject_verification_sas(state),
            };
            match result {
                Ok(()) => set_local_command_status(
                    hwnd,
                    format!(
                        "verification {} submitted",
                        verification_action_label(action)
                    ),
                ),
                Err(message) => set_local_command_status(hwnd, message),
            }
        }
        LocalSlashCommand::Invalid { feedback } => set_local_command_status(hwnd, feedback),
    }
}

unsafe fn set_local_command_status(hwnd: HWND, message: String) -> Result<(), String> {
    let has_status_view = {
        let state = state_mut(hwnd);
        state.model.status = message;
        state.views.contains_key(&ViewId::Status)
    };

    refresh_status_bar(hwnd)?;
    if has_status_view {
        refresh_view_content(hwnd, &ViewId::Status)?;
    }
    Ok(())
}

fn resolve_room_command_target(
    rooms: &[RoomSummary],
    target: &str,
) -> Result<String, WindowTargetError> {
    if let Some(room) = navigable_room_by_id(rooms, target) {
        return Ok(room.room_id.clone());
    }

    let mut matches = rooms
        .iter()
        .filter(|room| is_navigable_room(room) && room.display_name == target);
    let Some(room) = matches.next() else {
        return Err(WindowTargetError::Unknown);
    };
    if matches.next().is_some() {
        return Err(WindowTargetError::Ambiguous);
    }
    Ok(room.room_id.clone())
}

fn resolve_direct_room_query_target(model: &UiModel, user_id: &str) -> QueryTargetResolution {
    let mut matches = model
        .rooms
        .iter()
        .filter(|room| room.is_direct && room.direct_user_ids.iter().any(|id| id == user_id));
    let Some(room) = matches.next() else {
        return QueryTargetResolution::None;
    };
    if matches.next().is_some() {
        return QueryTargetResolution::Ambiguous;
    }
    QueryTargetResolution::Single(room.room_id.clone())
}

fn resolve_part_slash_command_room_id(
    state: &WindowState,
    target: Option<&str>,
) -> Result<String, String> {
    let Some(target) = target else {
        return match &state.active_view {
            ViewId::Room(room_id) => {
                if sendable_room_by_id(&state.model.rooms, room_id).is_some() {
                    Ok(room_id.clone())
                } else {
                    Err(format!("room is not joined: {room_id}"))
                }
            }
            ViewId::Thread { room_id, .. } => Ok(room_id.clone()),
            ViewId::Status => Err("no room selected".to_owned()),
        };
    };

    match resolve_room_command_target(&state.model.rooms, target) {
        Ok(room_id) => {
            if sendable_room_by_id(&state.model.rooms, &room_id).is_some() {
                Ok(room_id)
            } else {
                Err(format!("room is not joined: {target}"))
            }
        }
        Err(WindowTargetError::Unknown) => Err(format!("unknown part target: {target}")),
        Err(WindowTargetError::Ambiguous) => Err(format!("ambiguous part target: {target}")),
    }
}

fn resolve_invite_command_room_id(
    state: &WindowState,
    action: &str,
    target: Option<&str>,
) -> Result<String, String> {
    let Some(target) = target else {
        return match &state.active_view {
            ViewId::Room(room_id)
                if room_by_id(&state.model.rooms, room_id)
                    .is_some_and(is_invite_command_target) =>
            {
                Ok(room_id.clone())
            }
            _ => Err(format!("{action} failed: no invited room selected")),
        };
    };

    if let Some(room) = room_by_id(&state.model.rooms, target) {
        return if is_invite_command_target(room) {
            Ok(room.room_id.clone())
        } else if is_navigable_room(room) {
            Err(format!("{action} failed: room is not invited: {target}"))
        } else {
            Err(format!("{action} failed: invited room not found: {target}"))
        };
    }

    let invited_matches = state
        .model
        .rooms
        .iter()
        .filter(|room| is_invite_command_target(room) && room.display_name == target)
        .collect::<Vec<_>>();
    if invited_matches.len() > 1 {
        return Err(format!(
            "{action} failed: ambiguous invited room target: {target}"
        ));
    }
    if let Some(room) = invited_matches.first() {
        return Ok(room.room_id.clone());
    }

    if state
        .model
        .rooms
        .iter()
        .any(|room| is_joined_message_room(room) && room.display_name == target)
    {
        return Err(format!("{action} failed: room is not invited: {target}"));
    }

    Err(format!("{action} failed: invited room not found: {target}"))
}

fn submit_join_slash_command(state: &mut WindowState, target: String) -> Result<String, String> {
    let command = Command::JoinRoom(JoinRoomRequest {
        room_or_alias: target.clone(),
    });
    let sender = state
        .command_sender
        .as_ref()
        .ok_or_else(|| "core command sender is not available".to_owned())?;
    sender
        .send(command.clone())
        .map_err(|err| format!("failed to enqueue join room command: {err}"))?;
    state.recorded_commands.push(command);
    Ok(format!("join submitted: {target}"))
}

fn submit_query_slash_command(state: &mut WindowState, user_id: String) -> Result<String, String> {
    let sender = state
        .command_sender
        .as_ref()
        .ok_or_else(|| "core command sender is not available".to_owned())?;
    let command = Command::CreateDirectRoom(CreateDirectRoomRequest {
        user_id: user_id.clone(),
    });
    sender
        .send(command.clone())
        .map_err(|err| format!("failed to enqueue create direct room command: {err}"))?;
    state.recorded_commands.push(command);
    Ok(format!("query submitted: {user_id}"))
}

fn submit_accept_slash_command(
    state: &mut WindowState,
    target: Option<&str>,
) -> Result<String, String> {
    let room_id = resolve_invite_command_room_id(state, "accept", target)?;
    let command = Command::AcceptRoomInvite(AcceptRoomInviteRequest {
        room_id: room_id.clone(),
    });
    let sender = state
        .command_sender
        .as_ref()
        .ok_or_else(|| "core command sender is not available".to_owned())?;
    sender
        .send(command.clone())
        .map_err(|err| format!("failed to enqueue accept room invite command: {err}"))?;
    state.recorded_commands.push(command);
    Ok(format!("accept submitted: {room_id}"))
}

fn submit_reject_slash_command(
    state: &mut WindowState,
    target: Option<&str>,
) -> Result<String, String> {
    let room_id = resolve_invite_command_room_id(state, "reject", target)?;
    let command = Command::RejectRoomInvite(RejectRoomInviteRequest {
        room_id: room_id.clone(),
    });
    let sender = state
        .command_sender
        .as_ref()
        .ok_or_else(|| "core command sender is not available".to_owned())?;
    sender
        .send(command.clone())
        .map_err(|err| format!("failed to enqueue reject room invite command: {err}"))?;
    state.recorded_commands.push(command);
    Ok(format!("reject submitted: {room_id}"))
}

fn submit_part_slash_command(
    state: &mut WindowState,
    target: Option<&str>,
) -> Result<String, String> {
    let room_id = resolve_part_slash_command_room_id(state, target)?;
    let command = Command::LeaveRoom(LeaveRoomRequest {
        room_id: room_id.clone(),
    });
    let sender = state
        .command_sender
        .as_ref()
        .ok_or_else(|| "core command sender is not available".to_owned())?;
    sender
        .send(command.clone())
        .map_err(|err| format!("failed to enqueue leave room command: {err}"))?;
    state.recorded_commands.push(command);
    Ok(format!("part submitted: {room_id}"))
}

fn submit_react_slash_command(
    state: &mut WindowState,
    target_event_id: String,
    key: String,
) -> Result<String, String> {
    let room_id = resolve_part_slash_command_room_id(state, None)?;
    let command = Command::SendReaction(SendReactionRequest {
        room_id: room_id.clone(),
        target_event_id: target_event_id.clone(),
        key,
    });
    let sender = state
        .command_sender
        .as_ref()
        .ok_or_else(|| "core command sender is not available".to_owned())?;
    sender
        .send(command.clone())
        .map_err(|err| format!("failed to enqueue send reaction command: {err}"))?;
    state.recorded_commands.push(command);
    Ok(format!("Reaction queued for {target_event_id}"))
}

fn submit_edit_slash_command(
    state: &mut WindowState,
    target_event_id: String,
    body: String,
) -> Result<String, String> {
    let room_id = resolve_part_slash_command_room_id(state, None)?;
    let command = Command::EditMessage(EditMessageRequest {
        room_id: room_id.clone(),
        target_event_id: target_event_id.clone(),
        presentation: RoomMessagePresentationKind::Text,
        body,
    });
    let sender = state
        .command_sender
        .as_ref()
        .ok_or_else(|| "core command sender is not available".to_owned())?;
    sender
        .send(command.clone())
        .map_err(|err| format!("failed to enqueue edit message command: {err}"))?;
    state.recorded_commands.push(command);
    Ok(format!("Edit queued for {target_event_id}"))
}

fn submit_reply_slash_command(
    state: &mut WindowState,
    target_event_id: String,
    body: String,
) -> Result<String, String> {
    let destination = resolve_compose_destination(state, None)?;
    let sender = state
        .command_sender
        .as_ref()
        .ok_or_else(|| "core command sender is not available".to_owned())?;
    let clears_active_thread_marker = destination.thread.is_some();
    let command = Command::SendMessage(SendMessageRequest {
        room_id: destination.room_id.clone(),
        presentation: RoomMessagePresentationKind::Text,
        body,
        thread: destination.thread,
        reply: Some(SendMessageReplyTarget {
            event_id: target_event_id.clone(),
        }),
    });
    sender
        .send(command.clone())
        .map_err(|err| format!("failed to enqueue send message command: {err}"))?;
    state.recorded_commands.push(command);
    if clears_active_thread_marker {
        clear_view_activity_marker(state, &state.active_view.clone());
    }
    Ok(format!("Reply queued for {target_event_id}"))
}

fn submit_redact_event_command(state: &mut WindowState, event_id: String) -> Result<(), String> {
    let room_id = resolve_part_slash_command_room_id(state, None)?;
    let command = Command::RedactEvent(RedactEventRequest { room_id, event_id });
    let sender = state
        .command_sender
        .as_ref()
        .ok_or_else(|| "core command sender is not available".to_owned())?;
    sender
        .send(command.clone())
        .map_err(|err| format!("failed to enqueue redact event command: {err}"))?;
    state.recorded_commands.push(command);
    Ok(())
}

fn submit_redact_slash_command(
    state: &mut WindowState,
    event_id: String,
) -> Result<String, String> {
    submit_redact_event_command(state, event_id.clone())?;
    Ok(format!("Redaction queued for {event_id}"))
}

fn submit_unreact_slash_command(
    state: &mut WindowState,
    reaction_event_id: String,
) -> Result<String, String> {
    submit_redact_event_command(state, reaction_event_id.clone())?;
    Ok(format!("Reaction redaction queued for {reaction_event_id}"))
}

fn submit_topic_slash_command(state: &mut WindowState, topic: String) -> Result<String, String> {
    let room_id = resolve_part_slash_command_room_id(state, None)?;
    let command = Command::SetRoomTopic(SetRoomTopicRequest {
        room_id: room_id.clone(),
        topic,
    });
    let sender = state
        .command_sender
        .as_ref()
        .ok_or_else(|| "core command sender is not available".to_owned())?;
    sender
        .send(command.clone())
        .map_err(|err| format!("failed to enqueue set room topic command: {err}"))?;
    state.recorded_commands.push(command);
    Ok(format!("topic change submitted for {room_id}"))
}

unsafe fn navigate_compose_history(
    hwnd: HWND,
    direction: ComposeHistoryDirection,
) -> Result<(), String> {
    let edit = compose_edit(hwnd)?;
    let replacement = match direction {
        ComposeHistoryDirection::Previous => {
            let current_text = read_window_text(edit);
            let history = &mut state_mut(hwnd).compose_history;
            history.previous(current_text).map(str::to_owned)
        }
        ComposeHistoryDirection::Next => {
            let history = &mut state_mut(hwnd).compose_history;
            history.next()
        }
    };
    if let Some(text) = replacement {
        set_compose_edit_text(edit, &text);
    }
    Ok(())
}

fn submit_login(
    state: &mut WindowState,
    homeserver_url: String,
    username: String,
    password: String,
    device_display_name: String,
) -> Result<(), String> {
    if homeserver_url.is_empty() {
        return Err("homeserver URL must not be empty".to_owned());
    }
    if username.is_empty() {
        return Err("username must not be empty".to_owned());
    }
    if password.is_empty() {
        return Err("password must not be empty".to_owned());
    }

    let device_display_name = if device_display_name.is_empty() {
        DEFAULT_DEVICE_DISPLAY_NAME.to_owned()
    } else {
        device_display_name
    };

    let sender = state
        .command_sender
        .as_ref()
        .ok_or_else(|| "core command sender is not available".to_owned())?;
    sender
        .send(Command::Login(LoginRequest {
            homeserver_url: homeserver_url.clone(),
            username,
            password,
            device_display_name,
        }))
        .map_err(|err| format!("failed to enqueue login command: {err}"))?;

    state.pending_login_homeserver_url = Some(homeserver_url);
    state.model.status = "logging in".to_owned();
    state.model.lifecycle_state = LifecycleState::LoggingIn;
    Ok(())
}

fn submit_restore_session(
    state: &mut WindowState,
    homeserver_url: String,
    session: StoredSession,
) -> Result<(), String> {
    if homeserver_url.is_empty() {
        return Err("homeserver URL must not be empty".to_owned());
    }
    if session.user_id.is_empty() {
        return Err("session user_id must not be empty".to_owned());
    }
    if session.device_id.is_empty() {
        return Err("session device_id must not be empty".to_owned());
    }
    if session.access_token.is_empty() {
        return Err("session access_token must not be empty".to_owned());
    }

    let sender = state
        .command_sender
        .as_ref()
        .ok_or_else(|| "core command sender is not available".to_owned())?;
    sender
        .send(Command::RestoreSession(RestoreSessionRequest {
            homeserver_url: homeserver_url.clone(),
            session,
        }))
        .map_err(|err| format!("failed to enqueue restore session command: {err}"))?;

    state.pending_login_homeserver_url = None;
    state.current_session_homeserver_url = Some(homeserver_url);
    state.model.status = "restoring saved session".to_owned();
    state.model.lifecycle_state = LifecycleState::Restoring;
    Ok(())
}

fn submit_restore_saved_session(state: &mut WindowState) -> Result<(), String> {
    let saved = load_saved_session()?;
    submit_restore_session(state, saved.homeserver_url, saved.session)
}

fn submit_start_sync(state: &mut WindowState) -> Result<(), String> {
    let sender = state
        .command_sender
        .as_ref()
        .ok_or_else(|| "core command sender is not available".to_owned())?;
    sender
        .send(Command::StartSync)
        .map_err(|err| format!("failed to enqueue start sync command: {err}"))
}

fn submit_logout(state: &mut WindowState) -> Result<(), String> {
    let sender = state
        .command_sender
        .as_ref()
        .ok_or_else(|| "core command sender is not available".to_owned())?;
    sender
        .send(Command::Logout)
        .map_err(|err| format!("failed to enqueue logout command: {err}"))
}

fn submit_recover_e2ee(state: &mut WindowState, recovery_secret: String) -> Result<(), String> {
    if recovery_secret.is_empty() {
        return Err("recovery key or passphrase must not be empty".to_owned());
    }

    let command = Command::RecoverE2ee(RecoverE2eeRequest { recovery_secret });
    let sender = state
        .command_sender
        .as_ref()
        .ok_or_else(|| "core command sender is not available".to_owned())?;
    sender
        .send(command.clone())
        .map_err(|err| format!("failed to enqueue recover e2ee command: {err}"))?;
    #[cfg(test)]
    state.recorded_commands.push(command);
    state.model.status = "e2ee recovery submitted".to_owned();
    Ok(())
}

unsafe extern "system" fn edit_window_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    let parent = GetParent(hwnd);
    if parent.is_null() {
        return DefWindowProcW(hwnd, message, wparam, lparam);
    }

    if message == WM_KEYDOWN && wparam == VK_RETURN_KEY {
        if let Err(message) = submit_compose_from_edit(parent) {
            let _ = set_local_command_status(parent, message);
        }
        return 0;
    }

    if message == WM_KEYDOWN && wparam == VK_UP_KEY {
        let _ = navigate_compose_history(parent, ComposeHistoryDirection::Previous);
        return 0;
    }

    if message == WM_KEYDOWN && wparam == VK_DOWN_KEY {
        let _ = navigate_compose_history(parent, ComposeHistoryDirection::Next);
        return 0;
    }

    if message == WM_CHAR
        || message == WM_PASTE
        || (message == WM_KEYDOWN && wparam == VK_DELETE_KEY)
    {
        state_mut(parent).compose_history.reset_navigation();
    }

    let original = state_ref(parent).edit_window_proc;
    CallWindowProcW(original, hwnd, message, wparam, lparam)
}

fn message_loop() {
    let mut message = MSG::default();
    loop {
        let result = unsafe { GetMessageW(&mut message, null_mut(), 0, 0) };
        if result <= 0 {
            break;
        }

        unsafe {
            let frame = frame_window_for(message.hwnd);
            if !frame.is_null() {
                let mdi_client = state_ref(frame).controls.mdi_client;
                if !mdi_client.is_null() && TranslateMDISysAccel(mdi_client, &message) != 0 {
                    continue;
                }
            }
            TranslateMessage(&message);
            DispatchMessageW(&message);
        }
    }
}

unsafe fn frame_window_for(mut hwnd: HWND) -> HWND {
    let mut frame = hwnd;
    while !hwnd.is_null() {
        frame = hwnd;
        hwnd = GetParent(hwnd);
    }

    frame
}

unsafe fn state_ref(hwnd: HWND) -> &'static WindowState {
    let state = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *const WindowState;
    assert!(!state.is_null(), "window state must exist");
    &*state
}

unsafe fn state_mut(hwnd: HWND) -> &'static mut WindowState {
    let state = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut WindowState;
    assert!(!state.is_null(), "window state must exist");
    &mut *state
}

unsafe fn default_window_proc(hwnd: HWND, message: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    let state = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *const WindowState;
    if state.is_null() {
        return DefWindowProcW(hwnd, message, wparam, lparam);
    }

    let mdi_client = (*state).controls.mdi_client;
    if mdi_client.is_null() {
        DefWindowProcW(hwnd, message, wparam, lparam)
    } else {
        DefFrameProcW(hwnd, mdi_client, message, wparam, lparam)
    }
}

fn dump_submitted_membership_command(command: &Command) -> Option<DumpSubmittedMembershipCommand> {
    match command {
        Command::JoinRoom(request) => Some(DumpSubmittedMembershipCommand {
            action: DumpMembershipCommandAction::Join,
            target: request.room_or_alias.clone(),
        }),
        Command::AcceptRoomInvite(request) => Some(DumpSubmittedMembershipCommand {
            action: DumpMembershipCommandAction::AcceptInvite,
            target: request.room_id.clone(),
        }),
        Command::LeaveRoom(request) => Some(DumpSubmittedMembershipCommand {
            action: DumpMembershipCommandAction::Leave,
            target: request.room_id.clone(),
        }),
        Command::RejectRoomInvite(request) => Some(DumpSubmittedMembershipCommand {
            action: DumpMembershipCommandAction::RejectInvite,
            target: request.room_id.clone(),
        }),
        _ => None,
    }
}

fn dump_submitted_direct_room_command(command: &Command) -> Option<DumpSubmittedDirectRoomCommand> {
    match command {
        Command::CreateDirectRoom(request) => Some(DumpSubmittedDirectRoomCommand {
            user_id: request.user_id.clone(),
        }),
        _ => None,
    }
}

fn dump_submitted_room_topic_command(command: &Command) -> Option<DumpSubmittedRoomTopicCommand> {
    match command {
        Command::SetRoomTopic(request) => Some(DumpSubmittedRoomTopicCommand {
            room_id: request.room_id.clone(),
            topic: request.topic.clone(),
        }),
        _ => None,
    }
}

fn dump_submitted_reaction_command(command: &Command) -> Option<DumpSubmittedReactionCommand> {
    match command {
        Command::SendReaction(request) => Some(DumpSubmittedReactionCommand {
            room_id: request.room_id.clone(),
            target_event_id: request.target_event_id.clone(),
            key: request.key.clone(),
        }),
        _ => None,
    }
}

fn dump_submitted_edit_command(command: &Command) -> Option<DumpSubmittedEditCommand> {
    match command {
        Command::EditMessage(request) => Some(DumpSubmittedEditCommand {
            room_id: request.room_id.clone(),
            target_event_id: request.target_event_id.clone(),
            body: request.body.clone(),
        }),
        _ => None,
    }
}

fn dump_submitted_reply_command(command: &Command) -> Option<DumpSubmittedReplyCommand> {
    match command {
        Command::SendMessage(request) => {
            request
                .reply
                .as_ref()
                .map(|reply| DumpSubmittedReplyCommand {
                    room_id: request.room_id.clone(),
                    target_event_id: reply.event_id.clone(),
                    body: request.body.clone(),
                    thread: request
                        .thread
                        .as_ref()
                        .map(|thread| DumpSubmittedReplyThreadTarget {
                            thread_root_event_id: thread.thread_root_event_id.clone(),
                            latest_event_id: thread.latest_event_id.clone(),
                        }),
                })
        }
        _ => None,
    }
}

fn dump_submitted_redaction_command(command: &Command) -> Option<DumpSubmittedRedactionCommand> {
    match command {
        Command::RedactEvent(request) => Some(DumpSubmittedRedactionCommand {
            room_id: request.room_id.clone(),
            event_id: request.event_id.clone(),
        }),
        _ => None,
    }
}

fn dump_submitted_history_command(command: &Command) -> Option<DumpSubmittedHistoryCommand> {
    match command {
        Command::BackfillRoomHistory(request) => Some(DumpSubmittedHistoryCommand {
            room_id: request.room_id.clone(),
            from: request.from.clone(),
            limit: request.limit,
        }),
        _ => None,
    }
}

fn submitted_membership_commands_from_state(
    state: &WindowState,
) -> Vec<DumpSubmittedMembershipCommand> {
    state
        .recorded_commands
        .iter()
        .filter_map(dump_submitted_membership_command)
        .collect()
}

fn submitted_direct_room_commands_from_state(
    state: &WindowState,
) -> Vec<DumpSubmittedDirectRoomCommand> {
    state
        .recorded_commands
        .iter()
        .filter_map(dump_submitted_direct_room_command)
        .collect()
}

fn submitted_room_topic_commands_from_state(
    state: &WindowState,
) -> Vec<DumpSubmittedRoomTopicCommand> {
    state
        .recorded_commands
        .iter()
        .filter_map(dump_submitted_room_topic_command)
        .collect()
}

fn submitted_reaction_commands_from_state(
    state: &WindowState,
) -> Vec<DumpSubmittedReactionCommand> {
    state
        .recorded_commands
        .iter()
        .filter_map(dump_submitted_reaction_command)
        .collect()
}

fn submitted_edit_commands_from_state(state: &WindowState) -> Vec<DumpSubmittedEditCommand> {
    state
        .recorded_commands
        .iter()
        .filter_map(dump_submitted_edit_command)
        .collect()
}

fn submitted_reply_commands_from_state(state: &WindowState) -> Vec<DumpSubmittedReplyCommand> {
    state
        .recorded_commands
        .iter()
        .filter_map(dump_submitted_reply_command)
        .collect()
}

fn submitted_redaction_commands_from_state(
    state: &WindowState,
) -> Vec<DumpSubmittedRedactionCommand> {
    state
        .recorded_commands
        .iter()
        .filter_map(dump_submitted_redaction_command)
        .collect()
}

fn submitted_history_commands_from_state(state: &WindowState) -> Vec<DumpSubmittedHistoryCommand> {
    state
        .recorded_commands
        .iter()
        .filter_map(dump_submitted_history_command)
        .collect()
}

fn dump_submitted_verification_action(
    command: &Command,
) -> Option<DumpSubmittedVerificationAction> {
    let (action, flow) = match command {
        Command::AcceptVerification(flow) => (DumpVerificationActionKind::Accept, flow),
        Command::CancelVerification(flow) => (DumpVerificationActionKind::Cancel, flow),
        Command::ConfirmVerificationSas(flow) => (DumpVerificationActionKind::ConfirmSas, flow),
        Command::RejectVerificationSas(flow) => (DumpVerificationActionKind::RejectSas, flow),
        _ => return None,
    };
    Some(DumpSubmittedVerificationAction {
        action,
        flow_id: flow.flow_id.clone(),
        user_id: flow.user_id.clone(),
    })
}

fn submitted_verification_actions_from_state(
    state: &WindowState,
) -> Vec<DumpSubmittedVerificationAction> {
    state
        .recorded_commands
        .iter()
        .filter_map(dump_submitted_verification_action)
        .collect()
}

fn dump_model_from_state(state: &WindowState) -> DumpModel {
    let model = &state.model;
    DumpModel {
        status: model.status.clone(),
        lifecycle_state: model.lifecycle_state,
        rooms: model.rooms.iter().map(dump_room_from_summary).collect(),
        last_activity: model
            .last_activity
            .as_ref()
            .map(dump_room_message_activity_line_from_last_activity),
        e2ee_status: model.last_e2ee_status.clone(),
        last_verification: model
            .last_verification
            .as_ref()
            .map(dump_verification_from_state_event),
        submitted_membership_commands: submitted_membership_commands_from_state(state),
        submitted_direct_room_commands: submitted_direct_room_commands_from_state(state),
        submitted_room_topic_commands: submitted_room_topic_commands_from_state(state),
        submitted_reaction_commands: submitted_reaction_commands_from_state(state),
        submitted_edit_commands: submitted_edit_commands_from_state(state),
        submitted_reply_commands: submitted_reply_commands_from_state(state),
        submitted_redaction_commands: submitted_redaction_commands_from_state(state),
        submitted_history_commands: submitted_history_commands_from_state(state),
        submitted_verification_actions: submitted_verification_actions_from_state(state),
        last_error: model.last_error.as_ref().map(dump_error_from_core),
        selected_room: model.selected_room.clone(),
    }
}

fn verification_state_event_from_request(request: VerificationRequest) -> VerificationStateEvent {
    VerificationStateEvent {
        flow_id: request.flow_id,
        user_id: request.user_id,
        state: VerificationState::Requested,
    }
}

fn verification_state_label(state: &VerificationState) -> &'static str {
    match state {
        VerificationState::Requested => "requested",
        VerificationState::Accepted => "accepted",
        VerificationState::Ready => "ready",
        VerificationState::SasReady { .. } => "sas ready",
        VerificationState::Confirmed => "confirmed",
        VerificationState::Done => "done",
        VerificationState::Cancelled { .. } => "cancelled",
    }
}

fn e2ee_recovery_state_label(state: E2eeRecoveryState) -> &'static str {
    match state {
        E2eeRecoveryState::Unknown => "unknown",
        E2eeRecoveryState::Enabled => "enabled",
        E2eeRecoveryState::Disabled => "disabled",
        E2eeRecoveryState::Incomplete => "incomplete",
    }
}

fn e2ee_backup_state_label(state: E2eeBackupState) -> &'static str {
    match state {
        E2eeBackupState::Unknown => "unknown",
        E2eeBackupState::Creating => "creating",
        E2eeBackupState::Enabling => "enabling",
        E2eeBackupState::Resuming => "resuming",
        E2eeBackupState::Enabled => "enabled",
        E2eeBackupState::Downloading => "downloading",
        E2eeBackupState::Disabling => "disabling",
    }
}

fn e2ee_cross_signing_label(status: Option<&E2eeCrossSigningStatus>) -> &'static str {
    match status {
        Some(status) if status.complete => "complete",
        Some(_) => "incomplete",
        None => "unknown",
    }
}

fn e2ee_backup_exists_label(exists_on_server: Option<bool>) -> &'static str {
    match exists_on_server {
        Some(true) => "true",
        Some(false) => "false",
        None => "unknown",
    }
}

fn verification_status_message(event: &VerificationStateEvent) -> String {
    match &event.state {
        VerificationState::Requested => format!("verification requested by {}", event.user_id),
        VerificationState::Accepted => format!("verification accepted with {}", event.user_id),
        VerificationState::Ready => format!("verification ready with {}", event.user_id),
        VerificationState::SasReady { .. } => {
            format!("verification sas ready with {}", event.user_id)
        }
        VerificationState::Confirmed => format!("verification confirmed with {}", event.user_id),
        VerificationState::Done => format!("verification done with {}", event.user_id),
        VerificationState::Cancelled { .. } => {
            format!("verification cancelled with {}", event.user_id)
        }
    }
}

fn format_verification_emojis(emojis: &[VerificationEmoji]) -> String {
    emojis
        .iter()
        .map(|emoji| format!("{} {}", emoji.symbol, emoji.description))
        .collect::<Vec<_>>()
        .join(" | ")
}

fn verification_actions_text(state: &VerificationState) -> Option<&'static str> {
    match state {
        VerificationState::Requested | VerificationState::Accepted | VerificationState::Ready => {
            Some("/verify accept, /verify cancel")
        }
        VerificationState::SasReady { .. } => {
            Some("/verify confirm, /verify reject, /verify cancel")
        }
        VerificationState::Confirmed => Some("/verify cancel"),
        VerificationState::Done | VerificationState::Cancelled { .. } => None,
    }
}

fn verification_action_label(action: VerificationAction) -> &'static str {
    match action {
        VerificationAction::Accept => "accept",
        VerificationAction::Cancel => "cancel",
        VerificationAction::Confirm => "confirm",
        VerificationAction::Reject => "reject",
    }
}

fn verification_flow_id_from_action_args(
    model: &UiModel,
    args: &VerificationActionArgs,
) -> Result<VerificationFlowId, String> {
    match (&args.flow_id, &args.user_id) {
        (Some(flow_id), Some(user_id)) => Ok(VerificationFlowId {
            flow_id: flow_id.clone(),
            user_id: user_id.clone(),
        }),
        (None, None) => current_verification_flow_id(model),
        _ => unreachable!("verification action args must contain both flow_id and user_id"),
    }
}

fn current_verification_flow_id(model: &UiModel) -> Result<VerificationFlowId, String> {
    let event = model
        .last_verification
        .as_ref()
        .ok_or_else(|| "no verification flow available".to_owned())?;
    Ok(VerificationFlowId {
        flow_id: event.flow_id.clone(),
        user_id: event.user_id.clone(),
    })
}

fn submit_verification_command(
    state: &mut WindowState,
    command: Command,
    action_name: &'static str,
) -> Result<(), String> {
    {
        let sender = state
            .command_sender
            .as_ref()
            .ok_or_else(|| "core command sender is not available".to_owned())?;
        sender.send(command.clone()).map_err(|err| {
            format!("failed to enqueue {action_name} verification command: {err}")
        })?;
    }
    state.recorded_commands.push(command);
    Ok(())
}

unsafe fn submit_automation_verification_action(
    hwnd: HWND,
    action: VerificationAction,
    args: VerificationActionArgs,
) -> Result<(), String> {
    {
        let state = state_mut(hwnd);
        submit_verification_automation_action_to_state(state, action, args)?;
    }
    set_local_command_status(
        hwnd,
        format!(
            "verification {} submitted",
            verification_action_label(action)
        ),
    )
}

fn submit_verification_automation_action_to_state(
    state: &mut WindowState,
    action: VerificationAction,
    args: VerificationActionArgs,
) -> Result<(), String> {
    let flow = verification_flow_id_from_action_args(&state.model, &args)?;
    match action {
        VerificationAction::Accept => submit_accept_verification_flow(state, flow),
        VerificationAction::Cancel => submit_cancel_verification_flow(state, flow),
        VerificationAction::Confirm => submit_confirm_verification_sas_flow(state, flow),
        VerificationAction::Reject => submit_reject_verification_sas_flow(state, flow),
    }
}

fn submit_mark_room_read(
    state: &mut WindowState,
    room_id: &str,
    event_id: String,
) -> Result<(), String> {
    let command = Command::MarkRoomRead(MarkRoomReadRequest {
        room_id: room_id.to_owned(),
        event_id,
    });
    let Some(sender) = state.command_sender.as_ref() else {
        return Ok(());
    };
    sender
        .send(command.clone())
        .map_err(|err| format!("failed to enqueue mark room read command: {err}"))?;
    #[cfg(test)]
    state.recorded_commands.push(command);
    Ok(())
}

fn loading_room_history_room_id(room_views: &BTreeMap<String, RoomViewModel>) -> Option<String> {
    let mut loading_room_ids = room_views
        .iter()
        .filter(|(_, room_view)| {
            matches!(
                room_view.history.status,
                RoomHistoryBackfillStatus::Loading { .. }
            )
        })
        .map(|(room_id, _)| room_id.clone());
    let loading_room_id = loading_room_ids.next();
    #[cfg(test)]
    {
        assert!(
            loading_room_ids.next().is_none(),
            "room history backfill must keep a single in-flight loading room"
        );
    }
    loading_room_id
}

fn submit_backfill_room_history_for_room(
    state: &mut WindowState,
    room_id: &str,
) -> Result<bool, String> {
    if sendable_room_by_id(&state.model.rooms, room_id).is_none() {
        return Ok(false);
    }
    if loading_room_history_room_id(&state.model.room_views).is_some() {
        return Ok(false);
    }

    let Some(sender) = state.command_sender.as_ref().cloned() else {
        return Ok(false);
    };

    let command = {
        let room_view = state
            .model
            .room_views
            .entry(room_id.to_owned())
            .or_default();
        if matches!(
            room_view.history.status,
            RoomHistoryBackfillStatus::Exhausted
        ) {
            return Ok(false);
        }

        let from = room_view.history.next_token.clone();
        room_view.history.status = RoomHistoryBackfillStatus::Loading { from: from.clone() };
        Command::BackfillRoomHistory(BackfillRoomHistoryRequest {
            room_id: room_id.to_owned(),
            from,
            limit: ROOM_HISTORY_BACKFILL_DEFAULT_LIMIT,
        })
    };

    sender
        .send(command.clone())
        .map_err(|err| format!("failed to enqueue backfill room history command: {err}"))?;
    state.recorded_commands.push(command);
    Ok(true)
}

unsafe fn submit_backfill_room_history(hwnd: HWND) -> Result<(), String> {
    let room_content = GetParent(hwnd);
    if room_content.is_null() {
        return Ok(());
    }
    let mdi_child = GetParent(room_content);
    if mdi_child.is_null() {
        return Ok(());
    }

    let state = timeline_window_state_mut(hwnd);
    let ViewId::Room(room_id) = &state.active_view else {
        return Ok(());
    };
    let room_id = room_id.clone();
    let Some(active_view) = state.views.get(&state.active_view) else {
        return Ok(());
    };
    if active_view.child != mdi_child {
        return Ok(());
    }

    let _ = submit_backfill_room_history_for_room(state, &room_id)?;
    Ok(())
}

fn clear_view_activity_marker(state: &mut WindowState, view_id: &ViewId) -> (bool, Option<String>) {
    let read_target = state.room_activity_read_targets.remove(view_id);
    let cleared_view_activity = state.room_activity.remove(view_id);
    let read_target = if cleared_view_activity {
        read_target
    } else {
        None
    };
    (cleared_view_activity, read_target)
}

fn handle_view_activation(state: &mut WindowState, view_id: &ViewId) -> Result<bool, String> {
    let (cleared_view_activity, read_target) = clear_view_activity_marker(state, view_id);
    if let Some(event_id) = read_target {
        let room_id = view_id
            .room_id()
            .expect("room activity markers must target room or thread views");
        submit_mark_room_read(state, room_id, event_id)?;
    }
    Ok(cleared_view_activity)
}

fn handle_room_activation(state: &mut WindowState, room_id: &str) -> Result<bool, String> {
    handle_view_activation(state, &ViewId::Room(room_id.to_owned()))
}

fn handle_thread_activation(
    state: &mut WindowState,
    room_id: &str,
    thread_root_event_id: &str,
) -> Result<bool, String> {
    handle_view_activation(
        state,
        &ViewId::Thread {
            room_id: room_id.to_owned(),
            thread_root_event_id: thread_root_event_id.to_owned(),
        },
    )
}

fn submit_accept_verification_flow(
    state: &mut WindowState,
    flow: VerificationFlowId,
) -> Result<(), String> {
    submit_verification_command(state, Command::AcceptVerification(flow), "accept")
}

fn submit_accept_verification(state: &mut WindowState) -> Result<(), String> {
    let flow = current_verification_flow_id(&state.model)?;
    submit_accept_verification_flow(state, flow)
}

fn submit_cancel_verification_flow(
    state: &mut WindowState,
    flow: VerificationFlowId,
) -> Result<(), String> {
    submit_verification_command(state, Command::CancelVerification(flow), "cancel")
}

fn submit_cancel_verification(state: &mut WindowState) -> Result<(), String> {
    let flow = current_verification_flow_id(&state.model)?;
    submit_cancel_verification_flow(state, flow)
}

fn submit_confirm_verification_sas_flow(
    state: &mut WindowState,
    flow: VerificationFlowId,
) -> Result<(), String> {
    submit_verification_command(state, Command::ConfirmVerificationSas(flow), "confirm")
}

fn submit_confirm_verification_sas(state: &mut WindowState) -> Result<(), String> {
    let flow = current_verification_flow_id(&state.model)?;
    submit_confirm_verification_sas_flow(state, flow)
}

fn submit_reject_verification_sas_flow(
    state: &mut WindowState,
    flow: VerificationFlowId,
) -> Result<(), String> {
    submit_verification_command(state, Command::RejectVerificationSas(flow), "reject")
}

fn submit_reject_verification_sas(state: &mut WindowState) -> Result<(), String> {
    let flow = current_verification_flow_id(&state.model)?;
    submit_reject_verification_sas_flow(state, flow)
}

fn dump_room_activity_entry_from_room_entry(
    room_view: &RoomViewModel,
    activity: &RoomActivity,
) -> DumpRoomActivity {
    let mut dump = dump_room_activity_from_activity(activity);
    if let Some(event_id) = room_activity_event_id(activity) {
        dump.reactions = room_view.reaction_summaries_for_event(event_id);
    }
    let RoomActivityKind::Image(image) = &activity.kind else {
        return dump;
    };
    let DumpRoomActivityKind::Image(dump_image) = &mut dump.kind else {
        return dump;
    };
    dump_image.media_state = dump_room_image_media_state(
        room_view,
        &activity.activity_id,
        &image.event_id,
        &timeline_thumbnail_format(),
    );
    dump
}

fn dump_room_image_media_state(
    room_view: &RoomViewModel,
    activity_id: &str,
    event_id: &str,
    format: &RoomImageMediaFormat,
) -> Option<DumpRoomImageMediaState> {
    match room_view.image_media.get(activity_id) {
        Some(RoomImageMediaState::Pending {
            event_id: stored_event_id,
            format: stored_format,
        }) if stored_event_id == event_id && stored_format == format => {
            Some(DumpRoomImageMediaState::Pending)
        }
        Some(RoomImageMediaState::Loaded {
            event_id: stored_event_id,
            format: stored_format,
            width,
            height,
            ..
        }) if stored_event_id == event_id && stored_format == format => {
            Some(DumpRoomImageMediaState::Loaded {
                width: *width,
                height: *height,
            })
        }
        Some(RoomImageMediaState::Failed {
            event_id: stored_event_id,
            format: stored_format,
            message,
        }) if stored_event_id == event_id && stored_format == format => {
            Some(DumpRoomImageMediaState::Failed {
                message: message.clone(),
            })
        }
        _ => None,
    }
}

fn dump_room_message_activity_line_from_last_activity(
    activity: &LastMessageActivity,
) -> DumpRoomMessageActivityLine {
    DumpRoomMessageActivityLine {
        room_id: activity.room_id.clone(),
        activity_id: activity
            .event_id
            .as_ref()
            .map(|event_id| format!("event:{event_id}"))
            .or_else(|| {
                activity
                    .transaction_id
                    .as_ref()
                    .map(|transaction_id| format!("txn:{transaction_id}"))
            })
            .expect("last activity message must have an event id or transaction id"),
        timestamp_ms: activity.timestamp_ms,
        event_id: activity.event_id.clone(),
        transaction_id: activity.transaction_id.clone(),
        sender: activity.sender.clone(),
        presentation: activity.presentation,
        body: activity.body.clone(),
        send_state: dump_room_message_send_state_from_core(&activity.send_state),
    }
}

fn resolve_timeline_room_id(
    state: &WindowState,
    explicit_room_id: Option<&str>,
) -> Result<String, String> {
    let room_id = match explicit_room_id {
        Some(room_id) => room_id.to_owned(),
        None => state
            .model
            .selected_room
            .clone()
            .ok_or_else(|| "no room selected".to_owned())?,
    };

    if navigable_room_by_id(&state.model.rooms, &room_id).is_some() {
        Ok(room_id)
    } else {
        Err(format!("unknown room id: {room_id}"))
    }
}

fn operation_name(operation: Operation) -> &'static str {
    match operation {
        Operation::Login => "login",
        Operation::RestoreSession => "restore session",
        Operation::SessionUpdate => "session update",
        Operation::StartSync => "start sync",
        Operation::JoinRoom => "join room",
        Operation::AcceptRoomInvite => "accept room invitation",
        Operation::CreateDirectRoom => "create direct room",
        Operation::LeaveRoom => "leave room",
        Operation::RejectRoomInvite => "reject room invitation",
        Operation::SetRoomTopic => "set room topic",
        Operation::SendMessage => "send text",
        Operation::EditMessage => "edit message",
        Operation::SendReaction => "send reaction",
        Operation::RedactEvent => "redact event",
        Operation::MarkRoomRead => "mark room read",
        Operation::BackfillRoomHistory => "backfill room history",
        Operation::FetchRoomImageMedia => "fetch room image media",
        Operation::RoomList => "room list",
        Operation::RoomReadState => "room read state",
        Operation::E2eeStatus => "e2ee status",
        Operation::RecoverE2ee => "e2ee recovery",
        Operation::Verification => "verification",
        Operation::Logout => "logout",
        Operation::Shutdown => "shutdown",
    }
}

unsafe fn read_window_text(hwnd: HWND) -> String {
    let text_len = SendMessageW(hwnd, WM_GETTEXTLENGTH, 0, 0);
    if text_len <= 0 {
        return String::new();
    }

    let mut text = vec![0u16; text_len as usize + 1];
    SendMessageW(hwnd, WM_GETTEXT, text.len(), text.as_mut_ptr() as LPARAM);
    String::from_utf16_lossy(&text[..text_len as usize])
}

fn set_window_text(hwnd: HWND, text: &str) {
    let text = wide_null(text);
    unsafe {
        SendMessageW(hwnd, WM_SETTEXT, 0, text.as_ptr() as LPARAM);
    }
}

fn set_edit_text(hwnd: HWND, text: &str) {
    set_window_text(hwnd, text);
}

unsafe fn set_compose_edit_text(hwnd: HWND, text: &str) {
    set_edit_text(hwnd, text);
    let len = text.encode_utf16().count();
    SendMessageW(hwnd, EM_SETSEL, len, len as LPARAM);
}

fn show_error_message(hwnd: HWND, title: &str, message: &str) {
    let title = wide_null(title);
    let message = wide_null(message);
    unsafe {
        MessageBoxW(hwnd, message.as_ptr(), title.as_ptr(), MB_OK | MB_ICONERROR);
    }
}

fn wide_null(text: &str) -> Vec<u16> {
    OsStr::new(text).encode_wide().chain([0]).collect()
}

unsafe fn wide_ptr_to_string(text_ptr: *const u16) -> String {
    if text_ptr.is_null() {
        return String::new();
    }

    let mut len = 0usize;
    while *text_ptr.add(len) != 0 {
        len += 1;
    }
    String::from_utf16_lossy(std::slice::from_raw_parts(text_ptr, len))
}

fn last_os_error(operation: &str) -> String {
    let error = std::io::Error::last_os_error();
    format!("{operation} failed: {error}")
}

#[cfg(test)]
mod tests {
    use std::env;
    use std::fs;
    use std::sync::Mutex as StdMutex;
    use std::time::{SystemTime, UNIX_EPOCH};

    use mmatrix_core::RoomStateNoticeActivity;

    use super::*;

    static LOCAL_APP_DATA_LOCK: StdMutex<()> = StdMutex::new(());

    fn room(room_id: &str, display_name: &str, encrypted: bool) -> RoomSummary {
        RoomSummary {
            room_id: room_id.to_owned(),
            display_name: display_name.to_owned(),
            membership: RoomMembership::Join,
            encrypted,
            is_space: false,
            is_direct: false,
            direct_user_ids: Vec::new(),
            tags: Vec::new(),
            space_children: Vec::new(),
        }
    }

    fn invited_room(room_id: &str, display_name: &str) -> RoomSummary {
        RoomSummary {
            room_id: room_id.to_owned(),
            display_name: display_name.to_owned(),
            membership: RoomMembership::Invite,
            encrypted: false,
            is_space: false,
            is_direct: false,
            direct_user_ids: Vec::new(),
            tags: Vec::new(),
            space_children: Vec::new(),
        }
    }

    fn space(room_id: &str, display_name: &str) -> RoomSummary {
        RoomSummary {
            room_id: room_id.to_owned(),
            display_name: display_name.to_owned(),
            membership: RoomMembership::Join,
            encrypted: false,
            is_space: true,
            is_direct: false,
            direct_user_ids: Vec::new(),
            tags: Vec::new(),
            space_children: Vec::new(),
        }
    }

    fn e2ee_status(
        recovery: E2eeRecoveryState,
        backup_state: E2eeBackupState,
        backup_enabled_local: bool,
        backup_exists_on_server: Option<bool>,
        cross_signing_complete: Option<bool>,
    ) -> E2eeStatus {
        E2eeStatus {
            recovery,
            backup: mmatrix_core::E2eeBackupStatus {
                state: backup_state,
                enabled_locally: backup_enabled_local,
                exists_on_server: backup_exists_on_server,
            },
            cross_signing: cross_signing_complete.map(|complete| E2eeCrossSigningStatus {
                has_master: true,
                has_self_signing: complete,
                has_user_signing: complete,
                complete,
            }),
        }
    }

    fn sent_message_activity_with_presentation(
        room_id: &str,
        event_id: &str,
        transaction_id: Option<&str>,
        presentation: RoomMessagePresentationKind,
        body: &str,
        timestamp_ms: u64,
    ) -> RoomActivity {
        RoomActivity {
            room_id: room_id.to_owned(),
            activity_id: format!("event:{event_id}"),
            timestamp_ms,
            relation: None,
            kind: RoomActivityKind::Message(RoomMessageActivity {
                event_id: Some(event_id.to_owned()),
                transaction_id: transaction_id.map(str::to_owned),
                sender: "@alice:example".to_owned(),
                presentation,
                body: body.to_owned(),
                send_state: RoomMessageSendState::Sent,
            }),
        }
    }

    fn sent_message_activity_with_relation_and_presentation(
        room_id: &str,
        event_id: &str,
        transaction_id: Option<&str>,
        relation: Option<RoomActivityRelation>,
        presentation: RoomMessagePresentationKind,
        body: &str,
        timestamp_ms: u64,
    ) -> RoomActivity {
        RoomActivity {
            relation,
            ..sent_message_activity_with_presentation(
                room_id,
                event_id,
                transaction_id,
                presentation,
                body,
                timestamp_ms,
            )
        }
    }

    fn sent_message_activity(
        room_id: &str,
        event_id: &str,
        transaction_id: Option<&str>,
        body: &str,
        timestamp_ms: u64,
    ) -> RoomActivity {
        sent_message_activity_with_presentation(
            room_id,
            event_id,
            transaction_id,
            RoomMessagePresentationKind::Text,
            body,
            timestamp_ms,
        )
    }

    fn thread_relation(thread_root_event_id: &str) -> RoomActivityRelation {
        RoomActivityRelation::Thread {
            thread_root_event_id: thread_root_event_id.to_owned(),
            in_reply_to_event_id: Some(thread_root_event_id.to_owned()),
            is_falling_back: false,
        }
    }

    #[test]
    fn frame_size_defaults_for_run_command() {
        assert_eq!(
            frame_size_for_command(&AppCommand::Run(RunOptions {
                automation: false,
                render_state: None,
                frame_size: None,
            })),
            DEFAULT_FRAME_SIZE
        );
    }

    #[test]
    fn frame_size_uses_explicit_run_value() {
        assert_eq!(
            frame_size_for_command(&AppCommand::Run(RunOptions {
                automation: true,
                render_state: None,
                frame_size: Some(FrameSize {
                    width: 640,
                    height: 768,
                }),
            })),
            FrameSize {
                width: 640,
                height: 768,
            }
        );
    }

    #[test]
    fn frame_size_defaults_for_self_test() {
        assert_eq!(
            frame_size_for_command(&AppCommand::SelfTest),
            DEFAULT_FRAME_SIZE
        );
    }

    #[test]
    fn frame_size_defaults_for_auto_close() {
        assert_eq!(
            frame_size_for_command(&AppCommand::AutoClose),
            DEFAULT_FRAME_SIZE
        );
    }

    fn reply_relation(in_reply_to_event_id: &str) -> RoomActivityRelation {
        RoomActivityRelation::Reply {
            in_reply_to_event_id: in_reply_to_event_id.to_owned(),
        }
    }

    fn timeline(room_id: &str, body: &str) -> Event {
        let event_id = format!("${room_id}:{}", body.replace(' ', "_"));
        Event::RoomActivity(Box::new(sent_message_activity(
            room_id,
            &event_id,
            None,
            body,
            1_700_000_000_000,
        )))
    }

    fn timeline_with_relation_and_presentation(
        room_id: &str,
        event_id: &str,
        transaction_id: Option<&str>,
        relation: Option<RoomActivityRelation>,
        presentation: RoomMessagePresentationKind,
        body: &str,
        timestamp_ms: u64,
    ) -> Event {
        Event::RoomActivity(Box::new(
            sent_message_activity_with_relation_and_presentation(
                room_id,
                event_id,
                transaction_id,
                relation,
                presentation,
                body,
                timestamp_ms,
            ),
        ))
    }

    fn thread_timeline(room_id: &str, event_id: &str, root_event_id: &str, body: &str) -> Event {
        timeline_with_relation_and_presentation(
            room_id,
            event_id,
            None,
            Some(thread_relation(root_event_id)),
            RoomMessagePresentationKind::Text,
            body,
            1_700_000_120_000,
        )
    }

    fn timeline_with_transaction_id_with_presentation(
        room_id: &str,
        event_id: &str,
        transaction_id: Option<&str>,
        presentation: RoomMessagePresentationKind,
        body: &str,
        timestamp_ms: u64,
    ) -> Event {
        Event::RoomActivity(Box::new(sent_message_activity_with_presentation(
            room_id,
            event_id,
            transaction_id,
            presentation,
            body,
            timestamp_ms,
        )))
    }

    fn timeline_with_transaction_id(
        room_id: &str,
        event_id: &str,
        transaction_id: Option<&str>,
        body: &str,
        timestamp_ms: u64,
    ) -> Event {
        timeline_with_transaction_id_with_presentation(
            room_id,
            event_id,
            transaction_id,
            RoomMessagePresentationKind::Text,
            body,
            timestamp_ms,
        )
    }

    fn pending_message_activity_with_presentation(
        room_id: &str,
        transaction_id: &str,
        presentation: RoomMessagePresentationKind,
        body: &str,
    ) -> RoomActivity {
        RoomActivity {
            room_id: room_id.to_owned(),
            activity_id: format!("txn:{transaction_id}"),
            timestamp_ms: 1_700_000_000_000,
            relation: None,
            kind: RoomActivityKind::Message(RoomMessageActivity {
                event_id: None,
                transaction_id: Some(transaction_id.to_owned()),
                sender: "@alice:example".to_owned(),
                presentation,
                body: body.to_owned(),
                send_state: RoomMessageSendState::Pending,
            }),
        }
    }

    fn pending_message_activity_with_relation_and_presentation(
        room_id: &str,
        transaction_id: &str,
        relation: Option<RoomActivityRelation>,
        presentation: RoomMessagePresentationKind,
        body: &str,
    ) -> RoomActivity {
        let mut activity =
            pending_message_activity_with_presentation(room_id, transaction_id, presentation, body);
        activity.relation = relation;
        activity
    }

    fn pending_message_activity(room_id: &str, transaction_id: &str, body: &str) -> RoomActivity {
        pending_message_activity_with_presentation(
            room_id,
            transaction_id,
            RoomMessagePresentationKind::Text,
            body,
        )
    }

    fn outgoing_queued(room_id: &str, transaction_id: &str, body: &str) -> Event {
        Event::RoomActivity(Box::new(pending_message_activity(
            room_id,
            transaction_id,
            body,
        )))
    }

    fn outgoing_queued_with_presentation(
        room_id: &str,
        transaction_id: &str,
        presentation: RoomMessagePresentationKind,
        body: &str,
    ) -> Event {
        Event::RoomActivity(Box::new(pending_message_activity_with_presentation(
            room_id,
            transaction_id,
            presentation,
            body,
        )))
    }

    fn outgoing_queued_in_thread(
        room_id: &str,
        transaction_id: &str,
        root_event_id: &str,
    ) -> Event {
        Event::RoomActivity(Box::new(
            pending_message_activity_with_relation_and_presentation(
                room_id,
                transaction_id,
                Some(thread_relation(root_event_id)),
                RoomMessagePresentationKind::Text,
                "pending thread reply",
            ),
        ))
    }

    fn outgoing_sent(room_id: &str, transaction_id: &str, event_id: &str) -> Event {
        Event::RoomActivityUpdate(RoomActivityUpdate {
            room_id: room_id.to_owned(),
            target: RoomActivityUpdateTarget::Transaction {
                transaction_id: transaction_id.to_owned(),
            },
            kind: RoomActivityUpdateKind::MessageSent {
                event_id: event_id.to_owned(),
            },
        })
    }

    fn outgoing_failed(room_id: &str, transaction_id: &str, message: &str) -> Event {
        Event::RoomActivityUpdate(RoomActivityUpdate {
            room_id: room_id.to_owned(),
            target: RoomActivityUpdateTarget::Transaction {
                transaction_id: transaction_id.to_owned(),
            },
            kind: RoomActivityUpdateKind::MessageFailed {
                message: message.to_owned(),
                recoverable: true,
            },
        })
    }

    fn incoming_replaced(
        room_id: &str,
        target_event_id: &str,
        replacement_event_id: &str,
        presentation: RoomMessagePresentationKind,
        body: &str,
    ) -> Event {
        Event::RoomActivityUpdate(RoomActivityUpdate {
            room_id: room_id.to_owned(),
            target: RoomActivityUpdateTarget::Event {
                event_id: target_event_id.to_owned(),
            },
            kind: RoomActivityUpdateKind::MessageReplaced {
                replacement_event_id: replacement_event_id.to_owned(),
                sender: "@alice:example".to_owned(),
                timestamp_ms: 1_700_000_000_500,
                presentation,
                body: body.to_owned(),
            },
        })
    }

    fn incoming_redacted(room_id: &str, target_event_id: &str, redaction_event_id: &str) -> Event {
        Event::RoomActivityUpdate(RoomActivityUpdate {
            room_id: room_id.to_owned(),
            target: RoomActivityUpdateTarget::Event {
                event_id: target_event_id.to_owned(),
            },
            kind: RoomActivityUpdateKind::ActivityRedacted {
                redaction_event_id: redaction_event_id.to_owned(),
            },
        })
    }

    fn incoming_reaction_added(
        room_id: &str,
        target_event_id: &str,
        reaction_event_id: &str,
        sender: &str,
        key: &str,
        timestamp_ms: u64,
    ) -> Event {
        Event::RoomActivityUpdate(RoomActivityUpdate {
            room_id: room_id.to_owned(),
            target: RoomActivityUpdateTarget::Event {
                event_id: target_event_id.to_owned(),
            },
            kind: RoomActivityUpdateKind::ReactionAdded {
                reaction_event_id: reaction_event_id.to_owned(),
                sender: sender.to_owned(),
                timestamp_ms,
                key: key.to_owned(),
            },
        })
    }

    fn incoming_reaction_removed(
        room_id: &str,
        target_event_id: &str,
        reaction_event_id: &str,
        redaction_event_id: &str,
        key: &str,
    ) -> Event {
        Event::RoomActivityUpdate(RoomActivityUpdate {
            room_id: room_id.to_owned(),
            target: RoomActivityUpdateTarget::Event {
                event_id: target_event_id.to_owned(),
            },
            kind: RoomActivityUpdateKind::ReactionRemoved {
                reaction_event_id: reaction_event_id.to_owned(),
                redaction_event_id: redaction_event_id.to_owned(),
                key: key.to_owned(),
            },
        })
    }

    fn room_read_state(
        room_id: &str,
        latest_active_event_id: Option<&str>,
        fully_read_event_id: Option<&str>,
        marked_unread: bool,
        num_unread: u64,
        num_notifications: u64,
        num_mentions: u64,
    ) -> Event {
        Event::RoomReadState(RoomReadState {
            room_id: room_id.to_owned(),
            latest_active_event_id: latest_active_event_id.map(str::to_owned),
            fully_read_event_id: fully_read_event_id.map(str::to_owned),
            marked_unread,
            num_unread,
            num_notifications,
            num_mentions,
        })
    }

    fn history_backfill_event_from_event(event: Event) -> RoomHistoryBackfillEvent {
        match event {
            Event::RoomActivity(activity) => RoomHistoryBackfillEvent::Activity(activity),
            Event::RoomActivityUpdate(update) => RoomHistoryBackfillEvent::ActivityUpdate(update),
            other => panic!("expected room history backfill event payload, got {other:?}"),
        }
    }

    fn room_history_backfill(
        room_id: &str,
        start: &str,
        end: Option<&str>,
        events: Vec<RoomHistoryBackfillEvent>,
    ) -> Event {
        Event::RoomHistoryBackfill(RoomHistoryBackfill {
            room_id: room_id.to_owned(),
            start: start.to_owned(),
            end: end.map(str::to_owned),
            events,
        })
    }

    fn failed_message_activity_with_presentation(
        room_id: &str,
        transaction_id: &str,
        presentation: RoomMessagePresentationKind,
        body: &str,
        message: &str,
    ) -> RoomActivity {
        RoomActivity {
            room_id: room_id.to_owned(),
            activity_id: format!("txn:{transaction_id}"),
            timestamp_ms: 1_700_000_000_000,
            relation: None,
            kind: RoomActivityKind::Message(RoomMessageActivity {
                event_id: None,
                transaction_id: Some(transaction_id.to_owned()),
                sender: "@alice:example".to_owned(),
                presentation,
                body: body.to_owned(),
                send_state: RoomMessageSendState::Failed {
                    message: message.to_owned(),
                    recoverable: true,
                },
            }),
        }
    }

    fn failed_message_with_presentation(
        room_id: &str,
        transaction_id: &str,
        presentation: RoomMessagePresentationKind,
        body: &str,
        message: &str,
    ) -> Event {
        Event::RoomActivity(Box::new(failed_message_activity_with_presentation(
            room_id,
            transaction_id,
            presentation,
            body,
            message,
        )))
    }

    fn failed_message(room_id: &str, transaction_id: &str, body: &str, message: &str) -> Event {
        failed_message_with_presentation(
            room_id,
            transaction_id,
            RoomMessagePresentationKind::Text,
            body,
            message,
        )
    }

    fn topic_state(room_id: &str, topic: &str) -> Event {
        Event::RoomActivity(Box::new(RoomActivity {
            room_id: room_id.to_owned(),
            activity_id: format!("state:$topic:{room_id}:{topic}"),
            timestamp_ms: 1_700_000_060_000,
            relation: None,
            kind: RoomActivityKind::StateNotice(RoomStateNoticeActivity {
                event_id: format!("$topic:{room_id}:{topic}"),
                sender: "@alice:example".to_owned(),
                state_key: String::new(),
                content: RoomStateNoticeContent::Topic {
                    topic: topic.to_owned(),
                },
            }),
        }))
    }

    fn member_state(
        room_id: &str,
        state_key: &str,
        membership: &str,
        display_name: Option<&str>,
    ) -> Event {
        Event::RoomActivity(Box::new(RoomActivity {
            room_id: room_id.to_owned(),
            timestamp_ms: 1_700_000_120_000,
            activity_id: format!("state:$member:{room_id}:{state_key}:{membership}"),
            relation: None,
            kind: RoomActivityKind::StateNotice(RoomStateNoticeActivity {
                event_id: format!("$member:{room_id}:{state_key}:{membership}"),
                sender: "@alice:example".to_owned(),
                state_key: state_key.to_owned(),
                content: RoomStateNoticeContent::Member {
                    membership: membership.to_owned(),
                    displayname: display_name.map(str::to_owned),
                },
            }),
        }))
    }

    fn unsupported_message_activity(
        room_id: &str,
        event_id: &str,
        sender: &str,
        timestamp_ms: u64,
        msgtype: &str,
        body: &str,
    ) -> RoomActivity {
        RoomActivity {
            room_id: room_id.to_owned(),
            activity_id: format!("event:{event_id}"),
            timestamp_ms,
            relation: None,
            kind: RoomActivityKind::UnsupportedMessage(
                mmatrix_core::UnsupportedRoomMessageActivity {
                    event_id: event_id.to_owned(),
                    sender: sender.to_owned(),
                    msgtype: msgtype.to_owned(),
                    body: body.to_owned(),
                },
            ),
        }
    }

    fn unsupported_message(
        room_id: &str,
        event_id: &str,
        sender: &str,
        timestamp_ms: u64,
        msgtype: &str,
        body: &str,
    ) -> Event {
        Event::RoomActivity(Box::new(unsupported_message_activity(
            room_id,
            event_id,
            sender,
            timestamp_ms,
            msgtype,
            body,
        )))
    }

    fn image_activity(
        room_id: &str,
        event_id: &str,
        sender: &str,
        timestamp_ms: u64,
        body: &str,
        filename: Option<&str>,
        info: Option<RoomImageInfo>,
    ) -> RoomActivity {
        RoomActivity {
            room_id: room_id.to_owned(),
            activity_id: format!("event:{event_id}"),
            timestamp_ms,
            relation: None,
            kind: RoomActivityKind::Image(Box::new(mmatrix_core::RoomImageActivity {
                event_id: event_id.to_owned(),
                sender: sender.to_owned(),
                body: body.to_owned(),
                filename: filename.map(str::to_owned),
                source: mmatrix_core::RoomMediaSource::Plain {
                    url: "mxc://example.org/diagram".to_owned(),
                },
                info,
            })),
        }
    }

    fn file_activity(
        room_id: &str,
        event_id: &str,
        sender: &str,
        timestamp_ms: u64,
        body: &str,
        filename: Option<&str>,
        info: Option<RoomFileInfo>,
    ) -> RoomActivity {
        RoomActivity {
            room_id: room_id.to_owned(),
            activity_id: format!("event:{event_id}"),
            timestamp_ms,
            relation: None,
            kind: RoomActivityKind::File(Box::new(mmatrix_core::RoomFileActivity {
                event_id: event_id.to_owned(),
                sender: sender.to_owned(),
                body: body.to_owned(),
                filename: filename.map(str::to_owned),
                source: mmatrix_core::RoomMediaSource::Plain {
                    url: "mxc://example.org/report".to_owned(),
                },
                info,
            })),
        }
    }

    fn audio_activity(
        room_id: &str,
        event_id: &str,
        sender: &str,
        timestamp_ms: u64,
        body: &str,
        filename: Option<&str>,
        info: Option<RoomAudioInfo>,
    ) -> RoomActivity {
        RoomActivity {
            room_id: room_id.to_owned(),
            activity_id: format!("event:{event_id}"),
            timestamp_ms,
            relation: None,
            kind: RoomActivityKind::Audio(Box::new(mmatrix_core::RoomAudioActivity {
                event_id: event_id.to_owned(),
                sender: sender.to_owned(),
                body: body.to_owned(),
                filename: filename.map(str::to_owned),
                source: mmatrix_core::RoomMediaSource::Plain {
                    url: "mxc://example.org/audio".to_owned(),
                },
                info,
            })),
        }
    }

    fn image_message(
        room_id: &str,
        event_id: &str,
        sender: &str,
        timestamp_ms: u64,
        body: &str,
        filename: Option<&str>,
        info: Option<RoomImageInfo>,
    ) -> Event {
        Event::RoomActivity(Box::new(image_activity(
            room_id,
            event_id,
            sender,
            timestamp_ms,
            body,
            filename,
            info,
        )))
    }

    fn encrypted_image_activity(
        room_id: &str,
        event_id: &str,
        sender: &str,
        timestamp_ms: u64,
        body: &str,
        thumbnail_source: Option<RoomMediaSource>,
    ) -> RoomActivity {
        RoomActivity {
            room_id: room_id.to_owned(),
            activity_id: format!("event:{event_id}"),
            timestamp_ms,
            relation: None,
            kind: RoomActivityKind::Image(Box::new(mmatrix_core::RoomImageActivity {
                event_id: event_id.to_owned(),
                sender: sender.to_owned(),
                body: body.to_owned(),
                filename: None,
                source: encrypted_media_source("mxc://example.org/encrypted"),
                info: Some(RoomImageInfo {
                    mimetype: Some("image/png".to_owned()),
                    size: Some(12_345),
                    width: Some(640),
                    height: Some(480),
                    is_animated: Some(false),
                    thumbnail_source,
                }),
            })),
        }
    }

    fn encrypted_media_source(url: &str) -> RoomMediaSource {
        RoomMediaSource::Encrypted {
            url: url.to_owned(),
            key: mmatrix_core::RoomEncryptedFileKey {
                kty: "oct".to_owned(),
                key_ops: vec!["encrypt".to_owned(), "decrypt".to_owned()],
                alg: "A256CTR".to_owned(),
                k: "base64url-key".to_owned(),
                ext: true,
            },
            iv: "base64-iv".to_owned(),
            hashes: mmatrix_core::RoomEncryptedFileHashes {
                sha256: "base64-sha256".to_owned(),
            },
            v: "v2".to_owned(),
        }
    }

    fn room_image_media_event(
        room_id: &str,
        activity_id: &str,
        event_id: &str,
        kind: RoomImageMediaKind,
    ) -> Event {
        Event::RoomImageMedia(RoomImageMedia {
            room_id: room_id.to_owned(),
            activity_id: activity_id.to_owned(),
            event_id: event_id.to_owned(),
            kind,
        })
    }

    fn tiny_png_bytes() -> Vec<u8> {
        vec![
            137, 80, 78, 71, 13, 10, 26, 10, 0, 0, 0, 13, 73, 72, 68, 82, 0, 0, 0, 1, 0, 0, 0, 1,
            8, 4, 0, 0, 0, 181, 28, 12, 2, 0, 0, 0, 11, 73, 68, 65, 84, 120, 218, 99, 252, 255, 31,
            0, 3, 3, 2, 0, 239, 191, 107, 212, 0, 0, 0, 0, 73, 69, 78, 68, 174, 66, 96, 130,
        ]
    }

    fn typing_event(room_id: &str, user_ids: &[&str]) -> Event {
        Event::RoomTyping(RoomTyping {
            room_id: room_id.to_owned(),
            user_ids: user_ids
                .iter()
                .map(|user_id| (*user_id).to_owned())
                .collect(),
        })
    }

    fn stored_session() -> StoredSession {
        StoredSession {
            user_id: "@alice:example".to_owned(),
            device_id: "DEVICE".to_owned(),
            access_token: "secret-token".to_owned(),
            refresh_token: Some("refresh-token".to_owned()),
        }
    }

    fn saved_session_payload() -> SavedSession {
        SavedSession {
            version: SAVED_SESSION_VERSION,
            homeserver_url: "https://matrix.example".to_owned(),
            session: stored_session(),
        }
    }

    fn protected_saved_session_bytes(saved: &SavedSession) -> Vec<u8> {
        let serialized = serde_json::to_vec(saved).expect("saved session must serialize");
        protected_saved_session_payload_bytes(&serialized)
    }

    fn protected_saved_session_payload_bytes(payload: &[u8]) -> Vec<u8> {
        let protected =
            crate::dpapi::protect(payload, "mmatrix saved session test", SESSION_DPAPI_ENTROPY)
                .expect("saved session test payload must protect");
        let mut bytes = Vec::with_capacity(PROTECTED_SESSION_MAGIC.len() + protected.len());
        bytes.extend_from_slice(PROTECTED_SESSION_MAGIC);
        bytes.extend_from_slice(&protected);
        bytes
    }

    fn session_file_path(root: &Path) -> PathBuf {
        root.join(PROFILE_SUBDIR).join(SESSION_FILE_NAME)
    }

    fn write_session_bytes(root: &Path, bytes: &[u8]) {
        let path = session_file_path(root);
        let parent = path.parent().expect("session file must have parent");
        fs::create_dir_all(parent).expect("session file parent must exist");
        fs::write(path, bytes).expect("session bytes must write");
    }

    fn store_passphrase_file_path(root: &Path) -> PathBuf {
        root.join(PROFILE_SUBDIR).join(STORE_PASSPHRASE_FILE_NAME)
    }

    fn assert_bytes_do_not_contain(bytes: &[u8], needle: &str) {
        assert!(
            !bytes
                .windows(needle.len())
                .any(|window| window == needle.as_bytes()),
            "bytes unexpectedly contained {needle:?}"
        );
    }

    fn with_local_app_data<T>(test: impl FnOnce(PathBuf) -> T) -> T {
        let _guard = LOCAL_APP_DATA_LOCK
            .lock()
            .expect("LOCALAPPDATA test mutex must not be poisoned");
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock must be after unix epoch")
            .as_nanos();
        let root = env::temp_dir().join(format!("mmatrix-ui-local-app-data-{unique}"));
        fs::create_dir_all(&root).expect("test LOCALAPPDATA root must be created");
        let previous = env::var_os("LOCALAPPDATA");
        unsafe {
            env::set_var("LOCALAPPDATA", &root);
        }
        let result = test(root.clone());
        match previous {
            Some(previous) => unsafe {
                env::set_var("LOCALAPPDATA", previous);
            },
            None => unsafe {
                env::remove_var("LOCALAPPDATA");
            },
        }
        let _ = fs::remove_dir_all(root);
        result
    }

    fn test_state() -> WindowState {
        let (gdiplus_session, gdiplus_startup_error) = initialize_gdiplus_state();
        WindowState {
            controls: Controls::default(),
            event_queue: Arc::new(Mutex::new(VecDeque::new())),
            command_sender: None,
            inert_command_receiver: None,
            automation: None,
            render_state_mode: false,
            exit_code: Arc::new(AtomicI32::new(0)),
            event_generation: 0,
            last_idle_probe_generation: None,
            synthetic_load_in_progress: false,
            pending_login_homeserver_url: None,
            current_session_homeserver_url: None,
            tree_items: BTreeMap::new(),
            primary_tree_items: BTreeMap::new(),
            views: BTreeMap::new(),
            hwnd_views: BTreeMap::new(),
            active_view: ViewId::Status,
            find_states: BTreeMap::new(),
            switchbar_views: Vec::new(),
            room_activity: BTreeSet::new(),
            room_activity_read_targets: BTreeMap::new(),
            compose_history: ComposeHistory::default(),
            edit_window_proc: None,
            model: initial_model(),
            decoded_images: BTreeMap::new(),
            gdiplus_session,
            gdiplus_startup_error,
            #[cfg(test)]
            recorded_commands: Vec::new(),
        }
    }

    fn measure_ascii_width(text: &str) -> i32 {
        text.chars().count() as i32
    }

    fn tracked_view(hwnd_value: isize) -> MdiViewState {
        MdiViewState {
            child: hwnd_value as HWND,
            content: (hwnd_value + 1) as HWND,
        }
    }

    fn room_view_id(room_id: &str) -> ViewId {
        ViewId::Room(room_id.to_owned())
    }

    fn thread_view_id(room_id: &str, thread_root_event_id: &str) -> ViewId {
        ViewId::Thread {
            room_id: room_id.to_owned(),
            thread_root_event_id: thread_root_event_id.to_owned(),
        }
    }

    fn start_test_core() -> CoreHandle {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock must be after unix epoch")
            .as_nanos();
        let profile_dir = std::env::temp_dir().join(format!("mmatrix-ui-win32-tests-{unique}"));
        fs::create_dir_all(&profile_dir).expect("test profile dir must be created");

        start(CoreConfig {
            profile_dir,
            store_passphrase: "mmatrix-ui-win32-tests".to_owned(),
        })
        .expect("test core must start")
    }

    fn test_state_with_command_sender() -> (WindowState, CoreHandle) {
        let mut state = test_state();
        let handle = start_test_core();
        state.command_sender = Some(handle.command_sender().clone());
        (state, handle)
    }

    fn stop_test_core(handle: CoreHandle) {
        handle
            .command_sender()
            .send(Command::Shutdown)
            .expect("shutdown command must enqueue");
        handle.join().expect("test core must stop cleanly");
    }

    #[test]
    fn synthetic_event_validation_rejects_invalid_message_sent_sequence() {
        assert_eq!(
            validate_synthetic_room_activity_events(
                &BTreeMap::new(),
                &[outgoing_sent("!ops:example", "txn-1", "$event:example.org")]
            ),
            Err("outgoing sent requires an existing queued timeline row".to_owned())
        );
    }

    #[test]
    fn synthetic_event_validation_accepts_pending_then_sent_sequence() {
        assert_eq!(
            validate_synthetic_room_activity_events(
                &BTreeMap::new(),
                &[
                    outgoing_queued("!ops:example", "txn-1", "hello"),
                    outgoing_sent("!ops:example", "txn-1", "$event:example.org"),
                ]
            ),
            Ok(())
        );
    }

    #[test]
    fn synthetic_event_validation_rejects_message_edit_with_transaction_target() {
        assert_eq!(
            validate_synthetic_room_activity_events(
                &BTreeMap::new(),
                &[Event::RoomActivityUpdate(RoomActivityUpdate {
                    room_id: "!ops:example".to_owned(),
                    target: RoomActivityUpdateTarget::Transaction {
                        transaction_id: "txn-1".to_owned(),
                    },
                    kind: RoomActivityUpdateKind::MessageReplaced {
                        replacement_event_id: "$edit:example.org".to_owned(),
                        sender: "@alice:example".to_owned(),
                        timestamp_ms: 1_700_000_000_500,
                        presentation: RoomMessagePresentationKind::Text,
                        body: "edited".to_owned(),
                    },
                })]
            ),
            Err("message edit requires an event target".to_owned())
        );
    }

    #[test]
    fn synthetic_event_validation_rejects_message_edit_without_visible_target() {
        assert_eq!(
            validate_synthetic_room_activity_events(
                &BTreeMap::new(),
                &[incoming_replaced(
                    "!ops:example",
                    "$missing:example.org",
                    "$edit:example.org",
                    RoomMessagePresentationKind::Text,
                    "edited"
                )]
            ),
            Err("message edit requires an existing visible target event".to_owned())
        );
    }

    #[test]
    fn redaction_synthetic_event_validation_rejects_missing_event_target() {
        assert_eq!(
            validate_synthetic_room_activity_events(
                &BTreeMap::new(),
                &[incoming_redacted(
                    "!ops:example",
                    "$missing:example.org",
                    "$redaction:example.org",
                )]
            ),
            Err("activity redaction requires an existing visible target event".to_owned())
        );
    }

    #[test]
    fn redaction_synthetic_event_validation_rejects_transaction_target() {
        assert_eq!(
            validate_synthetic_room_activity_events(
                &BTreeMap::new(),
                &[Event::RoomActivityUpdate(RoomActivityUpdate {
                    room_id: "!ops:example".to_owned(),
                    target: RoomActivityUpdateTarget::Transaction {
                        transaction_id: "txn-1".to_owned(),
                    },
                    kind: RoomActivityUpdateKind::ActivityRedacted {
                        redaction_event_id: "$redaction:example.org".to_owned(),
                    },
                })]
            ),
            Err("activity redacted update requires an event target".to_owned())
        );
    }

    #[test]
    fn reaction_add_synthetic_event_validation_rejects_transaction_target() {
        assert_eq!(
            validate_synthetic_room_activity_events(
                &BTreeMap::new(),
                &[Event::RoomActivityUpdate(RoomActivityUpdate {
                    room_id: "!ops:example".to_owned(),
                    target: RoomActivityUpdateTarget::Transaction {
                        transaction_id: "txn-1".to_owned(),
                    },
                    kind: RoomActivityUpdateKind::ReactionAdded {
                        reaction_event_id: "$reaction:example.org".to_owned(),
                        sender: "@alice:example".to_owned(),
                        timestamp_ms: 1_700_000_000_500,
                        key: "+1".to_owned(),
                    },
                })]
            ),
            Err("reaction add requires an event target".to_owned())
        );
    }

    #[test]
    fn reaction_removed_synthetic_event_validation_rejects_transaction_target() {
        assert_eq!(
            validate_synthetic_room_activity_events(
                &BTreeMap::new(),
                &[Event::RoomActivityUpdate(RoomActivityUpdate {
                    room_id: "!ops:example".to_owned(),
                    target: RoomActivityUpdateTarget::Transaction {
                        transaction_id: "txn-1".to_owned(),
                    },
                    kind: RoomActivityUpdateKind::ReactionRemoved {
                        reaction_event_id: "$reaction:example.org".to_owned(),
                        redaction_event_id: "$redaction:example.org".to_owned(),
                        key: "+1".to_owned(),
                    },
                })]
            ),
            Err("reaction removal requires an event target".to_owned())
        );
    }

    #[test]
    fn room_activity_events_accumulate_per_room_in_order() {
        let mut state = test_state();

        apply_core_event(
            &mut state,
            Event::SyncedRooms(vec![
                room("!one:example", "One", false),
                room("!two:example", "Two", true),
            ]),
        )
        .unwrap();
        apply_core_event(&mut state, timeline("!one:example", "first")).unwrap();
        apply_core_event(&mut state, timeline("!one:example", "second")).unwrap();
        apply_core_event(&mut state, timeline("!two:example", "third")).unwrap();

        assert_eq!(
            state
                .model
                .room_views
                .get("!one:example")
                .map(|view| &view.activities),
            Some(&vec![
                sent_message_activity(
                    "!one:example",
                    "$!one:example:first",
                    None,
                    "first",
                    1_700_000_000_000,
                ),
                sent_message_activity(
                    "!one:example",
                    "$!one:example:second",
                    None,
                    "second",
                    1_700_000_000_000,
                ),
            ])
        );
        assert_eq!(
            state
                .model
                .room_views
                .get("!two:example")
                .map(|view| &view.activities),
            Some(&vec![sent_message_activity(
                "!two:example",
                "$!two:example:third",
                None,
                "third",
                1_700_000_000_000,
            )])
        );
        assert_eq!(
            state.model.last_activity,
            Some(LastMessageActivity {
                room_id: "!two:example".to_owned(),
                event_id: Some("$!two:example:third".to_owned()),
                transaction_id: None,
                sender: "@alice:example".to_owned(),
                timestamp_ms: 1_700_000_000_000,
                presentation: RoomMessagePresentationKind::Text,
                body: "third".to_owned(),
                send_state: RoomMessageSendState::Sent,
            })
        );
    }

    #[test]
    fn pending_room_activity_creates_one_pending_row_keyed_by_transaction_id() {
        let mut state = test_state();
        state.model.rooms = vec![room("!ops:example", "Ops", false)];

        apply_core_event(
            &mut state,
            outgoing_queued_with_presentation(
                "!ops:example",
                "txn-1",
                RoomMessagePresentationKind::Emote,
                "waves",
            ),
        )
        .unwrap();

        assert_eq!(
            state
                .model
                .room_views
                .get("!ops:example")
                .map(|view| &view.activities),
            Some(&vec![pending_message_activity_with_presentation(
                "!ops:example",
                "txn-1",
                RoomMessagePresentationKind::Emote,
                "waves"
            )])
        );
        assert_eq!(
            state.model.last_activity,
            Some(LastMessageActivity {
                room_id: "!ops:example".to_owned(),
                event_id: None,
                transaction_id: Some("txn-1".to_owned()),
                sender: "@alice:example".to_owned(),
                timestamp_ms: 1_700_000_000_000,
                presentation: RoomMessagePresentationKind::Emote,
                body: "waves".to_owned(),
                send_state: RoomMessageSendState::Pending,
            })
        );
        assert_eq!(
            render_room_lines(&state.model, "!ops:example"),
            vec![
                "Room: Ops (!ops:example)".to_owned(),
                "* [22:13] @alice:example waves (sending)".to_owned(),
            ]
        );
    }

    #[test]
    fn room_activity_update_sent_updates_existing_row_and_clears_pending_marker() {
        let mut state = test_state();
        state.model.rooms = vec![room("!ops:example", "Ops", false)];

        apply_core_event(
            &mut state,
            outgoing_queued("!ops:example", "txn-1", "hello"),
        )
        .unwrap();
        apply_core_event(
            &mut state,
            outgoing_sent("!ops:example", "txn-1", "$event:example.org"),
        )
        .unwrap();

        assert_eq!(
            state
                .model
                .room_views
                .get("!ops:example")
                .map(|view| &view.activities),
            Some(&vec![sent_message_activity(
                "!ops:example",
                "$event:example.org",
                Some("txn-1"),
                "hello",
                1_700_000_000_000,
            )])
        );
        assert_eq!(
            state.model.last_activity,
            Some(LastMessageActivity {
                room_id: "!ops:example".to_owned(),
                event_id: Some("$event:example.org".to_owned()),
                transaction_id: Some("txn-1".to_owned()),
                sender: "@alice:example".to_owned(),
                timestamp_ms: 1_700_000_000_000,
                presentation: RoomMessagePresentationKind::Text,
                body: "hello".to_owned(),
                send_state: RoomMessageSendState::Sent,
            })
        );
        assert_eq!(
            render_room_lines(&state.model, "!ops:example"),
            vec![
                "Room: Ops (!ops:example)".to_owned(),
                "[22:13] @alice:example: hello".to_owned(),
            ]
        );
    }

    #[test]
    fn room_activity_update_replaced_updates_existing_row_in_place() {
        let mut state = test_state();
        state.model.rooms = vec![room("!ops:example", "Ops", false)];

        apply_core_event(
            &mut state,
            timeline_with_relation_and_presentation(
                "!ops:example",
                "$event:example.org",
                None,
                None,
                RoomMessagePresentationKind::Text,
                "original body",
                1_700_000_000_000,
            ),
        )
        .unwrap();
        apply_core_event(
            &mut state,
            incoming_replaced(
                "!ops:example",
                "$event:example.org",
                "$edit:example.org",
                RoomMessagePresentationKind::Emote,
                "edited body",
            ),
        )
        .unwrap();

        assert_eq!(
            state
                .model
                .room_views
                .get("!ops:example")
                .map(|view| &view.activities),
            Some(&vec![sent_message_activity_with_presentation(
                "!ops:example",
                "$event:example.org",
                None,
                RoomMessagePresentationKind::Emote,
                "edited body",
                1_700_000_000_000,
            )])
        );
        assert_eq!(
            render_room_lines(&state.model, "!ops:example"),
            vec![
                "Room: Ops (!ops:example)".to_owned(),
                "* [22:13] @alice:example edited body".to_owned(),
            ]
        );
    }

    #[test]
    fn room_activity_update_replaced_refreshes_last_activity_only_for_target() {
        let mut state = test_state();
        state.model.rooms = vec![room("!ops:example", "Ops", false)];

        apply_core_event(
            &mut state,
            timeline_with_relation_and_presentation(
                "!ops:example",
                "$older:example.org",
                None,
                None,
                RoomMessagePresentationKind::Text,
                "older body",
                1_700_000_000_000,
            ),
        )
        .unwrap();
        apply_core_event(
            &mut state,
            timeline_with_relation_and_presentation(
                "!ops:example",
                "$latest:example.org",
                None,
                None,
                RoomMessagePresentationKind::Text,
                "latest body",
                1_700_000_000_100,
            ),
        )
        .unwrap();
        apply_core_event(
            &mut state,
            incoming_replaced(
                "!ops:example",
                "$older:example.org",
                "$older-edit:example.org",
                RoomMessagePresentationKind::Text,
                "older edited",
            ),
        )
        .unwrap();

        assert_eq!(
            state.model.last_activity,
            Some(LastMessageActivity {
                room_id: "!ops:example".to_owned(),
                event_id: Some("$latest:example.org".to_owned()),
                transaction_id: None,
                sender: "@alice:example".to_owned(),
                timestamp_ms: 1_700_000_000_100,
                presentation: RoomMessagePresentationKind::Text,
                body: "latest body".to_owned(),
                send_state: RoomMessageSendState::Sent,
            })
        );

        apply_core_event(
            &mut state,
            incoming_replaced(
                "!ops:example",
                "$latest:example.org",
                "$latest-edit:example.org",
                RoomMessagePresentationKind::Notice,
                "latest edited",
            ),
        )
        .unwrap();

        assert_eq!(
            state.model.last_activity,
            Some(LastMessageActivity {
                room_id: "!ops:example".to_owned(),
                event_id: Some("$latest:example.org".to_owned()),
                transaction_id: None,
                sender: "@alice:example".to_owned(),
                timestamp_ms: 1_700_000_000_100,
                presentation: RoomMessagePresentationKind::Notice,
                body: "latest edited".to_owned(),
                send_state: RoomMessageSendState::Sent,
            })
        );
    }

    #[test]
    fn redaction_room_activity_update_replaces_text_message_with_tombstone() {
        let mut state = test_state();
        state.model.rooms = vec![room("!ops:example", "Ops", false)];

        apply_core_event(
            &mut state,
            timeline_with_relation_and_presentation(
                "!ops:example",
                "$event:example.org",
                None,
                None,
                RoomMessagePresentationKind::Text,
                "secret body",
                1_700_000_000_000,
            ),
        )
        .unwrap();
        apply_core_event(
            &mut state,
            incoming_redacted(
                "!ops:example",
                "$event:example.org",
                "$redaction:example.org",
            ),
        )
        .unwrap();

        let room_view = state
            .model
            .room_views
            .get("!ops:example")
            .expect("room view must exist");
        assert!(matches!(
            room_view.activities.as_slice(),
            [RoomActivity {
                activity_id,
                kind: RoomActivityKind::Redacted(RoomRedactedActivity { event_id }),
                ..
            }] if activity_id == "event:$event:example.org" && event_id == "$event:example.org"
        ));
        assert_eq!(state.model.last_activity, None);

        let rendered = render_room_lines(&state.model, "!ops:example").join("\n");
        assert!(rendered.contains("message redacted"));
        assert!(!rendered.contains("secret body"));
    }

    #[test]
    fn redaction_room_activity_update_overrides_edited_message_and_blocks_late_replacement() {
        let mut state = test_state();
        state.model.rooms = vec![room("!ops:example", "Ops", false)];

        apply_core_event(
            &mut state,
            timeline_with_relation_and_presentation(
                "!ops:example",
                "$event:example.org",
                None,
                None,
                RoomMessagePresentationKind::Text,
                "original body",
                1_700_000_000_000,
            ),
        )
        .unwrap();
        apply_core_event(
            &mut state,
            incoming_replaced(
                "!ops:example",
                "$event:example.org",
                "$edit:example.org",
                RoomMessagePresentationKind::Emote,
                "edited body",
            ),
        )
        .unwrap();
        apply_core_event(
            &mut state,
            incoming_redacted(
                "!ops:example",
                "$event:example.org",
                "$redaction:example.org",
            ),
        )
        .unwrap();
        apply_core_event(
            &mut state,
            incoming_replaced(
                "!ops:example",
                "$event:example.org",
                "$late-edit:example.org",
                RoomMessagePresentationKind::Text,
                "late body",
            ),
        )
        .unwrap();

        let rendered = render_room_lines(&state.model, "!ops:example").join("\n");
        assert!(rendered.contains("message redacted"));
        assert!(!rendered.contains("original body"));
        assert!(!rendered.contains("edited body"));
        assert!(!rendered.contains("late body"));
    }

    #[test]
    fn reaction_projection_renders_single_summary_row_after_annotated_activity() {
        let mut state = test_state();
        state.model.rooms = vec![room("!ops:example", "Ops", false)];

        apply_core_event(
            &mut state,
            timeline_with_transaction_id(
                "!ops:example",
                "$event:example.org",
                None,
                "hello",
                1_700_000_000_000,
            ),
        )
        .unwrap();
        apply_core_event(
            &mut state,
            incoming_reaction_added(
                "!ops:example",
                "$event:example.org",
                "$reaction-1:example.org",
                "@bob:example.org",
                "+1",
                1_700_000_000_500,
            ),
        )
        .unwrap();

        let rows = build_room_timeline_rows(&state.model, "!ops:example");
        assert_eq!(
            rows.iter().map(|row| row.kind.clone()).collect::<Vec<_>>(),
            vec![
                TimelineRowKind::Header,
                TimelineRowKind::Message {
                    presentation: RoomMessagePresentationKind::Text,
                    send_state: TimelineMessageSendState::Sent,
                },
                TimelineRowKind::ReactionSummary,
            ]
        );
        assert_eq!(
            project_timeline_rows(&rows),
            vec![
                "Room: Ops (!ops:example)".to_owned(),
                "[22:13] @alice:example: hello".to_owned(),
                "    reactions: +1 x1".to_owned(),
            ]
        );
    }

    #[test]
    fn reaction_projection_aggregates_multiple_keys_deterministically_into_one_summary_row() {
        let mut state = test_state();
        state.model.rooms = vec![room("!ops:example", "Ops", false)];

        apply_core_event(&mut state, timeline("!ops:example", "hello")).unwrap();
        apply_core_event(
            &mut state,
            incoming_reaction_added(
                "!ops:example",
                "$!ops:example:hello",
                "$reaction-heart-1",
                "@alice:example.org",
                "heart",
                1_700_000_000_100,
            ),
        )
        .unwrap();
        apply_core_event(
            &mut state,
            incoming_reaction_added(
                "!ops:example",
                "$!ops:example:hello",
                "$reaction-plus",
                "@bob:example.org",
                "+1",
                1_700_000_000_200,
            ),
        )
        .unwrap();
        apply_core_event(
            &mut state,
            incoming_reaction_added(
                "!ops:example",
                "$!ops:example:hello",
                "$reaction-eyes",
                "@carol:example.org",
                "eyes",
                1_700_000_000_300,
            ),
        )
        .unwrap();
        apply_core_event(
            &mut state,
            incoming_reaction_added(
                "!ops:example",
                "$!ops:example:hello",
                "$reaction-heart-2",
                "@dave:example.org",
                "heart",
                1_700_000_000_400,
            ),
        )
        .unwrap();

        let rows = build_room_timeline_rows(&state.model, "!ops:example");
        assert_eq!(
            rows.iter()
                .filter(|row| row.kind == TimelineRowKind::ReactionSummary)
                .count(),
            1
        );
        assert_eq!(
            project_timeline_rows(&rows),
            vec![
                "Room: Ops (!ops:example)".to_owned(),
                "[22:13] @alice:example: hello".to_owned(),
                "    reactions: +1 x1  eyes x1  heart x2".to_owned(),
            ]
        );
    }

    #[test]
    fn same_reaction_event_id_does_not_double_count() {
        let mut state = test_state();
        state.model.rooms = vec![room("!ops:example", "Ops", false)];

        apply_core_event(&mut state, timeline("!ops:example", "hello")).unwrap();
        apply_core_event(
            &mut state,
            incoming_reaction_added(
                "!ops:example",
                "$!ops:example:hello",
                "$reaction-1",
                "@bob:example.org",
                "+1",
                1_700_000_000_100,
            ),
        )
        .unwrap();
        apply_core_event(
            &mut state,
            incoming_reaction_added(
                "!ops:example",
                "$!ops:example:hello",
                "$reaction-1",
                "@bob:example.org",
                "+1",
                1_700_000_000_100,
            ),
        )
        .unwrap();

        let room_view = state
            .model
            .room_views
            .get("!ops:example")
            .expect("room view must exist");
        assert_eq!(
            room_view.reaction_summaries_for_event("$!ops:example:hello"),
            vec![DumpRoomReactionSummary {
                key: "+1".to_owned(),
                count: 1,
                senders: vec!["@bob:example.org".to_owned()],
                reaction_event_ids: vec!["$reaction-1".to_owned()],
            }]
        );
    }

    #[test]
    fn different_reaction_event_ids_with_same_sender_and_key_count_separately() {
        let mut state = test_state();
        state.model.rooms = vec![room("!ops:example", "Ops", false)];

        apply_core_event(&mut state, timeline("!ops:example", "hello")).unwrap();
        apply_core_event(
            &mut state,
            incoming_reaction_added(
                "!ops:example",
                "$!ops:example:hello",
                "$reaction-1",
                "@bob:example.org",
                "+1",
                1_700_000_000_100,
            ),
        )
        .unwrap();
        apply_core_event(
            &mut state,
            incoming_reaction_added(
                "!ops:example",
                "$!ops:example:hello",
                "$reaction-2",
                "@bob:example.org",
                "+1",
                1_700_000_000_200,
            ),
        )
        .unwrap();

        let room_view = state
            .model
            .room_views
            .get("!ops:example")
            .expect("room view must exist");
        assert_eq!(
            room_view.reaction_summaries_for_event("$!ops:example:hello"),
            vec![DumpRoomReactionSummary {
                key: "+1".to_owned(),
                count: 2,
                senders: vec!["@bob:example.org".to_owned(), "@bob:example.org".to_owned(),],
                reaction_event_ids: vec!["$reaction-1".to_owned(), "$reaction-2".to_owned()],
            }]
        );
    }

    #[test]
    fn reaction_removed_removes_only_matching_event_id_and_key() {
        let mut state = test_state();
        state.model.rooms = vec![room("!ops:example", "Ops", false)];

        apply_core_event(&mut state, timeline("!ops:example", "hello")).unwrap();
        apply_core_event(
            &mut state,
            incoming_reaction_added(
                "!ops:example",
                "$!ops:example:hello",
                "$reaction-1",
                "@bob:example.org",
                "+1",
                1_700_000_000_100,
            ),
        )
        .unwrap();
        apply_core_event(
            &mut state,
            incoming_reaction_added(
                "!ops:example",
                "$!ops:example:hello",
                "$reaction-2",
                "@carol:example.org",
                "+1",
                1_700_000_000_200,
            ),
        )
        .unwrap();
        apply_core_event(
            &mut state,
            incoming_reaction_added(
                "!ops:example",
                "$!ops:example:hello",
                "$reaction-3",
                "@dave:example.org",
                "heart",
                1_700_000_000_300,
            ),
        )
        .unwrap();

        apply_core_event(
            &mut state,
            incoming_reaction_removed(
                "!ops:example",
                "$!ops:example:hello",
                "$reaction-1",
                "$redaction-wrong-key",
                "heart",
            ),
        )
        .unwrap();
        assert_eq!(
            render_room_lines(&state.model, "!ops:example"),
            vec![
                "Room: Ops (!ops:example)".to_owned(),
                "[22:13] @alice:example: hello".to_owned(),
                "    reactions: +1 x2  heart x1".to_owned(),
            ]
        );

        apply_core_event(
            &mut state,
            incoming_reaction_removed(
                "!ops:example",
                "$!ops:example:hello",
                "$reaction-1",
                "$redaction-right",
                "+1",
            ),
        )
        .unwrap();

        assert_eq!(
            render_room_lines(&state.model, "!ops:example"),
            vec![
                "Room: Ops (!ops:example)".to_owned(),
                "[22:13] @alice:example: hello".to_owned(),
                "    reactions: +1 x1  heart x1".to_owned(),
            ]
        );
    }

    #[test]
    fn redacting_target_removes_reaction_summary_and_leaves_no_reaction_row() {
        let mut state = test_state();
        state.model.rooms = vec![room("!ops:example", "Ops", false)];

        apply_core_event(
            &mut state,
            timeline_with_transaction_id(
                "!ops:example",
                "$event:example.org",
                None,
                "hello",
                1_700_000_000_000,
            ),
        )
        .unwrap();
        apply_core_event(
            &mut state,
            incoming_reaction_added(
                "!ops:example",
                "$event:example.org",
                "$reaction-1:example.org",
                "@bob:example.org",
                "+1",
                1_700_000_000_500,
            ),
        )
        .unwrap();
        apply_core_event(
            &mut state,
            incoming_redacted(
                "!ops:example",
                "$event:example.org",
                "$redaction:example.org",
            ),
        )
        .unwrap();

        let rows = build_room_timeline_rows(&state.model, "!ops:example");
        assert_eq!(
            rows.iter().map(|row| row.kind.clone()).collect::<Vec<_>>(),
            vec![TimelineRowKind::Header, TimelineRowKind::StateNotice]
        );
        assert_eq!(
            project_timeline_rows(&rows),
            vec![
                "Room: Ops (!ops:example)".to_owned(),
                "* [22:13] message redacted".to_owned(),
            ]
        );
    }

    #[test]
    fn dump_room_activity_entry_attaches_reactions_without_creating_pseudo_entries() {
        let mut state = test_state();
        state.model.rooms = vec![room("!ops:example", "Ops", false)];

        apply_core_event(
            &mut state,
            timeline_with_transaction_id(
                "!ops:example",
                "$event-1:example.org",
                None,
                "first",
                1_700_000_000_000,
            ),
        )
        .unwrap();
        apply_core_event(
            &mut state,
            timeline_with_transaction_id(
                "!ops:example",
                "$event-2:example.org",
                None,
                "second",
                1_700_000_000_100,
            ),
        )
        .unwrap();
        apply_core_event(
            &mut state,
            incoming_reaction_added(
                "!ops:example",
                "$event-1:example.org",
                "$reaction-1",
                "@bob:example.org",
                "heart",
                1_700_000_000_200,
            ),
        )
        .unwrap();
        apply_core_event(
            &mut state,
            incoming_reaction_added(
                "!ops:example",
                "$event-1:example.org",
                "$reaction-2",
                "@carol:example.org",
                "heart",
                1_700_000_000_300,
            ),
        )
        .unwrap();

        let room_view = state
            .model
            .room_views
            .get("!ops:example")
            .expect("room view must exist");
        let dumps = room_view
            .activities
            .iter()
            .map(|activity| dump_room_activity_entry_from_room_entry(room_view, activity))
            .collect::<Vec<_>>();

        assert_eq!(dumps.len(), room_view.activities.len());
        assert_eq!(
            dumps
                .iter()
                .map(|dump| dump.activity_id.clone())
                .collect::<Vec<_>>(),
            room_view
                .activities
                .iter()
                .map(|activity| activity.activity_id.clone())
                .collect::<Vec<_>>()
        );
        assert_eq!(
            dumps[0].reactions,
            vec![DumpRoomReactionSummary {
                key: "heart".to_owned(),
                count: 2,
                senders: vec![
                    "@bob:example.org".to_owned(),
                    "@carol:example.org".to_owned(),
                ],
                reaction_event_ids: vec!["$reaction-1".to_owned(), "$reaction-2".to_owned()],
            }]
        );
        assert!(dumps[1].reactions.is_empty());
    }

    #[test]
    fn thread_timeline_projection_includes_reactions_for_root_and_reply() {
        let mut state = test_state();
        state.model.rooms = vec![room("!ops:example", "Ops", false)];

        apply_core_event(
            &mut state,
            timeline_with_transaction_id(
                "!ops:example",
                "$thread-root",
                None,
                "root msg",
                1_700_000_060_000,
            ),
        )
        .unwrap();
        apply_core_event(
            &mut state,
            thread_timeline("!ops:example", "$thread-1", "$thread-root", "thread rep"),
        )
        .unwrap();
        apply_core_event(
            &mut state,
            incoming_reaction_added(
                "!ops:example",
                "$thread-root",
                "$reaction-root",
                "@bob:example.org",
                "heart",
                1_700_000_060_100,
            ),
        )
        .unwrap();
        apply_core_event(
            &mut state,
            incoming_reaction_added(
                "!ops:example",
                "$thread-1",
                "$reaction-reply",
                "@carol:example.org",
                "eyes",
                1_700_000_120_100,
            ),
        )
        .unwrap();

        let rows = build_thread_timeline_rows(&state.model, "!ops:example", "$thread-root");
        assert_eq!(
            rows.iter().map(|row| row.kind.clone()).collect::<Vec<_>>(),
            vec![
                TimelineRowKind::Header,
                TimelineRowKind::Message {
                    presentation: RoomMessagePresentationKind::Text,
                    send_state: TimelineMessageSendState::Sent,
                },
                TimelineRowKind::ReactionSummary,
                TimelineRowKind::Message {
                    presentation: RoomMessagePresentationKind::Text,
                    send_state: TimelineMessageSendState::Sent,
                },
                TimelineRowKind::ReactionSummary,
            ]
        );
        assert_eq!(
            project_timeline_rows(&rows),
            vec![
                "Room: Ops (!ops:example)".to_owned(),
                "[22:14] @alice:example: root msg".to_owned(),
                "    reactions: heart x1".to_owned(),
                "[22:15] @alice:example: thread rep".to_owned(),
                "    reactions: eyes x1".to_owned(),
            ]
        );
    }

    #[test]
    fn direct_message_edit_without_target_leaves_visible_timeline_unchanged() {
        let mut state = test_state();
        state.model.rooms = vec![room("!ops:example", "Ops", false)];

        apply_core_event(&mut state, timeline("!ops:example", "hello")).unwrap();
        let before = state
            .model
            .room_views
            .get("!ops:example")
            .expect("room view must exist")
            .activities
            .clone();

        apply_core_event(
            &mut state,
            incoming_replaced(
                "!ops:example",
                "$missing:example.org",
                "$edit:example.org",
                RoomMessagePresentationKind::Text,
                "edited",
            ),
        )
        .unwrap();

        assert_eq!(
            state
                .model
                .room_views
                .get("!ops:example")
                .map(|view| &view.activities),
            Some(&before)
        );
    }

    #[test]
    fn redaction_runtime_unknown_event_target_leaves_visible_timeline_unchanged() {
        let mut state = test_state();
        state.model.rooms = vec![room("!ops:example", "Ops", false)];

        apply_core_event(&mut state, timeline("!ops:example", "hello")).unwrap();
        let before_activities = state
            .model
            .room_views
            .get("!ops:example")
            .expect("room view must exist")
            .activities
            .clone();
        let before_last_activity = state.model.last_activity.clone();

        apply_core_event(
            &mut state,
            incoming_redacted(
                "!ops:example",
                "$missing:example.org",
                "$redaction:example.org",
            ),
        )
        .unwrap();

        assert_eq!(
            state
                .model
                .room_views
                .get("!ops:example")
                .map(|view| &view.activities),
            Some(&before_activities)
        );
        assert_eq!(state.model.last_activity, before_last_activity);
    }

    #[test]
    fn typed_sent_message_reconciles_by_transaction_id_without_duplicate_rows() {
        let mut state = test_state();
        state.model.rooms = vec![room("!ops:example", "Ops", false)];

        apply_core_event(
            &mut state,
            outgoing_queued_with_presentation(
                "!ops:example",
                "txn-1",
                RoomMessagePresentationKind::Emote,
                "waves",
            ),
        )
        .unwrap();
        apply_core_event(
            &mut state,
            outgoing_sent("!ops:example", "txn-1", "$event:example.org"),
        )
        .unwrap();
        apply_core_event(
            &mut state,
            timeline_with_transaction_id_with_presentation(
                "!ops:example",
                "$event:example.org",
                Some("txn-1"),
                RoomMessagePresentationKind::Emote,
                "waves",
                1_700_000_000_100,
            ),
        )
        .unwrap();

        assert_eq!(
            state
                .model
                .room_views
                .get("!ops:example")
                .map(|view| &view.activities),
            Some(&vec![sent_message_activity_with_presentation(
                "!ops:example",
                "$event:example.org",
                Some("txn-1"),
                RoomMessagePresentationKind::Emote,
                "waves",
                1_700_000_000_100,
            )])
        );
    }

    #[test]
    fn synced_message_before_sent_update_removes_duplicate_by_event_id_fallback() {
        let mut state = test_state();
        state.model.rooms = vec![room("!ops:example", "Ops", false)];

        apply_core_event(
            &mut state,
            outgoing_queued_with_presentation(
                "!ops:example",
                "txn-1",
                RoomMessagePresentationKind::Emote,
                "waves",
            ),
        )
        .unwrap();
        apply_core_event(
            &mut state,
            timeline_with_transaction_id_with_presentation(
                "!ops:example",
                "$event:example.org",
                None,
                RoomMessagePresentationKind::Emote,
                "waves",
                1_700_000_000_100,
            ),
        )
        .unwrap();
        apply_core_event(
            &mut state,
            outgoing_sent("!ops:example", "txn-1", "$event:example.org"),
        )
        .unwrap();

        assert_eq!(
            state
                .model
                .room_views
                .get("!ops:example")
                .map(|view| view.activities.len()),
            Some(1)
        );
        assert_eq!(
            render_room_lines(&state.model, "!ops:example"),
            vec![
                "Room: Ops (!ops:example)".to_owned(),
                "* [22:13] @alice:example waves".to_owned(),
            ]
        );
        assert_eq!(
            state.model.last_activity,
            Some(LastMessageActivity {
                room_id: "!ops:example".to_owned(),
                event_id: Some("$event:example.org".to_owned()),
                transaction_id: Some("txn-1".to_owned()),
                sender: "@alice:example".to_owned(),
                timestamp_ms: 1_700_000_000_100,
                presentation: RoomMessagePresentationKind::Emote,
                body: "waves".to_owned(),
                send_state: RoomMessageSendState::Sent,
            })
        );
    }

    #[test]
    fn synced_message_without_transaction_id_collapses_into_outgoing_row_after_sent_update() {
        let mut state = test_state();
        state.model.rooms = vec![room("!ops:example", "Ops", false)];

        apply_core_event(
            &mut state,
            outgoing_queued_with_presentation(
                "!ops:example",
                "txn-1",
                RoomMessagePresentationKind::Emote,
                "waves",
            ),
        )
        .unwrap();
        apply_core_event(
            &mut state,
            timeline_with_transaction_id_with_presentation(
                "!ops:example",
                "$event:example.org",
                None,
                RoomMessagePresentationKind::Emote,
                "waves",
                1_700_000_000_100,
            ),
        )
        .unwrap();
        apply_core_event(
            &mut state,
            outgoing_sent("!ops:example", "txn-1", "$event:example.org"),
        )
        .unwrap();

        assert_eq!(
            state
                .model
                .room_views
                .get("!ops:example")
                .map(|view| &view.activities),
            Some(&vec![sent_message_activity_with_presentation(
                "!ops:example",
                "$event:example.org",
                Some("txn-1"),
                RoomMessagePresentationKind::Emote,
                "waves",
                1_700_000_000_100,
            )])
        );
        assert_eq!(
            state
                .model
                .room_views
                .get("!ops:example")
                .map(|view| view.activities.len()),
            Some(1)
        );
        assert_eq!(
            render_room_lines(&state.model, "!ops:example"),
            vec![
                "Room: Ops (!ops:example)".to_owned(),
                "* [22:13] @alice:example waves".to_owned(),
            ]
        );
    }

    #[test]
    fn failed_message_update_keeps_visible_failed_row_and_error_details() {
        let mut state = test_state();
        state.model.rooms = vec![room("!ops:example", "Ops", false)];

        apply_core_event(
            &mut state,
            outgoing_queued_with_presentation(
                "!ops:example",
                "txn-1",
                RoomMessagePresentationKind::Emote,
                "waves",
            ),
        )
        .unwrap();
        apply_core_event(
            &mut state,
            outgoing_failed("!ops:example", "txn-1", "send failed"),
        )
        .unwrap();

        assert_eq!(
            state
                .model
                .room_views
                .get("!ops:example")
                .map(|view| &view.activities),
            Some(&vec![failed_message_activity_with_presentation(
                "!ops:example",
                "txn-1",
                RoomMessagePresentationKind::Emote,
                "waves",
                "send failed",
            )])
        );
        assert_eq!(
            state.model.last_activity,
            Some(LastMessageActivity {
                room_id: "!ops:example".to_owned(),
                event_id: None,
                transaction_id: Some("txn-1".to_owned()),
                sender: "@alice:example".to_owned(),
                timestamp_ms: 1_700_000_000_000,
                presentation: RoomMessagePresentationKind::Emote,
                body: "waves".to_owned(),
                send_state: RoomMessageSendState::Failed {
                    message: "send failed".to_owned(),
                    recoverable: true,
                },
            })
        );
        assert_eq!(
            render_room_lines(&state.model, "!ops:example"),
            vec![
                "Room: Ops (!ops:example)".to_owned(),
                "* [22:13] @alice:example waves (failed)".to_owned(),
            ]
        );
    }

    #[test]
    fn failed_room_activity_loaded_directly_renders_failed_row() {
        let mut state = test_state();
        state.model.rooms = vec![room("!ops:example", "Ops", false)];

        apply_core_event(
            &mut state,
            failed_message_with_presentation(
                "!ops:example",
                "txn-1",
                RoomMessagePresentationKind::Emote,
                "waves",
                "send failed",
            ),
        )
        .unwrap();

        assert_eq!(
            state
                .model
                .room_views
                .get("!ops:example")
                .map(|view| &view.activities),
            Some(&vec![failed_message_activity_with_presentation(
                "!ops:example",
                "txn-1",
                RoomMessagePresentationKind::Emote,
                "waves",
                "send failed",
            )])
        );
        assert_eq!(
            render_room_lines(&state.model, "!ops:example"),
            vec![
                "Room: Ops (!ops:example)".to_owned(),
                "* [22:13] @alice:example waves (failed)".to_owned(),
            ]
        );
    }

    #[test]
    fn clear_room_timeline_preserves_pending_room_activity_for_sent_update() {
        let mut state = test_state();
        state.model.rooms = vec![room("!ops:example", "Ops", false)];

        apply_core_event(&mut state, timeline("!ops:example", "first")).unwrap();
        apply_core_event(
            &mut state,
            outgoing_queued("!ops:example", "txn-1", "hello"),
        )
        .unwrap();

        state
            .model
            .room_views
            .get_mut("!ops:example")
            .expect("room view must exist after timeline update")
            .clear_visible_entries();

        assert_eq!(
            state
                .model
                .room_views
                .get("!ops:example")
                .map(|view| &view.activities),
            Some(&vec![pending_message_activity(
                "!ops:example",
                "txn-1",
                "hello"
            )])
        );

        apply_core_event(
            &mut state,
            outgoing_sent("!ops:example", "txn-1", "$event:example.org"),
        )
        .unwrap();

        assert_eq!(
            state
                .model
                .room_views
                .get("!ops:example")
                .map(|view| &view.activities),
            Some(&vec![sent_message_activity(
                "!ops:example",
                "$event:example.org",
                Some("txn-1"),
                "hello",
                1_700_000_000_000,
            )])
        );
    }

    #[test]
    fn clear_room_timeline_removes_unsupported_placeholders() {
        let mut state = test_state();
        state.model.rooms = vec![room("!ops:example", "Ops", false)];

        apply_core_event(
            &mut state,
            unsupported_message(
                "!ops:example",
                "$image1:example.org",
                "@alice:example.org",
                1_710_000_000_000,
                "m.video",
                "clip.mp4",
            ),
        )
        .unwrap();
        apply_core_event(
            &mut state,
            outgoing_queued("!ops:example", "txn-1", "hello"),
        )
        .unwrap();

        state
            .model
            .room_views
            .get_mut("!ops:example")
            .expect("room view must exist after timeline update")
            .clear_visible_entries();

        assert_eq!(
            state
                .model
                .room_views
                .get("!ops:example")
                .map(|view| &view.activities),
            Some(&vec![pending_message_activity(
                "!ops:example",
                "txn-1",
                "hello"
            )])
        );
    }

    #[test]
    fn clear_room_timeline_removes_image_rows() {
        let mut state = test_state();
        state.model.rooms = vec![room("!ops:example", "Ops", false)];

        apply_core_event(
            &mut state,
            image_message(
                "!ops:example",
                "$image1:example.org",
                "@alice:example.org",
                1_710_000_000_000,
                "diagram.png",
                Some("diagram.png"),
                Some(RoomImageInfo {
                    mimetype: Some("image/png".to_owned()),
                    size: Some(12_345),
                    width: Some(640),
                    height: Some(480),
                    is_animated: Some(false),
                    thumbnail_source: None,
                }),
            ),
        )
        .unwrap();
        apply_core_event(
            &mut state,
            outgoing_queued("!ops:example", "txn-1", "hello"),
        )
        .unwrap();

        state
            .model
            .room_views
            .get_mut("!ops:example")
            .expect("room view must exist after timeline update")
            .clear_visible_entries();

        assert_eq!(
            state
                .model
                .room_views
                .get("!ops:example")
                .map(|view| &view.activities),
            Some(&vec![pending_message_activity(
                "!ops:example",
                "txn-1",
                "hello"
            )])
        );
    }

    #[test]
    fn clear_room_timeline_removes_stale_image_media_state() {
        let mut state = test_state();
        state.model.rooms = vec![room("!ops:example", "Ops", false)];

        apply_core_event(
            &mut state,
            image_message(
                "!ops:example",
                "$image1:example.org",
                "@alice:example.org",
                1_710_000_000_000,
                "diagram.png",
                None,
                None,
            ),
        )
        .unwrap();
        apply_core_event(
            &mut state,
            room_image_media_event(
                "!ops:example",
                "event:$image1:example.org",
                "$image1:example.org",
                RoomImageMediaKind::Failed {
                    format: timeline_thumbnail_format(),
                    message: "unavailable".to_owned(),
                },
            ),
        )
        .unwrap();
        apply_core_event(
            &mut state,
            outgoing_queued("!ops:example", "txn-1", "hello"),
        )
        .unwrap();

        state
            .model
            .room_views
            .get_mut("!ops:example")
            .expect("room view must exist after timeline update")
            .clear_visible_entries();

        let room_view = state
            .model
            .room_views
            .get("!ops:example")
            .expect("room view must exist after clearing");
        assert_eq!(
            room_view.activities,
            vec![pending_message_activity("!ops:example", "txn-1", "hello")]
        );
        assert!(room_view.image_media.is_empty());
    }

    #[test]
    #[should_panic(expected = "outgoing sent requires an existing queued timeline row")]
    fn room_activity_update_sent_without_existing_queued_row_panics() {
        let mut state = test_state();
        state.model.rooms = vec![room("!ops:example", "Ops", false)];

        apply_core_event(
            &mut state,
            outgoing_sent("!ops:example", "txn-1", "$event:example.org"),
        )
        .unwrap();
    }

    #[test]
    #[should_panic(expected = "outgoing sent requires an existing queued timeline row")]
    fn room_activity_update_sent_after_sent_row_with_same_transaction_id_panics() {
        let mut state = test_state();
        state.model.rooms = vec![room("!ops:example", "Ops", false)];

        apply_core_event(
            &mut state,
            timeline_with_transaction_id(
                "!ops:example",
                "$event:example.org",
                Some("txn-1"),
                "hello",
                1_700_000_000_000,
            ),
        )
        .unwrap();
        apply_core_event(
            &mut state,
            outgoing_sent("!ops:example", "txn-1", "$event:example.org"),
        )
        .unwrap();
    }

    #[test]
    #[should_panic(expected = "outgoing sent requires an existing queued timeline row")]
    fn room_activity_update_sent_after_failed_row_with_same_transaction_id_panics() {
        let mut state = test_state();
        state.model.rooms = vec![room("!ops:example", "Ops", false)];

        apply_core_event(
            &mut state,
            failed_message("!ops:example", "txn-1", "hello", "send failed"),
        )
        .unwrap();
        apply_core_event(
            &mut state,
            outgoing_sent("!ops:example", "txn-1", "$event:example.org"),
        )
        .unwrap();
    }

    #[test]
    #[should_panic(expected = "outgoing failed requires an existing queued timeline row")]
    fn room_activity_update_failed_without_existing_queued_row_panics() {
        let mut state = test_state();
        state.model.rooms = vec![room("!ops:example", "Ops", false)];

        apply_core_event(
            &mut state,
            outgoing_failed("!ops:example", "txn-1", "send failed"),
        )
        .unwrap();
    }

    #[test]
    #[should_panic(expected = "outgoing failed requires an existing queued timeline row")]
    fn room_activity_update_failed_after_sent_row_with_same_transaction_id_panics() {
        let mut state = test_state();
        state.model.rooms = vec![room("!ops:example", "Ops", false)];

        apply_core_event(
            &mut state,
            timeline_with_transaction_id(
                "!ops:example",
                "$event:example.org",
                Some("txn-1"),
                "hello",
                1_700_000_000_000,
            ),
        )
        .unwrap();
        apply_core_event(
            &mut state,
            outgoing_failed("!ops:example", "txn-1", "send failed"),
        )
        .unwrap();
    }

    #[test]
    #[should_panic(expected = "outgoing failed requires an existing queued timeline row")]
    fn room_activity_update_failed_after_failed_row_with_same_transaction_id_panics() {
        let mut state = test_state();
        state.model.rooms = vec![room("!ops:example", "Ops", false)];

        apply_core_event(
            &mut state,
            failed_message("!ops:example", "txn-1", "hello", "send failed"),
        )
        .unwrap();
        apply_core_event(
            &mut state,
            outgoing_failed("!ops:example", "txn-1", "send failed again"),
        )
        .unwrap();
    }

    #[test]
    fn format_timestamp_hhmm_utc_uses_deterministic_utc_minutes() {
        assert_eq!(format_timestamp_hhmm_utc(1_700_000_000_000), "22:13");
    }

    #[test]
    fn build_room_timeline_rows_capture_kind_order_and_projection() {
        let mut state = test_state();
        state.model.rooms = vec![room("!ops:example", "Ops", true)];
        apply_core_event(&mut state, topic_state("!ops:example", "incident review")).unwrap();
        apply_core_event(
            &mut state,
            typing_event("!ops:example", &["@alice:example.org", "@bob:example.org"]),
        )
        .unwrap();
        apply_core_event(
            &mut state,
            timeline_with_transaction_id(
                "!ops:example",
                "$event-text:example.org",
                None,
                "<body>",
                1_700_000_000_000,
            ),
        )
        .unwrap();
        apply_core_event(
            &mut state,
            Event::RoomActivity(Box::new(sent_message_activity_with_presentation(
                "!ops:example",
                "$event-notice:example.org",
                None,
                RoomMessagePresentationKind::Notice,
                "maintenance window",
                1_700_000_030_000,
            ))),
        )
        .unwrap();
        apply_core_event(
            &mut state,
            Event::RoomActivity(Box::new(sent_message_activity_with_presentation(
                "!ops:example",
                "$event-emote:example.org",
                None,
                RoomMessagePresentationKind::Emote,
                "waves",
                1_700_000_090_000,
            ))),
        )
        .unwrap();
        apply_core_event(
            &mut state,
            outgoing_queued("!ops:example", "txn-pending", "pending local echo"),
        )
        .unwrap();
        apply_core_event(
            &mut state,
            failed_message("!ops:example", "txn-failed", "failed local echo", "network"),
        )
        .unwrap();
        apply_core_event(
            &mut state,
            image_message(
                "!ops:example",
                "$image1:example.org",
                "@alice:example.org",
                1_710_000_000_000,
                "body fallback",
                Some("diagram.png"),
                Some(RoomImageInfo {
                    mimetype: Some("image/png".to_owned()),
                    size: Some(12_345),
                    width: Some(640),
                    height: Some(480),
                    is_animated: Some(false),
                    thumbnail_source: None,
                }),
            ),
        )
        .unwrap();
        apply_core_event(
            &mut state,
            Event::RoomActivity(Box::new(file_activity(
                "!ops:example",
                "$file1:example.org",
                "@alice:example.org",
                1_710_000_060_000,
                "Quarterly report",
                Some("report.pdf"),
                Some(RoomFileInfo {
                    mimetype: Some("application/pdf".to_owned()),
                    size: Some(45_678),
                }),
            ))),
        )
        .unwrap();
        apply_core_event(
            &mut state,
            Event::RoomActivity(Box::new(audio_activity(
                "!ops:example",
                "$audio1:example.org",
                "@alice:example.org",
                1_710_000_120_000,
                "Voice note",
                Some("voice-note.ogg"),
                Some(RoomAudioInfo {
                    mimetype: Some("audio/ogg".to_owned()),
                    size: Some(23_456),
                    duration_ms: Some(12_000),
                }),
            ))),
        )
        .unwrap();
        apply_core_event(
            &mut state,
            unsupported_message(
                "!ops:example",
                "$video1:example.org",
                "@alice:example.org",
                1_710_000_180_000,
                "m.video",
                "clip.mp4",
            ),
        )
        .unwrap();
        apply_core_event(
            &mut state,
            member_state("!ops:example", "@carol:example", "join", Some("carol")),
        )
        .unwrap();
        apply_core_event(
            &mut state,
            member_state("!ops:example", "@dave:example", "leave", None),
        )
        .unwrap();

        let rows = build_room_timeline_rows(&state.model, "!ops:example");
        assert_eq!(
            rows.iter().map(|row| row.kind.clone()).collect::<Vec<_>>(),
            vec![
                TimelineRowKind::Header,
                TimelineRowKind::Topic,
                TimelineRowKind::Typing,
                TimelineRowKind::StateNotice,
                TimelineRowKind::Message {
                    presentation: RoomMessagePresentationKind::Text,
                    send_state: TimelineMessageSendState::Sent,
                },
                TimelineRowKind::Message {
                    presentation: RoomMessagePresentationKind::Notice,
                    send_state: TimelineMessageSendState::Sent,
                },
                TimelineRowKind::Message {
                    presentation: RoomMessagePresentationKind::Emote,
                    send_state: TimelineMessageSendState::Sent,
                },
                TimelineRowKind::Message {
                    presentation: RoomMessagePresentationKind::Text,
                    send_state: TimelineMessageSendState::Pending,
                },
                TimelineRowKind::Message {
                    presentation: RoomMessagePresentationKind::Text,
                    send_state: TimelineMessageSendState::Failed,
                },
                TimelineRowKind::Image,
                TimelineRowKind::File,
                TimelineRowKind::Audio,
                TimelineRowKind::Unsupported,
                TimelineRowKind::StateNotice,
                TimelineRowKind::StateNotice,
            ]
        );
        assert_eq!(
            render_room_lines(&state.model, "!ops:example"),
            project_timeline_rows(&rows)
        );
        assert_eq!(
            project_timeline_rows(&rows),
            vec![
                "Room: Ops [e2ee] (!ops:example)".to_owned(),
                "Topic: incident review".to_owned(),
                "* @alice:example.org, @bob:example.org are typing".to_owned(),
                "* [22:14] @alice:example set topic: incident review".to_owned(),
                "[22:13] @alice:example: <body>".to_owned(),
                "[22:13] -@alice:example- maintenance window".to_owned(),
                "* [22:14] @alice:example waves".to_owned(),
                "[22:13] @alice:example: pending local echo (sending)".to_owned(),
                "[22:13] @alice:example: failed local echo (failed)".to_owned(),
                "* [16:00] @alice:example.org sent image: diagram.png (image/png, 640x480, 12345 bytes)"
                    .to_owned(),
                "* [16:01] @alice:example.org sent file: report.pdf (application/pdf, 45678 bytes)"
                    .to_owned(),
                "* [16:02] @alice:example.org sent audio: voice-note.ogg (audio/ogg, 23456 bytes, 12000 ms)"
                    .to_owned(),
                "* [16:03] @alice:example.org sent unsupported m.video: clip.mp4"
                    .to_owned(),
                "* [22:15] carol joined".to_owned(),
                "* [22:15] @dave:example left".to_owned(),
            ]
        );
        assert_eq!(
            rows[0].visual.clone(),
            TimelineRowVisual::FullWidth {
                text: "Room: Ops [e2ee] (!ops:example)".to_owned(),
            }
        );
        assert_eq!(
            rows[4].visual.clone(),
            TimelineRowVisual::Log {
                timestamp: "[22:13]".to_owned(),
                sender_prefix: "@alice:example:".to_owned(),
                body: "<body>".to_owned(),
            }
        );
        assert_eq!(
            rows[5].visual.clone(),
            TimelineRowVisual::Log {
                timestamp: "[22:13]".to_owned(),
                sender_prefix: "-@alice:example-".to_owned(),
                body: "maintenance window".to_owned(),
            }
        );
        assert_eq!(
            rows[6].visual.clone(),
            TimelineRowVisual::Log {
                timestamp: "[22:14]".to_owned(),
                sender_prefix: "* @alice:example".to_owned(),
                body: "waves".to_owned(),
            }
        );
        assert_eq!(
            rows[9].visual.clone(),
            TimelineRowVisual::Log {
                timestamp: "[16:00]".to_owned(),
                sender_prefix: "* @alice:example.org".to_owned(),
                body: "sent image: diagram.png (image/png, 640x480, 12345 bytes)".to_owned(),
            }
        );
        assert_eq!(
            rows[10].visual.clone(),
            TimelineRowVisual::Log {
                timestamp: "[16:01]".to_owned(),
                sender_prefix: "* @alice:example.org".to_owned(),
                body: "sent file: report.pdf (application/pdf, 45678 bytes)".to_owned(),
            }
        );
        assert_eq!(
            rows[11].visual.clone(),
            TimelineRowVisual::Log {
                timestamp: "[16:02]".to_owned(),
                sender_prefix: "* @alice:example.org".to_owned(),
                body: "sent audio: voice-note.ogg (audio/ogg, 23456 bytes, 12000 ms)".to_owned(),
            }
        );
        assert_eq!(
            rows[12].visual.clone(),
            TimelineRowVisual::Log {
                timestamp: "[16:03]".to_owned(),
                sender_prefix: "* @alice:example.org".to_owned(),
                body: "sent unsupported m.video: clip.mp4".to_owned(),
            }
        );
        assert_eq!(
            rows[13].visual.clone(),
            TimelineRowVisual::Log {
                timestamp: "[22:15]".to_owned(),
                sender_prefix: "*".to_owned(),
                body: "carol joined".to_owned(),
            }
        );
        assert_eq!(
            rows[14].visual.clone(),
            TimelineRowVisual::Log {
                timestamp: "[22:15]".to_owned(),
                sender_prefix: "*".to_owned(),
                body: "@dave:example left".to_owned(),
            }
        );
    }

    #[test]
    fn render_room_lines_include_text_notice_emote_and_state_activities() {
        let mut state = test_state();
        state.model.rooms = vec![room("!ops:example", "Ops", true)];
        apply_core_event(&mut state, topic_state("!ops:example", "incident review")).unwrap();
        apply_core_event(
            &mut state,
            typing_event("!ops:example", &["@alice:example.org", "@bob:example.org"]),
        )
        .unwrap();
        apply_core_event(
            &mut state,
            timeline_with_transaction_id(
                "!ops:example",
                "$event-text:example.org",
                None,
                "<body>",
                1_700_000_000_000,
            ),
        )
        .unwrap();
        apply_core_event(
            &mut state,
            Event::RoomActivity(Box::new(sent_message_activity_with_presentation(
                "!ops:example",
                "$event-notice:example.org",
                None,
                RoomMessagePresentationKind::Notice,
                "maintenance window",
                1_700_000_030_000,
            ))),
        )
        .unwrap();
        apply_core_event(
            &mut state,
            Event::RoomActivity(Box::new(sent_message_activity_with_presentation(
                "!ops:example",
                "$event-emote:example.org",
                None,
                RoomMessagePresentationKind::Emote,
                "waves",
                1_700_000_090_000,
            ))),
        )
        .unwrap();
        apply_core_event(
            &mut state,
            member_state("!ops:example", "@carol:example", "join", Some("carol")),
        )
        .unwrap();
        apply_core_event(
            &mut state,
            member_state("!ops:example", "@dave:example", "leave", None),
        )
        .unwrap();
        apply_core_event(
            &mut state,
            member_state("!ops:example", "@erin:example", "ban", Some("erin")),
        )
        .unwrap();

        assert_eq!(
            render_room_lines(&state.model, "!ops:example"),
            vec![
                "Room: Ops [e2ee] (!ops:example)".to_owned(),
                "Topic: incident review".to_owned(),
                "* @alice:example.org, @bob:example.org are typing".to_owned(),
                "* [22:14] @alice:example set topic: incident review".to_owned(),
                "[22:13] @alice:example: <body>".to_owned(),
                "[22:13] -@alice:example- maintenance window".to_owned(),
                "* [22:14] @alice:example waves".to_owned(),
                "* [22:15] carol joined".to_owned(),
                "* [22:15] @dave:example left".to_owned(),
                "* [22:15] @erin:example membership: ban".to_owned(),
            ]
        );
    }

    #[test]
    fn room_timeline_rows_show_loading_history_backfill_after_context_rows() {
        let mut state = test_state();
        state.model.rooms = vec![room("!ops:example", "Ops", false)];
        apply_core_event(&mut state, topic_state("!ops:example", "incident review")).unwrap();
        apply_core_event(
            &mut state,
            typing_event("!ops:example", &["@alice:example.org"]),
        )
        .unwrap();
        apply_core_event(&mut state, timeline("!ops:example", "latest row")).unwrap();
        state
            .model
            .room_views
            .get_mut("!ops:example")
            .expect("room view exists after room activity")
            .history
            .status = RoomHistoryBackfillStatus::Loading { from: None };

        let rows = build_room_timeline_rows(&state.model, "!ops:example");

        assert_eq!(
            rows.iter().map(|row| row.kind.clone()).collect::<Vec<_>>(),
            vec![
                TimelineRowKind::Header,
                TimelineRowKind::Topic,
                TimelineRowKind::Typing,
                TimelineRowKind::Plain,
                TimelineRowKind::StateNotice,
                TimelineRowKind::Message {
                    presentation: RoomMessagePresentationKind::Text,
                    send_state: TimelineMessageSendState::Sent,
                },
            ]
        );
        assert_eq!(rows[3].logical_text, "* Loading older history...");
    }

    #[test]
    fn room_timeline_rows_show_failed_history_backfill_after_context_rows() {
        let mut state = test_state();
        state.model.rooms = vec![room("!ops:example", "Ops", false)];
        apply_core_event(&mut state, topic_state("!ops:example", "incident review")).unwrap();
        apply_core_event(
            &mut state,
            typing_event("!ops:example", &["@alice:example.org"]),
        )
        .unwrap();
        apply_core_event(&mut state, timeline("!ops:example", "latest row")).unwrap();
        state
            .model
            .room_views
            .get_mut("!ops:example")
            .expect("room view exists after room activity")
            .history
            .status = RoomHistoryBackfillStatus::Failed {
            from: Some("token-1".to_owned()),
            message: "homeserver unavailable".to_owned(),
        };

        let rows = build_room_timeline_rows(&state.model, "!ops:example");

        assert_eq!(rows[3].kind, TimelineRowKind::Plain);
        assert_eq!(
            rows[3].logical_text,
            "* Unable to load older history: homeserver unavailable. Scroll to top to retry."
        );
    }

    #[test]
    fn room_timeline_rows_show_exhausted_history_backfill_after_context_rows() {
        let mut state = test_state();
        state.model.rooms = vec![room("!ops:example", "Ops", false)];
        apply_core_event(&mut state, topic_state("!ops:example", "incident review")).unwrap();
        apply_core_event(
            &mut state,
            typing_event("!ops:example", &["@alice:example.org"]),
        )
        .unwrap();
        apply_core_event(&mut state, timeline("!ops:example", "latest row")).unwrap();
        state
            .model
            .room_views
            .get_mut("!ops:example")
            .expect("room view exists after room activity")
            .history
            .status = RoomHistoryBackfillStatus::Exhausted;

        let rows = build_room_timeline_rows(&state.model, "!ops:example");

        assert_eq!(rows[3].kind, TimelineRowKind::Plain);
        assert_eq!(rows[3].logical_text, "* No older history available.");
    }

    #[test]
    fn room_timeline_rows_skip_idle_history_backfill_row() {
        let mut state = test_state();
        state.model.rooms = vec![room("!ops:example", "Ops", false)];
        apply_core_event(&mut state, topic_state("!ops:example", "incident review")).unwrap();
        apply_core_event(
            &mut state,
            typing_event("!ops:example", &["@alice:example.org"]),
        )
        .unwrap();
        apply_core_event(&mut state, timeline("!ops:example", "latest row")).unwrap();

        let rows = build_room_timeline_rows(&state.model, "!ops:example");

        assert_eq!(
            rows.iter().map(|row| row.kind.clone()).collect::<Vec<_>>(),
            vec![
                TimelineRowKind::Header,
                TimelineRowKind::Topic,
                TimelineRowKind::Typing,
                TimelineRowKind::StateNotice,
                TimelineRowKind::Message {
                    presentation: RoomMessagePresentationKind::Text,
                    send_state: TimelineMessageSendState::Sent,
                },
            ]
        );
        assert!(
            rows.iter()
                .all(|row| !row.logical_text.contains("older history"))
        );
    }

    #[test]
    fn dump_ui_room_history_backfill_state_projects_all_variants() {
        assert_eq!(
            dump_ui_room_history_backfill_state(&RoomHistoryBackfillState::default()),
            UiRoomHistoryBackfillState::Idle
        );
        assert_eq!(
            dump_ui_room_history_backfill_state(&RoomHistoryBackfillState {
                next_token: None,
                status: RoomHistoryBackfillStatus::Loading {
                    from: Some("token-1".to_owned()),
                },
            }),
            UiRoomHistoryBackfillState::Loading {
                from: Some("token-1".to_owned()),
            }
        );
        assert_eq!(
            dump_ui_room_history_backfill_state(&RoomHistoryBackfillState {
                next_token: Some("token-2".to_owned()),
                status: RoomHistoryBackfillStatus::Failed {
                    from: Some("token-1".to_owned()),
                    message: "homeserver unavailable".to_owned(),
                },
            }),
            UiRoomHistoryBackfillState::Failed {
                from: Some("token-1".to_owned()),
                message: "homeserver unavailable".to_owned(),
            }
        );
        assert_eq!(
            dump_ui_room_history_backfill_state(&RoomHistoryBackfillState {
                next_token: None,
                status: RoomHistoryBackfillStatus::Exhausted,
            }),
            UiRoomHistoryBackfillState::Exhausted
        );
    }

    #[test]
    fn redaction_state_notice_renders_generic_tombstone_without_old_notice_text() {
        let mut state = test_state();
        state.model.rooms = vec![room("!ops:example", "Ops", false)];
        apply_core_event(
            &mut state,
            member_state("!ops:example", "@carol:example", "join", Some("carol")),
        )
        .unwrap();
        apply_core_event(
            &mut state,
            incoming_redacted(
                "!ops:example",
                "$member:!ops:example:@carol:example:join",
                "$redaction:example.org",
            ),
        )
        .unwrap();

        let rendered = render_room_lines(&state.model, "!ops:example").join("\n");
        assert!(rendered.contains("message redacted"));
        assert!(!rendered.contains("carol joined"));
        assert!(!rendered.contains("membership: join"));
    }

    #[test]
    fn room_tree_label_marks_direct_rooms_as_queries_before_e2ee() {
        let mut direct_room = room("!direct:example", "Direct", true);
        direct_room.is_direct = true;
        direct_room.direct_user_ids = vec!["@alice:example".to_owned()];

        assert_eq!(
            room_tree_label(&direct_room),
            "Direct [query: @alice:example] [e2ee]"
        );
    }

    #[test]
    fn room_tree_label_uses_query_fallback_for_direct_rooms_without_user_ids() {
        let mut direct_room = room("!direct:example", "Direct", true);
        direct_room.is_direct = true;

        assert_eq!(room_tree_label(&direct_room), "Direct [query] [e2ee]");
    }

    #[test]
    fn room_tree_label_ignores_direct_user_ids_when_room_is_not_direct() {
        let mut room = room("!ops:example", "Ops", true);
        room.direct_user_ids = vec!["@alice:example".to_owned()];

        assert_eq!(room_tree_label(&room), "Ops [e2ee]");
    }

    #[test]
    fn room_navigation_label_appends_read_state_suffix_after_existing_markers() {
        let mut state = test_state();
        let mut direct_room = room("!direct:example", "Direct", true);
        direct_room.is_direct = true;
        direct_room.direct_user_ids = vec!["@alice:example".to_owned()];
        state.model.rooms = vec![direct_room];
        state.model.room_read_states.insert(
            "!direct:example".to_owned(),
            RoomReadState {
                room_id: "!direct:example".to_owned(),
                latest_active_event_id: Some("$latest".to_owned()),
                fully_read_event_id: Some("$fully-read".to_owned()),
                marked_unread: true,
                num_unread: 3,
                num_notifications: 2,
                num_mentions: 1,
            },
        );

        assert_eq!(
            room_navigation_label(&state.model, &state.model.rooms[0]),
            "Direct [query: @alice:example] [e2ee] [@1 !2 *3 M]"
        );
    }

    #[test]
    fn room_navigation_label_handles_sparse_and_absent_read_state() {
        let mut state = test_state();
        state.model.rooms = vec![room("!todo:example", "Todo", false)];

        assert_eq!(
            room_navigation_label(&state.model, &state.model.rooms[0]),
            "Todo"
        );

        state.model.room_read_states.insert(
            "!todo:example".to_owned(),
            RoomReadState {
                room_id: "!todo:example".to_owned(),
                latest_active_event_id: None,
                fully_read_event_id: None,
                marked_unread: true,
                num_unread: 0,
                num_notifications: 0,
                num_mentions: 0,
            },
        );
        assert_eq!(
            room_navigation_label(&state.model, &state.model.rooms[0]),
            "Todo [M]"
        );

        state.model.room_read_states.insert(
            "!todo:example".to_owned(),
            RoomReadState {
                room_id: "!todo:example".to_owned(),
                latest_active_event_id: None,
                fully_read_event_id: None,
                marked_unread: false,
                num_unread: 7,
                num_notifications: 0,
                num_mentions: 0,
            },
        );
        assert_eq!(
            room_navigation_label(&state.model, &state.model.rooms[0]),
            "Todo [*7]"
        );

        state.model.room_read_states.insert(
            "!todo:example".to_owned(),
            RoomReadState {
                room_id: "!todo:example".to_owned(),
                latest_active_event_id: Some("$latest".to_owned()),
                fully_read_event_id: Some("$latest".to_owned()),
                marked_unread: false,
                num_unread: 0,
                num_notifications: 0,
                num_mentions: 0,
            },
        );
        assert_eq!(
            room_navigation_label(&state.model, &state.model.rooms[0]),
            "Todo"
        );
    }

    #[test]
    fn render_room_lines_include_query_marker_for_direct_rooms() {
        let mut state = test_state();
        let mut direct_room = room("!direct:example", "Direct", true);
        direct_room.is_direct = true;
        direct_room.direct_user_ids = vec!["@alice:example".to_owned()];
        state.model.rooms = vec![direct_room];

        assert_eq!(
            render_room_lines(&state.model, "!direct:example"),
            vec!["Room: Direct [query: @alice:example] [e2ee] (!direct:example)".to_owned()]
        );
    }

    #[test]
    fn render_room_lines_use_query_fallback_for_direct_rooms_without_user_ids() {
        let mut state = test_state();
        let mut direct_room = room("!direct:example", "Direct", true);
        direct_room.is_direct = true;
        state.model.rooms = vec![direct_room];

        assert_eq!(
            render_room_lines(&state.model, "!direct:example"),
            vec!["Room: Direct [query] [e2ee] (!direct:example)".to_owned()]
        );
    }

    #[test]
    fn render_room_lines_include_unsupported_message_placeholder() {
        let mut state = test_state();
        state.model.rooms = vec![room("!ops:example", "Ops", false)];
        apply_core_event(
            &mut state,
            unsupported_message(
                "!ops:example",
                "$image1:example.org",
                "@alice:example.org",
                1_710_000_000_000,
                "m.video",
                "clip.mp4",
            ),
        )
        .unwrap();

        assert_eq!(
            render_room_lines(&state.model, "!ops:example"),
            vec![
                "Room: Ops (!ops:example)".to_owned(),
                "* [16:00] @alice:example.org sent unsupported m.video: clip.mp4".to_owned(),
            ]
        );
    }

    #[test]
    fn redaction_unsupported_message_hides_msgtype_and_body() {
        let mut state = test_state();
        state.model.rooms = vec![room("!ops:example", "Ops", false)];
        apply_core_event(
            &mut state,
            unsupported_message(
                "!ops:example",
                "$unsupported:example.org",
                "@alice:example.org",
                1_710_000_000_000,
                "m.video",
                "clip.mp4",
            ),
        )
        .unwrap();
        apply_core_event(
            &mut state,
            incoming_redacted(
                "!ops:example",
                "$unsupported:example.org",
                "$redaction:example.org",
            ),
        )
        .unwrap();

        let rendered = render_room_lines(&state.model, "!ops:example").join("\n");
        assert!(rendered.contains("message redacted"));
        assert!(!rendered.contains("m.video"));
        assert!(!rendered.contains("clip.mp4"));
    }

    #[test]
    fn render_room_lines_include_typed_image_with_metadata() {
        let mut state = test_state();
        state.model.rooms = vec![room("!ops:example", "Ops", false)];
        apply_core_event(
            &mut state,
            image_message(
                "!ops:example",
                "$image1:example.org",
                "@alice:example.org",
                1_710_000_000_000,
                "body fallback",
                Some("diagram.png"),
                Some(RoomImageInfo {
                    mimetype: Some("image/png".to_owned()),
                    size: Some(12_345),
                    width: Some(640),
                    height: Some(480),
                    is_animated: Some(false),
                    thumbnail_source: None,
                }),
            ),
        )
        .unwrap();

        let lines = render_room_lines(&state.model, "!ops:example");
        assert_eq!(
            lines,
            vec![
                "Room: Ops (!ops:example)".to_owned(),
                "* [16:00] @alice:example.org sent image: diagram.png (image/png, 640x480, 12345 bytes)"
                    .to_owned(),
            ]
        );
        assert!(!lines.join("\n").contains("unsupported m.image"));
    }

    #[test]
    fn render_room_lines_omit_empty_image_metadata_suffix() {
        let mut state = test_state();
        state.model.rooms = vec![room("!ops:example", "Ops", false)];
        apply_core_event(
            &mut state,
            image_message(
                "!ops:example",
                "$image1:example.org",
                "@alice:example.org",
                1_710_000_000_000,
                "diagram.png",
                None,
                None,
            ),
        )
        .unwrap();

        assert_eq!(
            render_room_lines(&state.model, "!ops:example"),
            vec![
                "Room: Ops (!ops:example)".to_owned(),
                "* [16:00] @alice:example.org sent image: diagram.png".to_owned(),
            ]
        );
    }

    #[test]
    fn image_rows_project_pending_media_state_without_media_event() {
        let mut state = test_state();
        state.model.rooms = vec![room("!ops:example", "Ops", false)];
        apply_core_event(
            &mut state,
            image_message(
                "!ops:example",
                "$image1:example.org",
                "@alice:example.org",
                1_710_000_000_000,
                "diagram.png",
                None,
                None,
            ),
        )
        .unwrap();

        let rows = build_room_timeline_rows(&state.model, "!ops:example");
        let image_row = rows
            .iter()
            .find(|row| matches!(row.kind, TimelineRowKind::Image))
            .expect("image row must be present");

        assert_eq!(image_row.image_state, Some(TimelineImageState::Pending));
        assert!(
            !render_room_lines(&state.model, "!ops:example")
                .join("\n")
                .contains("loading image...")
        );
    }

    #[test]
    fn failed_room_image_media_event_projects_failed_image_state() {
        let mut state = test_state();
        state.model.rooms = vec![room("!ops:example", "Ops", false)];
        apply_core_event(
            &mut state,
            image_message(
                "!ops:example",
                "$image1:example.org",
                "@alice:example.org",
                1_710_000_000_000,
                "diagram.png",
                None,
                None,
            ),
        )
        .unwrap();
        apply_core_event(
            &mut state,
            room_image_media_event(
                "!ops:example",
                "event:$image1:example.org",
                "$image1:example.org",
                RoomImageMediaKind::Failed {
                    format: timeline_thumbnail_format(),
                    message: "unavailable".to_owned(),
                },
            ),
        )
        .unwrap();

        assert_eq!(
            state
                .model
                .room_views
                .get("!ops:example")
                .and_then(|room_view| room_view.image_media.get("event:$image1:example.org"))
                .cloned(),
            Some(RoomImageMediaState::Failed {
                event_id: "$image1:example.org".to_owned(),
                format: timeline_thumbnail_format(),
                message: "unavailable".to_owned(),
            })
        );

        let rows = build_room_timeline_rows(&state.model, "!ops:example");
        let image_row = rows
            .iter()
            .find(|row| matches!(row.kind, TimelineRowKind::Image))
            .expect("image row must be present");
        assert_eq!(
            image_row.image_state,
            Some(TimelineImageState::Failed {
                message: "unavailable".to_owned(),
            })
        );
        assert!(
            !render_room_lines(&state.model, "!ops:example")
                .join("\n")
                .contains("image unavailable")
        );
    }

    #[test]
    fn stale_or_mismatched_room_image_media_event_does_not_replace_current_state() {
        let mut state = test_state();
        state.model.rooms = vec![room("!ops:example", "Ops", false)];
        apply_core_event(
            &mut state,
            image_message(
                "!ops:example",
                "$image1:example.org",
                "@alice:example.org",
                1_710_000_000_000,
                "diagram.png",
                None,
                None,
            ),
        )
        .unwrap();
        apply_core_event(
            &mut state,
            room_image_media_event(
                "!ops:example",
                "event:$image1:example.org",
                "$image1:example.org",
                RoomImageMediaKind::Failed {
                    format: timeline_thumbnail_format(),
                    message: "current".to_owned(),
                },
            ),
        )
        .unwrap();

        apply_core_event(
            &mut state,
            room_image_media_event(
                "!ops:example",
                "event:$image1:example.org",
                "$different:example.org",
                RoomImageMediaKind::Failed {
                    format: timeline_thumbnail_format(),
                    message: "stale".to_owned(),
                },
            ),
        )
        .unwrap();
        apply_core_event(
            &mut state,
            room_image_media_event(
                "!ops:example",
                "event:$image1:example.org",
                "$image1:example.org",
                RoomImageMediaKind::Failed {
                    format: RoomImageMediaFormat::File,
                    message: "wrong format".to_owned(),
                },
            ),
        )
        .unwrap();

        assert_eq!(
            state
                .model
                .room_views
                .get("!ops:example")
                .and_then(|room_view| room_view.image_media.get("event:$image1:example.org"))
                .cloned(),
            Some(RoomImageMediaState::Failed {
                event_id: "$image1:example.org".to_owned(),
                format: timeline_thumbnail_format(),
                message: "current".to_owned(),
            })
        );
    }

    #[test]
    fn redaction_image_row_clears_media_state_and_ignores_late_media_updates() {
        let mut state = test_state();
        state.model.rooms = vec![room("!ops:example", "Ops", false)];
        apply_core_event(
            &mut state,
            image_message(
                "!ops:example",
                "$image1:example.org",
                "@alice:example.org",
                1_710_000_000_000,
                "body fallback",
                Some("diagram.png"),
                Some(RoomImageInfo {
                    mimetype: Some("image/png".to_owned()),
                    size: Some(12_345),
                    width: Some(640),
                    height: Some(480),
                    is_animated: Some(false),
                    thumbnail_source: None,
                }),
            ),
        )
        .unwrap();
        apply_core_event(
            &mut state,
            room_image_media_event(
                "!ops:example",
                "event:$image1:example.org",
                "$image1:example.org",
                RoomImageMediaKind::Failed {
                    format: timeline_thumbnail_format(),
                    message: "current".to_owned(),
                },
            ),
        )
        .unwrap();
        apply_core_event(
            &mut state,
            incoming_redacted(
                "!ops:example",
                "$image1:example.org",
                "$redaction:example.org",
            ),
        )
        .unwrap();

        let room_view = state
            .model
            .room_views
            .get("!ops:example")
            .expect("room view must exist");
        assert!(
            !room_view
                .image_media
                .contains_key("event:$image1:example.org"),
            "redaction should drop stored image media state"
        );
        assert!(
            collect_lazy_room_image_media_requests(room_view, room_view.activities.iter())
                .is_empty(),
            "redacted image rows must not trigger lazy image fetches"
        );

        let rendered = render_room_lines(&state.model, "!ops:example").join("\n");
        assert!(rendered.contains("message redacted"));
        assert!(!rendered.contains("diagram.png"));
        assert!(!rendered.contains("body fallback"));
        assert!(!rendered.contains("mxc://example.org/diagram"));

        apply_core_event(
            &mut state,
            room_image_media_event(
                "!ops:example",
                "event:$image1:example.org",
                "$image1:example.org",
                RoomImageMediaKind::Failed {
                    format: timeline_thumbnail_format(),
                    message: "late".to_owned(),
                },
            ),
        )
        .unwrap();

        let room_view = state
            .model
            .room_views
            .get("!ops:example")
            .expect("room view must exist");
        assert!(room_view.image_media.is_empty());
        let rendered = render_room_lines(&state.model, "!ops:example").join("\n");
        assert!(rendered.contains("message redacted"));
        assert!(!rendered.contains("late"));
    }

    #[test]
    fn invalid_loaded_room_image_media_event_projects_failed_image_state() {
        let mut state = test_state();
        state.model.rooms = vec![room("!ops:example", "Ops", false)];
        apply_core_event(
            &mut state,
            image_message(
                "!ops:example",
                "$image1:example.org",
                "@alice:example.org",
                1_710_000_000_000,
                "diagram.png",
                None,
                None,
            ),
        )
        .unwrap();
        apply_core_event(
            &mut state,
            room_image_media_event(
                "!ops:example",
                "event:$image1:example.org",
                "$image1:example.org",
                RoomImageMediaKind::Loaded {
                    format: timeline_thumbnail_format(),
                    bytes: vec![0, 1, 2, 3],
                },
            ),
        )
        .unwrap();

        let rows = build_room_timeline_rows(&state.model, "!ops:example");
        let image_row = rows
            .iter()
            .find(|row| matches!(row.kind, TimelineRowKind::Image))
            .expect("image row must be present");
        assert!(matches!(
            image_row.image_state,
            Some(TimelineImageState::Failed { .. })
        ));
    }

    #[test]
    fn loaded_room_image_media_event_projects_loaded_image_state() {
        let mut state = test_state();
        state.model.rooms = vec![room("!ops:example", "Ops", false)];
        apply_core_event(
            &mut state,
            image_message(
                "!ops:example",
                "$image1:example.org",
                "@alice:example.org",
                1_710_000_000_000,
                "diagram.png",
                None,
                None,
            ),
        )
        .unwrap();
        apply_core_event(
            &mut state,
            room_image_media_event(
                "!ops:example",
                "event:$image1:example.org",
                "$image1:example.org",
                RoomImageMediaKind::Loaded {
                    format: timeline_thumbnail_format(),
                    bytes: tiny_png_bytes(),
                },
            ),
        )
        .unwrap();

        let image_key = room_image_media_storage_key(
            "!ops:example",
            "event:$image1:example.org",
            "$image1:example.org",
            &timeline_thumbnail_format(),
        );
        assert_eq!(
            state
                .model
                .room_views
                .get("!ops:example")
                .and_then(|room_view| room_view.image_media.get("event:$image1:example.org"))
                .cloned(),
            Some(RoomImageMediaState::Loaded {
                event_id: "$image1:example.org".to_owned(),
                format: timeline_thumbnail_format(),
                image_key: image_key.clone(),
                width: 1,
                height: 1,
            })
        );
        assert!(state.decoded_images.contains_key(&image_key));

        let rows = build_room_timeline_rows(&state.model, "!ops:example");
        let image_row = rows
            .iter()
            .find(|row| matches!(row.kind, TimelineRowKind::Image))
            .expect("image row must be present");
        assert_eq!(
            image_row.image_state,
            Some(TimelineImageState::Loaded {
                key: image_key,
                width: 1,
                height: 1,
            })
        );
    }

    #[test]
    fn thread_image_rows_use_room_backed_image_media_state() {
        let mut state = test_state();
        state.model.rooms = vec![room("!ops:example", "Ops", false)];
        let mut activity = image_activity(
            "!ops:example",
            "$thread-image:example.org",
            "@alice:example.org",
            1_710_000_000_000,
            "diagram.png",
            None,
            None,
        );
        activity.relation = Some(thread_relation("$thread-root:example.org"));
        apply_core_event(&mut state, Event::RoomActivity(Box::new(activity))).unwrap();
        apply_core_event(
            &mut state,
            room_image_media_event(
                "!ops:example",
                "event:$thread-image:example.org",
                "$thread-image:example.org",
                RoomImageMediaKind::Failed {
                    format: timeline_thumbnail_format(),
                    message: "unavailable".to_owned(),
                },
            ),
        )
        .unwrap();

        let rows =
            build_thread_timeline_rows(&state.model, "!ops:example", "$thread-root:example.org");
        let image_row = rows
            .iter()
            .find(|row| matches!(row.kind, TimelineRowKind::Image))
            .expect("thread image row must be present");
        assert_eq!(
            image_row.image_state,
            Some(TimelineImageState::Failed {
                message: "unavailable".to_owned(),
            })
        );
    }

    #[test]
    fn room_image_media_dispatch_records_one_thumbnail_fetch_and_dedupes_pending() {
        let (mut state, handle) = test_state_with_command_sender();
        state.model.rooms = vec![room("!ops:example", "Ops", false)];
        apply_core_event(
            &mut state,
            image_message(
                "!ops:example",
                "$image1:example.org",
                "@alice:example.org",
                1_710_000_000_000,
                "diagram.png",
                None,
                None,
            ),
        )
        .unwrap();

        let requests = {
            let room_view = state
                .model
                .room_views
                .get("!ops:example")
                .expect("room view must exist");
            collect_lazy_room_image_media_requests(room_view, room_view.activities.iter())
        };
        dispatch_lazy_room_image_media_requests(&mut state, "!ops:example", requests).unwrap();
        assert_eq!(
            state.recorded_commands,
            vec![Command::FetchRoomImageMedia(FetchRoomImageMediaRequest {
                room_id: "!ops:example".to_owned(),
                activity_id: "event:$image1:example.org".to_owned(),
                event_id: "$image1:example.org".to_owned(),
                source: RoomMediaSource::Plain {
                    url: "mxc://example.org/diagram".to_owned(),
                },
                format: timeline_thumbnail_format(),
            })]
        );

        let requests = {
            let room_view = state
                .model
                .room_views
                .get("!ops:example")
                .expect("room view must exist");
            collect_lazy_room_image_media_requests(room_view, room_view.activities.iter())
        };
        assert!(requests.is_empty());

        stop_test_core(handle);
    }

    #[test]
    fn room_image_media_dispatch_without_command_sender_keeps_pending_state() {
        let mut state = test_state();
        state.model.rooms = vec![room("!ops:example", "Ops", false)];
        apply_core_event(
            &mut state,
            image_message(
                "!ops:example",
                "$image1:example.org",
                "@alice:example.org",
                1_710_000_000_000,
                "diagram.png",
                None,
                None,
            ),
        )
        .unwrap();

        let requests = {
            let room_view = state
                .model
                .room_views
                .get("!ops:example")
                .expect("room view must exist");
            collect_lazy_room_image_media_requests(room_view, room_view.activities.iter())
        };
        dispatch_lazy_room_image_media_requests(&mut state, "!ops:example", requests).unwrap();

        assert_eq!(
            state
                .model
                .room_views
                .get("!ops:example")
                .and_then(|room_view| room_view.image_media.get("event:$image1:example.org"))
                .cloned(),
            Some(RoomImageMediaState::Pending {
                event_id: "$image1:example.org".to_owned(),
                format: timeline_thumbnail_format(),
            })
        );
        assert!(state.recorded_commands.is_empty());
    }

    #[test]
    fn thread_image_media_dispatch_uses_room_backed_pending_state() {
        let (mut state, handle) = test_state_with_command_sender();
        state.model.rooms = vec![room("!ops:example", "Ops", false)];
        let mut activity = image_activity(
            "!ops:example",
            "$thread-image:example.org",
            "@alice:example.org",
            1_710_000_000_000,
            "diagram.png",
            None,
            None,
        );
        activity.relation = Some(thread_relation("$thread-root:example.org"));
        apply_core_event(&mut state, Event::RoomActivity(Box::new(activity))).unwrap();

        let requests = {
            let room_view = state
                .model
                .room_views
                .get("!ops:example")
                .expect("room view must exist");
            collect_lazy_room_image_media_requests(
                room_view,
                room_view.activities.iter().filter(|activity| {
                    room_activity_belongs_to_thread(activity, "$thread-root:example.org")
                }),
            )
        };
        dispatch_lazy_room_image_media_requests(&mut state, "!ops:example", requests).unwrap();
        assert_eq!(state.recorded_commands.len(), 1);

        let room_requests = {
            let room_view = state
                .model
                .room_views
                .get("!ops:example")
                .expect("room view must exist");
            collect_lazy_room_image_media_requests(room_view, room_view.activities.iter())
        };
        assert!(room_requests.is_empty());

        stop_test_core(handle);
    }

    #[test]
    fn duplicate_lazy_image_requests_in_single_dispatch_send_once() {
        let (mut state, handle) = test_state_with_command_sender();
        state.model.rooms = vec![room("!ops:example", "Ops", false)];
        apply_core_event(
            &mut state,
            image_message(
                "!ops:example",
                "$image1:example.org",
                "@alice:example.org",
                1_710_000_000_000,
                "diagram.png",
                None,
                None,
            ),
        )
        .unwrap();

        let duplicate_request = RoomImageMediaDispatch {
            activity_id: "event:$image1:example.org".to_owned(),
            event_id: "$image1:example.org".to_owned(),
            source: RoomMediaSource::Plain {
                url: "mxc://example.org/diagram".to_owned(),
            },
            format: timeline_thumbnail_format(),
        };
        dispatch_lazy_room_image_media_requests(
            &mut state,
            "!ops:example",
            vec![duplicate_request.clone(), duplicate_request],
        )
        .unwrap();

        assert_eq!(state.recorded_commands.len(), 1);

        stop_test_core(handle);
    }

    #[test]
    fn loaded_or_failed_image_media_state_skips_additional_fetch_dispatch() {
        let (mut state, handle) = test_state_with_command_sender();
        state.model.rooms = vec![room("!ops:example", "Ops", false)];
        apply_core_event(
            &mut state,
            image_message(
                "!ops:example",
                "$image1:example.org",
                "@alice:example.org",
                1_710_000_000_000,
                "diagram.png",
                None,
                None,
            ),
        )
        .unwrap();
        state
            .model
            .room_views
            .get_mut("!ops:example")
            .expect("room view must exist")
            .insert_image_media(
                "event:$image1:example.org".to_owned(),
                RoomImageMediaState::Loaded {
                    event_id: "$image1:example.org".to_owned(),
                    format: timeline_thumbnail_format(),
                    image_key: "loaded-key".to_owned(),
                    width: 64,
                    height: 48,
                },
            );

        let requests = {
            let room_view = state
                .model
                .room_views
                .get("!ops:example")
                .expect("room view must exist");
            collect_lazy_room_image_media_requests(room_view, room_view.activities.iter())
        };
        assert!(requests.is_empty());

        apply_core_event(
            &mut state,
            room_image_media_event(
                "!ops:example",
                "event:$image1:example.org",
                "$image1:example.org",
                RoomImageMediaKind::Failed {
                    format: timeline_thumbnail_format(),
                    message: "unavailable".to_owned(),
                },
            ),
        )
        .unwrap();
        let requests = {
            let room_view = state
                .model
                .room_views
                .get("!ops:example")
                .expect("room view must exist");
            collect_lazy_room_image_media_requests(room_view, room_view.activities.iter())
        };
        assert!(requests.is_empty());

        stop_test_core(handle);
    }

    #[test]
    fn encrypted_image_with_thumbnail_source_queues_thumbnail_request() {
        let (mut state, handle) = test_state_with_command_sender();
        state.model.rooms = vec![room("!ops:example", "Ops", false)];
        state
            .model
            .room_views
            .entry("!ops:example".to_owned())
            .or_default()
            .apply_activity(encrypted_image_activity(
                "!ops:example",
                "$image1:example.org",
                "@alice:example.org",
                1_710_000_000_000,
                "diagram.png",
                Some(encrypted_media_source("mxc://example.org/encrypted-thumb")),
            ));

        let requests = {
            let room_view = state
                .model
                .room_views
                .get("!ops:example")
                .expect("room view must exist");
            collect_lazy_room_image_media_requests(room_view, room_view.activities.iter())
        };
        dispatch_lazy_room_image_media_requests(&mut state, "!ops:example", requests).unwrap();

        assert_eq!(
            state.recorded_commands,
            vec![Command::FetchRoomImageMedia(FetchRoomImageMediaRequest {
                room_id: "!ops:example".to_owned(),
                activity_id: "event:$image1:example.org".to_owned(),
                event_id: "$image1:example.org".to_owned(),
                source: encrypted_media_source("mxc://example.org/encrypted-thumb"),
                format: timeline_thumbnail_format(),
            })]
        );

        stop_test_core(handle);
    }

    #[test]
    fn encrypted_image_without_thumbnail_source_queues_no_request() {
        let mut state = test_state();
        state.model.rooms = vec![room("!ops:example", "Ops", false)];
        state
            .model
            .room_views
            .entry("!ops:example".to_owned())
            .or_default()
            .apply_activity(encrypted_image_activity(
                "!ops:example",
                "$image1:example.org",
                "@alice:example.org",
                1_710_000_000_000,
                "diagram.png",
                None,
            ));

        let room_view = state
            .model
            .room_views
            .get("!ops:example")
            .expect("room view must exist");
        let requests =
            collect_lazy_room_image_media_requests(room_view, room_view.activities.iter());
        assert!(requests.is_empty());
    }

    #[test]
    fn plain_image_without_thumbnail_source_queues_original_source_request() {
        let (mut state, handle) = test_state_with_command_sender();
        state.model.rooms = vec![room("!ops:example", "Ops", false)];
        apply_core_event(
            &mut state,
            image_message(
                "!ops:example",
                "$image1:example.org",
                "@alice:example.org",
                1_710_000_000_000,
                "diagram.png",
                Some("diagram.png"),
                Some(RoomImageInfo {
                    mimetype: Some("image/png".to_owned()),
                    size: Some(12_345),
                    width: Some(640),
                    height: Some(480),
                    is_animated: Some(false),
                    thumbnail_source: None,
                }),
            ),
        )
        .unwrap();

        let requests = {
            let room_view = state
                .model
                .room_views
                .get("!ops:example")
                .expect("room view must exist");
            collect_lazy_room_image_media_requests(room_view, room_view.activities.iter())
        };
        dispatch_lazy_room_image_media_requests(&mut state, "!ops:example", requests).unwrap();

        assert_eq!(
            state.recorded_commands,
            vec![Command::FetchRoomImageMedia(FetchRoomImageMediaRequest {
                room_id: "!ops:example".to_owned(),
                activity_id: "event:$image1:example.org".to_owned(),
                event_id: "$image1:example.org".to_owned(),
                source: RoomMediaSource::Plain {
                    url: "mxc://example.org/diagram".to_owned(),
                },
                format: timeline_thumbnail_format(),
            })]
        );

        stop_test_core(handle);
    }

    #[test]
    fn plain_image_with_thumbnail_source_queues_thumbnail_request() {
        let (mut state, handle) = test_state_with_command_sender();
        state.model.rooms = vec![room("!ops:example", "Ops", false)];
        apply_core_event(
            &mut state,
            image_message(
                "!ops:example",
                "$image1:example.org",
                "@alice:example.org",
                1_710_000_000_000,
                "diagram.png",
                Some("diagram.png"),
                Some(RoomImageInfo {
                    mimetype: Some("image/png".to_owned()),
                    size: Some(12_345),
                    width: Some(640),
                    height: Some(480),
                    is_animated: Some(false),
                    thumbnail_source: Some(RoomMediaSource::Plain {
                        url: "mxc://example.org/diagram-thumb".to_owned(),
                    }),
                }),
            ),
        )
        .unwrap();

        let requests = {
            let room_view = state
                .model
                .room_views
                .get("!ops:example")
                .expect("room view must exist");
            collect_lazy_room_image_media_requests(room_view, room_view.activities.iter())
        };
        dispatch_lazy_room_image_media_requests(&mut state, "!ops:example", requests).unwrap();

        assert_eq!(
            state.recorded_commands,
            vec![Command::FetchRoomImageMedia(FetchRoomImageMediaRequest {
                room_id: "!ops:example".to_owned(),
                activity_id: "event:$image1:example.org".to_owned(),
                event_id: "$image1:example.org".to_owned(),
                source: RoomMediaSource::Plain {
                    url: "mxc://example.org/diagram-thumb".to_owned(),
                },
                format: timeline_thumbnail_format(),
            })]
        );

        stop_test_core(handle);
    }

    #[test]
    fn timeline_row_paint_lines_expand_image_slots_without_changing_logical_text_rows() {
        let mut image_row = TimelineRow::log(
            TimelineRowKind::Image,
            "* [16:00] @alice:example.org sent image: diagram.png".to_owned(),
            "[16:00]".to_owned(),
            "* @alice:example.org".to_owned(),
            "sent image: diagram.png".to_owned(),
        );
        image_row.image_state = Some(TimelineImageState::Pending);
        let rows = vec![
            image_row,
            TimelineRow::log(
                TimelineRowKind::Unsupported,
                "* [16:01] @alice:example.org sent unsupported m.video: clip.mp4".to_owned(),
                "[16:01]".to_owned(),
                "* @alice:example.org".to_owned(),
                "sent unsupported m.video: clip.mp4".to_owned(),
            ),
        ];

        let logical_lines = project_timeline_rows(&rows);
        let paint_lines = timeline_row_paint_lines_with_measure(&rows, 360, &measure_ascii_width);

        assert_eq!(
            logical_lines,
            vec![
                "* [16:00] @alice:example.org sent image: diagram.png".to_owned(),
                "* [16:01] @alice:example.org sent unsupported m.video: clip.mp4".to_owned(),
            ]
        );
        assert_eq!(
            paint_lines
                .iter()
                .filter(|line| line.image_slot_index.is_none())
                .map(|line| match &line.visual {
                    TimelinePaintLineVisual::Log {
                        timestamp,
                        sender_prefix,
                        body,
                    } => format!(
                        "{}|{}|{}",
                        timestamp.as_deref().unwrap_or(""),
                        sender_prefix.as_deref().unwrap_or(""),
                        body
                    ),
                    TimelinePaintLineVisual::FullWidth(text) => text.clone(),
                    TimelinePaintLineVisual::ImageSlot => String::new(),
                })
                .collect::<Vec<_>>(),
            vec![
                "[16:00]|* @alice:example.org|sent image: diagram.png".to_owned(),
                "[16:01]|* @alice:example.org|sent unsupported m.video: clip.mp4".to_owned(),
            ]
        );
        assert_eq!(
            paint_lines
                .iter()
                .filter(|line| line.image_slot_index.is_some())
                .count(),
            TIMELINE_IMAGE_MEDIA_SLOT_LINES
        );
        assert_eq!(clamped_timeline_top_line(6, paint_lines.len(), 3), 6);
    }

    #[test]
    fn timeline_row_paint_lines_wrap_log_rows_without_changing_logical_projection() {
        let rows = vec![TimelineRow::log(
            TimelineRowKind::Message {
                presentation: RoomMessagePresentationKind::Text,
                send_state: TimelineMessageSendState::Sent,
            },
            "[22:13] @alice:example: alpha beta gamma".to_owned(),
            "[22:13]".to_owned(),
            "@alice:example:".to_owned(),
            "alpha beta gamma".to_owned(),
        )];

        let paint_lines = timeline_row_paint_lines_with_measure(&rows, 280, &measure_ascii_width);

        assert_eq!(
            project_timeline_rows(&rows),
            vec!["[22:13] @alice:example: alpha beta gamma".to_owned()]
        );
        assert_eq!(paint_lines.len(), 3);
        assert_eq!(
            paint_lines[0].visual.clone(),
            TimelinePaintLineVisual::Log {
                timestamp: Some("[22:13]".to_owned()),
                sender_prefix: Some("@alice:example:".to_owned()),
                body: "alpha".to_owned(),
            }
        );
        assert_eq!(
            paint_lines[1].visual.clone(),
            TimelinePaintLineVisual::Log {
                timestamp: None,
                sender_prefix: None,
                body: " beta".to_owned(),
            }
        );
        assert_eq!(
            paint_lines[2].visual.clone(),
            TimelinePaintLineVisual::Log {
                timestamp: None,
                sender_prefix: None,
                body: " gamma".to_owned(),
            }
        );
    }

    #[test]
    fn timeline_row_paint_lines_hard_wrap_unbroken_tokens() {
        let rows = vec![TimelineRow::log(
            TimelineRowKind::Message {
                presentation: RoomMessagePresentationKind::Text,
                send_state: TimelineMessageSendState::Sent,
            },
            "[22:13] @alice:example: abcdefghijklmno".to_owned(),
            "[22:13]".to_owned(),
            "@alice:example:".to_owned(),
            "abcdefghijklmno".to_owned(),
        )];

        let paint_lines = timeline_row_paint_lines_with_measure(&rows, 276, &measure_ascii_width);

        assert_eq!(
            paint_lines
                .into_iter()
                .map(|line| match line.visual {
                    TimelinePaintLineVisual::Log {
                        timestamp,
                        sender_prefix,
                        body,
                    } => (timestamp.is_some(), sender_prefix.is_some(), body,),
                    _ => unreachable!("expected log paint lines"),
                })
                .collect::<Vec<_>>(),
            vec![
                (true, true, "abcd".to_owned()),
                (false, false, "efgh".to_owned()),
                (false, false, "ijkl".to_owned()),
                (false, false, "mno".to_owned()),
            ]
        );
    }

    #[test]
    fn timeline_row_paint_lines_preserve_image_slots_after_wrapped_caption_lines() {
        let mut row = TimelineRow::log(
            TimelineRowKind::Image,
            "* [16:00] @alice:example.org sent image: alpha beta gamma".to_owned(),
            "[16:00]".to_owned(),
            "* @alice:example.org".to_owned(),
            "sent image: alpha beta gamma".to_owned(),
        );
        row.image_state = Some(TimelineImageState::Pending);

        let paint_lines = timeline_row_paint_lines_with_measure(&[row], 280, &measure_ascii_width);
        let caption_lines = paint_lines
            .iter()
            .take_while(|line| line.image_slot_index.is_none())
            .collect::<Vec<_>>();
        let image_slot_lines = paint_lines
            .iter()
            .skip(caption_lines.len())
            .collect::<Vec<_>>();

        assert!(
            caption_lines.len() > 1,
            "caption should wrap before image slots"
        );
        assert_eq!(image_slot_lines.len(), TIMELINE_IMAGE_MEDIA_SLOT_LINES,);
        assert!(
            image_slot_lines
                .iter()
                .all(|line| matches!(&line.visual, TimelinePaintLineVisual::ImageSlot))
        );
    }

    #[test]
    fn clamped_timeline_top_line_respects_wrapped_visual_line_growth() {
        let rows = vec![TimelineRow::log(
            TimelineRowKind::Message {
                presentation: RoomMessagePresentationKind::Text,
                send_state: TimelineMessageSendState::Sent,
            },
            "[22:13] @alice:example: alpha beta gamma delta".to_owned(),
            "[22:13]".to_owned(),
            "@alice:example:".to_owned(),
            "alpha beta gamma delta".to_owned(),
        )];
        let paint_lines = timeline_row_paint_lines_with_measure(&rows, 280, &measure_ascii_width);

        assert_eq!(paint_lines.len(), 4);
        assert_eq!(clamped_timeline_top_line(8, paint_lines.len(), 3), 1);
    }

    #[test]
    fn room_typing_replaces_prior_user_list_and_empty_list_clears_it() {
        let mut state = test_state();

        apply_core_event(
            &mut state,
            typing_event("!ops:example", &["@alice:example.org"]),
        )
        .unwrap();
        assert_eq!(
            state
                .model
                .room_views
                .get("!ops:example")
                .map(|view| view.typing_user_ids.clone()),
            Some(vec!["@alice:example.org".to_owned()])
        );

        apply_core_event(
            &mut state,
            typing_event("!ops:example", &["@alice:example.org", "@bob:example.org"]),
        )
        .unwrap();
        assert_eq!(
            state
                .model
                .room_views
                .get("!ops:example")
                .map(|view| view.typing_user_ids.clone()),
            Some(vec![
                "@alice:example.org".to_owned(),
                "@bob:example.org".to_owned(),
            ])
        );

        apply_core_event(&mut state, typing_event("!ops:example", &[])).unwrap();
        assert_eq!(
            state
                .model
                .room_views
                .get("!ops:example")
                .map(|view| view.typing_user_ids.clone()),
            Some(Vec::new())
        );
    }

    #[test]
    fn render_room_lines_use_singular_typing_verb_for_one_user() {
        let mut state = test_state();
        state.model.rooms = vec![room("!ops:example", "Ops", false)];

        apply_core_event(
            &mut state,
            typing_event("!ops:example", &["@alice:example.org"]),
        )
        .unwrap();

        assert_eq!(
            render_room_lines(&state.model, "!ops:example"),
            vec![
                "Room: Ops (!ops:example)".to_owned(),
                "* @alice:example.org is typing".to_owned(),
            ]
        );
    }

    #[test]
    fn room_typing_does_not_change_last_activity_or_room_activity_markers() {
        let mut state = test_state();
        state.model.rooms = vec![room("!ops:example", "Ops", false)];
        state.active_view = ViewId::Status;

        apply_core_event(
            &mut state,
            typing_event("!ops:example", &["@alice:example.org"]),
        )
        .unwrap();

        assert_eq!(state.model.last_activity, None);
        assert!(state.room_activity.is_empty());
        assert_eq!(state.model.status, "offline");
    }

    #[test]
    fn verification_request_event_stores_requested_state_and_renders_actions() {
        let mut state = test_state();

        apply_core_event(
            &mut state,
            Event::VerificationRequest(VerificationRequest {
                flow_id: "flow-verify-123".to_owned(),
                user_id: "@alice:example".to_owned(),
            }),
        )
        .unwrap();

        assert_eq!(
            state.model.last_verification,
            Some(VerificationStateEvent {
                flow_id: "flow-verify-123".to_owned(),
                user_id: "@alice:example".to_owned(),
                state: VerificationState::Requested,
            })
        );

        let status_text = render_view_text(&render_status_lines(&state.model));
        assert!(status_text.contains("state: requested"));
        assert!(status_text.contains("flow_id: flow-verify-123"));
        assert!(status_text.contains("from_user: @alice:example"));
        assert!(status_text.contains("actions: /verify accept, /verify cancel"));
    }

    #[test]
    fn verification_sas_ready_renders_emoji_decimals_and_error_details() {
        let mut state = test_state();
        state.model.status = "verification sas ready with @alice:example".to_owned();
        state.model.last_verification = Some(VerificationStateEvent {
            flow_id: "flow-verify-123".to_owned(),
            user_id: "@alice:example".to_owned(),
            state: VerificationState::SasReady {
                emojis: vec![
                    VerificationEmoji {
                        symbol: "🐶".to_owned(),
                        description: "dog".to_owned(),
                    },
                    VerificationEmoji {
                        symbol: "🔥".to_owned(),
                        description: "fire".to_owned(),
                    },
                ],
                decimals: mmatrix_core::VerificationDecimals {
                    first: 1234,
                    second: 5678,
                    third: 9012,
                },
            },
        });
        state.model.last_error = Some(CoreErrorEvent {
            operation: Operation::SendMessage,
            message: "message send failed".to_owned(),
        });

        let status_text = render_view_text(&render_status_lines(&state.model));
        assert!(status_text.contains("state: sas ready"));
        assert!(status_text.contains("emoji: 🐶 dog | 🔥 fire"));
        assert!(status_text.contains("decimals: 1234-5678-9012"));
        assert!(status_text.contains("actions: /verify confirm, /verify reject, /verify cancel"));
        assert!(status_text.contains("operation: send text"));
        assert!(status_text.contains("message: message send failed"));
    }

    #[test]
    fn e2ee_status_event_updates_model_and_renders_status_section() {
        let mut state = test_state();
        let status = e2ee_status(
            E2eeRecoveryState::Incomplete,
            E2eeBackupState::Enabled,
            false,
            Some(true),
            Some(false),
        );

        apply_core_event(&mut state, Event::E2eeStatus(status.clone())).unwrap();

        assert_eq!(state.model.last_e2ee_status, Some(status));
        assert_eq!(state.model.status, "e2ee status updated");

        let status_text = render_view_text(&render_status_lines(&state.model));
        assert!(status_text.contains("e2ee"));
        assert!(status_text.contains("recovery: incomplete"));
        assert!(status_text.contains("backup: enabled"));
        assert!(status_text.contains("backup_enabled_local: false"));
        assert!(status_text.contains("backup_exists_on_server: true"));
        assert!(status_text.contains("cross_signing: incomplete"));
        assert!(status_text.contains("actions: Account > Recover E2EE..., /e2ee refresh"));
    }

    #[test]
    fn e2ee_recovery_succeeded_event_sets_status_message() {
        let mut state = test_state();

        apply_core_event(
            &mut state,
            Event::E2eeRecoverySucceeded(mmatrix_core::E2eeRecoverySucceeded {
                operation: mmatrix_core::E2eeRecoveryOperation::Recover,
            }),
        )
        .unwrap();

        assert_eq!(state.model.status, "e2ee recovery complete");
    }

    #[test]
    fn normalize_timeline_text_converts_embedded_newlines_to_crlf() {
        let text = "[22:13] @bob:example: multiline row 00 line a\nmultiline row 00 line b";
        let (normalized, lines) = normalize_timeline_text(text);

        assert_eq!(
            normalized,
            "[22:13] @bob:example: multiline row 00 line a\r\nmultiline row 00 line b"
        );
        assert_eq!(
            lines,
            vec![
                "[22:13] @bob:example: multiline row 00 line a".to_owned(),
                "multiline row 00 line b".to_owned(),
            ]
        );
    }

    #[test]
    fn normalize_timeline_text_preserves_trailing_blank_line() {
        let text = "trailing blank line a\n";
        let (normalized, lines) = normalize_timeline_text(text);

        assert_eq!(normalized, "trailing blank line a\r\n");
        assert_eq!(
            lines,
            vec!["trailing blank line a".to_owned(), String::new()]
        );
    }

    #[test]
    fn clamped_timeline_top_line_respects_visible_row_growth() {
        assert_eq!(clamped_timeline_top_line(8, 10, 3), 7);
        assert_eq!(clamped_timeline_top_line(8, 10, 5), 5);
        assert_eq!(clamped_timeline_top_line(2, 10, 20), 0);
    }

    #[test]
    fn ascii_find_matching_folds_ascii_only() {
        assert_eq!(fold_find_query("HeLLo Ä"), "hello Ä");
        assert!(ascii_case_insensitive_contains(
            "Hello world",
            &fold_find_query("hello")
        ));
        assert!(ascii_case_insensitive_contains(
            "AÄ",
            &fold_find_query("aÄ")
        ));
        assert!(!ascii_case_insensitive_contains("Ä", &fold_find_query("ä")));
    }

    #[test]
    fn ascii_find_utf16_range_counts_non_ascii_prefix_units() {
        assert_eq!(
            ascii_case_insensitive_utf16_range("🙂 Test λ", &fold_find_query("test")),
            Some((3, 7))
        );
    }

    #[test]
    fn status_find_matches_use_absolute_utf16_offsets_in_multiline_buffer() {
        let lines = vec![
            "alpha".to_owned(),
            "🙂 Test λ".to_owned(),
            "omega".to_owned(),
        ];

        let matches = compute_status_find_matches(&lines, &fold_find_query("test"));
        assert_eq!(
            matches,
            vec![FindMatch::Status(StatusFindMatch {
                line_index: 1,
                utf16_start: 10,
                utf16_end: 14,
                line_text: "🙂 Test λ".to_owned(),
            })]
        );
    }

    #[test]
    fn status_find_selection_uses_document_wide_caret_offsets() {
        let lines = vec![
            "alpha".to_owned(),
            "beta".to_owned(),
            "needle".to_owned(),
            "needle again".to_owned(),
        ];
        let matches = compute_status_find_matches(&lines, &fold_find_query("needle"));

        assert_eq!(
            choose_find_match(
                &matches,
                FindSelectionMode::Execute {
                    folded_query: "needle",
                    previous_state: None,
                    start: FindSelectionStart::StatusUtf16(13),
                },
            ),
            Some(0)
        );
        assert_eq!(
            choose_find_match(
                &matches,
                FindSelectionMode::Execute {
                    folded_query: "needle",
                    previous_state: None,
                    start: FindSelectionStart::StatusUtf16(20),
                },
            ),
            Some(1)
        );
    }

    #[test]
    fn timeline_find_matches_are_per_logical_row() {
        let rows = vec![
            TimelineRow::full_width(TimelineRowKind::Plain, "alpha needle needle".to_owned())
                .with_identity(TimelineRowIdentity::synthetic(
                    TimelineSyntheticRowLabel::Plain,
                    "alpha needle needle",
                )),
            TimelineRow::full_width(TimelineRowKind::Plain, "beta needle".to_owned())
                .with_identity(TimelineRowIdentity::synthetic(
                    TimelineSyntheticRowLabel::Plain,
                    "beta needle",
                )),
        ];

        let matches = compute_timeline_find_matches(&rows, &fold_find_query("needle"));
        assert_eq!(matches.len(), 2);
        assert_eq!(
            matches,
            vec![
                FindMatch::Timeline(TimelineFindMatch {
                    row_identity: TimelineRowIdentity::synthetic(
                        TimelineSyntheticRowLabel::Plain,
                        "alpha needle needle",
                    ),
                    source_row_index: 0,
                    logical_text: "alpha needle needle".to_owned(),
                }),
                FindMatch::Timeline(TimelineFindMatch {
                    row_identity: TimelineRowIdentity::synthetic(
                        TimelineSyntheticRowLabel::Plain,
                        "beta needle",
                    ),
                    source_row_index: 1,
                    logical_text: "beta needle".to_owned(),
                }),
            ]
        );
    }

    #[test]
    fn repeated_find_query_advances_and_wraps() {
        let matches = vec![
            FindMatch::Timeline(TimelineFindMatch {
                row_identity: TimelineRowIdentity::activity_primary("activity-1"),
                source_row_index: 1,
                logical_text: "first needle".to_owned(),
            }),
            FindMatch::Timeline(TimelineFindMatch {
                row_identity: TimelineRowIdentity::activity_primary("activity-2"),
                source_row_index: 4,
                logical_text: "second needle".to_owned(),
            }),
        ];
        let previous_state = build_view_find_state(
            "needle",
            &fold_find_query("needle"),
            matches.clone(),
            Some(1),
        );

        assert_eq!(
            choose_find_match(
                &matches,
                FindSelectionMode::Execute {
                    folded_query: "needle",
                    previous_state: Some(&previous_state),
                    start: FindSelectionStart::TimelineSourceRow(0),
                },
            ),
            Some(0)
        );
    }

    #[test]
    fn recompute_find_selection_preserves_timeline_identity_and_reselects_first_match_when_missing()
    {
        let previous_state = build_view_find_state(
            "needle",
            &fold_find_query("needle"),
            vec![
                FindMatch::Timeline(TimelineFindMatch {
                    row_identity: TimelineRowIdentity::activity_primary("activity-1"),
                    source_row_index: 0,
                    logical_text: "first needle".to_owned(),
                }),
                FindMatch::Timeline(TimelineFindMatch {
                    row_identity: TimelineRowIdentity::activity_primary("activity-2"),
                    source_row_index: 1,
                    logical_text: "second needle".to_owned(),
                }),
            ],
            Some(1),
        );
        let reordered_matches = vec![
            FindMatch::Timeline(TimelineFindMatch {
                row_identity: TimelineRowIdentity::activity_primary("activity-2"),
                source_row_index: 0,
                logical_text: "second needle".to_owned(),
            }),
            FindMatch::Timeline(TimelineFindMatch {
                row_identity: TimelineRowIdentity::activity_primary("activity-1"),
                source_row_index: 1,
                logical_text: "first needle".to_owned(),
            }),
        ];
        let missing_selected_row_matches = vec![FindMatch::Timeline(TimelineFindMatch {
            row_identity: TimelineRowIdentity::activity_primary("activity-1"),
            source_row_index: 0,
            logical_text: "first needle".to_owned(),
        })];

        assert_eq!(
            choose_find_match(
                &reordered_matches,
                FindSelectionMode::Recompute {
                    previous_state: &previous_state,
                },
            ),
            Some(0)
        );
        assert_eq!(
            choose_find_match(
                &missing_selected_row_matches,
                FindSelectionMode::Recompute {
                    previous_state: &previous_state,
                },
            ),
            Some(0)
        );
        assert_eq!(
            choose_find_match(
                &[],
                FindSelectionMode::Recompute {
                    previous_state: &previous_state,
                },
            ),
            None
        );
    }

    #[test]
    fn no_match_find_state_is_deterministic() {
        let state = build_view_find_state("missing", "missing", Vec::new(), None);

        assert!(state.matches.is_empty());
        assert_eq!(state.selected_match_index, None);
        assert_eq!(state.feedback, "find: no matches for \"missing\"");
    }

    #[test]
    fn frame_status_text_returns_lifecycle_status_only_when_no_details_exist() {
        let mut state = test_state();
        state.model.status = "authenticated".to_owned();

        assert_eq!(frame_status_text(&state), "authenticated");
    }

    #[test]
    fn frame_status_text_appends_e2ee_suffix_only_after_status_event() {
        let mut state = test_state();
        state.model.status = "authenticated".to_owned();

        assert_eq!(frame_status_text(&state), "authenticated");

        apply_core_event(
            &mut state,
            Event::E2eeStatus(e2ee_status(
                E2eeRecoveryState::Incomplete,
                E2eeBackupState::Enabled,
                true,
                Some(true),
                Some(true),
            )),
        )
        .unwrap();

        assert_eq!(
            frame_status_text(&state),
            "e2ee status updated | e2ee: recovery incomplete, backup enabled"
        );
    }

    #[test]
    fn frame_status_text_includes_flow_user_and_compact_state() {
        let mut state = test_state();
        state.model.status = "verification accepted with @alice:example".to_owned();
        state.model.last_verification = Some(VerificationStateEvent {
            flow_id: "flow-verify-123".to_owned(),
            user_id: "@alice:example".to_owned(),
            state: VerificationState::Accepted,
        });

        assert_eq!(
            frame_status_text(&state),
            "verification accepted with @alice:example | verification: accepted flow-verify-123 from @alice:example"
        );
    }

    #[test]
    fn frame_status_text_appends_error_details() {
        let mut state = test_state();
        state.model.status = "error during send text".to_owned();
        state.model.last_error = Some(CoreErrorEvent {
            operation: Operation::SendMessage,
            message: "message send failed".to_owned(),
        });

        assert_eq!(
            frame_status_text(&state),
            "error during send text | error: send text: message send failed"
        );
    }

    #[test]
    fn frame_status_text_appends_active_view_find_feedback_without_mutating_status() {
        let mut state = test_state();
        state.model.status = "authenticated".to_owned();
        state.find_states.insert(
            ViewId::Status,
            build_view_find_state("hello", "hello", Vec::new(), None),
        );

        assert_eq!(
            frame_status_text(&state),
            "authenticated | find: no matches for \"hello\""
        );
        assert_eq!(state.model.status, "authenticated");
    }

    #[test]
    fn frame_status_text_shows_active_invite_context() {
        let mut state = test_state();
        state.model.status = "synced 1 room(s)".to_owned();
        state.model.rooms = vec![invited_room("!invite:example", "Invite")];
        state.active_view = room_view_id("!invite:example");

        assert_eq!(
            frame_status_text(&state),
            "invited: Invite (!invite:example)"
        );
    }

    #[test]
    fn submission_status_survives_matching_error_event() {
        let mut state = test_state();
        state.model.status = "join submitted: #ops:example.org".to_owned();

        apply_core_event(
            &mut state,
            Event::Error(CoreErrorEvent {
                operation: Operation::JoinRoom,
                message: "join room requires an authenticated session".to_owned(),
            }),
        )
        .unwrap();

        assert_eq!(state.model.status, "join submitted: #ops:example.org");
        assert_eq!(state.model.lifecycle_state, LifecycleState::Error);
        assert_eq!(
            state.model.last_error,
            Some(CoreErrorEvent {
                operation: Operation::JoinRoom,
                message: "join room requires an authenticated session".to_owned(),
            })
        );

        state.model.status = "accept submitted: !invite:example".to_owned();

        apply_core_event(
            &mut state,
            Event::Error(CoreErrorEvent {
                operation: Operation::AcceptRoomInvite,
                message: "accept room invitation requires an authenticated session".to_owned(),
            }),
        )
        .unwrap();

        assert_eq!(state.model.status, "accept submitted: !invite:example");
        assert_eq!(state.model.lifecycle_state, LifecycleState::Error);
        assert_eq!(
            state.model.last_error,
            Some(CoreErrorEvent {
                operation: Operation::AcceptRoomInvite,
                message: "accept room invitation requires an authenticated session".to_owned(),
            })
        );

        state.model.status = "part submitted: !beta:example".to_owned();

        apply_core_event(
            &mut state,
            Event::Error(CoreErrorEvent {
                operation: Operation::LeaveRoom,
                message: "leave room requires an authenticated session".to_owned(),
            }),
        )
        .unwrap();

        assert_eq!(state.model.status, "part submitted: !beta:example");
        assert_eq!(state.model.lifecycle_state, LifecycleState::Error);
        assert_eq!(
            state.model.last_error,
            Some(CoreErrorEvent {
                operation: Operation::LeaveRoom,
                message: "leave room requires an authenticated session".to_owned(),
            })
        );

        state.model.status = "reject submitted: !invite:example".to_owned();

        apply_core_event(
            &mut state,
            Event::Error(CoreErrorEvent {
                operation: Operation::RejectRoomInvite,
                message: "reject room invitation requires an authenticated session".to_owned(),
            }),
        )
        .unwrap();

        assert_eq!(state.model.status, "reject submitted: !invite:example");
        assert_eq!(state.model.lifecycle_state, LifecycleState::Error);
        assert_eq!(
            state.model.last_error,
            Some(CoreErrorEvent {
                operation: Operation::RejectRoomInvite,
                message: "reject room invitation requires an authenticated session".to_owned(),
            })
        );

        state.model.status = "query submitted: @alice:example.org".to_owned();

        apply_core_event(
            &mut state,
            Event::Error(CoreErrorEvent {
                operation: Operation::CreateDirectRoom,
                message: "create direct room requires an authenticated session".to_owned(),
            }),
        )
        .unwrap();

        assert_eq!(state.model.status, "query submitted: @alice:example.org");
        assert_eq!(state.model.lifecycle_state, LifecycleState::Error);
        assert_eq!(
            state.model.last_error,
            Some(CoreErrorEvent {
                operation: Operation::CreateDirectRoom,
                message: "create direct room requires an authenticated session".to_owned(),
            })
        );

        state.model.status = "topic change submitted for !ops:example.org".to_owned();

        apply_core_event(
            &mut state,
            Event::Error(CoreErrorEvent {
                operation: Operation::SetRoomTopic,
                message: "set room topic requires an authenticated session".to_owned(),
            }),
        )
        .unwrap();

        assert_eq!(
            state.model.status,
            "topic change submitted for !ops:example.org"
        );
        assert_eq!(state.model.lifecycle_state, LifecycleState::Error);
        assert_eq!(
            state.model.last_error,
            Some(CoreErrorEvent {
                operation: Operation::SetRoomTopic,
                message: "set room topic requires an authenticated session".to_owned(),
            })
        );
    }

    #[test]
    fn frame_status_text_appends_verification_before_error_when_both_exist() {
        let mut state = test_state();
        state.model.status = "verification accepted with @alice:example".to_owned();
        state.model.last_verification = Some(VerificationStateEvent {
            flow_id: "flow-verify-123".to_owned(),
            user_id: "@alice:example".to_owned(),
            state: VerificationState::Accepted,
        });
        state.model.last_error = Some(CoreErrorEvent {
            operation: Operation::SendMessage,
            message: "message send failed".to_owned(),
        });

        assert_eq!(
            frame_status_text(&state),
            "verification accepted with @alice:example | verification: accepted flow-verify-123 from @alice:example | error: send text: message send failed"
        );
    }

    #[test]
    fn verification_done_renders_terminal_text_without_actions() {
        let mut state = test_state();
        state.model.status = "verification done with @alice:example".to_owned();
        state.model.last_verification = Some(VerificationStateEvent {
            flow_id: "flow-verify-123".to_owned(),
            user_id: "@alice:example".to_owned(),
            state: VerificationState::Done,
        });

        let status_text = render_view_text(&render_status_lines(&state.model));
        assert!(status_text.contains("state: done"));
        assert!(status_text.contains("result: verified"));
        assert!(!status_text.contains("/verify confirm"));
        assert!(!status_text.contains("/verify reject"));
        assert!(!status_text.contains("/verify cancel"));
    }

    #[test]
    fn verification_cancelled_renders_reason_code_without_actions() {
        let mut state = test_state();
        state.model.status = "verification cancelled with @alice:example".to_owned();
        state.model.last_verification = Some(VerificationStateEvent {
            flow_id: "flow-verify-123".to_owned(),
            user_id: "@alice:example".to_owned(),
            state: VerificationState::Cancelled {
                reason: "timed out".to_owned(),
                code: "m.timeout".to_owned(),
                cancelled_by_us: false,
            },
        });

        let status_text = render_view_text(&render_status_lines(&state.model));
        assert!(status_text.contains("state: cancelled"));
        assert!(status_text.contains("reason: timed out"));
        assert!(status_text.contains("code: m.timeout"));
        assert!(!status_text.contains("/verify confirm"));
        assert!(!status_text.contains("/verify reject"));
        assert!(!status_text.contains("/verify cancel"));
    }

    #[test]
    fn resolve_active_view_preserves_existing_active_room() {
        let model = UiModel {
            rooms: vec![
                room("!keep:example", "Keep", false),
                room("!other:example", "Other", false),
            ],
            ..UiModel::default()
        };

        assert_eq!(
            resolve_active_view(
                &ViewId::Room("!keep:example".to_owned()),
                Some("!other:example"),
                &model,
            ),
            ViewId::Room("!keep:example".to_owned())
        );
    }

    #[test]
    fn resolve_active_view_falls_back_to_first_room_when_active_room_is_stale() {
        let model = UiModel {
            rooms: vec![
                room("!alpha:example", "Alpha", false),
                room("!beta:example", "Beta", false),
            ],
            ..UiModel::default()
        };

        assert_eq!(
            resolve_active_view(&ViewId::Room("!gone:example".to_owned()), None, &model),
            ViewId::Room("!alpha:example".to_owned())
        );
    }

    #[test]
    fn resolve_active_view_skips_spaces_when_falling_back() {
        let model = UiModel {
            rooms: vec![
                space("!space:example", "Space"),
                room("!alpha:example", "Alpha", false),
                room("!beta:example", "Beta", false),
            ],
            ..UiModel::default()
        };

        assert_eq!(
            resolve_active_view(&ViewId::Room("!gone:example".to_owned()), None, &model),
            ViewId::Room("!alpha:example".to_owned())
        );
    }

    #[test]
    fn resolve_active_view_falls_back_to_status_when_no_rooms_remain() {
        let model = UiModel::default();
        assert_eq!(
            resolve_active_view(&ViewId::Room("!gone:example".to_owned()), None, &model),
            ViewId::Status
        );
    }

    #[test]
    fn resolve_active_view_preserves_existing_active_thread() {
        let mut room_view = RoomViewModel::default();
        room_view.threads.insert(
            "$thread-root".to_owned(),
            ThreadViewModel {
                label: "root msg".to_owned(),
            },
        );
        let model = UiModel {
            rooms: vec![room("!alpha:example", "Alpha", false)],
            room_views: BTreeMap::from([("!alpha:example".to_owned(), room_view)]),
            ..UiModel::default()
        };

        assert_eq!(
            resolve_active_view(
                &ViewId::Thread {
                    room_id: "!alpha:example".to_owned(),
                    thread_root_event_id: "$thread-root".to_owned(),
                },
                Some("!alpha:example"),
                &model,
            ),
            ViewId::Thread {
                room_id: "!alpha:example".to_owned(),
                thread_root_event_id: "$thread-root".to_owned(),
            }
        );
    }

    #[test]
    fn resolve_active_view_falls_back_to_parent_room_when_active_thread_is_stale() {
        let model = UiModel {
            rooms: vec![room("!alpha:example", "Alpha", false)],
            ..UiModel::default()
        };

        assert_eq!(
            resolve_active_view(
                &ViewId::Thread {
                    room_id: "!alpha:example".to_owned(),
                    thread_root_event_id: "$thread-root".to_owned(),
                },
                Some("!alpha:example"),
                &model,
            ),
            ViewId::Room("!alpha:example".to_owned())
        );
    }

    #[test]
    fn synced_rooms_preserves_only_valid_selection() {
        let mut state = test_state();
        state.model.selected_room = Some("!keep:example".to_owned());

        apply_core_event(
            &mut state,
            Event::SyncedRooms(vec![
                room("!keep:example", "Keep", false),
                room("!other:example", "Other", false),
            ]),
        )
        .unwrap();
        assert_eq!(state.model.selected_room, Some("!keep:example".to_owned()));

        apply_core_event(
            &mut state,
            Event::SyncedRooms(vec![room("!other:example", "Other", false)]),
        )
        .unwrap();
        assert_eq!(state.model.selected_room, None);
    }

    #[test]
    fn synced_rooms_clears_typing_for_non_actionable_rooms() {
        let mut state = test_state();

        apply_core_event(
            &mut state,
            typing_event("!keep:example", &["@alice:example.org"]),
        )
        .unwrap();
        apply_core_event(
            &mut state,
            typing_event("!drop:example", &["@bob:example.org"]),
        )
        .unwrap();

        apply_core_event(
            &mut state,
            Event::SyncedRooms(vec![room("!keep:example", "Keep", false)]),
        )
        .unwrap();

        assert_eq!(
            state
                .model
                .room_views
                .get("!keep:example")
                .map(|view| view.typing_user_ids.clone()),
            Some(vec!["@alice:example.org".to_owned()])
        );
        assert_eq!(
            state
                .model
                .room_views
                .get("!drop:example")
                .map(|view| view.typing_user_ids.clone()),
            Some(Vec::new())
        );
    }

    #[test]
    fn synced_rooms_clear_selected_room_when_room_becomes_space() {
        let mut state = test_state();
        state.model.selected_room = Some("!room:example".to_owned());

        apply_core_event(
            &mut state,
            Event::SyncedRooms(vec![space("!room:example", "Room Space")]),
        )
        .unwrap();

        assert_eq!(state.model.selected_room, None);
    }

    #[test]
    fn logged_out_clears_selection_timelines_and_diagnostics() {
        let mut state = test_state();
        state.model.rooms = vec![room("!room:example", "Room", false)];
        state.model.selected_room = Some("!room:example".to_owned());
        state.active_view = ViewId::Room("!room:example".to_owned());
        state.model.room_views.insert(
            "!room:example".to_owned(),
            RoomViewModel {
                topic: Some("incident review".to_owned()),
                activities: vec![sent_message_activity(
                    "!room:example",
                    "$!room:example:hello",
                    None,
                    "hello",
                    1_700_000_000_000,
                )],
                reactions_by_target: BTreeMap::new(),
                threads: BTreeMap::new(),
                image_media: BTreeMap::new(),
                typing_user_ids: vec!["@alice:example.org".to_owned()],
                members: Vec::new(),
                history: RoomHistoryBackfillState::default(),
            },
        );
        state.model.last_activity = Some(LastMessageActivity {
            room_id: "!room:example".to_owned(),
            event_id: Some("$!room:example:hello".to_owned()),
            transaction_id: None,
            sender: "@alice:example".to_owned(),
            timestamp_ms: 1_700_000_000_000,
            presentation: RoomMessagePresentationKind::Text,
            body: "hello".to_owned(),
            send_state: RoomMessageSendState::Sent,
        });
        state.model.last_e2ee_status = Some(e2ee_status(
            E2eeRecoveryState::Enabled,
            E2eeBackupState::Enabled,
            true,
            Some(true),
            Some(true),
        ));
        state.model.last_verification = Some(VerificationStateEvent {
            flow_id: "flow".to_owned(),
            user_id: "@alice:example".to_owned(),
            state: VerificationState::Requested,
        });
        state.model.last_error = Some(CoreErrorEvent {
            operation: Operation::SendMessage,
            message: "failed".to_owned(),
        });
        state.find_states.insert(
            ViewId::Room("!room:example".to_owned()),
            build_view_find_state("hello", "hello", Vec::new(), None),
        );
        state.views.insert(ViewId::Status, tracked_view(10));
        state
            .views
            .insert(ViewId::Room("!room:example".to_owned()), tracked_view(20));
        state.hwnd_views.insert(10, ViewId::Status);
        state
            .hwnd_views
            .insert(20, ViewId::Room("!room:example".to_owned()));

        with_local_app_data(|_| {
            apply_core_event(&mut state, Event::LoggedOut)
                .expect("logged out event must apply successfully");
        });
        let removed = take_stale_room_views(&mut state);

        assert!(state.model.rooms.is_empty());
        assert_eq!(state.model.selected_room, None);
        assert!(state.model.room_views.is_empty());
        assert_eq!(state.model.last_activity, None);
        assert_eq!(state.model.last_e2ee_status, None);
        assert_eq!(state.model.last_verification, None);
        assert_eq!(state.model.last_error, None);
        assert!(state.find_states.is_empty());
        assert_eq!(state.active_view, ViewId::Status);
        assert_eq!(removed.len(), 1);
        assert!(state.views.contains_key(&ViewId::Status));
        assert!(
            !state
                .views
                .contains_key(&ViewId::Room("!room:example".to_owned()))
        );
    }

    #[test]
    fn logged_out_clears_last_e2ee_status() {
        let mut state = test_state();
        state.model.last_e2ee_status = Some(e2ee_status(
            E2eeRecoveryState::Incomplete,
            E2eeBackupState::Enabled,
            true,
            Some(true),
            Some(false),
        ));

        with_local_app_data(|_| {
            apply_core_event(&mut state, Event::LoggedOut)
                .expect("logged out event must apply successfully");
        });

        assert_eq!(state.model.last_e2ee_status, None);
    }

    #[test]
    fn ordered_view_ids_follow_status_then_room_model_order_then_stale_views() {
        let mut state = test_state();
        state.model.rooms = vec![
            room("!beta:example", "Beta", false),
            room("!alpha:example", "Alpha", false),
        ];
        state.views.insert(ViewId::Status, tracked_view(10));
        state
            .views
            .insert(ViewId::Room("!alpha:example".to_owned()), tracked_view(20));
        state
            .views
            .insert(ViewId::Room("!orphan:example".to_owned()), tracked_view(30));
        state
            .views
            .insert(ViewId::Room("!beta:example".to_owned()), tracked_view(40));

        assert_eq!(
            ordered_view_ids(&state),
            vec![
                ViewId::Status,
                ViewId::Room("!beta:example".to_owned()),
                ViewId::Room("!alpha:example".to_owned()),
                ViewId::Room("!orphan:example".to_owned()),
            ]
        );
    }

    #[test]
    fn ordered_view_ids_exclude_spaces_from_model_order() {
        let mut state = test_state();
        state.model.rooms = vec![
            space("!space:example", "Space"),
            room("!beta:example", "Beta", false),
            room("!alpha:example", "Alpha", false),
        ];
        state.views.insert(ViewId::Status, tracked_view(10));
        state
            .views
            .insert(ViewId::Room("!space:example".to_owned()), tracked_view(20));
        state
            .views
            .insert(ViewId::Room("!alpha:example".to_owned()), tracked_view(30));
        state
            .views
            .insert(ViewId::Room("!beta:example".to_owned()), tracked_view(40));

        assert_eq!(
            ordered_view_ids(&state),
            vec![
                ViewId::Status,
                ViewId::Room("!beta:example".to_owned()),
                ViewId::Room("!alpha:example".to_owned()),
                ViewId::Room("!space:example".to_owned()),
            ]
        );
    }

    #[test]
    fn view_identity_round_trips_thread_view_id() {
        let view_id = ViewId::Thread {
            room_id: "!ops:example".to_owned(),
            thread_root_event_id: "$thread-root".to_owned(),
        };

        assert_eq!(
            view_identity_from_view_id(&view_id),
            ViewIdentity::Thread {
                room_id: "!ops:example".to_owned(),
                thread_root_event_id: "$thread-root".to_owned(),
            }
        );
        assert_eq!(
            view_id_from_identity(&ViewIdentity::Thread {
                room_id: "!ops:example".to_owned(),
                thread_root_event_id: "$thread-root".to_owned(),
            }),
            view_id
        );
    }

    #[test]
    fn thread_labels_use_observed_root_body_when_available_and_fall_back_to_root_event_id() {
        let mut room_view = RoomViewModel::default();

        room_view.apply_activity(sent_message_activity_with_relation_and_presentation(
            "!ops:example",
            "$thread-1",
            None,
            Some(thread_relation("$thread-root")),
            RoomMessagePresentationKind::Text,
            "thread rep",
            1_700_000_120_000,
        ));
        assert_eq!(
            room_view
                .threads
                .get("$thread-root")
                .map(|thread| thread.label.as_str()),
            Some("$thread-root")
        );

        room_view.apply_activity(sent_message_activity(
            "!ops:example",
            "$thread-root",
            None,
            "root msg",
            1_700_000_060_000,
        ));
        assert_eq!(
            room_view
                .threads
                .get("$thread-root")
                .map(|thread| thread.label.as_str()),
            Some("root msg")
        );

        let mut room_view = RoomViewModel::default();
        room_view.apply_activity(sent_message_activity(
            "!ops:example",
            "$thread-root",
            None,
            "root msg",
            1_700_000_060_000,
        ));
        room_view.apply_activity(sent_message_activity_with_relation_and_presentation(
            "!ops:example",
            "$thread-2",
            None,
            Some(thread_relation("$thread-root")),
            RoomMessagePresentationKind::Text,
            "thread rep",
            1_700_000_120_000,
        ));
        assert_eq!(
            room_view
                .threads
                .get("$thread-root")
                .map(|thread| thread.label.as_str()),
            Some("root msg")
        );
    }

    #[test]
    fn replacing_thread_root_updates_thread_label() {
        let mut room_view = RoomViewModel::default();
        room_view.apply_activity(sent_message_activity(
            "!ops:example",
            "$thread-root",
            None,
            "old root",
            1_700_000_060_000,
        ));
        room_view.apply_activity(sent_message_activity_with_relation_and_presentation(
            "!ops:example",
            "$thread-1",
            None,
            Some(thread_relation("$thread-root")),
            RoomMessagePresentationKind::Text,
            "thread rep",
            1_700_000_120_000,
        ));

        assert!(room_view.replace_message(
            "$thread-root",
            MessageReplacementFields {
                presentation: RoomMessagePresentationKind::Text,
                body: "edited root".to_owned(),
            }
        ));
        assert_eq!(
            room_view
                .threads
                .get("$thread-root")
                .map(|thread| thread.label.as_str()),
            Some("edited root")
        );
    }

    #[test]
    fn redaction_thread_root_keeps_thread_reachable_and_labels_it_message_redacted() {
        let mut state = test_state();
        state.model.rooms = vec![room("!ops:example", "Ops", false)];
        apply_core_event(
            &mut state,
            timeline_with_transaction_id(
                "!ops:example",
                "$thread-root",
                None,
                "old root",
                1_700_000_060_000,
            ),
        )
        .unwrap();
        apply_core_event(
            &mut state,
            thread_timeline("!ops:example", "$thread-1", "$thread-root", "thread rep"),
        )
        .unwrap();
        apply_core_event(
            &mut state,
            incoming_redacted("!ops:example", "$thread-root", "$redaction:example.org"),
        )
        .unwrap();

        let room_view = state
            .model
            .room_views
            .get("!ops:example")
            .expect("room view must exist");
        assert_eq!(
            room_view
                .threads
                .get("$thread-root")
                .map(|thread| thread.label.as_str()),
            Some("Message redacted")
        );

        let lines = project_timeline_rows(&build_thread_timeline_rows(
            &state.model,
            "!ops:example",
            "$thread-root",
        ));
        let rendered = lines.join("\n");
        assert!(rendered.contains("message redacted"));
        assert!(!rendered.contains("old root"));
    }

    #[test]
    fn thread_timeline_filters_room_activity() {
        let mut state = test_state();
        state.model.rooms = vec![room("!ops:example", "Ops", false)];
        apply_core_event(&mut state, topic_state("!ops:example", "incident review")).unwrap();
        apply_core_event(&mut state, timeline("!ops:example", "lobby")).unwrap();
        apply_core_event(
            &mut state,
            timeline_with_transaction_id(
                "!ops:example",
                "$thread-root",
                None,
                "root msg",
                1_700_000_060_000,
            ),
        )
        .unwrap();
        apply_core_event(
            &mut state,
            timeline_with_relation_and_presentation(
                "!ops:example",
                "$reply-1",
                None,
                Some(reply_relation("$lobby-root")),
                RoomMessagePresentationKind::Text,
                "plain",
                1_700_000_090_000,
            ),
        )
        .unwrap();
        apply_core_event(
            &mut state,
            thread_timeline("!ops:example", "$thread-1", "$thread-root", "thread rep"),
        )
        .unwrap();

        let lines = project_timeline_rows(&build_thread_timeline_rows(
            &state.model,
            "!ops:example",
            "$thread-root",
        ));

        assert!(
            lines
                .iter()
                .any(|line| line.contains("Room: Ops (!ops:example)"))
        );
        assert!(
            lines
                .iter()
                .any(|line| line.contains("Topic: incident review"))
        );
        assert!(lines.iter().any(|line| line.contains("root msg")));
        assert!(lines.iter().any(|line| line.contains("thread rep")));
        assert!(!lines.iter().any(|line| line.contains("lobby")));
        assert!(!lines.iter().any(|line| line.contains("plain")));
    }

    #[test]
    fn replacing_threaded_reply_updates_room_and_thread_timeline_projections() {
        let mut state = test_state();
        state.model.rooms = vec![room("!ops:example", "Ops", false)];
        apply_core_event(
            &mut state,
            timeline_with_transaction_id(
                "!ops:example",
                "$thread-root",
                None,
                "root msg",
                1_700_000_060_000,
            ),
        )
        .unwrap();
        apply_core_event(
            &mut state,
            thread_timeline("!ops:example", "$thread-1", "$thread-root", "old reply"),
        )
        .unwrap();
        apply_core_event(
            &mut state,
            incoming_replaced(
                "!ops:example",
                "$thread-1",
                "$thread-edit:example.org",
                RoomMessagePresentationKind::Text,
                "edited reply",
            ),
        )
        .unwrap();

        assert!(
            render_room_lines(&state.model, "!ops:example")
                .iter()
                .any(|line| line.contains("edited reply"))
        );
        let lines = project_timeline_rows(&build_thread_timeline_rows(
            &state.model,
            "!ops:example",
            "$thread-root",
        ));
        assert!(lines.iter().any(|line| line.contains("edited reply")));
        assert!(!lines.iter().any(|line| line.contains("old reply")));
    }

    #[test]
    fn redaction_threaded_reply_updates_room_and_thread_timeline_projections() {
        let mut state = test_state();
        state.model.rooms = vec![room("!ops:example", "Ops", false)];
        apply_core_event(
            &mut state,
            timeline_with_transaction_id(
                "!ops:example",
                "$thread-root",
                None,
                "root msg",
                1_700_000_060_000,
            ),
        )
        .unwrap();
        apply_core_event(
            &mut state,
            thread_timeline("!ops:example", "$thread-1", "$thread-root", "old reply"),
        )
        .unwrap();
        apply_core_event(
            &mut state,
            incoming_redacted("!ops:example", "$thread-1", "$redaction:example.org"),
        )
        .unwrap();

        let room_rendered = render_room_lines(&state.model, "!ops:example").join("\n");
        assert!(room_rendered.contains("message redacted"));
        assert!(!room_rendered.contains("old reply"));

        let thread_lines = project_timeline_rows(&build_thread_timeline_rows(
            &state.model,
            "!ops:example",
            "$thread-root",
        ));
        let thread_rendered = thread_lines.join("\n");
        assert!(thread_rendered.contains("message redacted"));
        assert!(!thread_rendered.contains("old reply"));
    }

    #[test]
    fn latest_durable_thread_event_id_falls_back_to_root_when_root_is_only_durable_event() {
        let mut state = test_state();
        state.model.rooms = vec![room("!ops:example", "Ops", false)];
        apply_core_event(
            &mut state,
            timeline_with_transaction_id(
                "!ops:example",
                "$thread-root:example",
                None,
                "root msg",
                1_700_000_060_000,
            ),
        )
        .unwrap();

        assert_eq!(
            latest_durable_thread_event_id(&state.model, "!ops:example", "$thread-root:example"),
            "$thread-root:example"
        );
    }

    #[test]
    fn latest_durable_thread_event_id_ignores_newer_pending_local_thread_activity() {
        let mut state = test_state();
        state.model.rooms = vec![room("!ops:example", "Ops", false)];
        apply_core_event(
            &mut state,
            timeline_with_transaction_id(
                "!ops:example",
                "$thread-root:example",
                None,
                "root msg",
                1_700_000_060_000,
            ),
        )
        .unwrap();
        apply_core_event(
            &mut state,
            timeline_with_relation_and_presentation(
                "!ops:example",
                "$thread-1:example",
                None,
                Some(thread_relation("$thread-root:example")),
                RoomMessagePresentationKind::Text,
                "durable reply",
                1_700_000_120_000,
            ),
        )
        .unwrap();
        apply_core_event(
            &mut state,
            outgoing_queued_in_thread("!ops:example", "thread-pending-1", "$thread-root:example"),
        )
        .unwrap();

        assert_eq!(
            latest_durable_thread_event_id(&state.model, "!ops:example", "$thread-root:example"),
            "$thread-1:example"
        );
    }

    #[test]
    fn ordered_view_ids_place_threads_after_parent_rooms() {
        let mut state = test_state();
        state.model.rooms = vec![
            room("!alpha:example", "Alpha", false),
            room("!beta:example", "Beta", false),
        ];
        state.model.room_views.insert(
            "!alpha:example".to_owned(),
            RoomViewModel {
                threads: BTreeMap::from([(
                    "$thread-root".to_owned(),
                    ThreadViewModel {
                        label: "root msg".to_owned(),
                    },
                )]),
                ..RoomViewModel::default()
            },
        );
        state.views.insert(ViewId::Status, tracked_view(10));
        state
            .views
            .insert(ViewId::Room("!alpha:example".to_owned()), tracked_view(20));
        state.views.insert(
            ViewId::Thread {
                room_id: "!alpha:example".to_owned(),
                thread_root_event_id: "$thread-root".to_owned(),
            },
            tracked_view(30),
        );
        state
            .views
            .insert(ViewId::Room("!beta:example".to_owned()), tracked_view(40));

        assert_eq!(
            ordered_view_ids(&state),
            vec![
                ViewId::Status,
                ViewId::Room("!alpha:example".to_owned()),
                ViewId::Thread {
                    room_id: "!alpha:example".to_owned(),
                    thread_root_event_id: "$thread-root".to_owned(),
                },
                ViewId::Room("!beta:example".to_owned()),
            ]
        );
    }

    #[test]
    fn stale_thread_cleanup_falls_back_to_parent_room_when_possible() {
        let mut state = test_state();
        state.model.rooms = vec![room("!alpha:example", "Alpha", false)];
        state.model.selected_room = Some("!alpha:example".to_owned());
        state.active_view = ViewId::Thread {
            room_id: "!alpha:example".to_owned(),
            thread_root_event_id: "$thread-root".to_owned(),
        };
        state.views.insert(ViewId::Status, tracked_view(10));
        state
            .views
            .insert(ViewId::Room("!alpha:example".to_owned()), tracked_view(20));
        state.views.insert(
            ViewId::Thread {
                room_id: "!alpha:example".to_owned(),
                thread_root_event_id: "$thread-root".to_owned(),
            },
            tracked_view(30),
        );
        state.hwnd_views.insert(10, ViewId::Status);
        state
            .hwnd_views
            .insert(20, ViewId::Room("!alpha:example".to_owned()));
        state.hwnd_views.insert(
            30,
            ViewId::Thread {
                room_id: "!alpha:example".to_owned(),
                thread_root_event_id: "$thread-root".to_owned(),
            },
        );

        let removed = take_stale_room_views(&mut state);

        assert_eq!(removed.len(), 1);
        assert_eq!(state.active_view, ViewId::Room("!alpha:example".to_owned()));
        assert!(!state.views.contains_key(&ViewId::Thread {
            room_id: "!alpha:example".to_owned(),
            thread_root_event_id: "$thread-root".to_owned(),
        }));
    }

    #[test]
    fn stale_thread_cleanup_removes_threads_when_room_disappears() {
        let mut state = test_state();
        state.model.selected_room = Some("!alpha:example".to_owned());
        state.active_view = ViewId::Thread {
            room_id: "!alpha:example".to_owned(),
            thread_root_event_id: "$thread-root".to_owned(),
        };
        state.views.insert(ViewId::Status, tracked_view(10));
        state.views.insert(
            ViewId::Thread {
                room_id: "!alpha:example".to_owned(),
                thread_root_event_id: "$thread-root".to_owned(),
            },
            tracked_view(20),
        );
        state.hwnd_views.insert(10, ViewId::Status);
        state.hwnd_views.insert(
            20,
            ViewId::Thread {
                room_id: "!alpha:example".to_owned(),
                thread_root_event_id: "$thread-root".to_owned(),
            },
        );

        let removed = take_stale_room_views(&mut state);

        assert_eq!(removed.len(), 1);
        assert_eq!(state.active_view, ViewId::Status);
        assert!(state.views.contains_key(&ViewId::Status));
    }

    #[test]
    fn switchbar_button_text_includes_room_read_suffix_without_changing_other_titles() {
        let mut state = test_state();
        let mut direct_room = room("!room:example", "Room", true);
        direct_room.is_direct = true;
        direct_room.direct_user_ids = vec!["@alice:example".to_owned()];
        state.model.rooms = vec![direct_room];
        state.model.room_read_states.insert(
            "!room:example".to_owned(),
            RoomReadState {
                room_id: "!room:example".to_owned(),
                latest_active_event_id: Some("$latest".to_owned()),
                fully_read_event_id: Some("$fully-read".to_owned()),
                marked_unread: true,
                num_unread: 3,
                num_notifications: 2,
                num_mentions: 1,
            },
        );
        state.room_activity.insert(room_view_id("!room:example"));
        state.model.room_views.insert(
            "!room:example".to_owned(),
            RoomViewModel {
                threads: BTreeMap::from([(
                    "$thread-root".to_owned(),
                    ThreadViewModel {
                        label: "root msg".to_owned(),
                    },
                )]),
                ..RoomViewModel::default()
            },
        );
        state
            .room_activity
            .insert(thread_view_id("!room:example", "$thread-root"));

        assert_eq!(
            view_title(&state.model, &room_view_id("!room:example")),
            "Room [query: @alice:example] [e2ee]"
        );
        assert_eq!(
            switchbar_button_text(&state, &room_view_id("!room:example")),
            "Room [query: @alice:example] [e2ee] [@1 !2 *3 M]"
        );
        assert_eq!(
            switchbar_button_text(&state, &thread_view_id("!room:example", "$thread-root")),
            "Thread: root msg"
        );
        assert_eq!(switchbar_button_text(&state, &ViewId::Status), "Status");
    }

    #[test]
    fn switchbar_button_state_tracks_checked_and_marked_bits() {
        let mut state = test_state();
        let room_view = room_view_id("!room:example");

        state.room_activity.insert(room_view.clone());
        assert_eq!(
            switchbar_button_state(&state, &room_view),
            (TBSTATE_ENABLED | TBSTATE_MARKED) as u8
        );

        state.room_activity.clear();
        state.active_view = room_view.clone();
        assert_eq!(
            switchbar_button_state(&state, &room_view),
            (TBSTATE_ENABLED | TBSTATE_CHECKED) as u8
        );

        state.room_activity.insert(room_view.clone());
        assert_eq!(
            switchbar_button_state(&state, &room_view),
            (TBSTATE_ENABLED | TBSTATE_CHECKED | TBSTATE_MARKED) as u8
        );
    }

    #[test]
    fn tree_item_state_bolds_only_matching_exact_view() {
        let mut state = test_state();
        let room_view = room_view_id("!room:example");
        let thread_view = thread_view_id("!room:example", "$thread-root");
        let other_room_view = room_view_id("!other:example");

        state.room_activity.insert(room_view.clone());
        assert_eq!(tree_item_state(&state, &room_view), TVIS_BOLD);
        assert_eq!(tree_item_state(&state, &thread_view), 0);
        assert_eq!(tree_item_state(&state, &ViewId::Status), 0);
        assert_eq!(tree_item_state(&state, &other_room_view), 0);

        state.room_activity.clear();
        state.room_activity.insert(thread_view.clone());
        assert_eq!(tree_item_state(&state, &room_view), 0);
        assert_eq!(tree_item_state(&state, &thread_view), TVIS_BOLD);
        assert_eq!(tree_item_state(&state, &ViewId::Status), 0);
        assert_eq!(tree_item_state(&state, &other_room_view), 0);
    }

    #[test]
    fn inactive_synced_messages_failed_updates_and_state_notices_mark_room_activity() {
        let mut state = test_state();
        state.model.rooms = vec![
            room("!active:example", "Active", false),
            room("!inactive:example", "Inactive", false),
        ];
        state.active_view = ViewId::Room("!active:example".to_owned());

        apply_core_event(&mut state, timeline("!inactive:example", "hello")).unwrap();
        apply_core_event(
            &mut state,
            outgoing_queued("!inactive:example", "txn-1", "draft"),
        )
        .unwrap();
        assert!(
            state
                .room_activity
                .contains(&room_view_id("!inactive:example"))
        );

        state.room_activity.clear();

        apply_core_event(
            &mut state,
            outgoing_sent("!inactive:example", "txn-1", "$event:sent"),
        )
        .unwrap();
        assert!(
            !state
                .room_activity
                .contains(&room_view_id("!inactive:example"))
        );

        apply_core_event(
            &mut state,
            outgoing_queued("!inactive:example", "txn-2", "retry"),
        )
        .unwrap();
        apply_core_event(
            &mut state,
            outgoing_failed("!inactive:example", "txn-2", "send failed"),
        )
        .unwrap();
        assert!(
            state
                .room_activity
                .contains(&room_view_id("!inactive:example"))
        );

        state.room_activity.clear();

        apply_core_event(
            &mut state,
            topic_state("!inactive:example", "incident review"),
        )
        .unwrap();

        assert!(
            state
                .room_activity
                .contains(&room_view_id("!inactive:example"))
        );
    }

    #[test]
    fn inactive_event_backed_messages_record_room_activity_read_targets() {
        let mut state = test_state();
        state.model.rooms = vec![
            room("!active:example", "Active", false),
            room("!inactive:example", "Inactive", false),
        ];
        state.active_view = ViewId::Room("!active:example".to_owned());

        apply_core_event(&mut state, timeline("!inactive:example", "hello")).unwrap();

        assert!(
            state
                .room_activity
                .contains(&room_view_id("!inactive:example"))
        );
        assert_eq!(
            state
                .room_activity_read_targets
                .get(&room_view_id("!inactive:example"))
                .map(String::as_str),
            Some("$!inactive:example:hello")
        );
    }

    #[test]
    fn synthetic_inactive_messages_mark_activity_without_read_targets() {
        let mut state = test_state();
        state.model.rooms = vec![
            room("!active:example", "Active", false),
            room("!inactive:example", "Inactive", false),
        ];
        state.active_view = ViewId::Room("!active:example".to_owned());
        state.synthetic_load_in_progress = true;

        apply_core_event(&mut state, timeline("!inactive:example", "hello")).unwrap();

        assert!(
            state
                .room_activity
                .contains(&room_view_id("!inactive:example"))
        );
        assert_eq!(
            state
                .room_activity_read_targets
                .get(&room_view_id("!inactive:example"))
                .map(String::as_str),
            None
        );
    }

    #[test]
    fn inactive_unsupported_messages_record_room_activity_read_targets() {
        let mut state = test_state();
        state.model.rooms = vec![
            room("!active:example", "Active", false),
            room("!inactive:example", "Inactive", false),
        ];
        state.active_view = ViewId::Room("!active:example".to_owned());

        apply_core_event(
            &mut state,
            unsupported_message(
                "!inactive:example",
                "$image1:example.org",
                "@alice:example.org",
                1_710_000_000_000,
                "m.video",
                "clip.mp4",
            ),
        )
        .unwrap();

        assert!(
            state
                .room_activity
                .contains(&room_view_id("!inactive:example"))
        );
        assert_eq!(
            state
                .room_activity_read_targets
                .get(&room_view_id("!inactive:example"))
                .map(String::as_str),
            Some("$image1:example.org")
        );
    }

    #[test]
    fn inactive_image_messages_record_room_activity_read_targets() {
        let mut state = test_state();
        state.model.rooms = vec![
            room("!active:example", "Active", false),
            room("!inactive:example", "Inactive", false),
        ];
        state.active_view = ViewId::Room("!active:example".to_owned());

        apply_core_event(
            &mut state,
            image_message(
                "!inactive:example",
                "$image1:example.org",
                "@alice:example.org",
                1_710_000_000_000,
                "diagram.png",
                Some("diagram.png"),
                Some(RoomImageInfo {
                    mimetype: Some("image/png".to_owned()),
                    size: Some(12_345),
                    width: Some(640),
                    height: Some(480),
                    is_animated: Some(false),
                    thumbnail_source: None,
                }),
            ),
        )
        .unwrap();

        assert!(
            state
                .room_activity
                .contains(&room_view_id("!inactive:example"))
        );
        assert_eq!(
            state
                .room_activity_read_targets
                .get(&room_view_id("!inactive:example"))
                .map(String::as_str),
            Some("$image1:example.org")
        );
    }

    #[test]
    fn inactive_file_messages_record_room_activity_read_targets() {
        let mut state = test_state();
        state.model.rooms = vec![
            room("!active:example", "Active", false),
            room("!inactive:example", "Inactive", false),
        ];
        state.active_view = ViewId::Room("!active:example".to_owned());

        apply_core_event(
            &mut state,
            Event::RoomActivity(Box::new(file_activity(
                "!inactive:example",
                "$file1:example.org",
                "@alice:example.org",
                1_710_000_000_000,
                "Quarterly report",
                Some("report.pdf"),
                Some(RoomFileInfo {
                    mimetype: Some("application/pdf".to_owned()),
                    size: Some(45_678),
                }),
            ))),
        )
        .unwrap();

        assert!(
            state
                .room_activity
                .contains(&room_view_id("!inactive:example"))
        );
        assert_eq!(
            state
                .room_activity_read_targets
                .get(&room_view_id("!inactive:example"))
                .map(String::as_str),
            Some("$file1:example.org")
        );
    }

    #[test]
    fn inactive_audio_messages_record_room_activity_read_targets() {
        let mut state = test_state();
        state.model.rooms = vec![
            room("!active:example", "Active", false),
            room("!inactive:example", "Inactive", false),
        ];
        state.active_view = ViewId::Room("!active:example".to_owned());

        apply_core_event(
            &mut state,
            Event::RoomActivity(Box::new(audio_activity(
                "!inactive:example",
                "$audio1:example.org",
                "@alice:example.org",
                1_710_000_000_000,
                "Voice note",
                Some("voice-note.ogg"),
                Some(RoomAudioInfo {
                    mimetype: Some("audio/ogg".to_owned()),
                    size: Some(23_456),
                    duration_ms: Some(12_000),
                }),
            ))),
        )
        .unwrap();

        assert!(
            state
                .room_activity
                .contains(&room_view_id("!inactive:example"))
        );
        assert_eq!(
            state
                .room_activity_read_targets
                .get(&room_view_id("!inactive:example"))
                .map(String::as_str),
            Some("$audio1:example.org")
        );
    }

    #[test]
    fn file_and_audio_room_activities_expose_event_ids_read_targets_and_thread_labels() {
        let file = file_activity(
            "!media:example.org",
            "$file1:example.org",
            "@alice:example.org",
            1_710_000_000_000,
            "Quarterly report",
            Some("report.pdf"),
            Some(RoomFileInfo {
                mimetype: Some("application/pdf".to_owned()),
                size: Some(45_678),
            }),
        );
        let audio = audio_activity(
            "!media:example.org",
            "$audio1:example.org",
            "@alice:example.org",
            1_710_000_060_000,
            "Voice note",
            Some("voice-note.ogg"),
            Some(RoomAudioInfo {
                mimetype: Some("audio/ogg".to_owned()),
                size: Some(23_456),
                duration_ms: Some(12_000),
            }),
        );

        assert_eq!(room_activity_event_id(&file), Some("$file1:example.org"));
        assert_eq!(
            read_target_for_room_activity(&file),
            Some("$file1:example.org")
        );
        assert_eq!(
            room_activity_thread_root_label(&file),
            Some("report.pdf".to_owned())
        );

        assert_eq!(room_activity_event_id(&audio), Some("$audio1:example.org"));
        assert_eq!(
            read_target_for_room_activity(&audio),
            Some("$audio1:example.org")
        );
        assert_eq!(
            room_activity_thread_root_label(&audio),
            Some("voice-note.ogg".to_owned())
        );
    }

    #[test]
    fn inactive_state_notices_record_room_activity_read_targets() {
        let mut state = test_state();
        state.model.rooms = vec![
            room("!active:example", "Active", false),
            room("!inactive:example", "Inactive", false),
        ];
        state.active_view = ViewId::Room("!active:example".to_owned());

        apply_core_event(
            &mut state,
            topic_state("!inactive:example", "incident review"),
        )
        .unwrap();

        assert!(
            state
                .room_activity
                .contains(&room_view_id("!inactive:example"))
        );
        assert_eq!(
            state
                .room_activity_read_targets
                .get(&room_view_id("!inactive:example"))
                .map(String::as_str),
            Some("$topic:!inactive:example:incident review")
        );
    }

    #[test]
    fn pending_eventless_and_failed_send_activity_do_not_replace_read_targets() {
        let mut state = test_state();
        state.model.rooms = vec![
            room("!active:example", "Active", false),
            room("!inactive:example", "Inactive", false),
        ];
        state.active_view = ViewId::Room("!active:example".to_owned());

        apply_core_event(&mut state, timeline("!inactive:example", "hello")).unwrap();
        let original_target = state
            .room_activity_read_targets
            .get(&room_view_id("!inactive:example"))
            .cloned();

        apply_core_event(
            &mut state,
            outgoing_queued("!inactive:example", "txn-1", "draft"),
        )
        .unwrap();
        apply_core_event(
            &mut state,
            outgoing_failed("!inactive:example", "txn-1", "send failed"),
        )
        .unwrap();

        assert_eq!(
            state
                .room_activity_read_targets
                .get(&room_view_id("!inactive:example")),
            original_target.as_ref()
        );
    }

    #[test]
    fn active_room_events_do_not_mark_room_view_activity() {
        let mut state = test_state();
        state.model.rooms = vec![room("!room:example", "Room", false)];
        state.active_view = ViewId::Room("!room:example".to_owned());

        apply_core_event(&mut state, timeline("!room:example", "hello")).unwrap();
        apply_core_event(&mut state, topic_state("!room:example", "incident review")).unwrap();

        assert!(!state.room_activity.contains(&room_view_id("!room:example")));
    }

    #[test]
    fn thread_view_is_not_treated_as_active_room_for_room_activity_read_targets() {
        let mut state = test_state();
        state.model.rooms = vec![room("!room:example", "Room", false)];
        state.active_view = thread_view_id("!room:example", "$thread-root");

        apply_core_event(&mut state, timeline("!room:example", "hello")).unwrap();

        assert!(state.room_activity.contains(&room_view_id("!room:example")));
        assert_eq!(
            state
                .room_activity_read_targets
                .get(&room_view_id("!room:example"))
                .map(String::as_str),
            Some("$!room:example:hello")
        );
    }

    #[test]
    fn threaded_activity_marks_thread_view_and_not_parent_room() {
        let mut state = test_state();
        state.model.rooms = vec![room("!room:example", "Room", false)];

        apply_core_event(
            &mut state,
            thread_timeline("!room:example", "$thread-1", "$thread-root", "thread reply"),
        )
        .unwrap();

        assert!(!state.room_activity.contains(&room_view_id("!room:example")));
        assert!(
            state
                .room_activity
                .contains(&thread_view_id("!room:example", "$thread-root"))
        );
        assert_eq!(
            state
                .room_activity_read_targets
                .get(&thread_view_id("!room:example", "$thread-root"))
                .map(String::as_str),
            Some("$thread-1")
        );
    }

    #[test]
    fn active_matching_thread_view_does_not_mark_thread_activity() {
        let mut state = test_state();
        state.model.rooms = vec![room("!room:example", "Room", false)];
        state.active_view = thread_view_id("!room:example", "$thread-root");

        apply_core_event(
            &mut state,
            thread_timeline("!room:example", "$thread-1", "$thread-root", "thread reply"),
        )
        .unwrap();

        assert!(
            !state
                .room_activity
                .contains(&thread_view_id("!room:example", "$thread-root"))
        );
    }

    #[test]
    fn active_parent_room_does_not_suppress_thread_activity_marker() {
        let mut state = test_state();
        state.model.rooms = vec![room("!room:example", "Room", false)];
        state.active_view = room_view_id("!room:example");

        apply_core_event(
            &mut state,
            thread_timeline("!room:example", "$thread-1", "$thread-root", "thread reply"),
        )
        .unwrap();

        assert!(!state.room_activity.contains(&room_view_id("!room:example")));
        assert!(
            state
                .room_activity
                .contains(&thread_view_id("!room:example", "$thread-root"))
        );
    }

    #[test]
    fn failed_thread_send_marks_inactive_thread_view() {
        let mut state = test_state();
        state.model.rooms = vec![
            room("!active:example", "Active", false),
            room("!room:example", "Room", false),
        ];
        state.active_view = room_view_id("!active:example");

        apply_core_event(
            &mut state,
            outgoing_queued_in_thread("!room:example", "thread-pending-1", "$thread-root"),
        )
        .unwrap();
        state.room_activity.clear();

        apply_core_event(
            &mut state,
            outgoing_failed("!room:example", "thread-pending-1", "send failed"),
        )
        .unwrap();

        assert!(!state.room_activity.contains(&room_view_id("!room:example")));
        assert!(
            state
                .room_activity
                .contains(&thread_view_id("!room:example", "$thread-root"))
        );
    }

    #[test]
    fn unsupported_room_activity_updates_last_activity_preview() {
        let activity = unsupported_message_activity(
            "!media:example.org",
            "$image1:example.org",
            "@alice:example.org",
            1_710_000_000_000,
            "m.video",
            "clip.mp4",
        );

        assert_eq!(
            last_activity_from_room_activity(&activity),
            Some(LastMessageActivity {
                room_id: "!media:example.org".to_owned(),
                event_id: Some("$image1:example.org".to_owned()),
                transaction_id: None,
                sender: "@alice:example.org".to_owned(),
                timestamp_ms: 1_710_000_000_000,
                presentation: RoomMessagePresentationKind::Text,
                body: "[unsupported m.video] clip.mp4".to_owned(),
                send_state: RoomMessageSendState::Sent,
            })
        );
    }

    #[test]
    fn image_room_activity_updates_last_activity_preview() {
        let activity = image_activity(
            "!media:example.org",
            "$image1:example.org",
            "@alice:example.org",
            1_710_000_000_000,
            "body fallback",
            Some("diagram.png"),
            Some(RoomImageInfo {
                mimetype: Some("image/png".to_owned()),
                size: Some(12_345),
                width: Some(640),
                height: Some(480),
                is_animated: Some(false),
                thumbnail_source: None,
            }),
        );

        assert_eq!(
            last_activity_from_room_activity(&activity),
            Some(LastMessageActivity {
                room_id: "!media:example.org".to_owned(),
                event_id: Some("$image1:example.org".to_owned()),
                transaction_id: None,
                sender: "@alice:example.org".to_owned(),
                timestamp_ms: 1_710_000_000_000,
                presentation: RoomMessagePresentationKind::Text,
                body: "[image] diagram.png".to_owned(),
                send_state: RoomMessageSendState::Sent,
            })
        );
    }

    #[test]
    fn file_room_activity_updates_last_activity_preview() {
        let activity = file_activity(
            "!media:example.org",
            "$file1:example.org",
            "@alice:example.org",
            1_710_000_000_000,
            "Quarterly report",
            Some("report.pdf"),
            Some(RoomFileInfo {
                mimetype: Some("application/pdf".to_owned()),
                size: Some(45_678),
            }),
        );

        assert_eq!(
            last_activity_from_room_activity(&activity),
            Some(LastMessageActivity {
                room_id: "!media:example.org".to_owned(),
                event_id: Some("$file1:example.org".to_owned()),
                transaction_id: None,
                sender: "@alice:example.org".to_owned(),
                timestamp_ms: 1_710_000_000_000,
                presentation: RoomMessagePresentationKind::Text,
                body: "[file] report.pdf".to_owned(),
                send_state: RoomMessageSendState::Sent,
            })
        );
    }

    #[test]
    fn audio_room_activity_updates_last_activity_preview() {
        let activity = audio_activity(
            "!media:example.org",
            "$audio1:example.org",
            "@alice:example.org",
            1_710_000_000_000,
            "Voice note",
            Some("voice-note.ogg"),
            Some(RoomAudioInfo {
                mimetype: Some("audio/ogg".to_owned()),
                size: Some(23_456),
                duration_ms: Some(12_000),
            }),
        );

        assert_eq!(
            last_activity_from_room_activity(&activity),
            Some(LastMessageActivity {
                room_id: "!media:example.org".to_owned(),
                event_id: Some("$audio1:example.org".to_owned()),
                transaction_id: None,
                sender: "@alice:example.org".to_owned(),
                timestamp_ms: 1_710_000_000_000,
                presentation: RoomMessagePresentationKind::Text,
                body: "[audio] voice-note.ogg".to_owned(),
                send_state: RoomMessageSendState::Sent,
            })
        );
    }

    #[test]
    fn server_read_state_clears_matching_activity_read_target_without_command() {
        let mut state = test_state();
        state.model.rooms = vec![
            room("!active:example", "Active", false),
            room("!room:example", "Room", false),
        ];
        state.active_view = ViewId::Room("!active:example".to_owned());
        state.room_activity.insert(room_view_id("!room:example"));
        state
            .room_activity
            .insert(thread_view_id("!room:example", "$thread-root"));
        state
            .room_activity_read_targets
            .insert(room_view_id("!room:example"), "$event".to_owned());
        state.room_activity_read_targets.insert(
            thread_view_id("!room:example", "$thread-root"),
            "$thread-1".to_owned(),
        );

        apply_core_event(
            &mut state,
            room_read_state("!room:example", Some("$event"), None, false, 0, 0, 0),
        )
        .unwrap();

        assert!(!state.room_activity.contains(&room_view_id("!room:example")));
        assert!(
            !state
                .room_activity_read_targets
                .contains_key(&room_view_id("!room:example"))
        );
        assert!(
            state
                .room_activity
                .contains(&thread_view_id("!room:example", "$thread-root"))
        );
        assert!(
            state
                .room_activity_read_targets
                .contains_key(&thread_view_id("!room:example", "$thread-root"))
        );
        assert_eq!(
            state.model.room_read_states.get("!room:example"),
            Some(&RoomReadState {
                room_id: "!room:example".to_owned(),
                latest_active_event_id: Some("$event".to_owned()),
                fully_read_event_id: None,
                marked_unread: false,
                num_unread: 0,
                num_notifications: 0,
                num_mentions: 0,
            })
        );
        assert!(state.recorded_commands.is_empty());
    }

    #[test]
    fn server_fully_read_marker_clears_matching_activity_read_target_without_command() {
        let mut state = test_state();
        state.model.rooms = vec![
            room("!active:example", "Active", false),
            room("!room:example", "Room", false),
        ];
        state.active_view = ViewId::Room("!active:example".to_owned());
        state.room_activity.insert(room_view_id("!room:example"));
        state
            .room_activity
            .insert(thread_view_id("!room:example", "$thread-root"));
        state
            .room_activity_read_targets
            .insert(room_view_id("!room:example"), "$event".to_owned());
        state.room_activity_read_targets.insert(
            thread_view_id("!room:example", "$thread-root"),
            "$thread-1".to_owned(),
        );

        apply_core_event(
            &mut state,
            room_read_state("!room:example", None, Some("$event"), false, 0, 0, 0),
        )
        .unwrap();

        assert!(!state.room_activity.contains(&room_view_id("!room:example")));
        assert!(
            !state
                .room_activity_read_targets
                .contains_key(&room_view_id("!room:example"))
        );
        assert!(
            state
                .room_activity
                .contains(&thread_view_id("!room:example", "$thread-root"))
        );
        assert!(
            state
                .room_activity_read_targets
                .contains_key(&thread_view_id("!room:example", "$thread-root"))
        );
        assert!(state.recorded_commands.is_empty());
    }

    #[test]
    fn server_zero_unread_state_clears_only_room_activity() {
        let mut state = test_state();
        state.model.rooms = vec![
            room("!active:example", "Active", false),
            room("!room:example", "Room", false),
        ];
        state.active_view = ViewId::Room("!active:example".to_owned());
        state.room_activity.insert(room_view_id("!room:example"));
        state
            .room_activity
            .insert(thread_view_id("!room:example", "$thread-root"));
        state
            .room_activity_read_targets
            .insert(room_view_id("!room:example"), "$event".to_owned());
        state.room_activity_read_targets.insert(
            thread_view_id("!room:example", "$thread-root"),
            "$thread-1".to_owned(),
        );

        apply_core_event(
            &mut state,
            room_read_state("!room:example", None, None, false, 0, 0, 0),
        )
        .unwrap();

        assert!(!state.room_activity.contains(&room_view_id("!room:example")));
        assert!(
            state
                .room_activity
                .contains(&thread_view_id("!room:example", "$thread-root"))
        );
        assert!(
            !state
                .room_activity_read_targets
                .contains_key(&room_view_id("!room:example"))
        );
        assert!(
            state
                .room_activity_read_targets
                .contains_key(&thread_view_id("!room:example", "$thread-root"))
        );
    }

    #[test]
    fn server_unread_state_marks_inactive_room_without_local_timeline_event() {
        let mut state = test_state();
        state.model.rooms = vec![
            room("!active:example", "Active", false),
            room("!room:example", "Room", false),
        ];
        state.active_view = ViewId::Room("!active:example".to_owned());

        apply_core_event(
            &mut state,
            room_read_state("!room:example", None, None, false, 1, 0, 0),
        )
        .unwrap();

        assert!(state.room_activity.contains(&room_view_id("!room:example")));
        assert!(
            !state
                .room_activity_read_targets
                .contains_key(&room_view_id("!room:example"))
        );
    }

    #[test]
    fn server_unread_state_does_not_mark_active_room() {
        let mut state = test_state();
        state.model.rooms = vec![room("!room:example", "Room", false)];
        state.active_view = ViewId::Room("!room:example".to_owned());

        apply_core_event(
            &mut state,
            room_read_state("!room:example", None, None, false, 1, 0, 0),
        )
        .unwrap();

        assert!(!state.room_activity.contains(&room_view_id("!room:example")));
    }

    #[test]
    fn caught_up_zero_read_state_clears_navigation_suffixes() {
        let mut state = test_state();
        state.model.rooms = vec![room("!room:example", "Room", false)];

        apply_core_event(
            &mut state,
            room_read_state(
                "!room:example",
                Some("$latest"),
                Some("$fully-read"),
                true,
                3,
                2,
                1,
            ),
        )
        .unwrap();
        assert_eq!(
            room_navigation_label(&state.model, &state.model.rooms[0]),
            "Room [@1 !2 *3 M]"
        );
        assert_eq!(
            switchbar_button_text(&state, &room_view_id("!room:example")),
            "Room [@1 !2 *3 M]"
        );

        apply_core_event(
            &mut state,
            room_read_state(
                "!room:example",
                Some("$latest"),
                Some("$latest"),
                false,
                0,
                0,
                0,
            ),
        )
        .unwrap();
        assert_eq!(
            room_navigation_label(&state.model, &state.model.rooms[0]),
            "Room"
        );
        assert_eq!(
            switchbar_button_text(&state, &room_view_id("!room:example")),
            "Room"
        );
    }

    #[test]
    fn server_read_state_ignores_unknown_room() {
        let mut state = test_state();

        apply_core_event(
            &mut state,
            room_read_state("!missing:example", None, None, false, 1, 0, 0),
        )
        .unwrap();

        assert!(state.room_activity.is_empty());
        assert!(state.room_activity_read_targets.is_empty());
        assert!(state.model.rooms.is_empty());
        assert!(state.model.room_read_states.is_empty());
        assert!(state.model.room_views.is_empty());
        assert!(state.views.is_empty());
    }

    #[test]
    fn synced_rooms_prunes_stale_room_activity() {
        let mut state = test_state();
        state.room_activity.insert(room_view_id("!keep:example"));
        state.room_activity.insert(room_view_id("!stale:example"));
        state
            .room_activity
            .insert(thread_view_id("!keep:example", "$thread-root"));

        apply_core_event(
            &mut state,
            Event::SyncedRooms(vec![room("!keep:example", "Keep", false)]),
        )
        .unwrap();

        assert!(state.room_activity.contains(&room_view_id("!keep:example")));
        assert!(
            !state
                .room_activity
                .contains(&room_view_id("!stale:example"))
        );
        assert!(
            state
                .room_activity
                .contains(&thread_view_id("!keep:example", "$thread-root"))
        );
    }

    #[test]
    fn synced_rooms_prune_room_activity_read_targets() {
        let mut state = test_state();
        state.room_activity.insert(room_view_id("!keep:example"));
        state.room_activity.insert(room_view_id("!stale:example"));
        state
            .room_activity
            .insert(thread_view_id("!keep:example", "$thread-root"));
        state
            .room_activity_read_targets
            .insert(room_view_id("!keep:example"), "$keep:example".to_owned());
        state
            .room_activity_read_targets
            .insert(room_view_id("!stale:example"), "$stale:example".to_owned());
        state.room_activity_read_targets.insert(
            thread_view_id("!keep:example", "$thread-root"),
            "$thread-1".to_owned(),
        );

        apply_core_event(
            &mut state,
            Event::SyncedRooms(vec![room("!keep:example", "Keep", false)]),
        )
        .unwrap();

        assert_eq!(
            state
                .room_activity_read_targets
                .get(&room_view_id("!keep:example"))
                .map(String::as_str),
            Some("$keep:example")
        );
        assert_eq!(
            state
                .room_activity_read_targets
                .get(&thread_view_id("!keep:example", "$thread-root"))
                .map(String::as_str),
            Some("$thread-1")
        );
        assert!(
            !state
                .room_activity_read_targets
                .contains_key(&room_view_id("!stale:example"))
        );
    }

    #[test]
    fn synced_rooms_prune_stale_room_read_states() {
        let mut state = test_state();
        state.model.room_read_states.insert(
            "!keep:example".to_owned(),
            RoomReadState {
                room_id: "!keep:example".to_owned(),
                latest_active_event_id: Some("$keep:example".to_owned()),
                fully_read_event_id: None,
                marked_unread: true,
                num_unread: 1,
                num_notifications: 0,
                num_mentions: 0,
            },
        );
        state.model.room_read_states.insert(
            "!stale:example".to_owned(),
            RoomReadState {
                room_id: "!stale:example".to_owned(),
                latest_active_event_id: Some("$stale:example".to_owned()),
                fully_read_event_id: None,
                marked_unread: false,
                num_unread: 0,
                num_notifications: 0,
                num_mentions: 0,
            },
        );

        apply_core_event(
            &mut state,
            Event::SyncedRooms(vec![
                room("!keep:example", "Keep", false),
                space("!space:example", "Space"),
            ]),
        )
        .unwrap();

        assert!(state.model.room_read_states.contains_key("!keep:example"));
        assert!(!state.model.room_read_states.contains_key("!stale:example"));
    }

    #[test]
    fn synced_rooms_prune_room_read_states_when_room_becomes_space() {
        let mut state = test_state();
        state.model.room_read_states.insert(
            "!room:example".to_owned(),
            RoomReadState {
                room_id: "!room:example".to_owned(),
                latest_active_event_id: Some("$room:example".to_owned()),
                fully_read_event_id: None,
                marked_unread: true,
                num_unread: 1,
                num_notifications: 0,
                num_mentions: 0,
            },
        );

        apply_core_event(
            &mut state,
            Event::SyncedRooms(vec![space("!room:example", "Room Space")]),
        )
        .unwrap();

        assert!(!state.model.room_read_states.contains_key("!room:example"));
    }

    #[test]
    fn synced_rooms_prune_space_room_activity() {
        let mut state = test_state();
        state.room_activity.insert(room_view_id("!keep:example"));
        state.room_activity.insert(room_view_id("!space:example"));
        state
            .room_activity
            .insert(thread_view_id("!space:example", "$thread-root"));

        apply_core_event(
            &mut state,
            Event::SyncedRooms(vec![
                room("!keep:example", "Keep", false),
                space("!space:example", "Space"),
            ]),
        )
        .unwrap();

        assert!(state.room_activity.contains(&room_view_id("!keep:example")));
        assert!(
            !state
                .room_activity
                .contains(&room_view_id("!space:example"))
        );
        assert!(
            !state
                .room_activity
                .contains(&thread_view_id("!space:example", "$thread-root"))
        );
    }

    #[test]
    fn synced_rooms_prune_space_room_activity_read_targets() {
        let mut state = test_state();
        state.room_activity.insert(room_view_id("!keep:example"));
        state.room_activity.insert(room_view_id("!space:example"));
        state
            .room_activity_read_targets
            .insert(room_view_id("!keep:example"), "$keep:example".to_owned());
        state
            .room_activity_read_targets
            .insert(room_view_id("!space:example"), "$space:example".to_owned());
        state.room_activity_read_targets.insert(
            thread_view_id("!space:example", "$thread-root"),
            "$thread-1".to_owned(),
        );

        apply_core_event(
            &mut state,
            Event::SyncedRooms(vec![
                room("!keep:example", "Keep", false),
                space("!space:example", "Space"),
            ]),
        )
        .unwrap();

        assert_eq!(
            state
                .room_activity_read_targets
                .get(&room_view_id("!keep:example"))
                .map(String::as_str),
            Some("$keep:example")
        );
        assert!(
            !state
                .room_activity_read_targets
                .contains_key(&room_view_id("!space:example"))
        );
        assert!(
            !state
                .room_activity_read_targets
                .contains_key(&thread_view_id("!space:example", "$thread-root"))
        );
    }

    #[test]
    fn logged_out_clears_room_activity_read_targets() {
        let mut state = test_state();
        state.room_activity.insert(room_view_id("!room:example"));
        state
            .room_activity_read_targets
            .insert(room_view_id("!room:example"), "$event:example".to_owned());

        with_local_app_data(|_| {
            apply_core_event(&mut state, Event::LoggedOut)
                .expect("logged out event must apply successfully");
        });

        assert!(state.room_activity_read_targets.is_empty());
    }

    #[test]
    fn logged_out_clears_room_activity() {
        let mut state = test_state();
        state.room_activity.insert(room_view_id("!room:example"));

        with_local_app_data(|_| {
            apply_core_event(&mut state, Event::LoggedOut)
                .expect("logged out event must apply successfully");
        });

        assert!(state.room_activity.is_empty());
    }

    #[test]
    fn logged_out_clears_room_read_states_before_same_room_id_returns() {
        let mut state = test_state();
        state.model.room_read_states.insert(
            "!room:example".to_owned(),
            RoomReadState {
                room_id: "!room:example".to_owned(),
                latest_active_event_id: Some("$room:example".to_owned()),
                fully_read_event_id: Some("$fully-read:example".to_owned()),
                marked_unread: true,
                num_unread: 1,
                num_notifications: 2,
                num_mentions: 1,
            },
        );

        with_local_app_data(|_| {
            apply_core_event(&mut state, Event::LoggedOut)
                .expect("logged out event must apply successfully");
        });
        apply_core_event(&mut state, Event::Started).unwrap();
        apply_core_event(
            &mut state,
            Event::SyncedRooms(vec![room("!room:example", "Room", false)]),
        )
        .unwrap();

        assert!(!state.model.room_read_states.contains_key("!room:example"));
    }

    #[test]
    fn room_activation_with_read_target_enqueues_mark_room_read() {
        let mut state = test_state();
        let handle = start_test_core();
        state.command_sender = Some(handle.command_sender().clone());
        state.model.rooms = vec![room("!room:example", "Room", false)];
        state.room_activity.insert(room_view_id("!room:example"));
        state
            .room_activity_read_targets
            .insert(room_view_id("!room:example"), "$event:example".to_owned());

        assert_eq!(
            handle_room_activation(&mut state, "!room:example"),
            Ok(true)
        );
        assert!(!state.room_activity.contains(&room_view_id("!room:example")));
        assert!(
            !state
                .room_activity_read_targets
                .contains_key(&room_view_id("!room:example"))
        );
        assert_eq!(
            state.recorded_commands,
            vec![Command::MarkRoomRead(MarkRoomReadRequest {
                room_id: "!room:example".to_owned(),
                event_id: "$event:example".to_owned(),
            })]
        );

        handle
            .command_sender()
            .send(Command::Shutdown)
            .expect("shutdown command must enqueue");
        handle.join().expect("test core must stop cleanly");
    }

    #[test]
    fn room_activation_without_room_activity_discards_stale_read_target_without_command() {
        let mut state = test_state();
        let handle = start_test_core();
        state.command_sender = Some(handle.command_sender().clone());
        state.model.rooms = vec![room("!room:example", "Room", false)];
        state
            .room_activity_read_targets
            .insert(room_view_id("!room:example"), "$event:example".to_owned());

        assert_eq!(
            handle_room_activation(&mut state, "!room:example"),
            Ok(false)
        );
        assert!(!state.room_activity.contains(&room_view_id("!room:example")));
        assert!(
            !state
                .room_activity_read_targets
                .contains_key(&room_view_id("!room:example"))
        );
        assert!(state.recorded_commands.is_empty());

        handle
            .command_sender()
            .send(Command::Shutdown)
            .expect("shutdown command must enqueue");
        handle.join().expect("test core must stop cleanly");
    }

    #[test]
    fn room_activation_without_read_target_clears_room_activity_without_command() {
        let mut state = test_state();
        let handle = start_test_core();
        state.command_sender = Some(handle.command_sender().clone());
        state.model.rooms = vec![room("!room:example", "Room", false)];
        state.room_activity.insert(room_view_id("!room:example"));

        assert_eq!(
            handle_room_activation(&mut state, "!room:example"),
            Ok(true)
        );
        assert!(!state.room_activity.contains(&room_view_id("!room:example")));
        assert!(state.recorded_commands.is_empty());

        handle
            .command_sender()
            .send(Command::Shutdown)
            .expect("shutdown command must enqueue");
        handle.join().expect("test core must stop cleanly");
    }

    #[test]
    fn thread_activation_with_read_target_clears_only_thread_activity_and_enqueues_mark_room_read()
    {
        let mut state = test_state();
        let handle = start_test_core();
        state.command_sender = Some(handle.command_sender().clone());
        state.model.rooms = vec![room("!room:example", "Room", false)];
        state.room_activity.insert(room_view_id("!room:example"));
        state
            .room_activity
            .insert(thread_view_id("!room:example", "$thread-root"));
        state.room_activity_read_targets.insert(
            thread_view_id("!room:example", "$thread-root"),
            "$thread-1".to_owned(),
        );

        assert_eq!(
            handle_thread_activation(&mut state, "!room:example", "$thread-root"),
            Ok(true)
        );
        assert!(state.room_activity.contains(&room_view_id("!room:example")));
        assert!(
            !state
                .room_activity
                .contains(&thread_view_id("!room:example", "$thread-root"))
        );
        assert!(
            !state
                .room_activity_read_targets
                .contains_key(&thread_view_id("!room:example", "$thread-root"))
        );
        assert_eq!(
            state.recorded_commands,
            vec![Command::MarkRoomRead(MarkRoomReadRequest {
                room_id: "!room:example".to_owned(),
                event_id: "$thread-1".to_owned(),
            })]
        );

        handle
            .command_sender()
            .send(Command::Shutdown)
            .expect("shutdown command must enqueue");
        handle.join().expect("test core must stop cleanly");
    }

    #[test]
    fn room_activation_leaves_thread_activity_intact() {
        let mut state = test_state();
        let handle = start_test_core();
        state.command_sender = Some(handle.command_sender().clone());
        state.model.rooms = vec![room("!room:example", "Room", false)];
        state.room_activity.insert(room_view_id("!room:example"));
        state
            .room_activity
            .insert(thread_view_id("!room:example", "$thread-root"));

        assert_eq!(
            handle_room_activation(&mut state, "!room:example"),
            Ok(true)
        );
        assert!(!state.room_activity.contains(&room_view_id("!room:example")));
        assert!(
            state
                .room_activity
                .contains(&thread_view_id("!room:example", "$thread-root"))
        );
        assert!(state.recorded_commands.is_empty());

        handle
            .command_sender()
            .send(Command::Shutdown)
            .expect("shutdown command must enqueue");
        handle.join().expect("test core must stop cleanly");
    }

    #[test]
    fn unregister_view_by_child_removes_destroyed_room_from_registries() {
        let mut state = test_state();
        state.model.rooms = vec![room("!alpha:example", "Alpha", false)];
        state.active_view = ViewId::Room("!beta:example".to_owned());
        state.views.insert(ViewId::Status, tracked_view(10));
        state
            .views
            .insert(ViewId::Room("!beta:example".to_owned()), tracked_view(20));
        state.hwnd_views.insert(10, ViewId::Status);
        state
            .hwnd_views
            .insert(20, ViewId::Room("!beta:example".to_owned()));

        let removed = unregister_view_by_child(&mut state, 20 as HWND);

        assert_eq!(removed, Some(ViewId::Room("!beta:example".to_owned())));
        assert!(!state.hwnd_views.contains_key(&20));
        assert!(
            !state
                .views
                .contains_key(&ViewId::Room("!beta:example".to_owned()))
        );
        assert_eq!(state.active_view, ViewId::Room("!alpha:example".to_owned()));
    }

    #[test]
    fn select_room_rejects_spaces_without_changing_existing_selection() {
        let mut state = test_state();
        state.model.rooms = vec![
            room("!room:example", "Room", false),
            space("!space:example", "Space"),
        ];

        assert_eq!(select_room(&mut state, "!room:example"), Ok(()));
        assert_eq!(state.model.selected_room, Some("!room:example".to_owned()));
        assert_eq!(
            select_room(&mut state, "!space:example"),
            Err("unknown room id: !space:example".to_owned())
        );
        assert_eq!(
            select_room(&mut state, "!missing:example"),
            Err("unknown room id: !missing:example".to_owned())
        );
        assert_eq!(state.model.selected_room, Some("!room:example".to_owned()));
    }

    #[test]
    fn parse_compose_submission_distinguishes_text_emote_literal_slash_and_commands() {
        assert_eq!(
            parse_compose_submission("hello"),
            ComposeSubmission::Message {
                presentation: RoomMessagePresentationKind::Text,
                body: Cow::Borrowed("hello"),
            }
        );
        assert_eq!(
            parse_compose_submission("/me waves"),
            ComposeSubmission::Message {
                presentation: RoomMessagePresentationKind::Emote,
                body: Cow::Borrowed("waves"),
            }
        );
        assert_eq!(
            parse_compose_submission("/ME    waves"),
            ComposeSubmission::Message {
                presentation: RoomMessagePresentationKind::Emote,
                body: Cow::Borrowed("waves"),
            }
        );
        assert_eq!(
            parse_compose_submission("/me"),
            ComposeSubmission::LocalCommand(LocalSlashCommand::Invalid {
                feedback: "usage: /me <action>".to_owned(),
            })
        );
        assert_eq!(
            parse_compose_submission("/me    "),
            ComposeSubmission::LocalCommand(LocalSlashCommand::Invalid {
                feedback: "usage: /me <action>".to_owned(),
            })
        );
        assert_eq!(
            parse_compose_submission("//me waves"),
            ComposeSubmission::Message {
                presentation: RoomMessagePresentationKind::Text,
                body: Cow::Owned("/me waves".to_owned()),
            }
        );
        assert_eq!(
            parse_compose_submission("//status"),
            ComposeSubmission::Message {
                presentation: RoomMessagePresentationKind::Text,
                body: Cow::Owned("/status".to_owned()),
            }
        );
        assert_eq!(
            parse_compose_submission("//find needle"),
            ComposeSubmission::Message {
                presentation: RoomMessagePresentationKind::Text,
                body: Cow::Owned("/find needle".to_owned()),
            }
        );
        assert_eq!(
            parse_compose_submission("//react $react-normal +1"),
            ComposeSubmission::Message {
                presentation: RoomMessagePresentationKind::Text,
                body: Cow::Owned("/react $react-normal +1".to_owned()),
            }
        );
        assert_eq!(
            parse_compose_submission("//edit $event replacement text"),
            ComposeSubmission::Message {
                presentation: RoomMessagePresentationKind::Text,
                body: Cow::Owned("/edit $event replacement text".to_owned()),
            }
        );
        assert_eq!(
            parse_compose_submission("//reply $event thanks"),
            ComposeSubmission::Message {
                presentation: RoomMessagePresentationKind::Text,
                body: Cow::Owned("/reply $event thanks".to_owned()),
            }
        );
        assert_eq!(
            parse_compose_submission("//accept"),
            ComposeSubmission::Message {
                presentation: RoomMessagePresentationKind::Text,
                body: Cow::Owned("/accept".to_owned()),
            }
        );
        assert_eq!(
            parse_compose_submission("//reject"),
            ComposeSubmission::Message {
                presentation: RoomMessagePresentationKind::Text,
                body: Cow::Owned("/reject".to_owned()),
            }
        );
        assert_eq!(
            parse_compose_submission("///status"),
            ComposeSubmission::Message {
                presentation: RoomMessagePresentationKind::Text,
                body: Cow::Owned("//status".to_owned()),
            }
        );
        assert_eq!(
            parse_compose_submission("/status"),
            ComposeSubmission::LocalCommand(LocalSlashCommand::Status)
        );
        assert_eq!(
            parse_compose_submission("/"),
            ComposeSubmission::LocalCommand(LocalSlashCommand::Invalid {
                feedback: "unknown command: /".to_owned(),
            })
        );
    }

    #[test]
    fn invalid_e2ee_recover_form_is_not_recorded_or_recalled_in_compose_history() {
        let mut state = test_state();
        let input = "/e2ee recover secret phrase".to_owned();
        let ComposeSubmission::LocalCommand(command) = parse_compose_submission(&input) else {
            panic!("expected local command submission");
        };

        let history_entry = local_command_history_entry(&input, &command);
        assert_eq!(history_entry, None);
        if let Some(entry) = history_entry {
            state.compose_history.record_submission(entry);
        }

        state
            .compose_history
            .record_submission("safe draft".to_owned());

        assert!(
            !state
                .compose_history
                .entries
                .iter()
                .any(|entry| entry.contains("secret"))
        );
        assert_eq!(
            state.compose_history.previous("later draft".to_owned()),
            Some("safe draft")
        );
        assert_eq!(
            state.compose_history.previous("ignored".to_owned()),
            Some("safe draft")
        );
    }

    #[test]
    fn parse_slash_command_accepts_verify_forms_and_rejects_bad_forms() {
        assert_eq!(parse_slash_command("/StAtUs"), LocalSlashCommand::Status);
        assert_eq!(
            parse_slash_command("/FIND Mixed Case"),
            LocalSlashCommand::Find {
                query: "Mixed Case".to_owned(),
            }
        );
        assert_eq!(
            parse_slash_command("/e2ee"),
            LocalSlashCommand::E2ee(E2eeSlashCommand::ShowStatus)
        );
        assert_eq!(
            parse_slash_command("/e2ee status"),
            LocalSlashCommand::E2ee(E2eeSlashCommand::ShowStatus)
        );
        assert_eq!(
            parse_slash_command("/e2ee refresh"),
            LocalSlashCommand::E2ee(E2eeSlashCommand::Refresh)
        );
        assert_eq!(
            parse_slash_command("/e2ee recover"),
            LocalSlashCommand::E2ee(E2eeSlashCommand::RecoverDialog)
        );
        assert_eq!(
            parse_slash_command("/verify accept"),
            LocalSlashCommand::Verify {
                action: VerificationAction::Accept,
            }
        );
        assert_eq!(
            parse_slash_command("/verify cancel"),
            LocalSlashCommand::Verify {
                action: VerificationAction::Cancel,
            }
        );
        assert_eq!(
            parse_slash_command("/verify confirm"),
            LocalSlashCommand::Verify {
                action: VerificationAction::Confirm,
            }
        );
        assert_eq!(
            parse_slash_command("/verify reject"),
            LocalSlashCommand::Verify {
                action: VerificationAction::Reject,
            }
        );
        assert_eq!(
            parse_slash_command("/wat now"),
            LocalSlashCommand::Invalid {
                feedback: "unknown command: /wat".to_owned(),
            }
        );
        assert_eq!(
            parse_slash_command("/status now"),
            LocalSlashCommand::Invalid {
                feedback: "usage: /status".to_owned(),
            }
        );
        assert_eq!(
            parse_slash_command("/find"),
            LocalSlashCommand::Invalid {
                feedback: "usage: /find <text>".to_owned(),
            }
        );
        assert_eq!(
            parse_slash_command("/window"),
            LocalSlashCommand::Invalid {
                feedback: "usage: /window <room-id-or-display-name>".to_owned(),
            }
        );
        assert_eq!(
            parse_slash_command("/clear now"),
            LocalSlashCommand::Invalid {
                feedback: "usage: /clear".to_owned(),
            }
        );
        let rejected = parse_slash_command("/e2ee recover secret");
        assert_eq!(
            rejected,
            LocalSlashCommand::Invalid {
                feedback: "usage: /e2ee [status|refresh|recover]".to_owned(),
            }
        );
        match rejected {
            LocalSlashCommand::Invalid { feedback } => {
                assert!(!feedback.contains("secret"));
            }
            other => panic!("expected invalid e2ee usage, got {other:?}"),
        }
        assert_eq!(
            parse_slash_command("/verify"),
            LocalSlashCommand::Invalid {
                feedback: "usage: /verify accept|cancel|confirm|reject".to_owned(),
            }
        );
        assert_eq!(
            parse_slash_command("/verify maybe"),
            LocalSlashCommand::Invalid {
                feedback: "usage: /verify accept|cancel|confirm|reject".to_owned(),
            }
        );
    }

    #[test]
    fn parse_slash_command_accepts_join_and_part_forms() {
        assert_eq!(
            parse_slash_command("/join #ops:example.org"),
            LocalSlashCommand::Join {
                target: "#ops:example.org".to_owned(),
            }
        );
        assert_eq!(
            parse_slash_command("/join !room:example.org"),
            LocalSlashCommand::Join {
                target: "!room:example.org".to_owned(),
            }
        );
        assert_eq!(
            parse_slash_command("/query @alice:example.org"),
            LocalSlashCommand::Query {
                user_id: "@alice:example.org".to_owned(),
            }
        );
        assert_eq!(
            parse_slash_command("/query   @alice:example.org   "),
            LocalSlashCommand::Query {
                user_id: "@alice:example.org".to_owned(),
            }
        );
        assert_eq!(
            parse_slash_command("/find needle"),
            LocalSlashCommand::Find {
                query: "needle".to_owned(),
            }
        );
        assert_eq!(
            parse_slash_command("/find   two   words   here   "),
            LocalSlashCommand::Find {
                query: "two   words   here".to_owned(),
            }
        );
        assert_eq!(
            parse_slash_command("/part"),
            LocalSlashCommand::Part { target: None }
        );
        assert_eq!(
            parse_slash_command("/part !room:example.org"),
            LocalSlashCommand::Part {
                target: Some("!room:example.org".to_owned()),
            }
        );
        assert_eq!(
            parse_slash_command("/react $react-normal +1"),
            LocalSlashCommand::React {
                target_event_id: "$react-normal".to_owned(),
                key: "+1".to_owned(),
            }
        );
        assert_eq!(
            parse_slash_command("/react $react-normal eyes"),
            LocalSlashCommand::React {
                target_event_id: "$react-normal".to_owned(),
                key: "eyes".to_owned(),
            }
        );
        assert_eq!(
            parse_slash_command("/react $react-normal two words"),
            LocalSlashCommand::React {
                target_event_id: "$react-normal".to_owned(),
                key: "two words".to_owned(),
            }
        );
        assert_eq!(
            parse_slash_command("/edit $event-to-edit replacement text"),
            LocalSlashCommand::Edit {
                target_event_id: "$event-to-edit".to_owned(),
                body: "replacement text".to_owned(),
            }
        );
        assert_eq!(
            parse_slash_command("/reply $event-to-reply hello there"),
            LocalSlashCommand::Reply {
                target_event_id: "$event-to-reply".to_owned(),
                body: "hello there".to_owned(),
            }
        );
        assert_eq!(
            parse_slash_command("/EDIT   $event-to-edit   replacement text   "),
            LocalSlashCommand::Edit {
                target_event_id: "$event-to-edit".to_owned(),
                body: "replacement text".to_owned(),
            }
        );
        assert_eq!(
            parse_slash_command("/REPLY   $event-to-reply   hello there   "),
            LocalSlashCommand::Reply {
                target_event_id: "$event-to-reply".to_owned(),
                body: "hello there".to_owned(),
            }
        );
        assert_eq!(
            parse_slash_command("/unreact $reaction-alice-plus-one"),
            LocalSlashCommand::Unreact {
                reaction_event_id: "$reaction-alice-plus-one".to_owned(),
            }
        );
        assert_eq!(
            parse_slash_command("/redact $redaction-target"),
            LocalSlashCommand::Redact {
                event_id: "$redaction-target".to_owned(),
            }
        );
        assert_eq!(
            parse_slash_command("/topic incident review"),
            LocalSlashCommand::Topic {
                topic: "incident review".to_owned(),
            }
        );
        assert_eq!(
            parse_slash_command("/topic   incident review   "),
            LocalSlashCommand::Topic {
                topic: "incident review".to_owned(),
            }
        );
    }

    #[test]
    fn parse_slash_command_accepts_accept_and_reject_forms() {
        assert_eq!(
            parse_slash_command("/accept"),
            LocalSlashCommand::Accept { target: None }
        );
        assert_eq!(
            parse_slash_command("/accept    "),
            LocalSlashCommand::Accept { target: None }
        );
        assert_eq!(
            parse_slash_command("/accept !room:example.org"),
            LocalSlashCommand::Accept {
                target: Some("!room:example.org".to_owned()),
            }
        );
        assert_eq!(
            parse_slash_command("/accept Invite Name"),
            LocalSlashCommand::Accept {
                target: Some("Invite Name".to_owned()),
            }
        );
        assert_eq!(
            parse_slash_command("/reject"),
            LocalSlashCommand::Reject { target: None }
        );
        assert_eq!(
            parse_slash_command("/reject Invite Name"),
            LocalSlashCommand::Reject {
                target: Some("Invite Name".to_owned()),
            }
        );
    }

    #[test]
    fn parse_slash_command_keeps_verification_accept_and_reject_distinct_from_invite_commands() {
        assert_eq!(
            parse_slash_command("/verify accept"),
            LocalSlashCommand::Verify {
                action: VerificationAction::Accept,
            }
        );
        assert_eq!(
            parse_slash_command("/verify reject"),
            LocalSlashCommand::Verify {
                action: VerificationAction::Reject,
            }
        );
    }

    #[test]
    fn parse_slash_command_rejects_join_without_target() {
        assert_eq!(
            parse_slash_command("/join"),
            LocalSlashCommand::Invalid {
                feedback: "usage: /join <room-id-or-alias>".to_owned(),
            }
        );
        assert_eq!(
            parse_slash_command("/query"),
            LocalSlashCommand::Invalid {
                feedback: "usage: /query <mxid>".to_owned(),
            }
        );
        assert_eq!(
            parse_slash_command("/query   "),
            LocalSlashCommand::Invalid {
                feedback: "usage: /query <mxid>".to_owned(),
            }
        );
        assert_eq!(
            parse_slash_command("/find   "),
            LocalSlashCommand::Invalid {
                feedback: "usage: /find <text>".to_owned(),
            }
        );
        assert_eq!(
            parse_slash_command("/topic"),
            LocalSlashCommand::Invalid {
                feedback: "usage: /topic <text>".to_owned(),
            }
        );
        assert_eq!(
            parse_slash_command("/topic   "),
            LocalSlashCommand::Invalid {
                feedback: "usage: /topic <text>".to_owned(),
            }
        );
        assert_eq!(
            parse_slash_command("/react"),
            LocalSlashCommand::Invalid {
                feedback: "usage: /react <event-id> <key>".to_owned(),
            }
        );
        assert_eq!(
            parse_slash_command("/react $react-normal"),
            LocalSlashCommand::Invalid {
                feedback: "usage: /react <event-id> <key>".to_owned(),
            }
        );
        assert_eq!(
            parse_slash_command("/react $react-normal   "),
            LocalSlashCommand::Invalid {
                feedback: "usage: /react <event-id> <key>".to_owned(),
            }
        );
        assert_eq!(
            parse_slash_command("/edit"),
            LocalSlashCommand::Invalid {
                feedback: "usage: /edit <event-id> <replacement text>".to_owned(),
            }
        );
        assert_eq!(
            parse_slash_command("/edit $event-to-edit"),
            LocalSlashCommand::Invalid {
                feedback: "usage: /edit <event-id> <replacement text>".to_owned(),
            }
        );
        assert_eq!(
            parse_slash_command("/edit $event-to-edit   "),
            LocalSlashCommand::Invalid {
                feedback: "usage: /edit <event-id> <replacement text>".to_owned(),
            }
        );
        assert_eq!(
            parse_slash_command("/reply"),
            LocalSlashCommand::Invalid {
                feedback: "usage: /reply <event-id> <message>".to_owned(),
            }
        );
        assert_eq!(
            parse_slash_command("/reply $event-to-reply"),
            LocalSlashCommand::Invalid {
                feedback: "usage: /reply <event-id> <message>".to_owned(),
            }
        );
        assert_eq!(
            parse_slash_command("/reply $event-to-reply   "),
            LocalSlashCommand::Invalid {
                feedback: "usage: /reply <event-id> <message>".to_owned(),
            }
        );
        assert_eq!(
            parse_slash_command("/unreact"),
            LocalSlashCommand::Invalid {
                feedback: "usage: /unreact <reaction-event-id>".to_owned(),
            }
        );
        assert_eq!(
            parse_slash_command("/unreact $one $two"),
            LocalSlashCommand::Invalid {
                feedback: "usage: /unreact <reaction-event-id>".to_owned(),
            }
        );
        assert_eq!(
            parse_slash_command("/redact"),
            LocalSlashCommand::Invalid {
                feedback: "usage: /redact <event-id>".to_owned(),
            }
        );
        assert_eq!(
            parse_slash_command("/redact   "),
            LocalSlashCommand::Invalid {
                feedback: "usage: /redact <event-id>".to_owned(),
            }
        );
        assert_eq!(
            parse_slash_command("/redact $event extra"),
            LocalSlashCommand::Invalid {
                feedback: "usage: /redact <event-id>".to_owned(),
            }
        );
    }

    #[test]
    fn resolve_room_command_target_accepts_room_id_and_unique_display_name() {
        let rooms = vec![
            room("!alpha:example", "Alpha", false),
            room("!beta:example", "Beta", false),
            room("!alpha-target:example", "!alpha:example", false),
        ];

        assert_eq!(
            resolve_room_command_target(&rooms, "!alpha:example"),
            Ok("!alpha:example".to_owned())
        );
        assert_eq!(
            resolve_room_command_target(&rooms, "Beta"),
            Ok("!beta:example".to_owned())
        );
    }

    #[test]
    fn resolve_room_command_target_rejects_unknown_spaces_and_ambiguous_display_names() {
        let rooms = vec![
            space("!work:example", "Work"),
            room("!shared-one:example", "Shared", false),
            room("!shared-two:example", "Shared", false),
            room("!alpha:example", "Alpha", false),
        ];

        assert_eq!(
            resolve_room_command_target(&rooms, "!work:example"),
            Err(WindowTargetError::Unknown)
        );
        assert_eq!(
            resolve_room_command_target(&rooms, "Work"),
            Err(WindowTargetError::Unknown)
        );
        assert_eq!(
            resolve_room_command_target(&rooms, "Missing"),
            Err(WindowTargetError::Unknown)
        );
        assert_eq!(
            resolve_room_command_target(&rooms, "Shared"),
            Err(WindowTargetError::Ambiguous)
        );
    }

    #[test]
    fn resolve_direct_room_query_target_distinguishes_none_single_and_ambiguous() {
        let mut single_room = room("!single:example", "Single", false);
        single_room.is_direct = true;
        single_room.direct_user_ids = vec!["@alice:example.org".to_owned()];

        let mut ambiguous_one = room("!ambiguous-one:example", "Ambiguous One", false);
        ambiguous_one.is_direct = true;
        ambiguous_one.direct_user_ids = vec!["@carol:example.org".to_owned()];

        let mut ambiguous_two = room("!ambiguous-two:example", "Ambiguous Two", false);
        ambiguous_two.is_direct = true;
        ambiguous_two.direct_user_ids = vec![
            "@carol:example.org".to_owned(),
            "@other:example.org".to_owned(),
        ];

        let model = UiModel {
            rooms: vec![
                room("!plain:example", "Plain", false),
                single_room,
                ambiguous_one,
                ambiguous_two,
            ],
            ..Default::default()
        };

        assert_eq!(
            resolve_direct_room_query_target(&model, "@missing:example.org"),
            QueryTargetResolution::None
        );
        assert_eq!(
            resolve_direct_room_query_target(&model, "@alice:example.org"),
            QueryTargetResolution::Single("!single:example".to_owned())
        );
        assert_eq!(
            resolve_direct_room_query_target(&model, "@carol:example.org"),
            QueryTargetResolution::Ambiguous
        );
    }

    #[test]
    fn resolve_invite_command_room_id_rejects_ambiguous_joined_unknown_and_missing_active_invites()
    {
        let mut state = test_state();
        state.model.rooms = vec![
            invited_room("!invite-one:example", "Shared"),
            invited_room("!invite-two:example", "Shared"),
            invited_room("!invite-unique:example", "Unique"),
            room("!joined:example", "Joined", false),
        ];

        assert_eq!(
            resolve_invite_command_room_id(&state, "accept", Some("Shared")),
            Err("accept failed: ambiguous invited room target: Shared".to_owned())
        );
        assert_eq!(
            resolve_invite_command_room_id(&state, "accept", Some("!joined:example")),
            Err("accept failed: room is not invited: !joined:example".to_owned())
        );
        assert_eq!(
            resolve_invite_command_room_id(&state, "accept", Some("Joined")),
            Err("accept failed: room is not invited: Joined".to_owned())
        );
        assert_eq!(
            resolve_invite_command_room_id(&state, "accept", Some("Missing")),
            Err("accept failed: invited room not found: Missing".to_owned())
        );
        assert_eq!(
            resolve_invite_command_room_id(&state, "accept", None),
            Err("accept failed: no invited room selected".to_owned())
        );

        state.active_view = room_view_id("!joined:example");
        assert_eq!(
            resolve_invite_command_room_id(&state, "reject", None),
            Err("reject failed: no invited room selected".to_owned())
        );
    }

    #[test]
    fn submit_join_slash_command_records_typed_join_and_returns_status() {
        let (mut state, handle) = test_state_with_command_sender();

        let status = submit_join_slash_command(&mut state, "#ops:example.org".to_owned()).unwrap();

        assert_eq!(status, "join submitted: #ops:example.org");
        assert_eq!(
            state.recorded_commands,
            vec![Command::JoinRoom(JoinRoomRequest {
                room_or_alias: "#ops:example.org".to_owned(),
            })]
        );

        stop_test_core(handle);
    }

    #[test]
    fn submit_accept_slash_command_records_typed_accept_and_returns_status() {
        let (mut state, handle) = test_state_with_command_sender();
        state.model.rooms = vec![invited_room("!invite:example", "Invite")];
        state.active_view = room_view_id("!invite:example");

        let status = submit_accept_slash_command(&mut state, None).unwrap();

        assert_eq!(status, "accept submitted: !invite:example");
        assert_eq!(
            state.recorded_commands,
            vec![Command::AcceptRoomInvite(AcceptRoomInviteRequest {
                room_id: "!invite:example".to_owned(),
            })]
        );
        assert_eq!(
            dump_model_from_state(&state).submitted_membership_commands,
            vec![DumpSubmittedMembershipCommand {
                action: DumpMembershipCommandAction::AcceptInvite,
                target: "!invite:example".to_owned(),
            }]
        );

        stop_test_core(handle);
    }

    #[test]
    fn submit_reject_slash_command_records_typed_reject_and_returns_status() {
        let (mut state, handle) = test_state_with_command_sender();
        state.model.rooms = vec![invited_room("!invite:example", "Invite")];
        state.active_view = room_view_id("!invite:example");

        let status = submit_reject_slash_command(&mut state, None).unwrap();

        assert_eq!(status, "reject submitted: !invite:example");
        assert_eq!(
            state.recorded_commands,
            vec![Command::RejectRoomInvite(RejectRoomInviteRequest {
                room_id: "!invite:example".to_owned(),
            })]
        );
        assert_eq!(
            dump_model_from_state(&state).submitted_membership_commands,
            vec![DumpSubmittedMembershipCommand {
                action: DumpMembershipCommandAction::RejectInvite,
                target: "!invite:example".to_owned(),
            }]
        );

        stop_test_core(handle);
    }

    #[test]
    fn submit_query_slash_command_records_typed_create_direct_room_and_returns_status() {
        let (mut state, handle) = test_state_with_command_sender();

        let status =
            submit_query_slash_command(&mut state, "@alice:example.org".to_owned()).unwrap();

        assert_eq!(status, "query submitted: @alice:example.org");
        assert_eq!(
            state.recorded_commands,
            vec![Command::CreateDirectRoom(CreateDirectRoomRequest {
                user_id: "@alice:example.org".to_owned(),
            })]
        );

        stop_test_core(handle);
    }

    #[test]
    fn room_history_backfill_submit_records_initial_loading_command() {
        let (mut state, handle) = test_state_with_command_sender();
        state.model.rooms = vec![room("!room:example", "Room", false)];

        assert_eq!(
            submit_backfill_room_history_for_room(&mut state, "!room:example"),
            Ok(true)
        );
        assert_eq!(
            state.recorded_commands,
            vec![Command::BackfillRoomHistory(BackfillRoomHistoryRequest {
                room_id: "!room:example".to_owned(),
                from: None,
                limit: ROOM_HISTORY_BACKFILL_DEFAULT_LIMIT,
            })]
        );
        assert_eq!(
            state
                .model
                .room_views
                .get("!room:example")
                .map(|room_view| &room_view.history),
            Some(&RoomHistoryBackfillState {
                next_token: None,
                status: RoomHistoryBackfillStatus::Loading { from: None },
            })
        );

        stop_test_core(handle);
    }

    #[test]
    fn room_history_backfill_submit_ignores_repeat_submit_while_loading() {
        let (mut state, handle) = test_state_with_command_sender();
        state.model.rooms = vec![room("!room:example", "Room", false)];

        assert_eq!(
            submit_backfill_room_history_for_room(&mut state, "!room:example"),
            Ok(true)
        );
        assert_eq!(
            submit_backfill_room_history_for_room(&mut state, "!room:example"),
            Ok(false)
        );
        assert_eq!(state.recorded_commands.len(), 1);
        assert_eq!(
            state
                .model
                .room_views
                .get("!room:example")
                .map(|room_view| &room_view.history),
            Some(&RoomHistoryBackfillState {
                next_token: None,
                status: RoomHistoryBackfillStatus::Loading { from: None },
            })
        );

        stop_test_core(handle);
    }

    #[test]
    fn room_history_backfill_success_sets_next_token_and_reuses_it_on_retry() {
        let (mut state, handle) = test_state_with_command_sender();
        state.model.rooms = vec![room("!room:example", "Room", false)];

        assert_eq!(
            submit_backfill_room_history_for_room(&mut state, "!room:example"),
            Ok(true)
        );
        apply_core_event(
            &mut state,
            room_history_backfill(
                "!room:example",
                "token-1",
                Some("token-2"),
                vec![history_backfill_event_from_event(timeline(
                    "!room:example",
                    "older row",
                ))],
            ),
        )
        .unwrap();

        assert_eq!(
            state
                .model
                .room_views
                .get("!room:example")
                .map(|room_view| &room_view.history),
            Some(&RoomHistoryBackfillState {
                next_token: Some("token-2".to_owned()),
                status: RoomHistoryBackfillStatus::Idle,
            })
        );

        assert_eq!(
            submit_backfill_room_history_for_room(&mut state, "!room:example"),
            Ok(true)
        );
        assert_eq!(
            state.recorded_commands.last(),
            Some(&Command::BackfillRoomHistory(BackfillRoomHistoryRequest {
                room_id: "!room:example".to_owned(),
                from: Some("token-2".to_owned()),
                limit: ROOM_HISTORY_BACKFILL_DEFAULT_LIMIT,
            }))
        );

        stop_test_core(handle);
    }

    #[test]
    fn room_history_backfill_success_with_no_end_exhausts_room() {
        let (mut state, handle) = test_state_with_command_sender();
        state.model.rooms = vec![room("!room:example", "Room", false)];

        assert_eq!(
            submit_backfill_room_history_for_room(&mut state, "!room:example"),
            Ok(true)
        );
        apply_core_event(
            &mut state,
            room_history_backfill(
                "!room:example",
                "token-1",
                None,
                vec![history_backfill_event_from_event(timeline(
                    "!room:example",
                    "older row",
                ))],
            ),
        )
        .unwrap();

        assert_eq!(
            state
                .model
                .room_views
                .get("!room:example")
                .map(|room_view| &room_view.history),
            Some(&RoomHistoryBackfillState {
                next_token: None,
                status: RoomHistoryBackfillStatus::Exhausted,
            })
        );
        assert_eq!(
            submit_backfill_room_history_for_room(&mut state, "!room:example"),
            Ok(false)
        );
        assert_eq!(state.recorded_commands.len(), 1);

        stop_test_core(handle);
    }

    #[test]
    fn room_history_backfill_error_preserves_retry_token_and_allows_retry() {
        let (mut state, handle) = test_state_with_command_sender();
        state.model.rooms = vec![room("!room:example", "Room", false)];
        state
            .model
            .room_views
            .entry("!room:example".to_owned())
            .or_default()
            .history
            .next_token = Some("token-2".to_owned());

        assert_eq!(
            submit_backfill_room_history_for_room(&mut state, "!room:example"),
            Ok(true)
        );
        apply_core_event(
            &mut state,
            Event::Error(CoreErrorEvent {
                operation: Operation::BackfillRoomHistory,
                message: "temporary backfill failure".to_owned(),
            }),
        )
        .unwrap();

        assert_eq!(
            state
                .model
                .room_views
                .get("!room:example")
                .map(|room_view| &room_view.history),
            Some(&RoomHistoryBackfillState {
                next_token: Some("token-2".to_owned()),
                status: RoomHistoryBackfillStatus::Failed {
                    from: Some("token-2".to_owned()),
                    message: "temporary backfill failure".to_owned(),
                },
            })
        );

        assert_eq!(
            submit_backfill_room_history_for_room(&mut state, "!room:example"),
            Ok(true)
        );
        assert_eq!(
            state.recorded_commands.last(),
            Some(&Command::BackfillRoomHistory(BackfillRoomHistoryRequest {
                room_id: "!room:example".to_owned(),
                from: Some("token-2".to_owned()),
                limit: ROOM_HISTORY_BACKFILL_DEFAULT_LIMIT,
            }))
        );

        stop_test_core(handle);
    }

    #[test]
    fn submit_part_slash_command_from_active_room_records_typed_leave_and_returns_status() {
        let (mut state, handle) = test_state_with_command_sender();
        state.active_view = room_view_id("!room:example");

        let status = submit_part_slash_command(&mut state, None).unwrap();

        assert_eq!(status, "part submitted: !room:example");
        assert_eq!(
            state.recorded_commands,
            vec![Command::LeaveRoom(LeaveRoomRequest {
                room_id: "!room:example".to_owned(),
            })]
        );

        stop_test_core(handle);
    }

    #[test]
    fn submit_part_slash_command_from_thread_targets_parent_room() {
        let (mut state, handle) = test_state_with_command_sender();
        state.active_view = thread_view_id("!room:example", "$thread-root");

        let status = submit_part_slash_command(&mut state, None).unwrap();

        assert_eq!(status, "part submitted: !room:example");
        assert_eq!(
            state.recorded_commands,
            vec![Command::LeaveRoom(LeaveRoomRequest {
                room_id: "!room:example".to_owned(),
            })]
        );

        stop_test_core(handle);
    }

    #[test]
    fn submit_part_slash_command_from_status_returns_no_room_selected() {
        let mut state = test_state();

        assert_eq!(
            submit_part_slash_command(&mut state, None),
            Err("no room selected".to_owned())
        );
        assert!(state.recorded_commands.is_empty());
    }

    #[test]
    fn submit_part_slash_command_resolves_explicit_room_target_like_window() {
        let (mut state, handle) = test_state_with_command_sender();
        state.model.rooms = vec![
            room("!alpha:example", "Alpha", false),
            room("!beta:example", "Beta", false),
        ];

        let status = submit_part_slash_command(&mut state, Some("Beta")).unwrap();

        assert_eq!(status, "part submitted: !beta:example");
        assert_eq!(
            state.recorded_commands,
            vec![Command::LeaveRoom(LeaveRoomRequest {
                room_id: "!beta:example".to_owned(),
            })]
        );

        stop_test_core(handle);
    }

    #[test]
    fn submit_part_slash_command_rejects_ambiguous_display_name_without_recording() {
        let mut state = test_state();
        state.model.rooms = vec![
            room("!shared-one:example", "Shared", false),
            room("!shared-two:example", "Shared", false),
        ];

        assert_eq!(
            submit_part_slash_command(&mut state, Some("Shared")),
            Err("ambiguous part target: Shared".to_owned())
        );
        assert!(state.recorded_commands.is_empty());
    }

    #[test]
    fn submit_react_slash_command_from_active_room_records_typed_request_and_returns_status() {
        let (mut state, handle) = test_state_with_command_sender();
        state.active_view = room_view_id("!room:example");

        let status =
            submit_react_slash_command(&mut state, "$react-normal".to_owned(), "+1".to_owned())
                .unwrap();

        assert_eq!(status, "Reaction queued for $react-normal");
        assert_eq!(
            state.recorded_commands,
            vec![Command::SendReaction(SendReactionRequest {
                room_id: "!room:example".to_owned(),
                target_event_id: "$react-normal".to_owned(),
                key: "+1".to_owned(),
            })]
        );
        assert_eq!(
            dump_model_from_state(&state).submitted_reaction_commands,
            vec![DumpSubmittedReactionCommand {
                room_id: "!room:example".to_owned(),
                target_event_id: "$react-normal".to_owned(),
                key: "+1".to_owned(),
            }]
        );

        stop_test_core(handle);
    }

    #[test]
    fn submit_react_slash_command_from_thread_targets_parent_room() {
        let (mut state, handle) = test_state_with_command_sender();
        state.active_view = thread_view_id("!room:example", "$thread-root");

        let status = submit_react_slash_command(
            &mut state,
            "$react-normal".to_owned(),
            "two words".to_owned(),
        )
        .unwrap();

        assert_eq!(status, "Reaction queued for $react-normal");
        assert_eq!(
            state.recorded_commands,
            vec![Command::SendReaction(SendReactionRequest {
                room_id: "!room:example".to_owned(),
                target_event_id: "$react-normal".to_owned(),
                key: "two words".to_owned(),
            })]
        );

        stop_test_core(handle);
    }

    #[test]
    fn submit_react_slash_command_from_status_returns_no_room_selected() {
        let mut state = test_state();

        assert_eq!(
            submit_react_slash_command(&mut state, "$react-normal".to_owned(), "+1".to_owned(),),
            Err("no room selected".to_owned())
        );
        assert!(state.recorded_commands.is_empty());
    }

    #[test]
    fn submit_reply_slash_command_from_active_room_records_typed_request_and_returns_status() {
        let (mut state, handle) = test_state_with_command_sender();
        state.model.rooms = vec![room("!room:example", "Room", false)];
        state.active_view = room_view_id("!room:example");

        let status =
            submit_reply_slash_command(&mut state, "$target".to_owned(), "hello there".to_owned())
                .unwrap();

        assert_eq!(status, "Reply queued for $target");
        assert_eq!(
            state.recorded_commands,
            vec![Command::SendMessage(SendMessageRequest {
                room_id: "!room:example".to_owned(),
                presentation: RoomMessagePresentationKind::Text,
                body: "hello there".to_owned(),
                thread: None,
                reply: Some(SendMessageReplyTarget {
                    event_id: "$target".to_owned(),
                }),
            })]
        );
        assert_eq!(
            dump_model_from_state(&state).submitted_reply_commands,
            vec![DumpSubmittedReplyCommand {
                room_id: "!room:example".to_owned(),
                target_event_id: "$target".to_owned(),
                body: "hello there".to_owned(),
                thread: None,
            }]
        );

        stop_test_core(handle);
    }

    #[test]
    fn submit_reply_slash_command_from_thread_uses_compose_thread_destination() {
        let (mut state, handle) = test_state_with_command_sender();
        state.model.rooms = vec![room("!ops:example", "Ops", false)];
        apply_core_event(
            &mut state,
            timeline_with_transaction_id(
                "!ops:example",
                "$thread-root:example",
                None,
                "root msg",
                1_700_000_060_000,
            ),
        )
        .unwrap();
        apply_core_event(
            &mut state,
            timeline_with_relation_and_presentation(
                "!ops:example",
                "$thread-1:example",
                None,
                Some(thread_relation("$thread-root:example")),
                RoomMessagePresentationKind::Text,
                "durable reply",
                1_700_000_120_000,
            ),
        )
        .unwrap();
        state.active_view = thread_view_id("!ops:example", "$thread-root:example");
        state
            .room_activity
            .insert(thread_view_id("!ops:example", "$thread-root:example"));
        state.room_activity_read_targets.insert(
            thread_view_id("!ops:example", "$thread-root:example"),
            "$thread-1:example".to_owned(),
        );

        let status = submit_reply_slash_command(
            &mut state,
            "$target".to_owned(),
            "threaded acknowledgement".to_owned(),
        )
        .unwrap();

        assert_eq!(status, "Reply queued for $target");
        assert_eq!(
            state.recorded_commands,
            vec![Command::SendMessage(SendMessageRequest {
                room_id: "!ops:example".to_owned(),
                presentation: RoomMessagePresentationKind::Text,
                body: "threaded acknowledgement".to_owned(),
                thread: Some(SendMessageThreadTarget {
                    thread_root_event_id: "$thread-root:example".to_owned(),
                    latest_event_id: "$thread-1:example".to_owned(),
                }),
                reply: Some(SendMessageReplyTarget {
                    event_id: "$target".to_owned(),
                }),
            })]
        );
        assert!(
            !state
                .room_activity
                .contains(&thread_view_id("!ops:example", "$thread-root:example"))
        );
        assert!(
            !state
                .room_activity_read_targets
                .contains_key(&thread_view_id("!ops:example", "$thread-root:example"))
        );
        assert_eq!(
            dump_model_from_state(&state).submitted_reply_commands,
            vec![DumpSubmittedReplyCommand {
                room_id: "!ops:example".to_owned(),
                target_event_id: "$target".to_owned(),
                body: "threaded acknowledgement".to_owned(),
                thread: Some(DumpSubmittedReplyThreadTarget {
                    thread_root_event_id: "$thread-root:example".to_owned(),
                    latest_event_id: "$thread-1:example".to_owned(),
                }),
            }]
        );

        stop_test_core(handle);
    }

    #[test]
    fn submit_reply_slash_command_from_status_returns_no_room_selected() {
        let mut state = test_state();

        assert_eq!(
            submit_reply_slash_command(&mut state, "$target".to_owned(), "hello".to_owned()),
            Err("no room selected".to_owned())
        );
        assert!(state.recorded_commands.is_empty());
    }

    #[test]
    fn submit_reply_slash_command_requires_available_command_sender() {
        let mut state = test_state();
        state.model.rooms = vec![room("!room:example", "Room", false)];
        state.active_view = room_view_id("!room:example");

        assert_eq!(
            submit_reply_slash_command(&mut state, "$target".to_owned(), "hello".to_owned()),
            Err("core command sender is not available".to_owned())
        );
        assert!(state.recorded_commands.is_empty());
    }

    #[test]
    fn submit_edit_slash_command_from_active_room_records_typed_request_and_returns_status() {
        let (mut state, handle) = test_state_with_command_sender();
        state.active_view = room_view_id("!room:example");

        let status = submit_edit_slash_command(
            &mut state,
            "$event-to-edit".to_owned(),
            "replacement text".to_owned(),
        )
        .unwrap();

        assert_eq!(status, "Edit queued for $event-to-edit");
        assert_eq!(
            state.recorded_commands,
            vec![Command::EditMessage(EditMessageRequest {
                room_id: "!room:example".to_owned(),
                target_event_id: "$event-to-edit".to_owned(),
                presentation: RoomMessagePresentationKind::Text,
                body: "replacement text".to_owned(),
            })]
        );
        assert_eq!(
            dump_model_from_state(&state).submitted_edit_commands,
            vec![DumpSubmittedEditCommand {
                room_id: "!room:example".to_owned(),
                target_event_id: "$event-to-edit".to_owned(),
                body: "replacement text".to_owned(),
            }]
        );

        stop_test_core(handle);
    }

    #[test]
    fn submit_edit_slash_command_from_thread_targets_parent_room() {
        let (mut state, handle) = test_state_with_command_sender();
        state.active_view = thread_view_id("!room:example", "$thread-root");

        let status = submit_edit_slash_command(
            &mut state,
            "$event-to-edit".to_owned(),
            "replacement text".to_owned(),
        )
        .unwrap();

        assert_eq!(status, "Edit queued for $event-to-edit");
        assert_eq!(
            state.recorded_commands,
            vec![Command::EditMessage(EditMessageRequest {
                room_id: "!room:example".to_owned(),
                target_event_id: "$event-to-edit".to_owned(),
                presentation: RoomMessagePresentationKind::Text,
                body: "replacement text".to_owned(),
            })]
        );

        stop_test_core(handle);
    }

    #[test]
    fn submit_edit_slash_command_from_status_returns_no_room_selected() {
        let mut state = test_state();

        assert_eq!(
            submit_edit_slash_command(
                &mut state,
                "$event-to-edit".to_owned(),
                "replacement text".to_owned(),
            ),
            Err("no room selected".to_owned())
        );
        assert!(state.recorded_commands.is_empty());
    }

    #[test]
    fn submit_edit_slash_command_requires_available_command_sender() {
        let mut state = test_state();
        state.active_view = room_view_id("!room:example");

        assert_eq!(
            submit_edit_slash_command(
                &mut state,
                "$event-to-edit".to_owned(),
                "replacement text".to_owned(),
            ),
            Err("core command sender is not available".to_owned())
        );
        assert!(state.recorded_commands.is_empty());
    }

    #[test]
    fn submit_redact_slash_command_from_active_room_records_typed_request_and_returns_status() {
        let (mut state, handle) = test_state_with_command_sender();
        state.active_view = room_view_id("!room:example");

        let status =
            submit_redact_slash_command(&mut state, "$event-to-redact".to_owned()).unwrap();

        assert_eq!(status, "Redaction queued for $event-to-redact");
        assert_eq!(
            state.recorded_commands,
            vec![Command::RedactEvent(RedactEventRequest {
                room_id: "!room:example".to_owned(),
                event_id: "$event-to-redact".to_owned(),
            })]
        );
        assert_eq!(
            dump_model_from_state(&state).submitted_redaction_commands,
            vec![DumpSubmittedRedactionCommand {
                room_id: "!room:example".to_owned(),
                event_id: "$event-to-redact".to_owned(),
            }]
        );

        stop_test_core(handle);
    }

    #[test]
    fn submit_redact_slash_command_from_thread_targets_parent_room() {
        let (mut state, handle) = test_state_with_command_sender();
        state.active_view = thread_view_id("!room:example", "$thread-root");

        let status =
            submit_redact_slash_command(&mut state, "$event-to-redact".to_owned()).unwrap();

        assert_eq!(status, "Redaction queued for $event-to-redact");
        assert_eq!(
            state.recorded_commands,
            vec![Command::RedactEvent(RedactEventRequest {
                room_id: "!room:example".to_owned(),
                event_id: "$event-to-redact".to_owned(),
            })]
        );

        stop_test_core(handle);
    }

    #[test]
    fn submit_unreact_slash_command_from_active_room_records_typed_request_and_returns_status() {
        let (mut state, handle) = test_state_with_command_sender();
        state.active_view = room_view_id("!room:example");

        let status =
            submit_unreact_slash_command(&mut state, "$reaction-alice-plus-one".to_owned())
                .unwrap();

        assert_eq!(
            status,
            "Reaction redaction queued for $reaction-alice-plus-one"
        );
        assert_eq!(
            state.recorded_commands,
            vec![Command::RedactEvent(RedactEventRequest {
                room_id: "!room:example".to_owned(),
                event_id: "$reaction-alice-plus-one".to_owned(),
            })]
        );
        assert_eq!(
            dump_model_from_state(&state).submitted_redaction_commands,
            vec![DumpSubmittedRedactionCommand {
                room_id: "!room:example".to_owned(),
                event_id: "$reaction-alice-plus-one".to_owned(),
            }]
        );

        stop_test_core(handle);
    }

    #[test]
    fn submit_unreact_slash_command_from_thread_targets_parent_room() {
        let (mut state, handle) = test_state_with_command_sender();
        state.active_view = thread_view_id("!room:example", "$thread-root");

        let status =
            submit_unreact_slash_command(&mut state, "$reaction-alice-plus-one".to_owned())
                .unwrap();

        assert_eq!(
            status,
            "Reaction redaction queued for $reaction-alice-plus-one"
        );
        assert_eq!(
            state.recorded_commands,
            vec![Command::RedactEvent(RedactEventRequest {
                room_id: "!room:example".to_owned(),
                event_id: "$reaction-alice-plus-one".to_owned(),
            })]
        );

        stop_test_core(handle);
    }

    #[test]
    fn submit_unreact_slash_command_from_status_returns_no_room_selected() {
        let mut state = test_state();

        assert_eq!(
            submit_unreact_slash_command(&mut state, "$reaction-alice-plus-one".to_owned()),
            Err("no room selected".to_owned())
        );
        assert!(state.recorded_commands.is_empty());
    }

    #[test]
    fn submit_topic_slash_command_from_active_room_records_typed_request_and_returns_status() {
        let (mut state, handle) = test_state_with_command_sender();
        state.active_view = room_view_id("!room:example");

        let status = submit_topic_slash_command(&mut state, "incident review".to_owned()).unwrap();

        assert_eq!(status, "topic change submitted for !room:example");
        assert_eq!(
            state.recorded_commands,
            vec![Command::SetRoomTopic(SetRoomTopicRequest {
                room_id: "!room:example".to_owned(),
                topic: "incident review".to_owned(),
            })]
        );
        assert_eq!(
            dump_model_from_state(&state).submitted_room_topic_commands,
            vec![DumpSubmittedRoomTopicCommand {
                room_id: "!room:example".to_owned(),
                topic: "incident review".to_owned(),
            }]
        );

        stop_test_core(handle);
    }

    #[test]
    fn submit_topic_slash_command_from_thread_targets_parent_room() {
        let (mut state, handle) = test_state_with_command_sender();
        state.active_view = thread_view_id("!room:example", "$thread-root");

        let status = submit_topic_slash_command(&mut state, "incident review".to_owned()).unwrap();

        assert_eq!(status, "topic change submitted for !room:example");
        assert_eq!(
            state.recorded_commands,
            vec![Command::SetRoomTopic(SetRoomTopicRequest {
                room_id: "!room:example".to_owned(),
                topic: "incident review".to_owned(),
            })]
        );

        stop_test_core(handle);
    }

    #[test]
    fn submit_topic_slash_command_from_status_returns_no_room_selected() {
        let mut state = test_state();

        assert_eq!(
            submit_topic_slash_command(&mut state, "incident review".to_owned()),
            Err("no room selected".to_owned())
        );
        assert!(state.recorded_commands.is_empty());
    }

    #[test]
    fn submit_topic_slash_command_does_not_mutate_local_topic_or_activity_before_inbound_event() {
        let (mut state, handle) = test_state_with_command_sender();
        state.model.rooms = vec![room("!room:example", "Room", false)];
        state.active_view = room_view_id("!room:example");
        apply_core_event(&mut state, topic_state("!room:example", "existing topic")).unwrap();
        apply_core_event(
            &mut state,
            timeline_with_transaction_id(
                "!room:example",
                "$event:example",
                None,
                "hello",
                1_700_000_000_000,
            ),
        )
        .unwrap();

        let before_activities = state
            .model
            .room_views
            .get("!room:example")
            .expect("room view must exist")
            .activities
            .clone();

        let status = submit_topic_slash_command(&mut state, "incident review".to_owned()).unwrap();

        assert_eq!(status, "topic change submitted for !room:example");
        assert_eq!(
            state
                .model
                .room_views
                .get("!room:example")
                .and_then(|view| view.topic.as_deref()),
            Some("existing topic")
        );
        assert_eq!(
            state
                .model
                .room_views
                .get("!room:example")
                .map(|view| view.activities.clone()),
            Some(before_activities)
        );
        assert_eq!(
            render_room_lines(&state.model, "!room:example"),
            vec![
                "Room: Room (!room:example)".to_owned(),
                "Topic: existing topic".to_owned(),
                "* [22:14] @alice:example set topic: existing topic".to_owned(),
                "[22:13] @alice:example: hello".to_owned(),
            ]
        );

        stop_test_core(handle);
    }

    #[test]
    fn submit_compose_enqueues_text_and_emote_messages_with_exact_requests() {
        let mut state = test_state();
        let handle = start_test_core();
        state.command_sender = Some(handle.command_sender().clone());
        state.model.rooms = vec![room("!selected:example", "Selected", false)];
        state.model.selected_room = Some("!selected:example".to_owned());
        state.active_view = ViewId::Room("!selected:example".to_owned());

        let selected_room_id = submit_compose(
            &mut state,
            RoomMessagePresentationKind::Text,
            "hello".to_owned(),
            None,
        )
        .unwrap();
        assert_eq!(selected_room_id, "!selected:example");
        let emote_room_id = submit_compose_with_history_entry(
            &mut state,
            RoomMessagePresentationKind::Emote,
            "waves".to_owned(),
            "/me waves".to_owned(),
            None,
        )
        .unwrap();
        assert_eq!(emote_room_id, "!selected:example");
        assert_eq!(
            state.compose_history.entries,
            vec!["hello".to_owned(), "/me waves".to_owned()]
        );
        assert_eq!(
            state.recorded_commands,
            vec![
                Command::SendMessage(SendMessageRequest {
                    room_id: "!selected:example".to_owned(),
                    presentation: RoomMessagePresentationKind::Text,
                    body: "hello".to_owned(),
                    thread: None,
                    reply: None,
                }),
                Command::SendMessage(SendMessageRequest {
                    room_id: "!selected:example".to_owned(),
                    presentation: RoomMessagePresentationKind::Emote,
                    body: "waves".to_owned(),
                    thread: None,
                    reply: None,
                }),
            ]
        );

        handle
            .command_sender()
            .send(Command::Shutdown)
            .expect("shutdown command must enqueue");
        handle.join().expect("test core must stop cleanly");
    }

    #[test]
    fn submit_compose_records_literal_slash_history_with_stripped_send_body() {
        let mut state = test_state();
        let handle = start_test_core();
        state.command_sender = Some(handle.command_sender().clone());
        state.model.rooms = vec![room("!beta:example", "Beta", false)];
        state.model.selected_room = Some("!beta:example".to_owned());
        state.active_view = ViewId::Room("!beta:example".to_owned());

        let room_id = submit_compose_with_history_entry(
            &mut state,
            RoomMessagePresentationKind::Text,
            "/status".to_owned(),
            "//status".to_owned(),
            None,
        )
        .unwrap();
        assert_eq!(room_id, "!beta:example");
        assert_eq!(state.compose_history.entries, vec!["//status".to_owned()]);
        assert_eq!(
            state.recorded_commands,
            vec![Command::SendMessage(SendMessageRequest {
                room_id: "!beta:example".to_owned(),
                presentation: RoomMessagePresentationKind::Text,
                body: "/status".to_owned(),
                thread: None,
                reply: None,
            })]
        );
        let dump = dump_model_from_state(&state);
        assert!(dump.submitted_reaction_commands.is_empty());
        assert!(dump.submitted_redaction_commands.is_empty());

        handle
            .command_sender()
            .send(Command::Shutdown)
            .expect("shutdown command must enqueue");
        handle.join().expect("test core must stop cleanly");
    }

    #[test]
    fn submit_compose_uses_active_thread_view_for_text_and_emote_messages() {
        let mut state = test_state();
        let handle = start_test_core();
        state.command_sender = Some(handle.command_sender().clone());
        state.model.rooms = vec![room("!ops:example", "Ops", false)];
        apply_core_event(
            &mut state,
            timeline_with_transaction_id(
                "!ops:example",
                "$thread-root:example",
                None,
                "root msg",
                1_700_000_060_000,
            ),
        )
        .unwrap();
        apply_core_event(
            &mut state,
            timeline_with_relation_and_presentation(
                "!ops:example",
                "$thread-1:example",
                None,
                Some(thread_relation("$thread-root:example")),
                RoomMessagePresentationKind::Text,
                "durable reply",
                1_700_000_120_000,
            ),
        )
        .unwrap();
        state.active_view = thread_view_id("!ops:example", "$thread-root:example");
        state
            .room_activity
            .insert(thread_view_id("!ops:example", "$thread-root:example"));
        state.room_activity_read_targets.insert(
            thread_view_id("!ops:example", "$thread-root:example"),
            "$thread-1:example".to_owned(),
        );

        let room_id = submit_compose(
            &mut state,
            RoomMessagePresentationKind::Text,
            "hello".to_owned(),
            None,
        )
        .unwrap();
        assert_eq!(room_id, "!ops:example");
        assert!(
            !state
                .room_activity
                .contains(&thread_view_id("!ops:example", "$thread-root:example"))
        );
        assert!(
            !state
                .room_activity_read_targets
                .contains_key(&thread_view_id("!ops:example", "$thread-root:example"))
        );
        let emote_room_id = submit_compose_with_history_entry(
            &mut state,
            RoomMessagePresentationKind::Emote,
            "waves".to_owned(),
            "/me waves".to_owned(),
            None,
        )
        .unwrap();
        assert_eq!(emote_room_id, "!ops:example");
        assert_eq!(
            state.recorded_commands,
            vec![
                Command::SendMessage(SendMessageRequest {
                    room_id: "!ops:example".to_owned(),
                    presentation: RoomMessagePresentationKind::Text,
                    body: "hello".to_owned(),
                    thread: Some(SendMessageThreadTarget {
                        thread_root_event_id: "$thread-root:example".to_owned(),
                        latest_event_id: "$thread-1:example".to_owned(),
                    }),
                    reply: None,
                }),
                Command::SendMessage(SendMessageRequest {
                    room_id: "!ops:example".to_owned(),
                    presentation: RoomMessagePresentationKind::Emote,
                    body: "waves".to_owned(),
                    thread: Some(SendMessageThreadTarget {
                        thread_root_event_id: "$thread-root:example".to_owned(),
                        latest_event_id: "$thread-1:example".to_owned(),
                    }),
                    reply: None,
                }),
            ]
        );

        handle
            .command_sender()
            .send(Command::Shutdown)
            .expect("shutdown command must enqueue");
        handle.join().expect("test core must stop cleanly");
    }

    #[test]
    fn submit_compose_with_explicit_room_id_ignores_active_thread_view() {
        let mut state = test_state();
        let handle = start_test_core();
        state.command_sender = Some(handle.command_sender().clone());
        state.model.rooms = vec![room("!ops:example", "Ops", false)];
        apply_core_event(
            &mut state,
            timeline_with_relation_and_presentation(
                "!ops:example",
                "$thread-1:example",
                None,
                Some(thread_relation("$thread-root:example")),
                RoomMessagePresentationKind::Text,
                "durable reply",
                1_700_000_120_000,
            ),
        )
        .unwrap();
        state.active_view = ViewId::Thread {
            room_id: "!ops:example".to_owned(),
            thread_root_event_id: "$thread-root:example".to_owned(),
        };

        let room_id = submit_compose(
            &mut state,
            RoomMessagePresentationKind::Text,
            "hello".to_owned(),
            Some("!ops:example"),
        )
        .unwrap();
        assert_eq!(room_id, "!ops:example");
        assert_eq!(
            state.recorded_commands,
            vec![Command::SendMessage(SendMessageRequest {
                room_id: "!ops:example".to_owned(),
                presentation: RoomMessagePresentationKind::Text,
                body: "hello".to_owned(),
                thread: None,
                reply: None,
            })]
        );

        handle
            .command_sender()
            .send(Command::Shutdown)
            .expect("shutdown command must enqueue");
        handle.join().expect("test core must stop cleanly");
    }

    #[test]
    fn submit_compose_rejects_text_and_emote_for_empty_body_invalid_room_and_missing_active_view() {
        let mut state = test_state();
        state.model.rooms = vec![
            room("!room:example", "Room", false),
            invited_room("!invite:example", "Invite"),
            space("!space:example", "Space"),
        ];

        assert_eq!(
            submit_compose(
                &mut state,
                RoomMessagePresentationKind::Text,
                String::new(),
                Some("!room:example"),
            ),
            Err("message body must not be empty".to_owned())
        );
        assert!(state.compose_history.entries.is_empty());
        state.model.selected_room = Some("!room:example".to_owned());
        assert_eq!(
            submit_compose(
                &mut state,
                RoomMessagePresentationKind::Emote,
                "waves".to_owned(),
                Some("!missing:example"),
            ),
            Err("unknown room id: !missing:example".to_owned())
        );
        assert!(state.compose_history.entries.is_empty());
        assert_eq!(
            submit_compose(
                &mut state,
                RoomMessagePresentationKind::Emote,
                "waves".to_owned(),
                Some("!invite:example"),
            ),
            Err("room is not joined: !invite:example".to_owned())
        );
        assert!(state.compose_history.entries.is_empty());
        assert_eq!(
            submit_compose(
                &mut state,
                RoomMessagePresentationKind::Emote,
                "waves".to_owned(),
                Some("!space:example"),
            ),
            Err("unknown room id: !space:example".to_owned())
        );
        assert!(state.compose_history.entries.is_empty());
        assert_eq!(
            submit_compose(
                &mut state,
                RoomMessagePresentationKind::Emote,
                "waves".to_owned(),
                None,
            ),
            Err("no room selected".to_owned())
        );
        assert!(state.compose_history.entries.is_empty());
        assert!(state.recorded_commands.is_empty());
    }

    #[test]
    fn submit_verification_helpers_enqueue_typed_commands_for_current_flow() {
        let mut state = test_state();
        let handle = start_test_core();
        state.command_sender = Some(handle.command_sender().clone());
        state.model.last_verification = Some(VerificationStateEvent {
            flow_id: "flow-verify-123".to_owned(),
            user_id: "@alice:example".to_owned(),
            state: VerificationState::SasReady {
                emojis: vec![VerificationEmoji {
                    symbol: "🐶".to_owned(),
                    description: "dog".to_owned(),
                }],
                decimals: mmatrix_core::VerificationDecimals {
                    first: 1234,
                    second: 5678,
                    third: 9012,
                },
            },
        });
        let last_verification = state.model.last_verification.clone();

        submit_accept_verification(&mut state).unwrap();
        submit_cancel_verification(&mut state).unwrap();
        submit_confirm_verification_sas(&mut state).unwrap();
        submit_reject_verification_sas(&mut state).unwrap();

        assert_eq!(state.model.last_verification, last_verification);
        assert_eq!(
            state.recorded_commands,
            vec![
                Command::AcceptVerification(VerificationFlowId {
                    flow_id: "flow-verify-123".to_owned(),
                    user_id: "@alice:example".to_owned(),
                }),
                Command::CancelVerification(VerificationFlowId {
                    flow_id: "flow-verify-123".to_owned(),
                    user_id: "@alice:example".to_owned(),
                }),
                Command::ConfirmVerificationSas(VerificationFlowId {
                    flow_id: "flow-verify-123".to_owned(),
                    user_id: "@alice:example".to_owned(),
                }),
                Command::RejectVerificationSas(VerificationFlowId {
                    flow_id: "flow-verify-123".to_owned(),
                    user_id: "@alice:example".to_owned(),
                }),
            ]
        );

        handle
            .command_sender()
            .send(Command::Shutdown)
            .expect("shutdown command must enqueue");
        handle.join().expect("test core must stop cleanly");
    }

    #[test]
    fn automation_verification_actions_use_current_flow_when_args_are_omitted() {
        let mut state = test_state();
        let handle = start_test_core();
        state.command_sender = Some(handle.command_sender().clone());
        state.model.last_verification = Some(VerificationStateEvent {
            flow_id: "flow-current".to_owned(),
            user_id: "@alice:example".to_owned(),
            state: VerificationState::Requested,
        });
        let last_verification = state.model.last_verification.clone();

        submit_verification_automation_action_to_state(
            &mut state,
            VerificationAction::Accept,
            VerificationActionArgs::default(),
        )
        .unwrap();
        submit_verification_automation_action_to_state(
            &mut state,
            VerificationAction::Cancel,
            VerificationActionArgs::default(),
        )
        .unwrap();
        submit_verification_automation_action_to_state(
            &mut state,
            VerificationAction::Confirm,
            VerificationActionArgs::default(),
        )
        .unwrap();
        submit_verification_automation_action_to_state(
            &mut state,
            VerificationAction::Reject,
            VerificationActionArgs::default(),
        )
        .unwrap();

        assert_eq!(state.model.last_verification, last_verification);
        assert_eq!(
            state.recorded_commands,
            vec![
                Command::AcceptVerification(VerificationFlowId {
                    flow_id: "flow-current".to_owned(),
                    user_id: "@alice:example".to_owned(),
                }),
                Command::CancelVerification(VerificationFlowId {
                    flow_id: "flow-current".to_owned(),
                    user_id: "@alice:example".to_owned(),
                }),
                Command::ConfirmVerificationSas(VerificationFlowId {
                    flow_id: "flow-current".to_owned(),
                    user_id: "@alice:example".to_owned(),
                }),
                Command::RejectVerificationSas(VerificationFlowId {
                    flow_id: "flow-current".to_owned(),
                    user_id: "@alice:example".to_owned(),
                }),
            ]
        );

        handle
            .command_sender()
            .send(Command::Shutdown)
            .expect("shutdown command must enqueue");
        handle.join().expect("test core must stop cleanly");
    }

    #[test]
    fn automation_verification_actions_use_explicit_flow_args_without_mutating_current_flow() {
        let mut state = test_state();
        let handle = start_test_core();
        state.command_sender = Some(handle.command_sender().clone());
        state.model.last_verification = Some(VerificationStateEvent {
            flow_id: "flow-current".to_owned(),
            user_id: "@alice:example".to_owned(),
            state: VerificationState::SasReady {
                emojis: vec![VerificationEmoji {
                    symbol: "🐶".to_owned(),
                    description: "dog".to_owned(),
                }],
                decimals: mmatrix_core::VerificationDecimals {
                    first: 1234,
                    second: 5678,
                    third: 9012,
                },
            },
        });
        let last_verification = state.model.last_verification.clone();

        submit_verification_automation_action_to_state(
            &mut state,
            VerificationAction::Accept,
            VerificationActionArgs {
                flow_id: Some("flow-explicit".to_owned()),
                user_id: Some("@bob:example".to_owned()),
            },
        )
        .unwrap();

        assert_eq!(state.model.last_verification, last_verification);
        assert_eq!(
            state.recorded_commands,
            vec![Command::AcceptVerification(VerificationFlowId {
                flow_id: "flow-explicit".to_owned(),
                user_id: "@bob:example".to_owned(),
            })]
        );

        handle
            .command_sender()
            .send(Command::Shutdown)
            .expect("shutdown command must enqueue");
        handle.join().expect("test core must stop cleanly");
    }

    #[test]
    fn automation_verification_partial_explicit_args_fail_during_parsing_before_submission() {
        let state = test_state();

        let error = parse_command_request(
            crate::automation::COMMAND_ID_ACCEPT_VERIFICATION,
            br#"{"id":"accept-verification","args":{"flow_id":"flow-1"}}"#,
        )
        .expect_err("partial explicit flow args must fail");

        assert_eq!(
            serde_json::to_value(error).unwrap()["err"]["code"],
            json!("invalid_args")
        );
        assert!(state.recorded_commands.is_empty());
    }

    #[test]
    fn submit_verification_helpers_require_current_flow() {
        let mut state = test_state();

        assert_eq!(
            submit_accept_verification(&mut state),
            Err("no verification flow available".to_owned())
        );
        assert_eq!(
            submit_cancel_verification(&mut state),
            Err("no verification flow available".to_owned())
        );
        assert_eq!(
            submit_confirm_verification_sas(&mut state),
            Err("no verification flow available".to_owned())
        );
        assert_eq!(
            submit_reject_verification_sas(&mut state),
            Err("no verification flow available".to_owned())
        );
    }

    #[test]
    fn submit_recover_e2ee_enqueues_typed_command_and_updates_status() {
        let mut state = test_state();
        let handle = start_test_core();
        state.command_sender = Some(handle.command_sender().clone());

        submit_recover_e2ee(&mut state, "secret phrase".to_owned()).unwrap();

        assert_eq!(state.model.status, "e2ee recovery submitted");
        assert_eq!(
            state.recorded_commands,
            vec![Command::RecoverE2ee(RecoverE2eeRequest {
                recovery_secret: "secret phrase".to_owned(),
            })]
        );

        handle
            .command_sender()
            .send(Command::Shutdown)
            .expect("shutdown command must enqueue");
        handle.join().expect("test core must stop cleanly");
    }

    #[test]
    fn submit_recover_e2ee_rejects_empty_secret() {
        let mut state = test_state();

        assert_eq!(
            submit_recover_e2ee(&mut state, String::new()),
            Err("recovery key or passphrase must not be empty".to_owned())
        );
    }

    #[test]
    fn resolve_timeline_room_id_rejects_spaces() {
        let mut state = test_state();
        state.model.rooms = vec![
            space("!space:example", "Space"),
            room("!room:example", "Room", false),
        ];
        state.model.selected_room = Some("!room:example".to_owned());

        assert_eq!(
            resolve_timeline_room_id(&state, Some("!space:example")),
            Err("unknown room id: !space:example".to_owned())
        );
        assert_eq!(
            resolve_timeline_room_id(&state, Some("!room:example")),
            Ok("!room:example".to_owned())
        );
    }

    #[test]
    fn submit_compose_requires_available_command_sender() {
        let mut state = test_state();
        state.model.rooms = vec![room("!room:example", "Room", false)];
        state.model.selected_room = Some("!room:example".to_owned());
        state.active_view = ViewId::Room("!room:example".to_owned());

        assert_eq!(
            submit_compose(
                &mut state,
                RoomMessagePresentationKind::Text,
                "hello".to_owned(),
                None,
            ),
            Err("core command sender is not available".to_owned())
        );
        assert!(state.compose_history.entries.is_empty());
    }

    #[test]
    fn compose_history_navigation_is_noop_when_empty() {
        let mut history = ComposeHistory::default();

        assert_eq!(history.previous("draft".to_owned()), None);
        assert_eq!(history.next(), None);
        assert!(history.navigation.is_none());
    }

    #[test]
    fn compose_history_navigates_entries_and_restores_draft() {
        let mut history = ComposeHistory {
            entries: vec!["first".to_owned(), "second".to_owned()],
            navigation: None,
        };

        assert_eq!(history.previous("draft".to_owned()), Some("second"));
        assert_eq!(history.previous("ignored".to_owned()), Some("first"));
        assert_eq!(history.previous("ignored".to_owned()), Some("first"));
        assert_eq!(history.next(), Some("second".to_owned()));
        assert_eq!(history.next(), Some("draft".to_owned()));
        assert_eq!(history.next(), None);
        assert!(history.navigation.is_none());
    }

    #[test]
    fn compose_history_reset_navigation_restarts_from_newest_with_fresh_draft() {
        let mut history = ComposeHistory {
            entries: vec!["first".to_owned(), "second".to_owned()],
            navigation: None,
        };

        assert_eq!(history.previous("draft".to_owned()), Some("second"));
        history.reset_navigation();
        assert!(history.navigation.is_none());
        assert_eq!(
            history.previous("replacement draft".to_owned()),
            Some("second")
        );
        assert_eq!(history.next(), Some("replacement draft".to_owned()));
    }

    #[test]
    fn lifecycle_events_update_model_and_persist_login_session() {
        with_local_app_data(|root| {
            let mut state = test_state();
            state.pending_login_homeserver_url = Some("https://matrix.example".to_owned());

            apply_core_event(&mut state, Event::Started).unwrap();
            assert_eq!(state.model.lifecycle_state, LifecycleState::CoreStarted);

            apply_core_event(
                &mut state,
                Event::LoggedIn {
                    session: stored_session(),
                },
            )
            .unwrap();
            assert_eq!(state.model.lifecycle_state, LifecycleState::Authenticated);
            assert_eq!(state.pending_login_homeserver_url, None);
            assert_eq!(
                state.current_session_homeserver_url,
                Some("https://matrix.example".to_owned())
            );

            let session_path = session_file_path(&root);
            let session_bytes = fs::read(&session_path).expect("saved session file must exist");
            assert_eq!(load_saved_session().unwrap(), saved_session_payload());
            assert_bytes_do_not_contain(&session_bytes, "secret-token");
            assert_bytes_do_not_contain(&session_bytes, "refresh-token");
            assert_bytes_do_not_contain(&session_bytes, "@alice:example");
            assert_bytes_do_not_contain(&session_bytes, "DEVICE");
            assert_bytes_do_not_contain(&session_bytes, "\"access_token\"");

            apply_core_event(&mut state, Event::SyncStarted).unwrap();
            assert_eq!(state.model.lifecycle_state, LifecycleState::Syncing);

            apply_core_event(
                &mut state,
                Event::SyncedRooms(vec![room("!room:example", "Room", false)]),
            )
            .unwrap();
            assert_eq!(state.model.lifecycle_state, LifecycleState::Ready);

            apply_core_event(&mut state, Event::RestoredSession).unwrap();
            assert_eq!(state.model.lifecycle_state, LifecycleState::Authenticated);

            apply_core_event(
                &mut state,
                Event::Error(CoreErrorEvent {
                    operation: Operation::RestoreSession,
                    message: "restore failed".to_owned(),
                }),
            )
            .unwrap();
            assert_eq!(state.model.lifecycle_state, LifecycleState::Error);
            assert_eq!(
                state.model.last_error,
                Some(CoreErrorEvent {
                    operation: Operation::RestoreSession,
                    message: "restore failed".to_owned(),
                })
            );

            apply_core_event(&mut state, Event::LoggedOut).unwrap();
            assert_eq!(state.model.lifecycle_state, LifecycleState::LoggedOut);
            assert_eq!(state.current_session_homeserver_url, None);
            assert!(!session_path.exists());

            apply_core_event(&mut state, Event::Stopped).unwrap();
            assert_eq!(state.model.lifecycle_state, LifecycleState::Stopped);
        });
    }

    #[test]
    fn logged_in_event_requires_pending_homeserver_url() {
        let mut state = test_state();

        assert_eq!(
            apply_core_event(
                &mut state,
                Event::LoggedIn {
                    session: stored_session(),
                },
            ),
            Err("received LoggedIn event without a pending homeserver URL".to_owned())
        );
    }

    #[test]
    fn synthetic_logged_in_event_updates_model_without_persisting_session() {
        with_local_app_data(|root| {
            let mut state = test_state();
            state.synthetic_load_in_progress = true;

            apply_core_event(
                &mut state,
                Event::LoggedIn {
                    session: stored_session(),
                },
            )
            .unwrap();

            assert_eq!(state.model.status, "authenticated");
            assert_eq!(state.model.lifecycle_state, LifecycleState::Authenticated);
            assert_eq!(state.current_session_homeserver_url, None);
            assert!(!root.join(PROFILE_SUBDIR).join(SESSION_FILE_NAME).exists());
        });
    }

    #[test]
    fn session_updated_event_persists_refreshed_tokens_without_leaking_them() {
        with_local_app_data(|root| {
            save_session("https://matrix.example", &stored_session()).unwrap();
            let mut state = test_state();
            state.current_session_homeserver_url = Some("https://matrix.example".to_owned());

            let updated_session = StoredSession {
                access_token: "updated-access-token".to_owned(),
                refresh_token: Some("updated-refresh-token".to_owned()),
                ..stored_session()
            };

            apply_core_event(
                &mut state,
                Event::SessionUpdated {
                    session: updated_session.clone(),
                },
            )
            .unwrap();

            assert_eq!(
                load_saved_session().unwrap(),
                SavedSession {
                    session: updated_session.clone(),
                    ..saved_session_payload()
                }
            );
            let session_bytes = fs::read(session_file_path(&root)).unwrap();
            assert_bytes_do_not_contain(&session_bytes, "updated-access-token");
            assert_bytes_do_not_contain(&session_bytes, "updated-refresh-token");
            assert!(
                !serde_json::to_string(&dump_model_from_state(&state))
                    .unwrap()
                    .contains("updated-access-token")
            );
            assert!(
                !serde_json::to_string(&state.model.status)
                    .unwrap()
                    .contains("updated-access-token")
            );
        });
    }

    #[test]
    fn saved_session_helper_round_trips_and_protects_sensitive_fields() {
        with_local_app_data(|root| {
            save_session("https://matrix.example", &stored_session()).unwrap();
            assert_eq!(load_saved_session().unwrap(), saved_session_payload());
            let saved_bytes = fs::read(session_file_path(&root)).unwrap();
            assert_bytes_do_not_contain(&saved_bytes, "secret-token");
            assert_bytes_do_not_contain(&saved_bytes, "refresh-token");
            assert_bytes_do_not_contain(&saved_bytes, "@alice:example");
            assert_bytes_do_not_contain(&saved_bytes, "DEVICE");
            assert_bytes_do_not_contain(&saved_bytes, "\"access_token\"");

            let path = session_file_path(&root);
            delete_saved_session().unwrap();
            delete_saved_session().unwrap();
            assert!(!path.exists());
        });
    }

    #[test]
    fn load_saved_session_rejects_legacy_cleartext_json_without_marker() {
        with_local_app_data(|root| {
            let cleartext = serde_json::to_vec(&saved_session_payload()).unwrap();
            write_session_bytes(&root, &cleartext);
            assert!(
                load_saved_session()
                    .unwrap_err()
                    .contains("is not in protected format")
            );
        });
    }

    #[test]
    fn load_saved_session_rejects_unsupported_version() {
        with_local_app_data(|root| {
            write_session_bytes(
                &root,
                &protected_saved_session_bytes(&SavedSession {
                    version: 2,
                    ..saved_session_payload()
                }),
            );
            assert!(
                load_saved_session()
                    .unwrap_err()
                    .contains("unsupported saved session version 2")
            );
        });
    }

    #[test]
    fn load_saved_session_rejects_invalid_protected_json() {
        with_local_app_data(|root| {
            write_session_bytes(
                &root,
                &protected_saved_session_payload_bytes(
                    br#"{"version":1,"homeserver_url":"https://matrix.example","session":{"user_id":"@alice:example""#,
                ),
            );
            assert!(
                load_saved_session()
                    .unwrap_err()
                    .contains("failed to parse saved session")
            );
        });
    }

    #[test]
    fn load_saved_session_rejects_corrupt_dpapi_payload() {
        with_local_app_data(|root| {
            write_session_bytes(
                &root,
                &[PROTECTED_SESSION_MAGIC, b"corrupt".as_slice()].concat(),
            );
            assert!(
                load_saved_session()
                    .unwrap_err()
                    .contains("failed to unprotect saved session")
            );
        });
    }

    #[test]
    fn store_passphrase_helper_round_trips_and_protects_secret() {
        with_local_app_data(|root| {
            let profile_dir = root.join(PROFILE_SUBDIR);
            let first = load_or_create_store_passphrase(&profile_dir).unwrap();
            let second = load_or_create_store_passphrase(&profile_dir).unwrap();
            let raw_bytes = fs::read(store_passphrase_file_path(&root)).unwrap();

            assert_eq!(first, second);
            assert_eq!(first.len(), 64);
            assert!(
                first
                    .bytes()
                    .all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'))
            );
            assert_bytes_do_not_contain(&raw_bytes, &first);
        });
    }

    #[test]
    fn load_or_create_store_passphrase_rejects_missing_passphrase_when_store_exists() {
        with_local_app_data(|root| {
            let profile_dir = root.join(PROFILE_SUBDIR);
            let matrix_store_dir = profile_dir.join("matrix-store");
            fs::create_dir_all(&matrix_store_dir).unwrap();
            assert!(
                load_or_create_store_passphrase(&profile_dir)
                    .unwrap_err()
                    .contains("exists but protected passphrase")
            );
        });
    }

    #[test]
    fn load_or_create_store_passphrase_rejects_corrupt_passphrase_file() {
        with_local_app_data(|root| {
            let profile_dir = root.join(PROFILE_SUBDIR);
            let passphrase_path = store_passphrase_file_path(&root);
            let parent = passphrase_path.parent().unwrap();
            fs::create_dir_all(parent).unwrap();
            fs::write(
                &passphrase_path,
                [PROTECTED_STORE_PASSPHRASE_MAGIC, b"corrupt".as_slice()].concat(),
            )
            .unwrap();
            assert!(
                load_or_create_store_passphrase(&profile_dir)
                    .unwrap_err()
                    .contains("failed to unprotect store passphrase")
            );
        });
    }

    #[test]
    fn submit_login_and_restore_helpers_validate_inputs_and_update_lifecycle() {
        let mut state = test_state();
        let handle = start_test_core();
        state.command_sender = Some(handle.command_sender().clone());

        submit_login(
            &mut state,
            "https://matrix.example".to_owned(),
            "@alice:example".to_owned(),
            "secret".to_owned(),
            String::new(),
        )
        .unwrap();
        assert_eq!(
            state.pending_login_homeserver_url,
            Some("https://matrix.example".to_owned())
        );
        assert_eq!(state.model.lifecycle_state, LifecycleState::LoggingIn);

        submit_restore_session(
            &mut state,
            "https://matrix.example".to_owned(),
            stored_session(),
        )
        .unwrap();
        assert_eq!(
            state.current_session_homeserver_url,
            Some("https://matrix.example".to_owned())
        );
        assert_eq!(state.model.lifecycle_state, LifecycleState::Restoring);

        assert_eq!(
            submit_login(
                &mut state,
                String::new(),
                "@alice:example".to_owned(),
                "secret".to_owned(),
                String::new(),
            ),
            Err("homeserver URL must not be empty".to_owned())
        );
        assert_eq!(
            submit_restore_session(&mut state, String::new(), stored_session()),
            Err("homeserver URL must not be empty".to_owned())
        );

        handle
            .command_sender()
            .send(Command::Shutdown)
            .expect("shutdown command must enqueue");
        handle.join().expect("test core must stop cleanly");
    }

    #[test]
    fn submit_restore_saved_session_uses_persisted_session_and_submitters_require_sender() {
        with_local_app_data(|_| {
            let mut state = test_state();

            let restore_error =
                submit_restore_saved_session(&mut state).expect_err("missing session must fail");
            assert!(restore_error.contains("failed to read saved session"));
            assert!(restore_error.contains(SESSION_FILE_NAME));
            assert_eq!(
                submit_start_sync(&mut state),
                Err("core command sender is not available".to_owned())
            );
            assert_eq!(
                submit_logout(&mut state),
                Err("core command sender is not available".to_owned())
            );

            let handle = start_test_core();
            state.command_sender = Some(handle.command_sender().clone());
            save_session("https://matrix.example", &stored_session()).unwrap();

            submit_restore_saved_session(&mut state).unwrap();
            assert_eq!(state.model.status, "restoring saved session");
            assert_eq!(state.model.lifecycle_state, LifecycleState::Restoring);

            submit_start_sync(&mut state).unwrap();
            submit_logout(&mut state).unwrap();

            handle
                .command_sender()
                .send(Command::Shutdown)
                .expect("shutdown command must enqueue");
            handle.join().expect("test core must stop cleanly");
        });
    }

    #[test]
    fn startup_restore_keeps_restoring_state_when_started_arrives_after_submit() {
        with_local_app_data(|_| {
            let mut state = test_state();
            let handle = start_test_core();
            state.command_sender = Some(handle.command_sender().clone());
            save_session("https://matrix.example", &stored_session()).unwrap();

            submit_restore_saved_session(&mut state).unwrap();
            assert_eq!(state.model.status, "restoring saved session");
            assert_eq!(state.model.lifecycle_state, LifecycleState::Restoring);

            apply_core_event(&mut state, Event::Started).unwrap();
            assert_eq!(state.model.status, "restoring saved session");
            assert_eq!(state.model.lifecycle_state, LifecycleState::Restoring);

            handle
                .command_sender()
                .send(Command::Shutdown)
                .expect("shutdown command must enqueue");
            handle.join().expect("test core must stop cleanly");
        });
    }
}
