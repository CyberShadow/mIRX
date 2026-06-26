#![allow(unsafe_op_in_unsafe_fn)]

use std::collections::{BTreeMap, VecDeque};
use std::ffi::{OsStr, c_void};
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
    Command, CommandSender, CoreConfig, CoreErrorEvent, CoreHandle, Event, Operation, RoomSummary,
    SendTextRequest, TimelineText, VerificationRequest, start,
};
use windows_sys::Win32::Foundation::{GetLastError, HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows_sys::Win32::Graphics::Gdi::{COLOR_WINDOW, HBRUSH};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::Controls::{
    ICC_TREEVIEW_CLASSES, INITCOMMONCONTROLSEX, InitCommonControlsEx, NMHDR, TVE_EXPAND,
    TVGN_CARET, TVI_LAST, TVI_ROOT, TVIF_TEXT, TVINSERTSTRUCTW, TVINSERTSTRUCTW_0, TVITEMEXW,
    TVM_DELETEITEM, TVM_EXPAND, TVM_GETNEXTITEM, TVM_INSERTITEMW, TVN_SELCHANGEDW,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CLIENTCREATESTRUCT, CREATESTRUCTW, CW_USEDEFAULT, CallWindowProcW, CreateWindowExW,
    DefFrameProcW, DefWindowProcW, DestroyWindow, DispatchMessageW, ES_AUTOHSCROLL, GWLP_USERDATA,
    GWLP_WNDPROC, GetClientRect, GetMessageW, GetParent, GetWindowLongPtrW, HMENU, IDC_ARROW,
    KillTimer, LoadCursorW, MSG, MoveWindow, PostMessageW, PostQuitMessage, RegisterClassExW,
    SW_SHOW, SendMessageW, SetTimer, SetWindowLongPtrW, ShowWindow, TranslateMessage,
    WINDOW_EX_STYLE, WM_APP, WM_CREATE, WM_DESTROY, WM_GETTEXT, WM_GETTEXTLENGTH, WM_KEYDOWN,
    WM_NCCREATE, WM_NCDESTROY, WM_NOTIFY, WM_SETTEXT, WM_SIZE, WM_TIMER, WNDCLASSEXW, WNDPROC,
    WS_BORDER, WS_CHILD, WS_CLIPCHILDREN, WS_EX_CLIENTEDGE, WS_OVERLAPPEDWINDOW, WS_VISIBLE,
};

use crate::{AppCommand, HELP, automation::SyntheticLoad};

const FRAME_CLASS_NAME: &str = "mmatrixWin32Frame";
const FRAME_TITLE: &str = "mmatrix";
const TREE_CLASS_NAME: &str = "SysTreeView32";
const MDI_CLASS_NAME: &str = "MDICLIENT";
const EDIT_CLASS_NAME: &str = "EDIT";
const TREE_ID: u16 = 1001;
const MDI_ID: u16 = 1002;
const EDIT_ID: u16 = 1003;
const WM_CORE_EVENT: u32 = WM_APP + 1;
const WM_SELF_TEST: u32 = WM_APP + 2;
const AUTO_CLOSE_TIMER_ID: usize = 1;
const AUTO_CLOSE_DELAY_MS: u32 = 250;
const TREE_WIDTH: i32 = 240;
const EDIT_HEIGHT: i32 = 28;
const PROFILE_SUBDIR: &str = "mmatrix\\ui-shell";
const VK_RETURN_KEY: WPARAM = 13;

#[derive(Default)]
struct Controls {
    tree: HWND,
    mdi_client: HWND,
    edit: HWND,
}

#[derive(Default)]
struct UiModel {
    status: String,
    rooms: Vec<RoomSummary>,
    selected_room: Option<String>,
    timelines: BTreeMap<String, Vec<String>>,
    last_timeline: Option<TimelineText>,
    last_verification: Option<VerificationRequest>,
    last_error: Option<CoreErrorEvent>,
}

struct WindowState {
    controls: Controls,
    event_queue: Arc<Mutex<VecDeque<Event>>>,
    command_sender: Option<CommandSender>,
    exit_code: Arc<AtomicI32>,
    event_generation: u64,
    #[allow(dead_code)]
    last_idle_probe_generation: Option<u64>,
    #[allow(dead_code)]
    synthetic_load_in_progress: bool,
    room_tree_items: BTreeMap<isize, String>,
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

    let instance = unsafe { GetModuleHandleW(null()) };
    if instance.is_null() {
        return Err(last_os_error("GetModuleHandleW"));
    }

    init_common_controls()?;
    register_frame_class(instance)?;

    let exit_code = Arc::new(AtomicI32::new(0));
    let event_queue = Arc::new(Mutex::new(VecDeque::new()));

    let state = Box::new(WindowState {
        controls: Controls::default(),
        event_queue: Arc::clone(&event_queue),
        command_sender: None,
        exit_code: Arc::clone(&exit_code),
        event_generation: 0,
        last_idle_probe_generation: None,
        synthetic_load_in_progress: false,
        room_tree_items: BTreeMap::new(),
        edit_window_proc: None,
        model: UiModel {
            status: "offline".to_owned(),
            ..UiModel::default()
        },
    });

    let hwnd = create_main_window(instance, state)?;
    let bridge = if matches!(command, AppCommand::Run(_) | AppCommand::AutoClose) {
        let bridge = start_core_bridge(hwnd, Arc::clone(&event_queue))?;
        unsafe {
            state_mut(hwnd).command_sender = Some(bridge.command_sender.clone());
        }
        Some(bridge)
    } else {
        None
    };

    unsafe {
        ShowWindow(hwnd, SW_SHOW);

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
                refresh_tree(hwnd);
                0
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
        WM_CORE_EVENT => {
            handle_core_events(hwnd);
            0
        }
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
                SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
                drop(Box::from_raw(state_ptr));
            }
            0
        }
        _ => default_window_proc(hwnd, message, wparam, lparam),
    }
}

unsafe fn create_controls(hwnd: HWND) -> Result<(), String> {
    let instance = GetModuleHandleW(null());
    let state = state_mut(hwnd);

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
    let events = drain_core_events(hwnd);
    apply_events_and_refresh(hwnd, events);
}

fn apply_core_event(state: &mut WindowState, event: Event) {
    match event {
        Event::Started => {
            state.model.status = "core started".to_owned();
        }
        Event::SyncedRooms(rooms) => {
            state.model.status = format!("synced {} room(s)", rooms.len());
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
            state.model.status = "logged out".to_owned();
            state.model.rooms.clear();
            state.model.selected_room = None;
            state.model.timelines.clear();
            state.model.last_timeline = None;
            state.model.last_verification = None;
            state.model.last_error = None;
        }
        Event::Stopped => {
            state.model.status = "core stopped".to_owned();
        }
        Event::Error(error) => {
            state.model.status = format!("error during {}", operation_name(error.operation));
            state.model.last_error = Some(error);
        }
        Event::LoggedIn | Event::RestoredSession => {}
    }
}

unsafe fn refresh_tree(hwnd: HWND) {
    let state = state_mut(hwnd);
    let tree = state.controls.tree;
    if tree.is_null() {
        return;
    }

    SendMessageW(tree, TVM_DELETEITEM, 0, TVI_ROOT as LPARAM);
    state.room_tree_items.clear();

    let root = insert_tree_item(tree, TVI_ROOT as isize, "mmatrix");
    insert_tree_item(tree, root, &format!("status: {}", state.model.status));

    let rooms_label = if state.model.rooms.is_empty() {
        "rooms: offline".to_owned()
    } else {
        format!("rooms: {}", state.model.rooms.len())
    };
    let rooms_root = insert_tree_item(tree, root, &rooms_label);
    let selected_room = state.model.selected_room.clone();
    for room in &state.model.rooms {
        let mut room_label = room.display_name.clone();
        if room.encrypted {
            room_label.push_str(" [e2ee]");
        }
        if selected_room.as_deref() == Some(room.room_id.as_str()) {
            room_label.push_str(" [selected]");
        }
        let room_item = insert_tree_item(tree, rooms_root, &room_label);
        state
            .room_tree_items
            .insert(room_item, room.room_id.clone());
    }

    if let Some(room_id) = selected_room.as_deref() {
        let timeline_root = insert_tree_item(tree, root, &format!("timeline: {room_id}"));
        if let Some(lines) = state.model.timelines.get(room_id) {
            for line in lines {
                insert_tree_item(tree, timeline_root, line);
            }
        }
    } else if let Some(timeline) = &state.model.last_timeline {
        let timeline_root = insert_tree_item(tree, root, "timeline");
        insert_tree_item(
            tree,
            timeline_root,
            &format!("{}: {}", timeline.room_id, timeline.body),
        );
    }

    if let Some(request) = &state.model.last_verification {
        let verification_root = insert_tree_item(tree, root, "verification");
        insert_tree_item(
            tree,
            verification_root,
            &format!("{} ({})", request.user_id, request.flow_id),
        );
    }

    if let Some(error) = &state.model.last_error {
        let error_root = insert_tree_item(tree, root, "error");
        insert_tree_item(
            tree,
            error_root,
            &format!("{}: {}", operation_name(error.operation), error.message),
        );
    }

    SendMessageW(tree, TVM_EXPAND, TVE_EXPAND as WPARAM, root as LPARAM);
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

    println!("{}", mmatrix_core::ui_self_test_line());
    Ok(())
}

unsafe fn drain_core_events(hwnd: HWND) -> Vec<Event> {
    let queue = &state_ref(hwnd).event_queue;
    let mut queue = queue.lock().expect("core event queue poisoned");
    queue.drain(..).collect()
}

unsafe fn apply_events_and_refresh(hwnd: HWND, events: Vec<Event>) {
    if events.is_empty() {
        return;
    }

    {
        let state = state_mut(hwnd);
        for event in events {
            apply_core_event(state, event);
            state.event_generation += 1;
        }
    }

    refresh_tree(hwnd);
}

#[allow(dead_code)]
unsafe fn apply_synthetic_load(hwnd: HWND, load: SyntheticLoad) -> Result<(usize, usize), String> {
    {
        let state = state_mut(hwnd);
        state.synthetic_load_in_progress = true;
        state.model = UiModel::default();
    }

    apply_events_and_refresh(hwnd, load.events);

    let selection_result = {
        let state = state_mut(hwnd);
        let selected_room = load
            .selected_room
            .clone()
            .or_else(|| state.model.rooms.first().map(|room| room.room_id.clone()));

        match selected_room {
            Some(room_id) => select_room(state, &room_id),
            None => {
                state.model.selected_room = None;
                Ok(())
            }
        }
    };

    state_mut(hwnd).synthetic_load_in_progress = false;
    selection_result?;
    refresh_tree(hwnd);

    Ok((load.room_count, load.timeline_event_count))
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

    let room_id = state_ref(hwnd).room_tree_items.get(&selected_item).cloned();
    if let Some(room_id) = room_id
        && select_room(state_mut(hwnd), &room_id).is_ok()
    {
        refresh_tree(hwnd);
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
    let edit = state_ref(hwnd).controls.edit;
    let body = read_window_text(edit);
    let room_id = submit_compose(state_mut(hwnd), body, None)?;
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
            TranslateMessage(&message);
            DispatchMessageW(&message);
        }
    }
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

fn wide_null(text: &str) -> Vec<u16> {
    OsStr::new(text).encode_wide().chain([0]).collect()
}

fn last_os_error(operation: &str) -> String {
    let error = std::io::Error::last_os_error();
    format!("{operation} failed: {error}")
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::*;

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

    fn test_state() -> WindowState {
        WindowState {
            controls: Controls::default(),
            event_queue: Arc::new(Mutex::new(VecDeque::new())),
            command_sender: None,
            exit_code: Arc::new(AtomicI32::new(0)),
            event_generation: 0,
            last_idle_probe_generation: None,
            synthetic_load_in_progress: false,
            room_tree_items: BTreeMap::new(),
            edit_window_proc: None,
            model: UiModel {
                status: "offline".to_owned(),
                ..UiModel::default()
            },
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
        );
        apply_core_event(
            &mut state,
            Event::TimelineText(timeline("!one:example", "first")),
        );
        apply_core_event(
            &mut state,
            Event::TimelineText(timeline("!one:example", "second")),
        );
        apply_core_event(
            &mut state,
            Event::TimelineText(timeline("!two:example", "third")),
        );

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
    fn synced_rooms_preserves_only_valid_selection() {
        let mut state = test_state();
        state.model.selected_room = Some("!keep:example".to_owned());

        apply_core_event(
            &mut state,
            Event::SyncedRooms(vec![
                room("!keep:example", "Keep", false),
                room("!other:example", "Other", false),
            ]),
        );
        assert_eq!(state.model.selected_room, Some("!keep:example".to_owned()));

        apply_core_event(
            &mut state,
            Event::SyncedRooms(vec![room("!other:example", "Other", false)]),
        );
        assert_eq!(state.model.selected_room, None);
    }

    #[test]
    fn logged_out_clears_selection_timelines_and_diagnostics() {
        let mut state = test_state();
        state.model.rooms = vec![room("!room:example", "Room", false)];
        state.model.selected_room = Some("!room:example".to_owned());
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

        apply_core_event(&mut state, Event::LoggedOut);

        assert!(state.model.rooms.is_empty());
        assert_eq!(state.model.selected_room, None);
        assert!(state.model.timelines.is_empty());
        assert_eq!(state.model.last_timeline, None);
        assert_eq!(state.model.last_verification, None);
        assert_eq!(state.model.last_error, None);
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
}
