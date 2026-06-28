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
    Command, CommandSender, CoreConfig, CoreErrorEvent, CoreHandle, Event, LoginRequest, Operation,
    OutgoingTextFailed, OutgoingTextQueued, OutgoingTextSent, RestoreSessionRequest,
    RoomStateContent, RoomStateEventKind, RoomSummary, SendTextRequest, StoredSession,
    TimelineText, VerificationRequest, start,
};
use serde::Serialize;
use serde_json::{Value, json};
use windows_sys::Win32::Foundation::{
    CloseHandle, GetLastError, HANDLE, HWND, INVALID_HANDLE_VALUE, LPARAM, LRESULT, RECT, WPARAM,
};
use windows_sys::Win32::Graphics::Gdi::{
    BeginPaint, COLOR_WINDOW, EndPaint, FillRect, HBRUSH, InvalidateRect, PAINTSTRUCT, SetBkMode,
    TRANSPARENT, TextOutW,
};
use windows_sys::Win32::System::DataExchange::COPYDATASTRUCT;
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::System::Memory::{
    CreateFileMappingW, FILE_MAP_ALL_ACCESS, MEMORY_MAPPED_VIEW_ADDRESS, MapViewOfFile,
    PAGE_READWRITE, UnmapViewOfFile,
};
use windows_sys::Win32::System::Threading::GetCurrentProcessId;
use windows_sys::Win32::UI::Controls::{
    BTNS_CHECK, BTNS_SHOWTEXT, EM_SETSEL, I_IMAGENONE, ICC_BAR_CLASSES, ICC_TREEVIEW_CLASSES,
    INITCOMMONCONTROLSEX, InitCommonControlsEx, NMHDR, SB_SETTEXTW, SBARS_SIZEGRIP,
    STATUSCLASSNAMEW, SetScrollInfo, TB_ADDBUTTONSW, TB_AUTOSIZE, TB_BUTTONCOUNT,
    TB_BUTTONSTRUCTSIZE, TB_CHECKBUTTON, TB_DELETEBUTTON, TB_SETBUTTONSIZE, TB_SETEXTENDEDSTYLE,
    TBBUTTON, TBSTATE_ENABLED, TBSTYLE_EX_MIXEDBUTTONS, TOOLBARCLASSNAMEW, TVE_EXPAND, TVGN_CARET,
    TVGN_CHILD, TVGN_NEXT, TVGN_PARENT, TVGN_ROOT, TVI_LAST, TVI_ROOT, TVIF_TEXT, TVINSERTSTRUCTW,
    TVINSERTSTRUCTW_0, TVITEMEXW, TVM_DELETEITEM, TVM_EXPAND, TVM_GETITEMW, TVM_GETNEXTITEM,
    TVM_INSERTITEMW, TVM_SELECTITEM, TVN_SELCHANGEDW,
};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::EnableWindow;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, BS_DEFPUSHBUTTON, BS_PUSHBUTTON, CLIENTCREATESTRUCT, CREATESTRUCTW, CW_USEDEFAULT,
    CallWindowProcW, CreateMenu, CreatePopupMenu, CreateWindowExW, DefFrameProcW, DefMDIChildProcW,
    DefWindowProcW, DestroyWindow, DispatchMessageW, DrawMenuBar, ES_AUTOHSCROLL, ES_AUTOVSCROLL,
    ES_MULTILINE, ES_PASSWORD, ES_READONLY, ES_WANTRETURN, GWL_STYLE, GWLP_USERDATA, GWLP_WNDPROC,
    GetClientRect, GetMessageW, GetParent, GetScrollInfo, GetWindowLongPtrW, HMENU, IDC_ARROW,
    IsWindow, KillTimer, LoadCursorW, MB_ICONERROR, MB_OK, MDICREATESTRUCTW, MF_POPUP, MF_STRING,
    MSG, MessageBoxW, MoveWindow, PostMessageW, PostQuitMessage, RegisterClassExW, SB_BOTTOM,
    SB_LINEDOWN, SB_LINEUP, SB_PAGEDOWN, SB_PAGEUP, SB_THUMBPOSITION, SB_THUMBTRACK, SB_TOP,
    SB_VERT, SCROLLINFO, SIF_ALL, SIF_PAGE, SIF_POS, SIF_RANGE, SIF_TRACKPOS, SW_SHOW,
    SendMessageW, SetMenu, SetTimer, SetWindowLongPtrW, ShowWindow, TranslateMDISysAccel,
    TranslateMessage, WINDOW_EX_STYLE, WM_APP, WM_CHAR, WM_CLOSE, WM_COMMAND, WM_COPYDATA,
    WM_CREATE, WM_DESTROY, WM_GETTEXT, WM_GETTEXTLENGTH, WM_KEYDOWN, WM_MDIACTIVATE, WM_MDICREATE,
    WM_MDIDESTROY, WM_MDIGETACTIVE, WM_MDIMAXIMIZE, WM_NCCREATE, WM_NCDESTROY, WM_NOTIFY, WM_PAINT,
    WM_PASTE, WM_SETTEXT, WM_SIZE, WM_TIMER, WM_VSCROLL, WNDCLASSEXW, WNDPROC, WS_BORDER,
    WS_CAPTION, WS_CHILD, WS_CLIPCHILDREN, WS_EX_CLIENTEDGE, WS_OVERLAPPED, WS_OVERLAPPEDWINDOW,
    WS_SYSMENU, WS_TABSTOP, WS_VISIBLE, WS_VSCROLL,
};

use crate::{
    AppCommand, HELP, RunOptions,
    automation::{
        AcceptedResponse, COMMAND_ID_DUMP_COMPOSE_TEXT, COMMAND_ID_DUMP_TIMELINE,
        COMMAND_ID_LOAD_SYNTHETIC_STATE, COMMAND_ID_PING, COMMAND_ID_WAIT_IDLE, ComposeKey,
        ComposeTextResponse, DumpModel, DumpRoomMemberState,
        DumpRoomStateContent as AutomationRoomStateContent,
        DumpRoomStateEventKind as AutomationRoomStateEventKind, DumpRoomStateLine,
        DumpRoomTopicState, DumpTimelineEntry, DumpTimelineResponse, DumpTimelineSendError,
        DumpTimelineSendState, FailureEnvelope, GetVersionResponse, LifecycleState,
        ListRoomsResponse, LoadEventsResponse, MdiState, MdiView, NavigationNode,
        NavigationPathSegment, ParsedCommand, PongResponse, ResponseEnvelope, SelectRoomResponse,
        SelectViewResponse, SelectedNavigationPathResponse, SendTextResponse, StatusBarState,
        SwitchbarItem, SwitchbarState, UiState, ViewIdentity, WaitIdleResponse,
        dump_error_from_core, dump_room_from_summary, dump_verification_from_request,
        failure_response, parse_command_request, success_response,
    },
};

const FRAME_CLASS_NAME: &str = "mmatrixWin32Frame";
const MDI_CHILD_CLASS_NAME: &str = "mmatrixWin32MdiChild";
const TIMELINE_CLASS_NAME: &str = "mmatrixWin32Timeline";
const LOGIN_DIALOG_CLASS_NAME: &str = "mmatrixWin32LoginDialog";
const FRAME_TITLE: &str = "mmatrix";
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
const SWITCHBAR_COMMAND_FIRST: usize = 41001;
const SWITCHBAR_COMMAND_LAST: usize = 41999;
const LOGIN_DIALOG_HOME_SERVER_ID: u16 = 5001;
const LOGIN_DIALOG_USERNAME_ID: u16 = 5002;
const LOGIN_DIALOG_PASSWORD_ID: u16 = 5003;
const LOGIN_DIALOG_DEVICE_ID: u16 = 5004;
const LOGIN_DIALOG_OK_ID: usize = 5005;
const LOGIN_DIALOG_CANCEL_ID: usize = 5006;
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
const TIMELINE_ROW_HEIGHT: i32 = 20;
const TIMELINE_LEFT_PADDING: i32 = 8;
const TIMELINE_VERTICAL_PADDING: i32 = 6;
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
}

struct MdiViewState {
    child: HWND,
    content: HWND,
}

struct MdiChildCreateState {
    view_id: ViewId,
}

struct TimelineState {
    rendered_text: String,
    lines: Vec<String>,
    top_line: usize,
    line_height: i32,
    vertical_padding: i32,
}

impl Default for TimelineState {
    fn default() -> Self {
        Self {
            rendered_text: String::new(),
            lines: Vec::new(),
            top_line: 0,
            line_height: TIMELINE_ROW_HEIGHT,
            vertical_padding: TIMELINE_VERTICAL_PADDING,
        }
    }
}

#[derive(Default)]
struct UiModel {
    status: String,
    lifecycle_state: LifecycleState,
    rooms: Vec<RoomSummary>,
    selected_room: Option<String>,
    room_views: BTreeMap<String, RoomViewModel>,
    last_timeline: Option<LastTimelineText>,
    last_verification: Option<VerificationRequest>,
    last_error: Option<CoreErrorEvent>,
}

#[derive(Default)]
struct RoomViewModel {
    topic: Option<String>,
    entries: Vec<RoomTimelineEntry>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TimelineSendState {
    Pending,
    Sent,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct TimelineSendError {
    message: String,
    recoverable: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct LastTimelineText {
    room_id: String,
    event_id: Option<String>,
    transaction_id: Option<String>,
    sender: String,
    timestamp_ms: u64,
    body: String,
    send_state: TimelineSendState,
    send_error: Option<TimelineSendError>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum RoomTimelineEntry {
    Text {
        event_id: Option<String>,
        transaction_id: Option<String>,
        sender: String,
        timestamp_ms: u64,
        body: String,
        send_state: TimelineSendState,
        send_error: Option<TimelineSendError>,
    },
    System {
        sender: String,
        timestamp_ms: u64,
        kind: RoomStateEventKind,
        state_key: String,
        display_name: Option<String>,
        text: String,
    },
}

impl RoomViewModel {
    fn clear_visible_entries(&mut self) {
        self.entries.retain(|entry| {
            matches!(
                entry,
                RoomTimelineEntry::Text {
                    send_state: TimelineSendState::Pending,
                    ..
                }
            )
        });
    }

    fn upsert_synced_text(&mut self, timeline: TimelineText) {
        let new_entry = RoomTimelineEntry::Text {
            event_id: Some(timeline.event_id),
            transaction_id: timeline.transaction_id,
            sender: timeline.sender,
            timestamp_ms: timeline.timestamp_ms,
            body: timeline.body,
            send_state: TimelineSendState::Sent,
            send_error: None,
        };

        if let Some(index) =
            self.find_text_entry_index(text_transaction_id(&new_entry), text_event_id(&new_entry))
        {
            self.entries[index] = new_entry;
        } else {
            self.entries.push(new_entry);
        }
    }

    fn insert_or_update_outgoing_queued(&mut self, queued: OutgoingTextQueued) {
        let new_entry = RoomTimelineEntry::Text {
            event_id: None,
            transaction_id: Some(queued.transaction_id),
            sender: queued.sender,
            timestamp_ms: queued.timestamp_ms,
            body: queued.body,
            send_state: TimelineSendState::Pending,
            send_error: None,
        };

        if let Some(index) =
            self.find_text_entry_index(text_transaction_id(&new_entry), text_event_id(&new_entry))
        {
            self.entries[index] = new_entry;
        } else {
            self.entries.push(new_entry);
        }
    }

    fn mark_outgoing_sent(&mut self, sent: OutgoingTextSent) {
        let transaction_index = self
            .find_text_entry_index(Some(sent.transaction_id.as_str()), None)
            .expect("outgoing sent requires an existing queued timeline row");
        let duplicate_event_index = self
            .find_text_entry_index(None, Some(sent.event_id.as_str()))
            .filter(|event_index| *event_index != transaction_index);
        let duplicate_event =
            duplicate_event_index.map(|event_index| self.entries[event_index].clone());

        let RoomTimelineEntry::Text {
            event_id,
            sender,
            timestamp_ms,
            body,
            send_state,
            send_error,
            ..
        } = self
            .entries
            .get_mut(transaction_index)
            .expect("transaction row index must resolve")
        else {
            panic!("outgoing sent requires a text timeline row");
        };

        *event_id = Some(sent.event_id);
        if let Some(RoomTimelineEntry::Text {
            sender: synced_sender,
            timestamp_ms: synced_timestamp_ms,
            body: synced_body,
            ..
        }) = duplicate_event
        {
            *sender = synced_sender;
            *timestamp_ms = synced_timestamp_ms;
            *body = synced_body;
        }
        *send_state = TimelineSendState::Sent;
        *send_error = None;

        if let Some(duplicate_event_index) = duplicate_event_index {
            self.entries.remove(duplicate_event_index);
        }
    }

    fn mark_outgoing_failed(&mut self, failed: OutgoingTextFailed) {
        let RoomTimelineEntry::Text {
            send_state,
            send_error,
            ..
        } = self
            .find_text_entry_mut(Some(failed.transaction_id.as_str()), None)
            .expect("outgoing failed requires an existing queued timeline row")
        else {
            panic!("outgoing failed requires a text timeline row");
        };

        *send_state = TimelineSendState::Failed;
        *send_error = Some(TimelineSendError {
            message: failed.message,
            recoverable: failed.recoverable,
        });
    }

    fn find_text_entry_index(
        &self,
        transaction_id: Option<&str>,
        event_id: Option<&str>,
    ) -> Option<usize> {
        if let Some(transaction_id) = transaction_id
            && let Some(index) = self.entries.iter().position(|entry| {
                matches!(
                    entry,
                    RoomTimelineEntry::Text {
                        transaction_id: Some(existing_transaction_id),
                        ..
                    } if existing_transaction_id == transaction_id
                )
            })
        {
            return Some(index);
        }

        if let Some(event_id) = event_id
            && let Some(index) = self.entries.iter().position(|entry| {
                matches!(
                    entry,
                    RoomTimelineEntry::Text {
                        event_id: Some(existing_event_id),
                        ..
                    } if existing_event_id == event_id
                )
            })
        {
            return Some(index);
        }

        None
    }

    fn find_text_entry_mut(
        &mut self,
        transaction_id: Option<&str>,
        event_id: Option<&str>,
    ) -> Option<&mut RoomTimelineEntry> {
        let index = self.find_text_entry_index(transaction_id, event_id)?;
        self.entries.get_mut(index)
    }
}

fn text_event_id(entry: &RoomTimelineEntry) -> Option<&str> {
    match entry {
        RoomTimelineEntry::Text { event_id, .. } => event_id.as_deref(),
        RoomTimelineEntry::System { .. } => None,
    }
}

fn text_transaction_id(entry: &RoomTimelineEntry) -> Option<&str> {
    match entry {
        RoomTimelineEntry::Text { transaction_id, .. } => transaction_id.as_deref(),
        RoomTimelineEntry::System { .. } => None,
    }
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
    Text { body: Cow<'a, str> },
    LocalCommand(LocalSlashCommand<'a>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum LocalSlashCommand<'a> {
    Status,
    Window { target: &'a str },
    Clear,
    Invalid { feedback: String },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WindowTargetError {
    Unknown,
    Ambiguous,
}

struct WindowState {
    controls: Controls,
    event_queue: Arc<Mutex<VecDeque<Event>>>,
    command_sender: Option<CommandSender>,
    automation: Option<AutomationState>,
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
    switchbar_views: Vec<ViewId>,
    room_activity: BTreeSet<String>,
    compose_history: ComposeHistory,
    edit_window_proc: WNDPROC,
    model: UiModel,
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
    let automation_enabled = automation_enabled_for_command(&command);

    let instance = unsafe { GetModuleHandleW(null()) };
    if instance.is_null() {
        return Err(last_os_error("GetModuleHandleW"));
    }

    init_common_controls()?;
    register_frame_class(instance)?;
    register_mdi_child_class(instance)?;
    register_timeline_class(instance)?;
    register_login_dialog_class(instance)?;

    let exit_code = Arc::new(AtomicI32::new(0));
    let event_queue = Arc::new(Mutex::new(VecDeque::new()));

    let state = Box::new(WindowState {
        controls: Controls::default(),
        event_queue: Arc::clone(&event_queue),
        command_sender: None,
        automation: None,
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
        switchbar_views: Vec::new(),
        room_activity: BTreeSet::new(),
        compose_history: ComposeHistory::default(),
        edit_window_proc: None,
        model: initial_model(),
    });

    let hwnd = create_main_window(instance, state)?;

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
            None
        };

        unsafe {
            ShowWindow(hwnd, SW_SHOW);
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

fn create_main_window(instance: *mut c_void, state: Box<WindowState>) -> Result<HWND, String> {
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
            1024,
            768,
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
            clamp_timeline_top_line(hwnd);
            update_timeline_scrollbar(hwnd);
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

unsafe fn set_timeline_text(hwnd: HWND, text_ptr: *const u16) -> LRESULT {
    let (rendered_text, lines) = normalize_timeline_text(&wide_ptr_to_string(text_ptr));
    let state = timeline_state_mut(hwnd);
    state.rendered_text = rendered_text;
    state.lines = lines;
    clamp_timeline_top_line(hwnd);
    update_timeline_scrollbar(hwnd);
    InvalidateRect(hwnd, null(), 1);
    1
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
    state.top_line = clamped_timeline_top_line(state.top_line, state.lines.len(), visible_rows);
}

unsafe fn update_timeline_scrollbar(hwnd: HWND) {
    let visible_rows = timeline_visible_rows(hwnd);
    let state = timeline_state_ref(hwnd);
    let expected_max = state.lines.len().saturating_sub(1) as i32;
    let expected_page = visible_rows.max(1).min(state.lines.len().max(1)) as u32;
    let scroll_info = SCROLLINFO {
        cbSize: size_of::<SCROLLINFO>() as u32,
        fMask: SIF_RANGE | SIF_PAGE | SIF_POS,
        nMin: 0,
        nMax: expected_max,
        nPage: visible_rows as u32,
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
            state.lines.len(),
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

unsafe fn scroll_timeline(hwnd: HWND, wparam: WPARAM) -> LRESULT {
    let command = (wparam & 0xffff) as i32;
    let visible_rows = timeline_visible_rows(hwnd);
    let max_top_line = {
        let state = timeline_state_ref(hwnd);
        timeline_max_top_line(state.lines.len(), visible_rows)
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

    0
}

unsafe fn paint_timeline(hwnd: HWND) {
    let mut paint = PAINTSTRUCT::default();
    let hdc = BeginPaint(hwnd, &mut paint);
    FillRect(hdc, &paint.rcPaint, (COLOR_WINDOW as isize + 1) as HBRUSH);
    SetBkMode(hdc, TRANSPARENT as i32);

    let state = timeline_state_ref(hwnd);
    let visible_rows = timeline_visible_rows(hwnd);
    for (row, line) in state
        .lines
        .iter()
        .skip(state.top_line)
        .take(visible_rows)
        .enumerate()
    {
        let text = wide_null(line);
        TextOutW(
            hdc,
            TIMELINE_LEFT_PADDING,
            state.vertical_padding + row as i32 * state.line_height,
            text.as_ptr(),
            text.len() as i32 - 1,
        );
    }

    EndPaint(hwnd, &paint);
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

unsafe fn create_controls(hwnd: HWND) -> Result<(), String> {
    let instance = GetModuleHandleW(null());
    let state = state_mut(hwnd);

    create_account_menu(hwnd)?;

    state.controls.switchbar = CreateWindowExW(
        0,
        TOOLBARCLASSNAMEW,
        null(),
        WS_CHILD | WS_VISIBLE,
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

unsafe fn login_dialog_state(hwnd: HWND) -> Option<&'static LoginDialogState> {
    let state = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *const LoginDialogState;
    (!state.is_null()).then_some(&*state)
}

unsafe fn login_dialog_state_mut(hwnd: HWND) -> Option<&'static mut LoginDialogState> {
    let state = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut LoginDialogState;
    (!state.is_null()).then_some(&mut *state)
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
                .is_some_and(|room_id| actionable_room_by_id(&rooms, room_id).is_none())
            {
                state.model.selected_room = None;
            }
            state
                .room_activity
                .retain(|room_id| actionable_room_by_id(&rooms, room_id).is_some());
            state.model.rooms = rooms;
        }
        Event::TimelineText(timeline) => {
            if state.active_view != ViewId::Room(timeline.room_id.clone())
                && actionable_room_by_id(&state.model.rooms, &timeline.room_id).is_some()
            {
                state.room_activity.insert(timeline.room_id.clone());
            }
            state.model.status = format!("timeline update from {}", timeline.room_id);
            let room_id = timeline.room_id.clone();
            let room_view = state.model.room_views.entry(room_id.clone()).or_default();
            room_view.upsert_synced_text(timeline.clone());
            state.model.last_timeline = Some(LastTimelineText {
                room_id,
                event_id: Some(timeline.event_id),
                transaction_id: timeline.transaction_id,
                sender: timeline.sender,
                timestamp_ms: timeline.timestamp_ms,
                body: timeline.body,
                send_state: TimelineSendState::Sent,
                send_error: None,
            });
        }
        Event::OutgoingTextQueued(queued) => {
            state.model.status = format!("timeline update from {}", queued.room_id);
            state
                .model
                .room_views
                .entry(queued.room_id.clone())
                .or_default()
                .insert_or_update_outgoing_queued(queued.clone());
            state.model.last_timeline = Some(LastTimelineText {
                room_id: queued.room_id,
                event_id: None,
                transaction_id: Some(queued.transaction_id),
                sender: queued.sender,
                timestamp_ms: queued.timestamp_ms,
                body: queued.body,
                send_state: TimelineSendState::Pending,
                send_error: None,
            });
        }
        Event::OutgoingTextSent(sent) => {
            state.model.status = format!("timeline update from {}", sent.room_id);
            state
                .model
                .room_views
                .entry(sent.room_id.clone())
                .or_default()
                .mark_outgoing_sent(sent.clone());
            if let Some(last_timeline) = state.model.last_timeline.as_mut()
                && last_timeline.room_id == sent.room_id
                && last_timeline.transaction_id.as_deref() == Some(sent.transaction_id.as_str())
            {
                last_timeline.event_id = Some(sent.event_id);
                last_timeline.send_state = TimelineSendState::Sent;
                last_timeline.send_error = None;
            }
        }
        Event::OutgoingTextFailed(failed) => {
            if state.active_view != ViewId::Room(failed.room_id.clone())
                && actionable_room_by_id(&state.model.rooms, &failed.room_id).is_some()
            {
                state.room_activity.insert(failed.room_id.clone());
            }
            state.model.status = format!("timeline update from {}", failed.room_id);
            state
                .model
                .room_views
                .entry(failed.room_id.clone())
                .or_default()
                .mark_outgoing_failed(failed.clone());
            if let Some(last_timeline) = state.model.last_timeline.as_mut()
                && last_timeline.room_id == failed.room_id
                && last_timeline.transaction_id.as_deref() == Some(failed.transaction_id.as_str())
            {
                last_timeline.send_state = TimelineSendState::Failed;
                last_timeline.send_error = Some(TimelineSendError {
                    message: failed.message,
                    recoverable: failed.recoverable,
                });
            }
        }
        Event::RoomState(room_state) => {
            if state.active_view != ViewId::Room(room_state.room_id.clone())
                && actionable_room_by_id(&state.model.rooms, &room_state.room_id).is_some()
            {
                state.room_activity.insert(room_state.room_id.clone());
            }
            state.model.status = format!("timeline update from {}", room_state.room_id);
            let room_view = state
                .model
                .room_views
                .entry(room_state.room_id)
                .or_default();
            match room_state.event_kind {
                RoomStateEventKind::Topic => {
                    let RoomStateContent::Topic(topic) = room_state.content else {
                        panic!("topic room state must carry topic content");
                    };
                    room_view.topic = Some(topic.topic.clone());
                    room_view.entries.push(RoomTimelineEntry::System {
                        sender: room_state.sender,
                        timestamp_ms: room_state.timestamp_ms,
                        kind: RoomStateEventKind::Topic,
                        state_key: room_state.state_key,
                        display_name: None,
                        text: topic.topic,
                    });
                }
                RoomStateEventKind::Member => {
                    let RoomStateContent::Member(member) = room_state.content else {
                        panic!("member room state must carry member content");
                    };
                    room_view.entries.push(RoomTimelineEntry::System {
                        sender: room_state.sender,
                        timestamp_ms: room_state.timestamp_ms,
                        kind: RoomStateEventKind::Member,
                        state_key: room_state.state_key,
                        display_name: member.displayname,
                        text: member.membership,
                    });
                }
            }
        }
        Event::VerificationRequest(request) => {
            state.model.status = format!("verification request from {}", request.user_id);
            state.model.last_verification = Some(request);
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
            state.model.last_timeline = None;
            state.model.last_verification = None;
            state.model.last_error = None;
            state.room_activity.clear();
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
            state.model.status = format!("error during {}", operation_name(error.operation));
            state.model.lifecycle_state = LifecycleState::Error;
            state.model.last_error = Some(error);
        }
    }
    Ok(())
}

fn render_room_lines(model: &UiModel, room_id: &str) -> Vec<String> {
    let room = room_by_id(&model.rooms, room_id).expect("room view must target an existing room");
    let mut lines = Vec::new();
    let mut header = format!("Room: {}", room.display_name);
    if room.encrypted {
        header.push_str(" [e2ee]");
    }
    header.push_str(&format!(" ({room_id})"));
    lines.push(header);

    let room_view = model.room_views.get(room_id);
    if let Some(topic) = room_view.and_then(|view| view.topic.as_deref()) {
        lines.push(format!("Topic: {topic}"));
    }

    if let Some(room_view) = room_view {
        for entry in &room_view.entries {
            match entry {
                RoomTimelineEntry::Text {
                    send_state,
                    sender,
                    timestamp_ms,
                    body,
                    ..
                } => {
                    let suffix = match send_state {
                        TimelineSendState::Pending => " (sending)",
                        TimelineSendState::Sent => "",
                        TimelineSendState::Failed => " (failed)",
                    };
                    lines.push(format!(
                        "[{}] {sender}: {body}{suffix}",
                        format_timestamp_hhmm_utc(*timestamp_ms)
                    ));
                }
                RoomTimelineEntry::System {
                    sender,
                    timestamp_ms,
                    kind,
                    state_key,
                    display_name,
                    text,
                } => match kind {
                    RoomStateEventKind::Topic => lines.push(format!(
                        "* [{}] {sender} set topic: {text}",
                        format_timestamp_hhmm_utc(*timestamp_ms)
                    )),
                    RoomStateEventKind::Member => {
                        let timestamp = format_timestamp_hhmm_utc(*timestamp_ms);
                        match text.as_str() {
                            "join" => {
                                let member = display_name.as_deref().unwrap_or(state_key);
                                lines.push(format!("* [{timestamp}] {member} joined"));
                            }
                            "leave" => {
                                let member = display_name.as_deref().unwrap_or(state_key);
                                lines.push(format!("* [{timestamp}] {member} left"));
                            }
                            membership => lines.push(format!(
                                "* [{timestamp}] {state_key} membership: {membership}"
                            )),
                        }
                    }
                },
            }
        }
    }

    lines
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

    if let Some(request) = &model.last_verification {
        lines.push(String::new());
        lines.push("verification".to_owned());
        lines.push(format!("flow_id: {}", request.flow_id));
        lines.push(format!("from_user: {}", request.user_id));
    }

    if let Some(error) = &model.last_error {
        lines.push(String::new());
        lines.push("error".to_owned());
        lines.push(format!("operation: {}", operation_name(error.operation)));
        lines.push(format!("message: {}", error.message));
    }

    lines
}

fn frame_status_text(model: &UiModel) -> String {
    let mut text = model.status.clone();

    if let Some(request) = &model.last_verification {
        text.push_str(" | verification: ");
        text.push_str(&request.flow_id);
        text.push_str(" from ");
        text.push_str(&request.user_id);
    }

    if let Some(error) = &model.last_error {
        text.push_str(" | error: ");
        text.push_str(operation_name(error.operation));
        text.push_str(": ");
        text.push_str(&error.message);
    }

    text
}

unsafe fn refresh_status_bar(hwnd: HWND) -> Result<(), String> {
    let status_bar = state_ref(hwnd).controls.status_bar;
    if status_bar.is_null() {
        return Err("status bar control was not created".to_owned());
    }

    let text = wide_null(&frame_status_text(&state_ref(hwnd).model));
    if SendMessageW(status_bar, SB_SETTEXTW, 0, text.as_ptr() as LPARAM) == 0 {
        return Err("failed to update status bar text".to_owned());
    }

    Ok(())
}

fn render_view_text(lines: &[String]) -> String {
    lines.join("\r\n")
}

fn resolve_active_view(
    active_view: &ViewId,
    selected_room: Option<&str>,
    rooms: &[RoomSummary],
) -> ViewId {
    match active_view {
        ViewId::Status => ViewId::Status,
        ViewId::Room(room_id) if actionable_room_by_id(rooms, room_id).is_some() => {
            ViewId::Room(room_id.clone())
        }
        ViewId::Room(_) => {
            if let Some(room_id) =
                selected_room.filter(|room_id| actionable_room_by_id(rooms, room_id).is_some())
            {
                ViewId::Room(room_id.to_owned())
            } else if let Some(room) = rooms.iter().find(|room| is_actionable_room(room)) {
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
        .filter(|view_id| {
            matches!(
                view_id,
                ViewId::Room(room_id)
                    if actionable_room_by_id(&state.model.rooms, room_id).is_none()
            )
        })
        .cloned()
        .collect::<Vec<_>>();

    let removed = stale_view_ids
        .into_iter()
        .filter_map(|view_id| {
            let view = state.views.remove(&view_id)?;
            state.hwnd_views.remove(&(view.child as isize));
            Some(view)
        })
        .collect::<Vec<_>>();

    if !matches!(
        resolve_active_view(
            &state.active_view,
            state.model.selected_room.as_deref(),
            &state.model.rooms,
        ),
        ViewId::Room(_)
    ) {
        state.active_view = ViewId::Status;
    }

    removed
}

fn is_actionable_room(room: &RoomSummary) -> bool {
    !room.is_space
}

fn room_by_id<'a>(rooms: &'a [RoomSummary], room_id: &str) -> Option<&'a RoomSummary> {
    rooms.iter().find(|room| room.room_id == room_id)
}

fn actionable_room_by_id<'a>(rooms: &'a [RoomSummary], room_id: &str) -> Option<&'a RoomSummary> {
    room_by_id(rooms, room_id).filter(|room| is_actionable_room(room))
}

fn room_tree_label(room: &RoomSummary) -> String {
    let mut label = room.display_name.clone();
    if room.encrypted {
        label.push_str(" [e2ee]");
    }
    label
}

fn space_tree_label(space: &RoomSummary) -> String {
    space.display_name.clone()
}

fn view_title(model: &UiModel, view_id: &ViewId) -> String {
    match view_id {
        ViewId::Status => "Status".to_owned(),
        ViewId::Room(room_id) => {
            let room = room_by_id(&model.rooms, room_id)
                .expect("room view activation must target an existing room");
            room_tree_label(room)
        }
    }
}

fn switchbar_button_text(state: &WindowState, view_id: &ViewId) -> String {
    let mut text = view_title(&state.model, view_id);
    if matches!(view_id, ViewId::Room(room_id) if state.room_activity.contains(room_id)) {
        text.insert_str(0, "* ");
    }
    text
}

fn view_identity_from_view_id(view_id: &ViewId) -> ViewIdentity {
    match view_id {
        ViewId::Status => ViewIdentity::Status,
        ViewId::Room(room_id) => ViewIdentity::Room {
            room_id: room_id.clone(),
        },
    }
}

fn view_id_from_identity(identity: &ViewIdentity) -> ViewId {
    match identity {
        ViewIdentity::Status => ViewId::Status,
        ViewIdentity::Room { room_id } => ViewId::Room(room_id.clone()),
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
        ViewId::Room(_) => (
            TIMELINE_CLASS_NAME,
            WS_CHILD | WS_VISIBLE | WS_VSCROLL,
            WS_EX_CLIENTEDGE,
        ),
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

fn unregister_view_by_child(state: &mut WindowState, child: HWND) -> Option<ViewId> {
    let view_id = state.hwnd_views.remove(&(child as isize))?;
    let removed = state
        .views
        .remove(&view_id)
        .expect("HWND-to-view registry must match tracked views");
    assert_eq!(
        removed.child, child,
        "tracked child HWND must match registry"
    );
    if state.active_view == view_id {
        state.active_view = resolve_active_view(
            &state.active_view,
            state.model.selected_room.as_deref(),
            &state.model.rooms,
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
    if actionable_room_by_id(&state_ref(hwnd).model.rooms, room_id).is_none() {
        return Err(format!("unknown room id: {room_id}"));
    }

    ensure_view(hwnd, ViewId::Room(room_id.to_owned()))
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

unsafe fn refresh_view_content(hwnd: HWND, view_id: &ViewId) -> Result<(), String> {
    let (child, content, title, text) = {
        let state = state_ref(hwnd);
        let view = state
            .views
            .get(view_id)
            .ok_or_else(|| "view state was not tracked".to_owned())?;
        let title = view_title(&state.model, view_id);
        let lines = match view_id {
            ViewId::Status => render_status_lines(&state.model),
            ViewId::Room(room_id) => render_room_lines(&state.model, room_id),
        };
        (view.child, view.content, title, render_view_text(&lines))
    };

    let title = wide_null(&title);
    SendMessageW(child, WM_SETTEXT, 0, title.as_ptr() as LPARAM);
    match view_id {
        ViewId::Status => set_edit_text(content, &text),
        ViewId::Room(_) => set_window_text(content, &text),
    }
    Ok(())
}

unsafe fn activate_view(
    hwnd: HWND,
    view_id: &ViewId,
    preferred_tree_item: Option<isize>,
) -> Result<(), String> {
    let mut cleared_room_activity = false;
    let child = match view_id {
        ViewId::Status => {
            state_mut(hwnd).model.selected_room = None;
            ensure_status_view(hwnd)?
        }
        ViewId::Room(room_id) => {
            {
                let state = state_mut(hwnd);
                select_room(state, room_id)?;
                cleared_room_activity = state.room_activity.remove(room_id);
            }
            ensure_room_view(hwnd, room_id)?
        }
    };

    {
        let state = state_mut(hwnd);
        state.active_view = view_id.clone();
    }

    refresh_view_content(hwnd, view_id)?;
    select_tree_item(hwnd, view_id, preferred_tree_item);

    let mdi_client = state_ref(hwnd).controls.mdi_client;
    SendMessageW(mdi_client, WM_MDIACTIVATE, child as WPARAM, 0);
    SendMessageW(mdi_client, WM_MDIMAXIMIZE, child as WPARAM, 0);
    if cleared_room_activity {
        rebuild_switchbar(hwnd)?;
    } else {
        refresh_switchbar_state(hwnd)?;
    }
    Ok(())
}

unsafe fn sync_views_and_tree(hwnd: HWND) -> Result<(), String> {
    ensure_status_view(hwnd)?;

    let room_ids = state_ref(hwnd)
        .model
        .rooms
        .iter()
        .filter(|room| is_actionable_room(room))
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
            &state.model.rooms,
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
    let actionable_rooms = rooms
        .iter()
        .filter(|room| is_actionable_room(room))
        .cloned()
        .collect::<Vec<_>>();
    let rooms_label = if rooms.is_empty() {
        "rooms: offline".to_owned()
    } else {
        format!("rooms: {}", actionable_rooms.len())
    };
    let rooms_root = insert_tree_item(tree, root, &rooms_label);

    let flat_mode = rooms
        .iter()
        .all(|room| !room.is_space && room.space_children.is_empty());
    if flat_mode {
        for room in &actionable_rooms {
            render_room_tree_leaf(state, rooms_root, room);
        }
    } else {
        let room_map = rooms
            .iter()
            .cloned()
            .map(|room| (room.room_id.clone(), room))
            .collect::<BTreeMap<_, _>>();
        let valid_space_children = rooms
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

        let mut rendered_roots = rooms
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
        for room in rooms.iter().filter(|room| room.is_space) {
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

        let unspaced_rooms = actionable_rooms
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

    expand_tree_item(tree, root);
    expand_tree_item(tree, rooms_root);
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

unsafe fn expand_tree_item(tree: HWND, item: isize) {
    SendMessageW(tree, TVM_EXPAND, TVE_EXPAND as WPARAM, item as LPARAM);
}

unsafe fn insert_mapped_tree_item(
    state: &mut WindowState,
    parent: isize,
    label: &str,
    view_id: ViewId,
) -> isize {
    let item = insert_tree_item(state.controls.tree, parent, label);
    state.tree_items.insert(item, view_id.clone());
    state.primary_tree_items.entry(view_id).or_insert(item);
    item
}

unsafe fn render_room_tree_leaf(state: &mut WindowState, parent: isize, room: &RoomSummary) {
    insert_mapped_tree_item(
        state,
        parent,
        &room_tree_label(room),
        ViewId::Room(room.room_id.clone()),
    );
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
            let dump = dump_model_from_ui(&state_ref(hwnd).model);
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
        ParsedCommand::DumpTimeline { id, args } => {
            let room_id = resolve_timeline_room_id(state_ref(hwnd), args.room_id.as_deref())
                .map_err(|message| failure_response(Some(id.clone()), "unknown_room", message))?;
            let entries = state_ref(hwnd)
                .model
                .room_views
                .get(&room_id)
                .map(|room_view| {
                    room_view
                        .entries
                        .iter()
                        .map(|entry| dump_timeline_entry_from_room_entry(&room_id, entry))
                        .collect()
                })
                .unwrap_or_default();
            serialize_success_json(&id, DumpTimelineResponse { room_id, entries })
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
            let (rooms, timeline_events) =
                apply_event_load(hwnd, args.events).map_err(|message| {
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
        | ParsedCommand::DumpTimeline { id, .. }
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
        | ParsedCommand::SelectNavigationPath { id, .. } => id,
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
        if !is_actionable_room(room) {
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
                if actionable_room_by_id(&state.model.rooms, room_id).is_none() =>
            {
                Some(room_id.clone())
            }
            ViewId::Room(_) => None,
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
    let buttons = view_ids
        .iter()
        .enumerate()
        .map(|(index, _)| TBBUTTON {
            iBitmap: I_IMAGENONE,
            idCommand: (SWITCHBAR_COMMAND_FIRST + index) as i32,
            fsState: TBSTATE_ENABLED as u8,
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

    let active_view = state_ref(hwnd).active_view.clone();
    let view_ids = state_ref(hwnd).switchbar_views.clone();
    for (index, view_id) in view_ids.iter().enumerate() {
        let command_id = SWITCHBAR_COMMAND_FIRST + index;
        let checked = usize::from(*view_id == active_view);
        if SendMessageW(switchbar, TB_CHECKBUTTON, command_id, checked as LPARAM) == 0 {
            return Err(format!("failed to refresh switchbar button {command_id}"));
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

unsafe fn read_navigation_node(tree: HWND, item: isize) -> Result<NavigationNode, String> {
    let text = read_tree_item_text(tree, item)?;
    let mut children = Vec::new();
    let mut child =
        SendMessageW(tree, TVM_GETNEXTITEM, TVGN_CHILD as WPARAM, item as LPARAM) as isize;
    while child != 0 {
        children.push(read_navigation_node(tree, child)?);
        child = SendMessageW(tree, TVM_GETNEXTITEM, TVGN_NEXT as WPARAM, child as LPARAM) as isize;
    }

    Ok(NavigationNode { text, children })
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

    Ok(())
}

unsafe fn dump_status_bar_state(hwnd: HWND) -> Result<StatusBarState, String> {
    if state_ref(hwnd).controls.status_bar.is_null() {
        return Err("status bar control was not created".to_owned());
    }

    Ok(StatusBarState {
        text: frame_status_text(&state_ref(hwnd).model),
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
                lines: split_rendered_lines(&read_window_text(view.content)),
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
    let items = ordered_view_ids(state)
        .into_iter()
        .map(|view_id| SwitchbarItem {
            identity: view_identity_from_view_id(&view_id),
            title: switchbar_button_text(state, &view_id),
            active: view_identity_from_view_id(&view_id) == active,
            has_activity: matches!(
                &view_id,
                ViewId::Room(room_id) if state.room_activity.contains(room_id)
            ),
        })
        .collect();

    Ok(SwitchbarState { items })
}

unsafe fn dump_ui_state(hwnd: HWND) -> Result<UiState, String> {
    Ok(UiState {
        navigation: dump_navigation_tree(hwnd)?,
        mdi: dump_mdi_state(hwnd)?,
        status_bar: dump_status_bar_state(hwnd)?,
        switchbar: dump_switchbar_state(hwnd)?,
    })
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
                encrypted: false,
                is_space: false,
                space_children: Vec::new(),
            },
            RoomSummary {
                room_id: "!two:example".to_owned(),
                display_name: "Two".to_owned(),
                encrypted: true,
                is_space: false,
                space_children: Vec::new(),
            },
        ]),
        Event::TimelineText(TimelineText {
            room_id: "!one:example".to_owned(),
            event_id: "$one-first".to_owned(),
            transaction_id: None,
            sender: "@alice:example".to_owned(),
            timestamp_ms: 1_700_000_000_000,
            body: "first line".to_owned(),
        }),
        Event::TimelineText(TimelineText {
            room_id: "!one:example".to_owned(),
            event_id: "$one-second".to_owned(),
            transaction_id: None,
            sender: "@alice:example".to_owned(),
            timestamp_ms: 1_700_000_060_000,
            body: "second line".to_owned(),
        }),
        Event::TimelineText(TimelineText {
            room_id: "!two:example".to_owned(),
            event_id: "$two-third".to_owned(),
            transaction_id: None,
            sender: "@bob:example".to_owned(),
            timestamp_ms: 1_700_000_120_000,
            body: "third line".to_owned(),
        }),
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
        COMMAND_ID_DUMP_TIMELINE,
        json!({"id":"timeline","args":{"room_id":"!one:example"}}),
    )?;
    if timeline["ok"] != json!(true)
        || timeline["json"]
            != json!({
                "room_id":"!one:example",
                "entries":[
                    {
                        "type":"text",
                        "room_id":"!one:example",
                        "event_id":"$one-first",
                        "transaction_id":null,
                        "sender":"@alice:example",
                        "timestamp_ms":1700000000000u64,
                        "body":"first line",
                        "send_state":"sent",
                        "send_error":null
                    },
                    {
                        "type":"text",
                        "room_id":"!one:example",
                        "event_id":"$one-second",
                        "transaction_id":null,
                        "sender":"@alice:example",
                        "timestamp_ms":1700000060000u64,
                        "body":"second line",
                        "send_state":"sent",
                        "send_error":null
                    }
                ]
            })
    {
        return Err(format!(
            "self-test failed: unexpected dump-timeline response {timeline}"
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

    {
        let state = state_mut(hwnd);
        for event in events {
            apply_core_event(state, event)?;
            state.event_generation += 1;
        }
    }

    sync_views_and_tree(hwnd)?;
    Ok(())
}

unsafe fn apply_event_load(hwnd: HWND, events: Vec<Event>) -> Result<(usize, usize), String> {
    {
        let state = state_mut(hwnd);
        state.synthetic_load_in_progress = true;
        state.pending_login_homeserver_url = None;
        state.current_session_homeserver_url = None;
        state.switchbar_views.clear();
        state.room_activity.clear();
        state.model = initial_model();
    }

    apply_events_and_refresh(hwnd, events)?;

    let (rooms, timeline_events, active_view) = {
        let state = state_mut(hwnd);
        let rooms = state.model.rooms.len();
        let timeline_events = state
            .model
            .room_views
            .values()
            .map(|room_view| {
                room_view
                    .entries
                    .iter()
                    .filter(|entry| matches!(entry, RoomTimelineEntry::Text { .. }))
                    .count()
            })
            .sum();
        let selected_room = state
            .model
            .rooms
            .iter()
            .find(|room| is_actionable_room(room))
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

    state_mut(hwnd).synthetic_load_in_progress = false;
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
    if let Some(view_id) = view_id {
        let _ = activate_view(hwnd, &view_id, Some(selected_item));
    }

    true
}

fn select_room(state: &mut WindowState, room_id: &str) -> Result<(), String> {
    if actionable_room_by_id(&state.model.rooms, room_id).is_none() {
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
        ComposeSubmission::Text { body } => {
            submit_compose_with_history_entry(state_mut(hwnd), body.into_owned(), input, None)?;
        }
        ComposeSubmission::LocalCommand(command) => {
            execute_local_slash_command(hwnd, command)?;
            state_mut(hwnd).compose_history.record_submission(input);
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
    let room_id = submit_compose(state_mut(hwnd), body, explicit_room_id)?;
    let empty = wide_null("");
    SendMessageW(edit, WM_SETTEXT, 0, empty.as_ptr() as LPARAM);
    Ok(room_id)
}

fn parse_compose_submission(input: &str) -> ComposeSubmission<'_> {
    if !input.starts_with('/') {
        return ComposeSubmission::Text {
            body: Cow::Borrowed(input),
        };
    }

    if input.starts_with("//") {
        return ComposeSubmission::Text {
            body: Cow::Owned(input[1..].to_owned()),
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
        "window" if !argument.is_empty() => LocalSlashCommand::Window { target: argument },
        "window" => LocalSlashCommand::Invalid {
            feedback: "usage: /window <room-id-or-display-name>".to_owned(),
        },
        "clear" if argument.is_empty() => LocalSlashCommand::Clear,
        "clear" => LocalSlashCommand::Invalid {
            feedback: "usage: /clear".to_owned(),
        },
        _ => LocalSlashCommand::Invalid {
            feedback: format!("unknown command: {}", &input[..command_end]),
        },
    }
}

fn submit_compose(
    state: &mut WindowState,
    body: String,
    explicit_room_id: Option<&str>,
) -> Result<String, String> {
    let history_entry = body.clone();
    submit_compose_with_history_entry(state, body, history_entry, explicit_room_id)
}

fn submit_compose_with_history_entry(
    state: &mut WindowState,
    body: String,
    history_entry: String,
    explicit_room_id: Option<&str>,
) -> Result<String, String> {
    if body.is_empty() {
        return Err("message body must not be empty".to_owned());
    }

    let room_id = match explicit_room_id {
        Some(room_id) => room_id.to_owned(),
        None => state
            .model
            .selected_room
            .clone()
            .ok_or_else(|| "no room selected".to_owned())?,
    };

    if actionable_room_by_id(&state.model.rooms, &room_id).is_none() {
        return Err(format!("unknown room id: {room_id}"));
    }

    let sender = state
        .command_sender
        .as_ref()
        .ok_or_else(|| "core command sender is not available".to_owned())?;
    let send_body = body.clone();
    sender
        .send(Command::SendText(SendTextRequest {
            room_id: room_id.clone(),
            body: send_body,
        }))
        .map_err(|err| format!("failed to enqueue send text command: {err}"))?;
    state.compose_history.record_submission(history_entry);

    Ok(room_id)
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
        LocalSlashCommand::Window { target } => {
            let room_id = match resolve_window_command_target(&state_ref(hwnd).model.rooms, target)
            {
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
        LocalSlashCommand::Clear => match state_ref(hwnd).active_view.clone() {
            ViewId::Room(room_id) => {
                if let Some(view) = state_mut(hwnd).model.room_views.get_mut(&room_id) {
                    view.clear_visible_entries();
                }
                refresh_view_content(hwnd, &ViewId::Room(room_id.clone()))?;
                set_local_command_status(hwnd, format!("cleared window: {room_id}"))
            }
            ViewId::Status => {
                set_local_command_status(hwnd, "nothing to clear in status view".to_owned())
            }
        },
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

fn resolve_window_command_target(
    rooms: &[RoomSummary],
    target: &str,
) -> Result<String, WindowTargetError> {
    if let Some(room) = actionable_room_by_id(rooms, target) {
        return Ok(room.room_id.clone());
    }

    let mut matches = rooms
        .iter()
        .filter(|room| is_actionable_room(room) && room.display_name == target);
    let Some(room) = matches.next() else {
        return Err(WindowTargetError::Unknown);
    };
    if matches.next().is_some() {
        return Err(WindowTargetError::Ambiguous);
    }
    Ok(room.room_id.clone())
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
        let _ = submit_compose_from_edit(parent);
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

fn dump_model_from_ui(model: &UiModel) -> DumpModel {
    DumpModel {
        status: model.status.clone(),
        lifecycle_state: model.lifecycle_state,
        rooms: model.rooms.iter().map(dump_room_from_summary).collect(),
        last_timeline: model
            .last_timeline
            .as_ref()
            .map(dump_timeline_line_from_last_timeline),
        last_verification: model
            .last_verification
            .as_ref()
            .map(dump_verification_from_request),
        last_error: model.last_error.as_ref().map(dump_error_from_core),
        selected_room: model.selected_room.clone(),
    }
}

fn dump_timeline_entry_from_room_entry(
    room_id: &str,
    entry: &RoomTimelineEntry,
) -> DumpTimelineEntry {
    match entry {
        RoomTimelineEntry::Text {
            event_id,
            transaction_id,
            sender,
            timestamp_ms,
            body,
            send_state,
            send_error,
        } => DumpTimelineEntry::Text(crate::automation::DumpTimelineLine {
            room_id: room_id.to_owned(),
            event_id: event_id.clone(),
            transaction_id: transaction_id.clone(),
            sender: sender.clone(),
            timestamp_ms: *timestamp_ms,
            body: body.clone(),
            send_state: dump_timeline_send_state(*send_state),
            send_error: send_error.as_ref().map(dump_timeline_send_error_from_model),
        }),
        RoomTimelineEntry::System {
            sender,
            timestamp_ms,
            kind,
            state_key,
            display_name,
            text,
        } => DumpTimelineEntry::RoomState(DumpRoomStateLine {
            room_id: room_id.to_owned(),
            sender: sender.clone(),
            timestamp_ms: *timestamp_ms,
            event_kind: dump_room_state_event_kind(*kind),
            state_key: state_key.clone(),
            content: match kind {
                RoomStateEventKind::Topic => {
                    AutomationRoomStateContent::Topic(DumpRoomTopicState {
                        topic: text.clone(),
                    })
                }
                RoomStateEventKind::Member => {
                    AutomationRoomStateContent::Member(DumpRoomMemberState {
                        membership: text.clone(),
                        displayname: display_name.clone(),
                    })
                }
            },
        }),
    }
}

fn dump_timeline_line_from_last_timeline(
    timeline: &LastTimelineText,
) -> crate::automation::DumpTimelineLine {
    crate::automation::DumpTimelineLine {
        room_id: timeline.room_id.clone(),
        event_id: timeline.event_id.clone(),
        transaction_id: timeline.transaction_id.clone(),
        sender: timeline.sender.clone(),
        timestamp_ms: timeline.timestamp_ms,
        body: timeline.body.clone(),
        send_state: dump_timeline_send_state(timeline.send_state),
        send_error: timeline
            .send_error
            .as_ref()
            .map(dump_timeline_send_error_from_model),
    }
}

fn dump_timeline_send_state(send_state: TimelineSendState) -> DumpTimelineSendState {
    match send_state {
        TimelineSendState::Pending => DumpTimelineSendState::Pending,
        TimelineSendState::Sent => DumpTimelineSendState::Sent,
        TimelineSendState::Failed => DumpTimelineSendState::Failed,
    }
}

fn dump_timeline_send_error_from_model(send_error: &TimelineSendError) -> DumpTimelineSendError {
    DumpTimelineSendError {
        message: send_error.message.clone(),
        recoverable: send_error.recoverable,
    }
}

fn dump_room_state_event_kind(kind: RoomStateEventKind) -> AutomationRoomStateEventKind {
    match kind {
        RoomStateEventKind::Topic => AutomationRoomStateEventKind::Topic,
        RoomStateEventKind::Member => AutomationRoomStateEventKind::Member,
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

    if actionable_room_by_id(&state.model.rooms, &room_id).is_some() {
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
        Operation::SendText => "send text",
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

    use super::*;

    static LOCAL_APP_DATA_LOCK: StdMutex<()> = StdMutex::new(());

    fn room(room_id: &str, display_name: &str, encrypted: bool) -> RoomSummary {
        RoomSummary {
            room_id: room_id.to_owned(),
            display_name: display_name.to_owned(),
            encrypted,
            is_space: false,
            space_children: Vec::new(),
        }
    }

    fn space(room_id: &str, display_name: &str) -> RoomSummary {
        RoomSummary {
            room_id: room_id.to_owned(),
            display_name: display_name.to_owned(),
            encrypted: false,
            is_space: true,
            space_children: Vec::new(),
        }
    }

    fn timeline(room_id: &str, body: &str) -> TimelineText {
        TimelineText {
            room_id: room_id.to_owned(),
            event_id: format!("${room_id}:{}", body.replace(' ', "_")),
            transaction_id: None,
            sender: "@alice:example".to_owned(),
            timestamp_ms: 1_700_000_000_000,
            body: body.to_owned(),
        }
    }

    fn outgoing_queued(room_id: &str, transaction_id: &str, body: &str) -> Event {
        Event::OutgoingTextQueued(OutgoingTextQueued {
            room_id: room_id.to_owned(),
            transaction_id: transaction_id.to_owned(),
            sender: "@alice:example".to_owned(),
            timestamp_ms: 1_700_000_000_000,
            body: body.to_owned(),
        })
    }

    fn outgoing_sent(room_id: &str, transaction_id: &str, event_id: &str) -> Event {
        Event::OutgoingTextSent(OutgoingTextSent {
            room_id: room_id.to_owned(),
            transaction_id: transaction_id.to_owned(),
            event_id: event_id.to_owned(),
        })
    }

    fn outgoing_failed(room_id: &str, transaction_id: &str, message: &str) -> Event {
        Event::OutgoingTextFailed(OutgoingTextFailed {
            room_id: room_id.to_owned(),
            transaction_id: transaction_id.to_owned(),
            message: message.to_owned(),
            recoverable: true,
        })
    }

    fn topic_state(room_id: &str, topic: &str) -> Event {
        Event::RoomState(mmatrix_core::RoomStateEntry {
            room_id: room_id.to_owned(),
            sender: "@alice:example".to_owned(),
            timestamp_ms: 1_700_000_060_000,
            event_kind: RoomStateEventKind::Topic,
            state_key: String::new(),
            content: RoomStateContent::Topic(mmatrix_core::RoomTopicState {
                topic: topic.to_owned(),
            }),
        })
    }

    fn member_state(
        room_id: &str,
        state_key: &str,
        membership: &str,
        display_name: Option<&str>,
    ) -> Event {
        Event::RoomState(mmatrix_core::RoomStateEntry {
            room_id: room_id.to_owned(),
            sender: "@alice:example".to_owned(),
            timestamp_ms: 1_700_000_120_000,
            event_kind: RoomStateEventKind::Member,
            state_key: state_key.to_owned(),
            content: RoomStateContent::Member(mmatrix_core::RoomMemberState {
                membership: membership.to_owned(),
                displayname: display_name.map(str::to_owned),
            }),
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
        WindowState {
            controls: Controls::default(),
            event_queue: Arc::new(Mutex::new(VecDeque::new())),
            command_sender: None,
            automation: None,
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
            switchbar_views: Vec::new(),
            room_activity: BTreeSet::new(),
            compose_history: ComposeHistory::default(),
            edit_window_proc: None,
            model: initial_model(),
        }
    }

    fn tracked_view(hwnd_value: isize) -> MdiViewState {
        MdiViewState {
            child: hwnd_value as HWND,
            content: (hwnd_value + 1) as HWND,
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

    #[test]
    fn timeline_events_accumulate_per_room_in_order() {
        let mut state = test_state();

        apply_core_event(
            &mut state,
            Event::SyncedRooms(vec![
                room("!one:example", "One", false),
                room("!two:example", "Two", true),
            ]),
        )
        .unwrap();
        apply_core_event(
            &mut state,
            Event::TimelineText(timeline("!one:example", "first")),
        )
        .unwrap();
        apply_core_event(
            &mut state,
            Event::TimelineText(timeline("!one:example", "second")),
        )
        .unwrap();
        apply_core_event(
            &mut state,
            Event::TimelineText(timeline("!two:example", "third")),
        )
        .unwrap();

        assert_eq!(
            state
                .model
                .room_views
                .get("!one:example")
                .map(|view| &view.entries),
            Some(&vec![
                RoomTimelineEntry::Text {
                    event_id: Some("$!one:example:first".to_owned()),
                    transaction_id: None,
                    sender: "@alice:example".to_owned(),
                    timestamp_ms: 1_700_000_000_000,
                    body: "first".to_owned(),
                    send_state: TimelineSendState::Sent,
                    send_error: None,
                },
                RoomTimelineEntry::Text {
                    event_id: Some("$!one:example:second".to_owned()),
                    transaction_id: None,
                    sender: "@alice:example".to_owned(),
                    timestamp_ms: 1_700_000_000_000,
                    body: "second".to_owned(),
                    send_state: TimelineSendState::Sent,
                    send_error: None,
                },
            ])
        );
        assert_eq!(
            state
                .model
                .room_views
                .get("!two:example")
                .map(|view| &view.entries),
            Some(&vec![RoomTimelineEntry::Text {
                event_id: Some("$!two:example:third".to_owned()),
                transaction_id: None,
                sender: "@alice:example".to_owned(),
                timestamp_ms: 1_700_000_000_000,
                body: "third".to_owned(),
                send_state: TimelineSendState::Sent,
                send_error: None,
            }])
        );
        assert_eq!(
            state.model.last_timeline,
            Some(LastTimelineText {
                room_id: "!two:example".to_owned(),
                event_id: Some("$!two:example:third".to_owned()),
                transaction_id: None,
                sender: "@alice:example".to_owned(),
                timestamp_ms: 1_700_000_000_000,
                body: "third".to_owned(),
                send_state: TimelineSendState::Sent,
                send_error: None,
            })
        );
    }

    #[test]
    fn outgoing_text_queued_creates_one_pending_row_keyed_by_transaction_id() {
        let mut state = test_state();
        state.model.rooms = vec![room("!ops:example", "Ops", false)];

        apply_core_event(
            &mut state,
            outgoing_queued("!ops:example", "txn-1", "hello"),
        )
        .unwrap();

        assert_eq!(
            state
                .model
                .room_views
                .get("!ops:example")
                .map(|view| &view.entries),
            Some(&vec![RoomTimelineEntry::Text {
                event_id: None,
                transaction_id: Some("txn-1".to_owned()),
                sender: "@alice:example".to_owned(),
                timestamp_ms: 1_700_000_000_000,
                body: "hello".to_owned(),
                send_state: TimelineSendState::Pending,
                send_error: None,
            }])
        );
        assert_eq!(
            state.model.last_timeline,
            Some(LastTimelineText {
                room_id: "!ops:example".to_owned(),
                event_id: None,
                transaction_id: Some("txn-1".to_owned()),
                sender: "@alice:example".to_owned(),
                timestamp_ms: 1_700_000_000_000,
                body: "hello".to_owned(),
                send_state: TimelineSendState::Pending,
                send_error: None,
            })
        );
        assert_eq!(
            render_room_lines(&state.model, "!ops:example"),
            vec![
                "Room: Ops (!ops:example)".to_owned(),
                "[22:13] @alice:example: hello (sending)".to_owned(),
            ]
        );
    }

    #[test]
    fn outgoing_text_sent_updates_existing_row_and_clears_pending_marker() {
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
                .map(|view| &view.entries),
            Some(&vec![RoomTimelineEntry::Text {
                event_id: Some("$event:example.org".to_owned()),
                transaction_id: Some("txn-1".to_owned()),
                sender: "@alice:example".to_owned(),
                timestamp_ms: 1_700_000_000_000,
                body: "hello".to_owned(),
                send_state: TimelineSendState::Sent,
                send_error: None,
            }])
        );
        assert_eq!(
            state.model.last_timeline,
            Some(LastTimelineText {
                room_id: "!ops:example".to_owned(),
                event_id: Some("$event:example.org".to_owned()),
                transaction_id: Some("txn-1".to_owned()),
                sender: "@alice:example".to_owned(),
                timestamp_ms: 1_700_000_000_000,
                body: "hello".to_owned(),
                send_state: TimelineSendState::Sent,
                send_error: None,
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
    fn outgoing_text_reconciles_by_transaction_id_without_duplicate_rows() {
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
        apply_core_event(
            &mut state,
            Event::TimelineText(TimelineText {
                room_id: "!ops:example".to_owned(),
                event_id: "$event:example.org".to_owned(),
                transaction_id: Some("txn-1".to_owned()),
                sender: "@alice:example".to_owned(),
                timestamp_ms: 1_700_000_000_100,
                body: "hello".to_owned(),
            }),
        )
        .unwrap();

        assert_eq!(
            state
                .model
                .room_views
                .get("!ops:example")
                .map(|view| &view.entries),
            Some(&vec![RoomTimelineEntry::Text {
                event_id: Some("$event:example.org".to_owned()),
                transaction_id: Some("txn-1".to_owned()),
                sender: "@alice:example".to_owned(),
                timestamp_ms: 1_700_000_000_100,
                body: "hello".to_owned(),
                send_state: TimelineSendState::Sent,
                send_error: None,
            }])
        );
    }

    #[test]
    fn synced_text_before_sent_update_remains_idempotent() {
        let mut state = test_state();
        state.model.rooms = vec![room("!ops:example", "Ops", false)];

        apply_core_event(
            &mut state,
            outgoing_queued("!ops:example", "txn-1", "hello"),
        )
        .unwrap();
        apply_core_event(
            &mut state,
            Event::TimelineText(TimelineText {
                room_id: "!ops:example".to_owned(),
                event_id: "$event:example.org".to_owned(),
                transaction_id: Some("txn-1".to_owned()),
                sender: "@alice:example".to_owned(),
                timestamp_ms: 1_700_000_000_100,
                body: "hello".to_owned(),
            }),
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
                .map(|view| view.entries.len()),
            Some(1)
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
    fn synced_text_without_transaction_id_collapses_into_outgoing_row_after_sent_update() {
        let mut state = test_state();
        state.model.rooms = vec![room("!ops:example", "Ops", false)];

        apply_core_event(
            &mut state,
            outgoing_queued("!ops:example", "txn-1", "hello"),
        )
        .unwrap();
        apply_core_event(
            &mut state,
            Event::TimelineText(TimelineText {
                room_id: "!ops:example".to_owned(),
                event_id: "$event:example.org".to_owned(),
                transaction_id: None,
                sender: "@alice:example".to_owned(),
                timestamp_ms: 1_700_000_000_100,
                body: "hello".to_owned(),
            }),
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
                .map(|view| &view.entries),
            Some(&vec![RoomTimelineEntry::Text {
                event_id: Some("$event:example.org".to_owned()),
                transaction_id: Some("txn-1".to_owned()),
                sender: "@alice:example".to_owned(),
                timestamp_ms: 1_700_000_000_100,
                body: "hello".to_owned(),
                send_state: TimelineSendState::Sent,
                send_error: None,
            }])
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
    fn outgoing_text_failure_keeps_visible_failed_row_and_error_details() {
        let mut state = test_state();
        state.model.rooms = vec![room("!ops:example", "Ops", false)];

        apply_core_event(
            &mut state,
            outgoing_queued("!ops:example", "txn-1", "hello"),
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
                .map(|view| &view.entries),
            Some(&vec![RoomTimelineEntry::Text {
                event_id: None,
                transaction_id: Some("txn-1".to_owned()),
                sender: "@alice:example".to_owned(),
                timestamp_ms: 1_700_000_000_000,
                body: "hello".to_owned(),
                send_state: TimelineSendState::Failed,
                send_error: Some(TimelineSendError {
                    message: "send failed".to_owned(),
                    recoverable: true,
                }),
            }])
        );
        assert_eq!(
            render_room_lines(&state.model, "!ops:example"),
            vec![
                "Room: Ops (!ops:example)".to_owned(),
                "[22:13] @alice:example: hello (failed)".to_owned(),
            ]
        );
    }

    #[test]
    fn clear_room_timeline_preserves_pending_outgoing_row_for_sent_update() {
        let mut state = test_state();
        state.model.rooms = vec![room("!ops:example", "Ops", false)];

        apply_core_event(
            &mut state,
            Event::TimelineText(timeline("!ops:example", "first")),
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
                .map(|view| &view.entries),
            Some(&vec![RoomTimelineEntry::Text {
                event_id: None,
                transaction_id: Some("txn-1".to_owned()),
                sender: "@alice:example".to_owned(),
                timestamp_ms: 1_700_000_000_000,
                body: "hello".to_owned(),
                send_state: TimelineSendState::Pending,
                send_error: None,
            }])
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
                .map(|view| &view.entries),
            Some(&vec![RoomTimelineEntry::Text {
                event_id: Some("$event:example.org".to_owned()),
                transaction_id: Some("txn-1".to_owned()),
                sender: "@alice:example".to_owned(),
                timestamp_ms: 1_700_000_000_000,
                body: "hello".to_owned(),
                send_state: TimelineSendState::Sent,
                send_error: None,
            }])
        );
    }

    #[test]
    #[should_panic(expected = "outgoing sent requires an existing queued timeline row")]
    fn outgoing_text_sent_without_existing_queued_row_panics() {
        let mut state = test_state();
        state.model.rooms = vec![room("!ops:example", "Ops", false)];

        apply_core_event(
            &mut state,
            outgoing_sent("!ops:example", "txn-1", "$event:example.org"),
        )
        .unwrap();
    }

    #[test]
    #[should_panic(expected = "outgoing failed requires an existing queued timeline row")]
    fn outgoing_text_failed_without_existing_queued_row_panics() {
        let mut state = test_state();
        state.model.rooms = vec![room("!ops:example", "Ops", false)];

        apply_core_event(
            &mut state,
            outgoing_failed("!ops:example", "txn-1", "send failed"),
        )
        .unwrap();
    }

    #[test]
    fn format_timestamp_hhmm_utc_uses_deterministic_utc_minutes() {
        assert_eq!(format_timestamp_hhmm_utc(1_700_000_000_000), "22:13");
    }

    #[test]
    fn render_room_lines_include_header_topic_text_and_system_entries() {
        let mut state = test_state();
        state.model.rooms = vec![room("!ops:example", "Ops", true)];
        apply_core_event(&mut state, topic_state("!ops:example", "incident review")).unwrap();
        apply_core_event(
            &mut state,
            Event::TimelineText(TimelineText {
                room_id: "!ops:example".to_owned(),
                event_id: "$event:example.org".to_owned(),
                transaction_id: None,
                sender: "@alice:example".to_owned(),
                timestamp_ms: 1_700_000_000_000,
                body: "<body>".to_owned(),
            }),
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
                "* [22:14] @alice:example set topic: incident review".to_owned(),
                "[22:13] @alice:example: <body>".to_owned(),
                "* [22:15] carol joined".to_owned(),
                "* [22:15] @dave:example left".to_owned(),
                "* [22:15] @erin:example membership: ban".to_owned(),
            ]
        );
    }

    #[test]
    fn render_status_lines_include_verification_and_error_details() {
        let mut state = test_state();
        state.model.status = "verification request from @alice:example".to_owned();
        state.model.last_verification = Some(VerificationRequest {
            flow_id: "flow-verify-123".to_owned(),
            user_id: "@alice:example".to_owned(),
        });
        state.model.last_error = Some(CoreErrorEvent {
            operation: Operation::SendText,
            message: "message send failed".to_owned(),
        });

        let status_text = render_view_text(&render_status_lines(&state.model));
        assert!(status_text.contains("flow_id: flow-verify-123"));
        assert!(status_text.contains("operation: send text"));
        assert!(status_text.contains("message: message send failed"));
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
    fn frame_status_text_returns_lifecycle_status_only_when_no_details_exist() {
        let mut state = test_state();
        state.model.status = "authenticated".to_owned();

        assert_eq!(frame_status_text(&state.model), "authenticated");
    }

    #[test]
    fn frame_status_text_appends_verification_details() {
        let mut state = test_state();
        state.model.status = "verification request from @alice:example".to_owned();
        state.model.last_verification = Some(VerificationRequest {
            flow_id: "flow-verify-123".to_owned(),
            user_id: "@alice:example".to_owned(),
        });

        assert_eq!(
            frame_status_text(&state.model),
            "verification request from @alice:example | verification: flow-verify-123 from @alice:example"
        );
    }

    #[test]
    fn frame_status_text_appends_error_details() {
        let mut state = test_state();
        state.model.status = "error during send text".to_owned();
        state.model.last_error = Some(CoreErrorEvent {
            operation: Operation::SendText,
            message: "message send failed".to_owned(),
        });

        assert_eq!(
            frame_status_text(&state.model),
            "error during send text | error: send text: message send failed"
        );
    }

    #[test]
    fn frame_status_text_appends_verification_before_error_when_both_exist() {
        let mut state = test_state();
        state.model.status = "verification request from @alice:example".to_owned();
        state.model.last_verification = Some(VerificationRequest {
            flow_id: "flow-verify-123".to_owned(),
            user_id: "@alice:example".to_owned(),
        });
        state.model.last_error = Some(CoreErrorEvent {
            operation: Operation::SendText,
            message: "message send failed".to_owned(),
        });

        assert_eq!(
            frame_status_text(&state.model),
            "verification request from @alice:example | verification: flow-verify-123 from @alice:example | error: send text: message send failed"
        );
    }

    #[test]
    fn resolve_active_view_preserves_existing_active_room() {
        let rooms = vec![
            room("!keep:example", "Keep", false),
            room("!other:example", "Other", false),
        ];

        assert_eq!(
            resolve_active_view(
                &ViewId::Room("!keep:example".to_owned()),
                Some("!other:example"),
                &rooms,
            ),
            ViewId::Room("!keep:example".to_owned())
        );
    }

    #[test]
    fn resolve_active_view_falls_back_to_first_room_when_active_room_is_stale() {
        let rooms = vec![
            room("!alpha:example", "Alpha", false),
            room("!beta:example", "Beta", false),
        ];

        assert_eq!(
            resolve_active_view(&ViewId::Room("!gone:example".to_owned()), None, &rooms),
            ViewId::Room("!alpha:example".to_owned())
        );
    }

    #[test]
    fn resolve_active_view_skips_spaces_when_falling_back() {
        let rooms = vec![
            space("!space:example", "Space"),
            room("!alpha:example", "Alpha", false),
            room("!beta:example", "Beta", false),
        ];

        assert_eq!(
            resolve_active_view(&ViewId::Room("!gone:example".to_owned()), None, &rooms),
            ViewId::Room("!alpha:example".to_owned())
        );
    }

    #[test]
    fn resolve_active_view_falls_back_to_status_when_no_rooms_remain() {
        assert_eq!(
            resolve_active_view(&ViewId::Room("!gone:example".to_owned()), None, &[]),
            ViewId::Status
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
                entries: vec![RoomTimelineEntry::Text {
                    event_id: Some("$!room:example:hello".to_owned()),
                    transaction_id: None,
                    sender: "@alice:example".to_owned(),
                    timestamp_ms: 1_700_000_000_000,
                    body: "hello".to_owned(),
                    send_state: TimelineSendState::Sent,
                    send_error: None,
                }],
            },
        );
        state.model.last_timeline = Some(LastTimelineText {
            room_id: "!room:example".to_owned(),
            event_id: Some("$!room:example:hello".to_owned()),
            transaction_id: None,
            sender: "@alice:example".to_owned(),
            timestamp_ms: 1_700_000_000_000,
            body: "hello".to_owned(),
            send_state: TimelineSendState::Sent,
            send_error: None,
        });
        state.model.last_verification = Some(VerificationRequest {
            flow_id: "flow".to_owned(),
            user_id: "@alice:example".to_owned(),
        });
        state.model.last_error = Some(CoreErrorEvent {
            operation: Operation::SendText,
            message: "failed".to_owned(),
        });
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
        assert_eq!(state.model.last_timeline, None);
        assert_eq!(state.model.last_verification, None);
        assert_eq!(state.model.last_error, None);
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
    fn switchbar_button_text_prefixes_activity_and_preserves_room_title() {
        let mut state = test_state();
        state.model.rooms = vec![room("!room:example", "Room", true)];
        state.room_activity.insert("!room:example".to_owned());

        assert_eq!(
            switchbar_button_text(&state, &ViewId::Room("!room:example".to_owned())),
            "* Room [e2ee]"
        );
        assert_eq!(switchbar_button_text(&state, &ViewId::Status), "Status");
    }

    #[test]
    fn inactive_room_events_mark_room_activity() {
        let mut state = test_state();
        state.model.rooms = vec![
            room("!active:example", "Active", false),
            room("!inactive:example", "Inactive", false),
        ];
        state.active_view = ViewId::Room("!active:example".to_owned());

        apply_core_event(
            &mut state,
            Event::TimelineText(timeline("!inactive:example", "hello")),
        )
        .unwrap();
        apply_core_event(
            &mut state,
            topic_state("!inactive:example", "incident review"),
        )
        .unwrap();

        assert!(state.room_activity.contains("!inactive:example"));
    }

    #[test]
    fn active_room_events_do_not_mark_room_activity() {
        let mut state = test_state();
        state.model.rooms = vec![room("!room:example", "Room", false)];
        state.active_view = ViewId::Room("!room:example".to_owned());

        apply_core_event(
            &mut state,
            Event::TimelineText(timeline("!room:example", "hello")),
        )
        .unwrap();
        apply_core_event(&mut state, topic_state("!room:example", "incident review")).unwrap();

        assert!(!state.room_activity.contains("!room:example"));
    }

    #[test]
    fn synced_rooms_prunes_stale_room_activity() {
        let mut state = test_state();
        state.room_activity.insert("!keep:example".to_owned());
        state.room_activity.insert("!stale:example".to_owned());

        apply_core_event(
            &mut state,
            Event::SyncedRooms(vec![room("!keep:example", "Keep", false)]),
        )
        .unwrap();

        assert!(state.room_activity.contains("!keep:example"));
        assert!(!state.room_activity.contains("!stale:example"));
    }

    #[test]
    fn synced_rooms_prune_space_room_activity() {
        let mut state = test_state();
        state.room_activity.insert("!keep:example".to_owned());
        state.room_activity.insert("!space:example".to_owned());

        apply_core_event(
            &mut state,
            Event::SyncedRooms(vec![
                room("!keep:example", "Keep", false),
                space("!space:example", "Space"),
            ]),
        )
        .unwrap();

        assert!(state.room_activity.contains("!keep:example"));
        assert!(!state.room_activity.contains("!space:example"));
    }

    #[test]
    fn logged_out_clears_room_activity() {
        let mut state = test_state();
        state.room_activity.insert("!room:example".to_owned());

        with_local_app_data(|_| {
            apply_core_event(&mut state, Event::LoggedOut)
                .expect("logged out event must apply successfully");
        });

        assert!(state.room_activity.is_empty());
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
    fn parse_compose_submission_distinguishes_text_literal_slash_and_commands() {
        assert_eq!(
            parse_compose_submission("hello"),
            ComposeSubmission::Text {
                body: Cow::Borrowed("hello"),
            }
        );
        assert_eq!(
            parse_compose_submission("//status"),
            ComposeSubmission::Text {
                body: Cow::Owned("/status".to_owned()),
            }
        );
        assert_eq!(
            parse_compose_submission("///status"),
            ComposeSubmission::Text {
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
    fn parse_slash_command_rejects_unknown_and_usage_errors() {
        assert_eq!(parse_slash_command("/StAtUs"), LocalSlashCommand::Status);
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
    }

    #[test]
    fn resolve_window_command_target_accepts_room_id_and_unique_display_name() {
        let rooms = vec![
            room("!alpha:example", "Alpha", false),
            room("!beta:example", "Beta", false),
            room("!alpha-target:example", "!alpha:example", false),
        ];

        assert_eq!(
            resolve_window_command_target(&rooms, "!alpha:example"),
            Ok("!alpha:example".to_owned())
        );
        assert_eq!(
            resolve_window_command_target(&rooms, "Beta"),
            Ok("!beta:example".to_owned())
        );
    }

    #[test]
    fn resolve_window_command_target_rejects_unknown_spaces_and_ambiguous_display_names() {
        let rooms = vec![
            space("!work:example", "Work"),
            room("!shared-one:example", "Shared", false),
            room("!shared-two:example", "Shared", false),
            room("!alpha:example", "Alpha", false),
        ];

        assert_eq!(
            resolve_window_command_target(&rooms, "!work:example"),
            Err(WindowTargetError::Unknown)
        );
        assert_eq!(
            resolve_window_command_target(&rooms, "Work"),
            Err(WindowTargetError::Unknown)
        );
        assert_eq!(
            resolve_window_command_target(&rooms, "Missing"),
            Err(WindowTargetError::Unknown)
        );
        assert_eq!(
            resolve_window_command_target(&rooms, "Shared"),
            Err(WindowTargetError::Ambiguous)
        );
    }

    #[test]
    fn submit_compose_uses_explicit_or_selected_room() {
        let mut state = test_state();
        let handle = start_test_core();
        state.command_sender = Some(handle.command_sender().clone());
        state.model.rooms = vec![
            room("!selected:example", "Selected", false),
            room("!explicit:example", "Explicit", false),
        ];
        state.model.selected_room = Some("!selected:example".to_owned());

        let selected_room_id = submit_compose(&mut state, "hello".to_owned(), None).unwrap();
        assert_eq!(selected_room_id, "!selected:example");

        let explicit_room_id =
            submit_compose(&mut state, "hi".to_owned(), Some("!explicit:example")).unwrap();
        assert_eq!(explicit_room_id, "!explicit:example");
        assert_eq!(
            state.compose_history.entries,
            vec!["hello".to_owned(), "hi".to_owned()]
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

        let room_id = submit_compose_with_history_entry(
            &mut state,
            "/status".to_owned(),
            "//status".to_owned(),
            None,
        )
        .unwrap();
        assert_eq!(room_id, "!beta:example");
        assert_eq!(state.compose_history.entries, vec!["//status".to_owned()]);

        handle
            .command_sender()
            .send(Command::Shutdown)
            .expect("shutdown command must enqueue");
        handle.join().expect("test core must stop cleanly");
    }

    #[test]
    fn submit_compose_rejects_empty_body_unknown_room_and_missing_selection() {
        let mut state = test_state();
        state.model.rooms = vec![
            room("!room:example", "Room", false),
            space("!space:example", "Space"),
        ];

        assert_eq!(
            submit_compose(&mut state, String::new(), Some("!room:example")),
            Err("message body must not be empty".to_owned())
        );
        assert!(state.compose_history.entries.is_empty());
        assert_eq!(
            submit_compose(&mut state, "hello".to_owned(), Some("!missing:example")),
            Err("unknown room id: !missing:example".to_owned())
        );
        assert!(state.compose_history.entries.is_empty());
        assert_eq!(
            submit_compose(&mut state, "hello".to_owned(), Some("!space:example")),
            Err("unknown room id: !space:example".to_owned())
        );
        assert!(state.compose_history.entries.is_empty());
        assert_eq!(
            submit_compose(&mut state, "hello".to_owned(), None),
            Err("no room selected".to_owned())
        );
        assert!(state.compose_history.entries.is_empty());
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

        assert_eq!(
            submit_compose(&mut state, "hello".to_owned(), None),
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
                !serde_json::to_string(&dump_model_from_ui(&state.model))
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
