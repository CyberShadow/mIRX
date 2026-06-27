#![allow(unsafe_op_in_unsafe_fn)]

use std::collections::{BTreeMap, VecDeque};
use std::ffi::{OsStr, c_void};
use std::fs;
use std::io::ErrorKind;
use std::mem::{size_of, zeroed};
use std::os::windows::ffi::OsStrExt;
use std::path::PathBuf;
use std::process::ExitCode;
use std::ptr::{null, null_mut};
use std::sync::atomic::{AtomicI32, Ordering};
use std::sync::{Arc, Mutex, mpsc::RecvTimeoutError};
use std::thread;
use std::time::Duration;

use mmatrix_core::{
    Command, CommandSender, CoreConfig, CoreErrorEvent, CoreHandle, Event, LoginRequest, Operation,
    RestoreSessionRequest, RoomSummary, SendTextRequest, StoredSession, TimelineText,
    VerificationRequest, start,
};
use serde::Serialize;
use serde_json::{Value, json};
use windows_sys::Win32::Foundation::{
    CloseHandle, GetLastError, HANDLE, HWND, INVALID_HANDLE_VALUE, LPARAM, LRESULT, RECT, WPARAM,
};
use windows_sys::Win32::Graphics::Gdi::{COLOR_WINDOW, HBRUSH};
use windows_sys::Win32::System::DataExchange::COPYDATASTRUCT;
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::System::Memory::{
    CreateFileMappingW, FILE_MAP_ALL_ACCESS, MEMORY_MAPPED_VIEW_ADDRESS, MapViewOfFile,
    PAGE_READWRITE, UnmapViewOfFile,
};
use windows_sys::Win32::System::Threading::GetCurrentProcessId;
use windows_sys::Win32::UI::Controls::{
    ICC_TREEVIEW_CLASSES, INITCOMMONCONTROLSEX, InitCommonControlsEx, NMHDR, TVE_EXPAND,
    TVGN_CARET, TVGN_CHILD, TVGN_NEXT, TVGN_ROOT, TVI_LAST, TVI_ROOT, TVIF_TEXT, TVINSERTSTRUCTW,
    TVINSERTSTRUCTW_0, TVITEMEXW, TVM_DELETEITEM, TVM_EXPAND, TVM_GETITEMW, TVM_GETNEXTITEM,
    TVM_INSERTITEMW, TVM_SELECTITEM, TVN_SELCHANGEDW,
};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::EnableWindow;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, BS_DEFPUSHBUTTON, BS_PUSHBUTTON, CLIENTCREATESTRUCT, CREATESTRUCTW, CW_USEDEFAULT,
    CallWindowProcW, CreateMenu, CreatePopupMenu, CreateWindowExW, DefFrameProcW, DefMDIChildProcW,
    DefWindowProcW, DestroyWindow, DispatchMessageW, DrawMenuBar, ES_AUTOHSCROLL, ES_AUTOVSCROLL,
    ES_MULTILINE, ES_PASSWORD, ES_READONLY, ES_WANTRETURN, GWLP_USERDATA, GWLP_WNDPROC,
    GetClientRect, GetMessageW, GetParent, GetWindowLongPtrW, HMENU, IDC_ARROW, IsWindow,
    KillTimer, LoadCursorW, MB_ICONERROR, MB_OK, MDICREATESTRUCTW, MF_POPUP, MF_STRING, MSG,
    MessageBoxW, MoveWindow, PostMessageW, PostQuitMessage, RegisterClassExW, SW_SHOW,
    SendMessageW, SetMenu, SetTimer, SetWindowLongPtrW, ShowWindow, TranslateMDISysAccel,
    TranslateMessage, WINDOW_EX_STYLE, WM_APP, WM_CLOSE, WM_COMMAND, WM_COPYDATA, WM_CREATE,
    WM_DESTROY, WM_GETTEXT, WM_GETTEXTLENGTH, WM_KEYDOWN, WM_MDIACTIVATE, WM_MDICREATE,
    WM_MDIDESTROY, WM_MDIGETACTIVE, WM_MDIMAXIMIZE, WM_NCCREATE, WM_NCDESTROY, WM_NOTIFY,
    WM_SETTEXT, WM_SIZE, WM_TIMER, WNDCLASSEXW, WNDPROC, WS_BORDER, WS_CAPTION, WS_CHILD,
    WS_CLIPCHILDREN, WS_EX_CLIENTEDGE, WS_OVERLAPPED, WS_OVERLAPPEDWINDOW, WS_SYSMENU, WS_TABSTOP,
    WS_VISIBLE, WS_VSCROLL,
};

use crate::{
    AppCommand, HELP, RunOptions,
    automation::{
        COMMAND_ID_DUMP_TIMELINE, COMMAND_ID_LOAD_SYNTHETIC_STATE, COMMAND_ID_PING,
        COMMAND_ID_WAIT_IDLE, DumpModel, DumpTimelineResponse, FailureEnvelope, GetVersionResponse,
        LifecycleState, ListRoomsResponse, LoadEventsResponse, MdiState, MdiView, NavigationNode,
        ParsedCommand, PongResponse, ResponseEnvelope, SelectRoomResponse, SelectViewResponse,
        SendTextResponse, UiState, ViewIdentity, WaitIdleResponse, dump_error_from_core,
        dump_room_from_summary, dump_timeline_line_from_text, dump_verification_from_request,
        failure_response, parse_command_request, success_response,
    },
};

const FRAME_CLASS_NAME: &str = "mmatrixWin32Frame";
const MDI_CHILD_CLASS_NAME: &str = "mmatrixWin32MdiChild";
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
const VIEW_EDIT_ID: u16 = 2001;
const MENU_ID_LOGIN: usize = 40001;
const MENU_ID_RESTORE_SAVED_SESSION: usize = 40002;
const MENU_ID_START_SYNC: usize = 40003;
const MENU_ID_LOGOUT: usize = 40004;
const LOGIN_DIALOG_HOME_SERVER_ID: u16 = 5001;
const LOGIN_DIALOG_USERNAME_ID: u16 = 5002;
const LOGIN_DIALOG_PASSWORD_ID: u16 = 5003;
const LOGIN_DIALOG_DEVICE_ID: u16 = 5004;
const LOGIN_DIALOG_OK_ID: usize = 5005;
const LOGIN_DIALOG_CANCEL_ID: usize = 5006;
const WM_CORE_EVENT: u32 = WM_APP + 1;
const WM_SELF_TEST: u32 = WM_APP + 2;
const AUTO_CLOSE_TIMER_ID: usize = 1;
const AUTO_CLOSE_DELAY_MS: u32 = 250;
const AUTOMATION_RESPONSE_CAPACITY: usize = 4 * 1024 * 1024;
const TREE_WIDTH: i32 = 240;
const EDIT_HEIGHT: i32 = 28;
const PROFILE_SUBDIR: &str = "mmatrix\\ui-shell";
const SESSION_FILE_NAME: &str = "session.json";
const SAVED_SESSION_VERSION: u32 = 1;
const DEFAULT_DEVICE_DISPLAY_NAME: &str = "mmatrix-ui";
const VK_RETURN_KEY: WPARAM = 13;

#[derive(Default)]
struct Controls {
    tree: HWND,
    mdi_client: HWND,
    edit: HWND,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
enum ViewId {
    Status,
    Room(String),
}

struct MdiViewState {
    child: HWND,
    edit: HWND,
}

#[derive(Default)]
struct UiModel {
    status: String,
    lifecycle_state: LifecycleState,
    rooms: Vec<RoomSummary>,
    selected_room: Option<String>,
    timelines: BTreeMap<String, Vec<String>>,
    last_timeline: Option<TimelineText>,
    last_verification: Option<VerificationRequest>,
    last_error: Option<CoreErrorEvent>,
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
    tree_items: BTreeMap<isize, ViewId>,
    views: BTreeMap<ViewId, MdiViewState>,
    hwnd_views: BTreeMap<isize, ViewId>,
    active_view: ViewId,
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
        tree_items: BTreeMap::new(),
        views: BTreeMap::new(),
        hwnd_views: BTreeMap::new(),
        active_view: ViewId::Status,
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
    let handle = start(CoreConfig {
        profile_dir: profile_dir()?,
        store_passphrase: "mmatrix-ui-store-passphrase".to_owned(),
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
    let saved = serde_json::from_slice::<SavedSession>(&bytes)
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
    let bytes = serde_json::to_vec(&payload).map_err(|err| {
        format!(
            "failed to serialize saved session {}: {err}",
            path.display()
        )
    })?;
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

fn init_common_controls() -> Result<(), String> {
    let controls = INITCOMMONCONTROLSEX {
        dwSize: size_of::<INITCOMMONCONTROLSEX>() as u32,
        dwICC: ICC_TREEVIEW_CLASSES,
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
            if handle_menu_command(hwnd, wparam) {
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
            let instance = GetModuleHandleW(null());
            let edit = CreateWindowExW(
                WS_EX_CLIENTEDGE,
                wide_null(EDIT_CLASS_NAME).as_ptr(),
                wide_null("").as_ptr(),
                WS_CHILD
                    | WS_VISIBLE
                    | ES_MULTILINE as u32
                    | ES_READONLY as u32
                    | ES_AUTOVSCROLL as u32
                    | ES_WANTRETURN as u32
                    | WS_VSCROLL,
                0,
                0,
                0,
                0,
                hwnd,
                VIEW_EDIT_ID as usize as HMENU,
                instance,
                null_mut(),
            );
            if edit.is_null() {
                return -1;
            }
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, edit as isize);
            0
        }
        WM_SIZE => {
            let edit = mdi_child_edit(hwnd);
            if !edit.is_null() {
                let mut rect = RECT::default();
                GetClientRect(hwnd, &mut rect);
                MoveWindow(
                    edit,
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
            let command_id = (_wparam & 0xffff) as usize;
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
    let command_id = (wparam & 0xffff) as usize;
    let result = match command_id {
        MENU_ID_LOGIN => show_login_menu_dialog(hwnd).map(|handled| handled.then_some(())),
        MENU_ID_RESTORE_SAVED_SESSION => {
            submit_restore_saved_session(state_mut(hwnd)).map(|_| Some(()))
        }
        MENU_ID_START_SYNC => submit_start_sync(state_mut(hwnd)).map(|_| Some(())),
        MENU_ID_LOGOUT => submit_logout(state_mut(hwnd)).map(|_| Some(())),
        _ => return false,
    };

    if let Err(message) = result {
        show_error_message(hwnd, "Account", &message);
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
    Ok(true)
}

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
    {
        return;
    }

    let mut rect = RECT::default();
    GetClientRect(hwnd, &mut rect);

    let width = rect.right - rect.left;
    let height = rect.bottom - rect.top;
    let tree_width = TREE_WIDTH.min(width);
    let edit_height = EDIT_HEIGHT.min(height);
    let mdi_width = (width - tree_width).max(0);
    let mdi_height = (height - edit_height).max(0);

    MoveWindow(state.controls.tree, 0, 0, tree_width, mdi_height, 1);
    MoveWindow(
        state.controls.mdi_client,
        tree_width,
        0,
        mdi_width,
        mdi_height,
        1,
    );
    MoveWindow(state.controls.edit, 0, mdi_height, width, edit_height, 1);
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
            let homeserver_url = state.pending_login_homeserver_url.clone().ok_or_else(|| {
                "received LoggedIn event without a pending homeserver URL".to_owned()
            })?;
            save_session(&homeserver_url, &session)?;
            state.pending_login_homeserver_url = None;
            state.model.status = "authenticated".to_owned();
            state.model.lifecycle_state = LifecycleState::Authenticated;
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
                .is_some_and(|room_id| !room_exists(&rooms, room_id))
            {
                state.model.selected_room = None;
            }
            state.model.rooms = rooms;
        }
        Event::TimelineText(timeline) => {
            state.model.status = format!("timeline update from {}", timeline.room_id);
            state
                .model
                .timelines
                .entry(timeline.room_id.clone())
                .or_default()
                .push(timeline.body.clone());
            state.model.last_timeline = Some(timeline);
        }
        Event::VerificationRequest(request) => {
            state.model.status = format!("verification request from {}", request.user_id);
            state.model.last_verification = Some(request);
        }
        Event::LoggedOut => {
            delete_saved_session()?;
            state.pending_login_homeserver_url = None;
            state.model.status = "logged out".to_owned();
            state.model.lifecycle_state = LifecycleState::LoggedOut;
            state.model.rooms.clear();
            state.model.selected_room = None;
            state.model.timelines.clear();
            state.model.last_timeline = None;
            state.model.last_verification = None;
            state.model.last_error = None;
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
    model.timelines.get(room_id).cloned().unwrap_or_default()
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
        ViewId::Room(room_id) if room_exists(rooms, room_id) => ViewId::Room(room_id.clone()),
        ViewId::Room(_) => {
            if let Some(room_id) = selected_room.filter(|room_id| room_exists(rooms, room_id)) {
                ViewId::Room(room_id.to_owned())
            } else if let Some(room) = rooms.first() {
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
            matches!(view_id, ViewId::Room(room_id) if !room_exists(&state.model.rooms, room_id))
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

fn room_summary<'a>(rooms: &'a [RoomSummary], room_id: &str) -> Option<&'a RoomSummary> {
    rooms.iter().find(|room| room.room_id == room_id)
}

fn view_title(model: &UiModel, view_id: &ViewId) -> String {
    match view_id {
        ViewId::Status => "Status".to_owned(),
        ViewId::Room(room_id) => {
            let room = room_summary(&model.rooms, room_id)
                .expect("room view activation must target an existing room");
            let mut title = room.display_name.clone();
            if room.encrypted {
                title.push_str(" [e2ee]");
            }
            title
        }
    }
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

unsafe fn create_mdi_child(frame_hwnd: HWND, title: &str) -> Result<HWND, String> {
    let mdi_client = state_ref(frame_hwnd).controls.mdi_client;
    if mdi_client.is_null() {
        return Err("MDI client control was not created".to_owned());
    }

    let instance = GetModuleHandleW(null());
    let mut class_name = wide_null(MDI_CHILD_CLASS_NAME);
    let mut title_wide = wide_null(title);
    let create = MDICREATESTRUCTW {
        szClass: class_name.as_mut_ptr(),
        szTitle: title_wide.as_mut_ptr(),
        hOwner: instance,
        x: CW_USEDEFAULT,
        y: CW_USEDEFAULT,
        cx: CW_USEDEFAULT,
        cy: CW_USEDEFAULT,
        style: WS_CHILD | WS_VISIBLE,
        lParam: 0,
    };
    let child = SendMessageW(
        mdi_client,
        WM_MDICREATE,
        0,
        (&create as *const MDICREATESTRUCTW) as LPARAM,
    ) as HWND;
    if child.is_null() {
        return Err(last_os_error("WM_MDICREATE"));
    }

    Ok(child)
}

unsafe fn mdi_child_edit(hwnd: HWND) -> HWND {
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
    IsWindow(view.child) != 0 && IsWindow(view.edit) != 0 && mdi_child_edit(view.child) == view.edit
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
    let child = create_mdi_child(hwnd, &title)?;
    let edit = mdi_child_edit(child);
    if edit.is_null() {
        return Err("MDI child edit control was not created".to_owned());
    }

    let state = state_mut(hwnd);
    state.hwnd_views.insert(child as isize, view_id.clone());
    state.views.insert(view_id, MdiViewState { child, edit });
    Ok(child)
}

unsafe fn ensure_status_view(hwnd: HWND) -> Result<HWND, String> {
    ensure_view(hwnd, ViewId::Status)
}

unsafe fn ensure_room_view(hwnd: HWND, room_id: &str) -> Result<HWND, String> {
    if room_summary(&state_ref(hwnd).model.rooms, room_id).is_none() {
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
    let (child, edit, title, text) = {
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
        (view.child, view.edit, title, render_view_text(&lines))
    };

    let title = wide_null(&title);
    SendMessageW(child, WM_SETTEXT, 0, title.as_ptr() as LPARAM);
    set_edit_text(edit, &text);
    Ok(())
}

unsafe fn activate_view(hwnd: HWND, view_id: &ViewId) -> Result<(), String> {
    let child = match view_id {
        ViewId::Status => {
            state_mut(hwnd).model.selected_room = None;
            ensure_status_view(hwnd)?
        }
        ViewId::Room(room_id) => {
            {
                let state = state_mut(hwnd);
                select_room(state, room_id)?;
            }
            ensure_room_view(hwnd, room_id)?
        }
    };

    {
        let state = state_mut(hwnd);
        state.active_view = view_id.clone();
    }

    refresh_view_content(hwnd, view_id)?;
    select_tree_item(hwnd, view_id);

    let mdi_client = state_ref(hwnd).controls.mdi_client;
    SendMessageW(mdi_client, WM_MDIACTIVATE, child as WPARAM, 0);
    SendMessageW(mdi_client, WM_MDIMAXIMIZE, child as WPARAM, 0);
    Ok(())
}

unsafe fn sync_views_and_tree(hwnd: HWND) -> Result<(), String> {
    ensure_status_view(hwnd)?;

    let room_ids = state_ref(hwnd)
        .model
        .rooms
        .iter()
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
    activate_view(hwnd, &active_view)
}

unsafe fn refresh_tree(hwnd: HWND) {
    let state = state_mut(hwnd);
    let tree = state.controls.tree;
    if tree.is_null() {
        return;
    }

    SendMessageW(tree, TVM_DELETEITEM, 0, TVI_ROOT as LPARAM);
    state.tree_items.clear();

    let root = insert_tree_item(tree, TVI_ROOT as isize, "mmatrix");
    let status = insert_tree_item(tree, root, "status");
    state.tree_items.insert(status, ViewId::Status);

    let rooms_label = if state.model.rooms.is_empty() {
        "rooms: offline".to_owned()
    } else {
        format!("rooms: {}", state.model.rooms.len())
    };
    let rooms_root = insert_tree_item(tree, root, &rooms_label);
    for room in &state.model.rooms {
        let mut room_label = room.display_name.clone();
        if room.encrypted {
            room_label.push_str(" [e2ee]");
        }
        let room_item = insert_tree_item(tree, rooms_root, &room_label);
        state
            .tree_items
            .insert(room_item, ViewId::Room(room.room_id.clone()));
    }

    SendMessageW(tree, TVM_EXPAND, TVE_EXPAND as WPARAM, root as LPARAM);
    SendMessageW(tree, TVM_EXPAND, TVE_EXPAND as WPARAM, rooms_root as LPARAM);
}

unsafe fn select_tree_item(hwnd: HWND, view_id: &ViewId) {
    let (tree, item) = {
        let state = state_ref(hwnd);
        let item = state
            .tree_items
            .iter()
            .find_map(|(item, mapped_view)| (mapped_view == view_id).then_some(*item));
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
        ParsedCommand::DumpTimeline { id, args } => {
            let room_id = resolve_timeline_room_id(state_ref(hwnd), args.room_id.as_deref())
                .map_err(|message| failure_response(Some(id.clone()), "unknown_room", message))?;
            let lines = state_ref(hwnd)
                .model
                .timelines
                .get(&room_id)
                .cloned()
                .unwrap_or_default();
            serialize_success_json(&id, DumpTimelineResponse { room_id, lines })
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
            activate_view(hwnd, &ViewId::Room(args.room_id.clone()))
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
            activate_view(hwnd, &requested_view)
                .map_err(|message| failure_response(Some(id.clone()), "unknown_view", message))?;
            let active = active_view_identity(hwnd)
                .map_err(|message| failure_response(Some(id.clone()), "unknown_view", message))?;
            serialize_success_json(&id, SelectViewResponse { active })
        }
        ParsedCommand::SendText { id, args } => {
            let body = args.body;
            set_edit_text(state_ref(hwnd).controls.edit, &body);
            let room_id = submit_compose_from_control(hwnd, args.room_id.as_deref())
                .map_err(|message| failure_response(Some(id.clone()), "send_failed", message))?;
            serialize_success_json(
                &id,
                SendTextResponse {
                    accepted: true,
                    room_id,
                },
            )
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
    }
}

fn command_request_id(command: &ParsedCommand) -> &str {
    match command {
        ParsedCommand::Ping { id }
        | ParsedCommand::GetVersion { id }
        | ParsedCommand::WaitIdle { id, .. }
        | ParsedCommand::DumpModel { id }
        | ParsedCommand::DumpUiState { id }
        | ParsedCommand::DumpTimeline { id, .. }
        | ParsedCommand::ListRooms { id }
        | ParsedCommand::SelectRoom { id, .. }
        | ParsedCommand::SelectView { id, .. }
        | ParsedCommand::SendText { id, .. }
        | ParsedCommand::LoadSyntheticState { id, .. } => id,
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
    text.lines()
        .map(|line| line.trim_end_matches('\r').to_owned())
        .collect()
}

fn ordered_view_ids(state: &WindowState) -> Vec<ViewId> {
    let mut ordered = Vec::new();
    if state.views.contains_key(&ViewId::Status) {
        ordered.push(ViewId::Status);
    }

    for room in &state.model.rooms {
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
                if !state
                    .model
                    .rooms
                    .iter()
                    .any(|room| room.room_id == *room_id) =>
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
                    "tracked view {:?} referenced a dead child or edit control",
                    view_id
                ));
            }

            let identity = view_identity_from_view_id(&view_id);
            Ok(MdiView {
                title: read_window_text(view.child),
                active: identity == active,
                lines: split_rendered_lines(&read_window_text(view.edit)),
                identity,
            })
        })
        .collect::<Result<Vec<_>, String>>()?;

    Ok(MdiState { active, views })
}

unsafe fn dump_ui_state(hwnd: HWND) -> Result<UiState, String> {
    Ok(UiState {
        navigation: dump_navigation_tree(hwnd)?,
        mdi: dump_mdi_state(hwnd)?,
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
            },
            RoomSummary {
                room_id: "!two:example".to_owned(),
                display_name: "Two".to_owned(),
                encrypted: true,
            },
        ]),
        Event::TimelineText(TimelineText {
            room_id: "!one:example".to_owned(),
            body: "first line".to_owned(),
        }),
        Event::TimelineText(TimelineText {
            room_id: "!one:example".to_owned(),
            body: "second line".to_owned(),
        }),
        Event::TimelineText(TimelineText {
            room_id: "!two:example".to_owned(),
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
            != json!({"room_id":"!one:example","lines":["first line","second line"]})
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
        state.model = initial_model();
    }

    apply_events_and_refresh(hwnd, events)?;

    let (rooms, timeline_events, active_view) = {
        let state = state_mut(hwnd);
        let rooms = state.model.rooms.len();
        let timeline_events = state.model.timelines.values().map(Vec::len).sum();
        let selected_room = state.model.rooms.first().map(|room| room.room_id.clone());

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
    activate_view(hwnd, &active_view)?;

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
        let _ = activate_view(hwnd, &view_id);
    }

    true
}

fn select_room(state: &mut WindowState, room_id: &str) -> Result<(), String> {
    if !room_exists(&state.model.rooms, room_id) {
        return Err(format!("unknown room id: {room_id}"));
    }

    state.model.selected_room = Some(room_id.to_owned());
    Ok(())
}

unsafe fn submit_compose_from_edit(hwnd: HWND) -> Result<String, String> {
    submit_compose_from_control(hwnd, None)
}

unsafe fn submit_compose_from_control(
    hwnd: HWND,
    explicit_room_id: Option<&str>,
) -> Result<String, String> {
    let edit = state_ref(hwnd).controls.edit;
    let body = read_window_text(edit);
    let room_id = submit_compose(state_mut(hwnd), body, explicit_room_id)?;
    let empty = wide_null("");
    SendMessageW(edit, WM_SETTEXT, 0, empty.as_ptr() as LPARAM);
    Ok(room_id)
}

fn submit_compose(
    state: &mut WindowState,
    body: String,
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

    if !room_exists(&state.model.rooms, &room_id) {
        return Err(format!("unknown room id: {room_id}"));
    }

    let sender = state
        .command_sender
        .as_ref()
        .ok_or_else(|| "core command sender is not available".to_owned())?;
    sender
        .send(Command::SendText(SendTextRequest {
            room_id: room_id.clone(),
            body,
        }))
        .map_err(|err| format!("failed to enqueue send text command: {err}"))?;

    Ok(room_id)
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
            homeserver_url,
            session,
        }))
        .map_err(|err| format!("failed to enqueue restore session command: {err}"))?;

    state.pending_login_homeserver_url = None;
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
    if message == WM_KEYDOWN && wparam == VK_RETURN_KEY {
        let parent = GetParent(hwnd);
        if !parent.is_null() {
            let _ = submit_compose_from_edit(parent);
        }
        return 0;
    }

    let parent = GetParent(hwnd);
    if parent.is_null() {
        return DefWindowProcW(hwnd, message, wparam, lparam);
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
            .map(dump_timeline_line_from_text),
        last_verification: model
            .last_verification
            .as_ref()
            .map(dump_verification_from_request),
        last_error: model.last_error.as_ref().map(dump_error_from_core),
        selected_room: model.selected_room.clone(),
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

    if room_exists(&state.model.rooms, &room_id) {
        Ok(room_id)
    } else {
        Err(format!("unknown room id: {room_id}"))
    }
}

fn operation_name(operation: Operation) -> &'static str {
    match operation {
        Operation::Login => "login",
        Operation::RestoreSession => "restore session",
        Operation::StartSync => "start sync",
        Operation::SendText => "send text",
        Operation::Logout => "logout",
        Operation::Shutdown => "shutdown",
    }
}

fn room_exists(rooms: &[RoomSummary], room_id: &str) -> bool {
    rooms.iter().any(|room| room.room_id == room_id)
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

fn set_edit_text(hwnd: HWND, text: &str) {
    let text = wide_null(text);
    unsafe {
        SendMessageW(hwnd, WM_SETTEXT, 0, text.as_ptr() as LPARAM);
    }
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
        }
    }

    fn timeline(room_id: &str, body: &str) -> TimelineText {
        TimelineText {
            room_id: room_id.to_owned(),
            body: body.to_owned(),
        }
    }

    fn stored_session() -> StoredSession {
        StoredSession {
            user_id: "@alice:example".to_owned(),
            device_id: "DEVICE".to_owned(),
            access_token: "secret-token".to_owned(),
            refresh_token: None,
        }
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
            tree_items: BTreeMap::new(),
            views: BTreeMap::new(),
            hwnd_views: BTreeMap::new(),
            active_view: ViewId::Status,
            edit_window_proc: None,
            model: initial_model(),
        }
    }

    fn tracked_view(hwnd_value: isize) -> MdiViewState {
        MdiViewState {
            child: hwnd_value as HWND,
            edit: (hwnd_value + 1) as HWND,
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
            state.model.timelines.get("!one:example"),
            Some(&vec!["first".to_owned(), "second".to_owned()])
        );
        assert_eq!(
            state.model.timelines.get("!two:example"),
            Some(&vec!["third".to_owned()])
        );
        assert_eq!(
            state.model.last_timeline,
            Some(timeline("!two:example", "third"))
        );
    }

    #[test]
    fn render_room_lines_returns_timeline_lines_including_multiline_entries() {
        let mut state = test_state();
        state.model.timelines.insert(
            "!ops:example".to_owned(),
            vec![
                "incident review".to_owned(),
                "deploy window opens\n18:00 UTC".to_owned(),
            ],
        );

        assert_eq!(
            render_room_lines(&state.model, "!ops:example"),
            vec![
                "incident review".to_owned(),
                "deploy window opens\n18:00 UTC".to_owned(),
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
    fn logged_out_clears_selection_timelines_and_diagnostics() {
        let mut state = test_state();
        state.model.rooms = vec![room("!room:example", "Room", false)];
        state.model.selected_room = Some("!room:example".to_owned());
        state.active_view = ViewId::Room("!room:example".to_owned());
        state
            .model
            .timelines
            .insert("!room:example".to_owned(), vec!["hello".to_owned()]);
        state.model.last_timeline = Some(timeline("!room:example", "hello"));
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
        assert!(state.model.timelines.is_empty());
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
    fn select_room_accepts_only_known_room_ids() {
        let mut state = test_state();
        state.model.rooms = vec![room("!room:example", "Room", false)];

        assert_eq!(select_room(&mut state, "!room:example"), Ok(()));
        assert_eq!(state.model.selected_room, Some("!room:example".to_owned()));
        assert_eq!(
            select_room(&mut state, "!missing:example"),
            Err("unknown room id: !missing:example".to_owned())
        );
        assert_eq!(state.model.selected_room, Some("!room:example".to_owned()));
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

        handle
            .command_sender()
            .send(Command::Shutdown)
            .expect("shutdown command must enqueue");
        handle.join().expect("test core must stop cleanly");
    }

    #[test]
    fn submit_compose_rejects_empty_body_unknown_room_and_missing_selection() {
        let mut state = test_state();
        state.model.rooms = vec![room("!room:example", "Room", false)];

        assert_eq!(
            submit_compose(&mut state, String::new(), Some("!room:example")),
            Err("message body must not be empty".to_owned())
        );
        assert_eq!(
            submit_compose(&mut state, "hello".to_owned(), Some("!missing:example")),
            Err("unknown room id: !missing:example".to_owned())
        );
        assert_eq!(
            submit_compose(&mut state, "hello".to_owned(), None),
            Err("no room selected".to_owned())
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

            let session_path = root.join(PROFILE_SUBDIR).join(SESSION_FILE_NAME);
            let saved = serde_json::from_slice::<SavedSession>(
                &fs::read(&session_path).expect("saved session file must exist"),
            )
            .expect("saved session file must deserialize");
            assert_eq!(
                saved,
                SavedSession {
                    version: SAVED_SESSION_VERSION,
                    homeserver_url: "https://matrix.example".to_owned(),
                    session: stored_session(),
                }
            );
            let session_json =
                String::from_utf8(fs::read(&session_path).expect("session file must be readable"))
                    .expect("session file must be utf-8 json");
            assert!(!session_json.contains("password"));

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
    fn saved_session_helpers_round_trip_and_reject_invalid_json() {
        with_local_app_data(|root| {
            save_session("https://matrix.example", &stored_session()).unwrap();
            assert_eq!(
                load_saved_session().unwrap(),
                SavedSession {
                    version: SAVED_SESSION_VERSION,
                    homeserver_url: "https://matrix.example".to_owned(),
                    session: stored_session(),
                }
            );

            let path = root.join(PROFILE_SUBDIR).join(SESSION_FILE_NAME);
            fs::write(
                &path,
                br#"{"version":2,"homeserver_url":"https://matrix.example","session":{"user_id":"@alice:example","device_id":"DEVICE","access_token":"secret-token","refresh_token":null}}"#,
            )
            .unwrap();
            assert!(
                load_saved_session()
                    .unwrap_err()
                    .contains("unsupported saved session version 2")
            );

            fs::write(
                &path,
                br#"{"version":1,"homeserver_url":"https://matrix.example","session":{"user_id":"@alice:example","device_id":"DEVICE","access_token":"secret-token","refresh_token":null},"unexpected":true}"#,
            )
            .unwrap();
            assert!(
                load_saved_session()
                    .unwrap_err()
                    .contains("unknown field `unexpected`")
            );

            fs::write(
                &path,
                br#"{"version":1,"homeserver_url":"https://matrix.example","session":{"user_id":"@alice:example","device_id":"DEVICE","access_token":"secret-token","refresh_token":null,"unexpected":"value"}}"#,
            )
            .unwrap();
            assert!(
                load_saved_session()
                    .unwrap_err()
                    .contains("unknown field `unexpected`")
            );

            delete_saved_session().unwrap();
            delete_saved_session().unwrap();
            assert!(!path.exists());
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
