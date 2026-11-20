//! The MDI view windows: creating, refreshing, activating and closing them.

use windows_sys::Win32::Graphics::Gdi::{HDC, SetBkColor};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    WM_CTLCOLOREDIT, WM_CTLCOLORLISTBOX, WM_ERASEBKGND,
};

use super::*;

const MDI_CHILD_CLASS_NAME: &str = "mirxWin32MdiChild";

/// How often the refresh path did its costly work since the frame started:
/// rows built for a window, log rows wrapped (measured), and treebar and
/// switchbar rebuilds. They only count up, so tests read deltas from the
/// automation dump; they assert work, never time.
#[derive(Debug, Default)]
pub(super) struct RefreshCounters {
    timeline_row_builds: u64,
    rows_wrapped: u64,
    treebar_rebuilds: u64,
    switchbar_rebuilds: u64,
    /// `timeline_row_builds` of room windows, by room.
    row_builds_by_room: BTreeMap<String, u64>,
}

impl RefreshCounters {
    fn count_row_build(&mut self, view_id: &ViewId) {
        self.timeline_row_builds += 1;
        if let ViewId::Room(room_id) = view_id {
            *self.row_builds_by_room.entry(room_id.clone()).or_default() += 1;
        }
    }

    pub(super) fn count_rows_wrapped(&mut self, rows: usize) {
        self.rows_wrapped += rows as u64;
    }

    pub(super) fn count_treebar_rebuild(&mut self) {
        self.treebar_rebuilds += 1;
    }

    pub(super) fn count_switchbar_rebuild(&mut self) {
        self.switchbar_rebuilds += 1;
    }

    pub(super) fn timeline_row_builds(&self) -> u64 {
        self.timeline_row_builds
    }

    pub(super) fn rows_wrapped(&self) -> u64 {
        self.rows_wrapped
    }

    pub(super) fn treebar_rebuilds(&self) -> u64 {
        self.treebar_rebuilds
    }

    pub(super) fn switchbar_rebuilds(&self) -> u64 {
        self.switchbar_rebuilds
    }

    pub(super) fn room_row_builds(&self, room_id: &str) -> u64 {
        self.row_builds_by_room.get(room_id).copied().unwrap_or(0)
    }
}
pub(super) const ROOM_CONTENT_CLASS_NAME: &str = "mirxWin32RoomContent";
/// `WC_LISTVIEWW`, the common controls' list view.
const ROOM_LIST_CLASS_NAME: &str = "SysListView32";

const VIEW_CONTENT_ID: u16 = 2001;
/// Every MDI child's editbox; a sibling of its `VIEW_CONTENT_ID` window.
const VIEW_EDIT_ID: u16 = 2002;
/// The window menu's Font... item. `WM_SYSCOMMAND` ids are below 0xF000
/// with the low four bits clear: Windows uses those bits itself.
const SYSMENU_ID_FONT: usize = 0x0110;
const SYSMENU_FONT_LABEL: &str = "&Font...";
/// The window menu's Find Text item, mIRC's window-menu Find.
const SYSMENU_ID_FIND: usize = 0x0120;
const SYSMENU_FIND_LABEL: &str = "Find &Text...\tCtrl+F";
/// A room window's window-menu Send File..., mIRC's DCC Send dialog.
const SYSMENU_ID_SEND_FILE: usize = 0x0130;
const SYSMENU_SEND_FILE_LABEL: &str = "Send &File...";
/// The window menu's checkable Logging, mIRC's: whether the window logs.
const SYSMENU_ID_LOGGING: usize = 0x0140;
const SYSMENU_LOGGING_LABEL: &str = "&Logging";

struct MdiChildCreateState {
    view_id: ViewId,
}

struct RoomContentState {
    timeline: HWND,
    members: HWND,
}

pub(super) fn register_mdi_child_class(instance: *mut c_void) -> Result<(), String> {
    let class_name = wide_null(MDI_CHILD_CLASS_NAME);
    let window_class = WNDCLASSEXW {
        cbSize: size_of::<WNDCLASSEXW>() as u32,
        style: 0,
        lpfnWndProc: Some(mdi_child_window_proc),
        cbClsExtra: 0,
        cbWndExtra: 0,
        hInstance: instance,
        hIcon: app_icon(instance, IconSize::Large),
        hCursor: unsafe { LoadCursorW(null_mut(), IDC_ARROW) },
        hbrBackground: (COLOR_WINDOW as isize + 1) as HBRUSH,
        lpszMenuName: null(),
        lpszClassName: class_name.as_ptr(),
        hIconSm: app_icon(instance, IconSize::Small),
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

pub(super) fn register_room_content_class(instance: *mut c_void) -> Result<(), String> {
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
        // None: `WM_ERASEBKGND` paints the scheme's Background.
        hbrBackground: null_mut(),
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

unsafe extern "system" fn mdi_child_window_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match message {
        WM_CREATE => {
            match create_mdi_child_controls(hwnd, &mdi_child_create_state(lparam).view_id) {
                Ok(()) => 0,
                Err(message) => {
                    eprintln!("{message}");
                    -1
                }
            }
        }
        WM_SIZE => {
            layout_mdi_child(hwnd);
            // MDI tracks maximization and the frame title here.
            DefMDIChildProcW(hwnd, message, wparam, lparam)
        }
        WM_SETFOCUS => {
            // Activating a window puts the caret in its editbox, as in mIRC.
            let result = DefMDIChildProcW(hwnd, message, wparam, lparam);
            SetFocus(mdi_child_focus(hwnd));
            result
        }
        // Its one editbox, in the scheme's Editbox colours.
        WM_CTLCOLOREDIT => {
            assert_eq!(
                lparam as HWND,
                mdi_child_edit(hwnd),
                "an MDI child's only edit control is its editbox"
            );
            let state = state_ref(frame_window_for(hwnd));
            let colors = &state.settings.settings().colors;
            SetTextColor(wparam as HDC, colors.colorref(ColorItem::EditboxText));
            SetBkColor(wparam as HDC, colors.colorref(ColorItem::Editbox));
            state.scheme_brushes.editbox as LRESULT
        }
        WM_MDIACTIVATE => {
            if lparam as HWND == hwnd
                && let Err(message) = handle_native_mdi_child_activation(hwnd)
            {
                eprintln!("{message}");
            }
            DefMDIChildProcW(hwnd, message, wparam, lparam)
        }
        WM_SYSCOMMAND if wparam & 0xFFF0 == SYSMENU_ID_FIND => {
            let frame = frame_window_for(hwnd);
            if let Err(message) = begin_find_dialog(frame) {
                show_error_message(frame, "Find", &message);
            }
            0
        }
        WM_SYSCOMMAND if wparam & 0xFFF0 == SYSMENU_ID_SEND_FILE => {
            let frame = frame_window_for(hwnd);
            let ViewId::Room(room_id) = state_ref(frame)
                .hwnd_views
                .get(&(hwnd as isize))
                .cloned()
                .expect("every MDI child is a tracked view")
            else {
                unreachable!("only room windows have Send File...");
            };
            let sent = run_send_file_dialog(frame, room_id).and_then(|status| match status {
                Some(status) => set_local_command_status(frame, status),
                None => Ok(()),
            });
            if let Err(message) = sent {
                show_error_message(frame, "Send File", &message);
            }
            0
        }
        WM_SYSCOMMAND if wparam & 0xFFF0 == SYSMENU_ID_LOGGING => {
            let frame = frame_window_for(hwnd);
            if let Err(message) = toggle_window_logging(frame, hwnd) {
                show_error_message(frame, "Logging", &message);
            }
            0
        }
        WM_SYSCOMMAND if wparam & 0xFFF0 == SYSMENU_ID_FONT => {
            let frame = frame_window_for(hwnd);
            if let Err(message) = run_local_command(frame, SlashCommand::Font(FontCommand::Dialog))
            {
                show_error_message(frame, "Font", &message);
            }
            0
        }
        WM_CLOSE => {
            let frame = frame_window_for(hwnd);
            if close_view_requested(frame, hwnd) {
                let result = DefMDIChildProcW(hwnd, message, wparam, lparam);
                // The treebar and switchbar follow the views left.
                if let Err(message) = sync_views_and_tree(frame) {
                    show_error_message(frame, "Window", &message);
                }
                result
            } else {
                0
            }
        }
        WM_NCDESTROY => {
            unregister_destroyed_view(hwnd);
            DefMDIChildProcW(hwnd, message, wparam, lparam)
        }
        // Only the Rooms List's list notifies its MDI child.
        WM_NOTIFY => match handle_room_list_notify(frame_window_for(hwnd), lparam) {
            Some(result) => result,
            None => DefMDIChildProcW(hwnd, message, wparam, lparam),
        },
        _ => DefMDIChildProcW(hwnd, message, wparam, lparam),
    }
}

/// Which membership change closing a room window asks for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum CloseLeave {
    Part,
    RejectInvite,
}

/// What closing a view's window does, after mIRC.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum CloseAction {
    /// The Status window cannot be closed.
    Refuse,
    /// Thread windows just close; no sync reopens them.
    Destroy,
    /// A query window closes, and stays closed until the other side talks
    /// or the user opens it again.
    HideQuery(String),
    /// Closing a channel parts it, and closing an invite rejects it. Unlike
    /// mIRC this asks first, as leaving an invite-only Matrix room cannot be
    /// undone with a rejoin. The window stays until the room leaves the room
    /// list, as mIRC's closes when the PART lands.
    Confirm { room_id: String, leave: CloseLeave },
    /// Closing the Rooms List drops its listing; pages still on the way
    /// are ignored.
    DropRoomList,
}

pub(super) fn close_action(model: &UiModel, view_id: &ViewId) -> CloseAction {
    match view_id {
        ViewId::Status => CloseAction::Refuse,
        ViewId::RoomList => CloseAction::DropRoomList,
        ViewId::Thread { .. } => CloseAction::Destroy,
        ViewId::Room(room_id) => {
            let room = navigable_room_by_id(&model.rooms, room_id)
                .expect("a room window's room must be navigable");
            match room_window_kind(room) {
                RoomWindowKind::Channel => CloseAction::Confirm {
                    room_id: room_id.clone(),
                    leave: CloseLeave::Part,
                },
                RoomWindowKind::Query => CloseAction::HideQuery(room_id.clone()),
                RoomWindowKind::Invite => CloseAction::Confirm {
                    room_id: room_id.clone(),
                    leave: CloseLeave::RejectInvite,
                },
            }
        }
    }
}

fn close_confirmation_text(leave: CloseLeave, room: &RoomSummary) -> String {
    match leave {
        CloseLeave::Part => format!(
            "Leave {}?\n\nYou may need a new invite to rejoin.",
            room.display_name
        ),
        CloseLeave::RejectInvite => format!("Reject the invite to {}?", room.display_name),
    }
}

/// Handles `WM_CLOSE` on an MDI child (caption X, Ctrl+F4, the system
/// menu); returns whether to destroy the window.
unsafe fn close_view_requested(frame: HWND, child: HWND) -> bool {
    let view_id = state_ref(frame)
        .hwnd_views
        .get(&(child as isize))
        .cloned()
        .expect("every MDI child is a tracked view");
    let action = close_action(&state_ref(frame).model, &view_id);
    match &action {
        CloseAction::Refuse => false,
        CloseAction::Destroy => true,
        CloseAction::HideQuery(room_id) => {
            state_mut(frame).closed_queries.insert(room_id.clone());
            true
        }
        // Before the window goes, so no view sync recreates it.
        CloseAction::DropRoomList => {
            state_mut(frame).model.room_list = None;
            true
        }
        CloseAction::Confirm { room_id, leave } => {
            let text = close_confirmation_text(
                *leave,
                room_by_id(&state_ref(frame).model.rooms, room_id)
                    .expect("a room window's room must exist"),
            );
            let text = wide_null(&text);
            let caption = wide_null(FRAME_TITLE);
            let answer = MessageBoxW(
                frame,
                text.as_ptr(),
                caption.as_ptr(),
                MB_YESNO | MB_ICONQUESTION,
            );
            // Core events keep arriving during the box's modal loop: the
            // room may have left, taking its window with it, or changed.
            if answer != IDYES
                || state_ref(frame).hwnd_views.get(&(child as isize)) != Some(&view_id)
                || close_action(&state_ref(frame).model, &view_id) != action
            {
                return false;
            }
            let result = {
                let state = state_mut(frame);
                match leave {
                    CloseLeave::Part => submit_part_slash_command(state, Some(room_id)),
                    CloseLeave::RejectInvite => submit_reject_slash_command(state, Some(room_id)),
                }
            };
            match result {
                Ok(message) | Err(message) => {
                    if let Err(error) = set_local_command_status(frame, message) {
                        show_error_message(frame, "Window", &error);
                    }
                }
            }
            false
        }
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
                window_data::adopt(hwnd, *state);
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
        // Only the gap between the text area and the nick list shows: the
        // text area's Background.
        WM_ERASEBKGND => {
            let mut rect = RECT::default();
            GetClientRect(hwnd, &mut rect);
            FillRect(
                wparam as HDC,
                &rect,
                state_ref(frame_window_for(hwnd)).scheme_brushes.background,
            );
            1
        }
        // The nick list below its last nick; the nicks draw themselves.
        WM_CTLCOLORLISTBOX => {
            let state = state_ref(frame_window_for(hwnd));
            SetBkColor(
                wparam as HDC,
                state
                    .settings
                    .settings()
                    .colors
                    .colorref(ColorItem::Listbox),
            );
            state.scheme_brushes.listbox as LRESULT
        }
        // From the owner-drawn nick list. The first arrives while the list
        // is created, before this window's state exists: the height comes
        // from the frame.
        WM_MEASUREITEM => {
            let measure = &mut *(lparam as *mut MEASUREITEMSTRUCT);
            measure.itemHeight = text_font_item_height(hwnd);
            1
        }
        WM_DRAWITEM => {
            draw_nick_list_item(hwnd, &*(lparam as *const DRAWITEMSTRUCT));
            1
        }
        WM_NCDESTROY => {
            window_data::drop_owned::<RoomContentState>(hwnd);
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

unsafe fn room_content_state_ref(hwnd: HWND) -> &'static RoomContentState {
    window_data::get(hwnd).expect("room content state must exist while the control is alive")
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
        WS_CHILD
            | WS_VISIBLE
            | WS_VSCROLL
            | (LBS_NOINTEGRALHEIGHT | LBS_OWNERDRAWFIXED | LBS_HASSTRINGS) as u32,
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
    let Some(state) = window_data::get::<RoomContentState>(hwnd) else {
        return;
    };
    let mut rect = RECT::default();
    GetClientRect(hwnd, &mut rect);
    let height = rect.bottom - rect.top;
    let layout = mdi::room_content_layout(rect.right - rect.left);
    MoveWindow(state.timeline, 0, 0, layout.timeline_width, height, 1);
    MoveWindow(
        state.members,
        layout.members_x,
        0,
        layout.members_width,
        height,
        1,
    );
}

/// The view to keep active: the active one if it still has a window, else
/// its room, the selected room, the first room or Status. A closed query
/// has no window, so it is never picked.
pub(super) fn resolve_active_view(
    active_view: &ViewId,
    selected_room: Option<&str>,
    model: &UiModel,
    closed_queries: &BTreeSet<String>,
) -> ViewId {
    let has_window = |room_id: &str| {
        navigable_room_by_id(&model.rooms, room_id).is_some() && !closed_queries.contains(room_id)
    };
    match active_view {
        ViewId::Status => ViewId::Status,
        ViewId::RoomList if model.room_list.is_some() => ViewId::RoomList,
        ViewId::Room(room_id) if has_window(room_id) => ViewId::Room(room_id.clone()),
        ViewId::Thread {
            room_id,
            thread_root_event_id,
        } if projected_thread_by_id(model, room_id, thread_root_event_id).is_some() => {
            ViewId::Thread {
                room_id: room_id.clone(),
                thread_root_event_id: thread_root_event_id.clone(),
            }
        }
        ViewId::Thread { room_id, .. } if has_window(room_id) => ViewId::Room(room_id.clone()),
        ViewId::Room(_) | ViewId::Thread { .. } | ViewId::RoomList => {
            if let Some(room_id) = selected_room.filter(|room_id| has_window(room_id)) {
                ViewId::Room(room_id.to_owned())
            } else if let Some(room) = model.rooms.iter().find(|room| has_window(&room.room_id)) {
                ViewId::Room(room.room_id.clone())
            } else {
                ViewId::Status
            }
        }
    }
}

pub(super) fn take_stale_room_views(state: &mut WindowState) -> Vec<MdiViewState> {
    let stale_view_ids = state
        .views
        .keys()
        .filter(|view_id| !view_id_is_valid(&state.model, view_id))
        .cloned()
        .collect::<Vec<_>>();

    for view_id in &stale_view_ids {
        state.find_states.remove(view_id);
        cancel_original_jump_for_view(state, view_id);
    }

    let removed = stale_view_ids
        .into_iter()
        .filter_map(|view_id| {
            let view = state.views.remove(&view_id)?;
            state.hwnd_views.remove(&(view.child as isize));
            room_window_gone(state, &view_id);
            Some(view)
        })
        .collect::<Vec<_>>();

    let resolved_active_view = resolve_active_view(
        &state.active_view,
        state.model.selected_room.as_deref(),
        &state.model,
        &state.closed_queries,
    );
    if state.active_view != resolved_active_view {
        state.selected_timeline_event = None;
        hide_color_index(state);
    }
    state.active_view = resolved_active_view;

    removed
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
        style: WS_CHILD
            | WS_VISIBLE
            | if new_child_should_maximize(mdi_client) {
                WS_MAXIMIZE
            } else {
                0
            },
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

/// The first window opens maximized, and later ones follow the active
/// window, as on Windows. Wine's MDI restores the active maximized window
/// instead of maximizing the new one unless the new one asks for it.
unsafe fn new_child_should_maximize(mdi_client: HWND) -> bool {
    let mut active_maximized = 0;
    let active = SendMessageW(
        mdi_client,
        WM_MDIGETACTIVE,
        0,
        &mut active_maximized as *mut i32 as LPARAM,
    );
    active == 0 || active_maximized != 0
}

/// Shows a view's window maximized, normal or minimized.
pub(super) unsafe fn set_view_window_state(
    hwnd: HWND,
    view_id: &ViewId,
    window_state: MdiWindowState,
) -> Result<(), String> {
    let state = state_ref(hwnd);
    let child = state
        .views
        .get(view_id)
        .ok_or_else(|| format!("view has no window: {view_id:?}"))?
        .child;
    let mdi_client = state.controls.mdi_client;
    match window_state {
        MdiWindowState::Maximized => {
            SendMessageW(mdi_client, WM_MDIMAXIMIZE, child as WPARAM, 0);
        }
        MdiWindowState::Normal => {
            SendMessageW(mdi_client, WM_MDIRESTORE, child as WPARAM, 0);
        }
        MdiWindowState::Minimized => {
            ShowWindow(child, SW_MINIMIZE);
        }
    }
    Ok(())
}

pub(super) unsafe fn mdi_window_placement(mdi_client: HWND, child: HWND) -> MdiWindowPlacement {
    let state = if IsZoomed(child) != 0 {
        MdiWindowState::Maximized
    } else if IsIconic(child) != 0 {
        MdiWindowState::Minimized
    } else {
        MdiWindowState::Normal
    };
    let mut rect = RECT::default();
    GetWindowRect(child, &mut rect);
    MapWindowPoints(
        null_mut(),
        mdi_client,
        (&mut rect as *mut RECT).cast::<POINT>(),
        2,
    );
    MdiWindowPlacement {
        state,
        x: rect.left,
        y: rect.top,
        width: rect.right - rect.left,
        height: rect.bottom - rect.top,
    }
}

/// Creates the child's content window for its view and its editbox, and
/// lays them out.
unsafe fn create_mdi_child_controls(child: HWND, view_id: &ViewId) -> Result<(), String> {
    let instance = GetModuleHandleW(null());
    // Status is a message window like any other, drawn by the same timeline.
    let (class_name, style, ex_style) = match view_id {
        ViewId::Status => (
            TIMELINE_CLASS_NAME,
            WS_CHILD | WS_VISIBLE | WS_VSCROLL,
            WS_EX_CLIENTEDGE,
        ),
        ViewId::Room(_) | ViewId::Thread { .. } => {
            (ROOM_CONTENT_CLASS_NAME, WS_CHILD | WS_VISIBLE, 0)
        }
        // An owner-data list: the rows live in `UiModel.room_list`, and the
        // list asks for each cell's text as it paints.
        ViewId::RoomList => (
            ROOM_LIST_CLASS_NAME,
            WS_CHILD
                | WS_VISIBLE
                | WS_TABSTOP
                | LVS_REPORT
                | LVS_SINGLESEL
                | LVS_SHOWSELALWAYS
                | LVS_OWNERDATA,
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

    let frame = frame_window_for(child);
    if view_has_editbox(view_id) {
        let edit = create_child_window(
            child,
            EDIT_CLASS_NAME,
            "",
            WS_VISIBLE | COMPOSE_EDIT_STYLE,
            COMPOSE_EDIT_EX_STYLE,
            VIEW_EDIT_ID,
            instance,
            null_mut(),
        )?;
        subclass_compose_edit(frame, edit)?;
    } else {
        add_room_list_columns(content)?;
    }
    // Before any rows: the timeline wraps them in its font.
    set_view_text_font(view_id, child, state_ref(frame).text_font.hfont());

    let system_menu = GetSystemMenu(child, 0);
    if matches!(view_id, ViewId::Status) {
        // As in mIRC, the Status window cannot be closed (see WM_CLOSE), so
        // there is always a window to activate when another one closes.
        if EnableMenuItem(system_menu, SC_CLOSE, MF_BYCOMMAND | MF_GRAYED) == -1 {
            return Err(last_os_error("EnableMenuItem Status SC_CLOSE"));
        }
    }
    // mIRC's window menu has Find Text, Logging and the font too. The
    // Rooms List has no text of its own to find or log. Only a room takes
    // files: a thread cannot hold an attachment yet.
    let find_label = wide_null(SYSMENU_FIND_LABEL);
    let send_file_label = wide_null(SYSMENU_SEND_FILE_LABEL);
    let logging_label = wide_null(SYSMENU_LOGGING_LABEL);
    let logging_flags = if logging_item_checked(state_ref(frame), view_id) {
        MF_STRING | MF_CHECKED
    } else {
        MF_STRING
    };
    let font_label = wide_null(SYSMENU_FONT_LABEL);
    if AppendMenuW(system_menu, MF_SEPARATOR, 0, null()) == 0
        || (view_has_editbox(view_id)
            && AppendMenuW(system_menu, MF_STRING, SYSMENU_ID_FIND, find_label.as_ptr()) == 0)
        || (matches!(view_id, ViewId::Room(_))
            && AppendMenuW(
                system_menu,
                MF_STRING,
                SYSMENU_ID_SEND_FILE,
                send_file_label.as_ptr(),
            ) == 0)
        || (view_has_editbox(view_id)
            && AppendMenuW(
                system_menu,
                logging_flags,
                SYSMENU_ID_LOGGING,
                logging_label.as_ptr(),
            ) == 0)
        || AppendMenuW(system_menu, MF_STRING, SYSMENU_ID_FONT, font_label.as_ptr()) == 0
    {
        return Err(last_os_error(
            "AppendMenuW window menu Find Text..., Send File..., Logging and Font...",
        ));
    }

    layout_mdi_child(child);
    Ok(())
}

/// The window menu's Logging: turns the log of the window that owns the
/// menu on or off. That window need not be the active one.
unsafe fn toggle_window_logging(frame: HWND, child: HWND) -> Result<(), String> {
    let view_id = log_view(&state_ref(frame).hwnd_views[&(child as isize)])?;
    let on = view_log_path(state_ref(frame), &view_id).is_none();
    let line = set_view_logging(state_mut(frame), &view_id, on)?;
    echo_in_active_view(frame, [line])
}

/// Checks or clears a window menu's Logging item.
pub(super) unsafe fn check_window_menu_logging(child: HWND, check: u32) {
    let previous = CheckMenuItem(
        GetSystemMenu(child, 0),
        SYSMENU_ID_LOGGING as u32,
        MF_BYCOMMAND | check,
    );
    assert_ne!(
        previous,
        u32::MAX,
        "every window with text has a Logging item"
    );
}

pub(super) unsafe fn layout_mdi_child(child: HWND) {
    let mut rect = RECT::default();
    GetClientRect(child, &mut rect);
    // The Rooms List has no editbox: its list fills the window.
    let edit = mdi_child_optional_edit(child);
    let edit_height = edit.map_or(0, |_| {
        state_ref(frame_window_for(child)).text_font.edit_height
    });
    let layout = mdi::child_layout(rect.right - rect.left, rect.bottom - rect.top, edit_height);
    let place = layout.content;
    MoveWindow(
        mdi_child_content(child),
        place.x,
        place.y,
        place.width,
        place.height,
        1,
    );
    if let Some(edit) = edit {
        let place = layout.edit;
        MoveWindow(edit, place.x, place.y, place.width, place.height, 1);
    }
}

/// The child's editbox; only the Rooms List has none.
unsafe fn mdi_child_optional_edit(child: HWND) -> Option<HWND> {
    let edit = GetDlgItem(child, VIEW_EDIT_ID as i32);
    (!edit.is_null()).then_some(edit)
}

/// Where the keyboard goes in an active window: the editbox, as in mIRC,
/// or the Rooms List's list.
unsafe fn mdi_child_focus(child: HWND) -> HWND {
    mdi_child_optional_edit(child).unwrap_or_else(|| mdi_child_content(child))
}

pub(super) unsafe fn mdi_child_content(child: HWND) -> HWND {
    let content = GetDlgItem(child, VIEW_CONTENT_ID as i32);
    assert!(!content.is_null(), "MDI child must have its content window");
    content
}

pub(super) unsafe fn mdi_child_edit(child: HWND) -> HWND {
    let edit = GetDlgItem(child, VIEW_EDIT_ID as i32);
    assert!(!edit.is_null(), "MDI child must have its editbox");
    edit
}

pub(super) unsafe fn room_content_timeline(content: HWND) -> HWND {
    room_content_state_ref(content).timeline
}

/// A view's timeline: the Status content is one, a room or thread content
/// holds one beside its nick list.
pub(super) unsafe fn view_timeline(view_id: &ViewId, content: HWND) -> HWND {
    match view_id {
        ViewId::Status => content,
        ViewId::Room(_) | ViewId::Thread { .. } => room_content_timeline(content),
        ViewId::RoomList => unreachable!("the Rooms List has no timeline"),
    }
}

pub(super) unsafe fn room_content_member_list(content: HWND) -> HWND {
    room_content_state_ref(content).members
}

/// A window left the views: if it was a room's, the room's next window
/// logs automatically from the lines this one did not show. The mark lives
/// in the room's view model, with the arrivals it counts, so a sign-out
/// resets both; a room without one has had nothing arrive, so its next
/// window starts at the first arrival anyway.
fn room_window_gone(state: &mut WindowState, view_id: &ViewId) {
    if let ViewId::Room(room_id) = view_id
        && let Some(room_view) = state.model.room_views.get_mut(room_id)
    {
        room_view.arrivals.window_closed();
    }
}

fn unregister_view_by_child(state: &mut WindowState, child: HWND) -> Option<ViewId> {
    let view_id = state.hwnd_views.remove(&(child as isize))?;
    let removed = state
        .views
        .remove(&view_id)
        .expect("HWND-to-view registry must match tracked views");
    state.find_states.remove(&view_id);
    cancel_original_jump_for_view(state, &view_id);
    room_window_gone(state, &view_id);
    assert_eq!(
        removed.child, child,
        "tracked child HWND must match registry"
    );
    if state.active_view == view_id {
        state.selected_timeline_event = None;
        hide_color_index(state);
        state.active_view = resolve_active_view(
            &state.active_view,
            state.model.selected_room.as_deref(),
            &state.model,
            &state.closed_queries,
        );
    }
    Some(view_id)
}

unsafe fn unregister_destroyed_view(child: HWND) {
    let _ = unregister_view_by_child(state_mut(frame_window_for(child)), child);
}

pub(super) unsafe fn cached_view_is_alive(view: &MdiViewState) -> bool {
    IsWindow(view.child) != 0
        && GetDlgItem(view.child, VIEW_CONTENT_ID as i32) == view.content
        && GetDlgItem(view.child, VIEW_EDIT_ID as i32)
            == view
                .compose
                .as_ref()
                .map_or(null_mut(), |compose| compose.edit)
}

/// Every window has an editbox but the Rooms List, as in mIRC.
fn view_has_editbox(view_id: &ViewId) -> bool {
    match view_id {
        ViewId::Status | ViewId::Room(_) | ViewId::Thread { .. } => true,
        ViewId::RoomList => false,
    }
}

pub(super) unsafe fn ensure_view(hwnd: HWND, view_id: ViewId) -> Result<HWND, String> {
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

    let title = view_caption(&state_ref(hwnd).model, &view_id);
    let child = create_mdi_child(hwnd, &title, &view_id)?;
    let view = MdiViewState {
        child,
        content: mdi_child_content(child),
        compose: view_has_editbox(&view_id).then(|| ViewCompose {
            edit: mdi_child_edit(child),
            history: ComposeHistory::default(),
            target: None,
        }),
        log: ViewLog::for_new_view(&view_id),
    };

    let state = state_mut(hwnd);
    state.hwnd_views.insert(child as isize, view_id.clone());
    state.views.insert(view_id, view);
    Ok(child)
}

pub(super) unsafe fn ensure_status_view(hwnd: HWND) -> Result<HWND, String> {
    ensure_view(hwnd, ViewId::Status)
}

pub(super) unsafe fn ensure_room_view(hwnd: HWND, room_id: &str) -> Result<HWND, String> {
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

pub(super) unsafe fn destroy_stale_room_views(hwnd: HWND) {
    let (mdi_client, stale_views) = {
        let state = state_mut(hwnd);
        (state.controls.mdi_client, take_stale_room_views(state))
    };

    // WM_MDIDESTROY, not WM_CLOSE: closing a room window asks to leave it.
    for view in stale_views {
        SendMessageW(mdi_client, WM_MDIDESTROY, view.child as WPARAM, 0);
    }
}

pub(super) unsafe fn refresh_view_content(hwnd: HWND, view_id: &ViewId) -> Result<(), String> {
    let (child, content, title) = {
        let state = state_ref(hwnd);
        let view = state
            .views
            .get(view_id)
            .ok_or_else(|| "view state was not tracked".to_owned())?;
        let title = view_caption(&state.model, view_id);
        (view.child, view.content, title)
    };

    let title = wide_null(&title);
    SendMessageW(child, WM_SETTEXT, 0, title.as_ptr() as LPARAM);
    if *view_id == ViewId::RoomList {
        // No text and no editbox: only the list's rows change.
        refresh_room_list_rows(state_ref(hwnd), content);
        return Ok(());
    }
    disarm_vanished_compose_target(state_mut(hwnd), view_id);
    match view_id {
        ViewId::RoomList => unreachable!("the Rooms List returned above"),
        // Each branch logs first: a log that fails prints why in the
        // window, which the rows built next then show.
        ViewId::Status => {
            write_view_log(state_mut(hwnd), view_id);
            let rows = {
                let state = state_ref(hwnd);
                build_status_timeline_rows(&state.model.status_log, state.timestamps())
            };
            set_timeline_rows(content, &rows);
            state_mut(hwnd).refresh_counters.count_row_build(view_id);
            recompute_find_for_view(state_mut(hwnd), view_id, &rows);
            apply_find_to_active_view(hwnd, view_id)?;
            InvalidateRect(content, null(), 1);
        }
        ViewId::Room(room_id) => {
            decide_auto_log(state_mut(hwnd), view_id);
            write_view_log(state_mut(hwnd), view_id);
            let (rows, member_rows, image_requests, reply_target_requests) = {
                let state = state_ref(hwnd);
                let room = room_by_id(&state.model.rooms, room_id)
                    .expect("room view must target an existing room");
                let room_view = state.model.room_views.get(room_id);
                let timestamps = state.timestamps();
                let mut rows = build_room_timeline_rows(&state.model, room_id, timestamps);
                rows.extend(armed_compose_target_row(state, view_id));
                let (image_requests, reply_target_requests) = match room_view {
                    Some(room_view) if is_sendable_room(room) => (
                        collect_lazy_room_preview_media_requests(
                            room_view,
                            room_view.activities.iter(),
                        ),
                        collect_reply_target_requests(room_view, room_view.activities.iter()),
                    ),
                    _ => (Vec::new(), BTreeSet::new()),
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
                (rows, member_rows, image_requests, reply_target_requests)
            };
            dispatch_lazy_room_preview_media_requests(state_mut(hwnd), room_id, image_requests)?;
            dispatch_reply_target_fetches(state_mut(hwnd), room_id, reply_target_requests)?;
            validate_selected_timeline_event(state_mut(hwnd), view_id, &rows);
            let timeline = room_content_timeline(content);
            set_timeline_rows(timeline, &rows);
            state_mut(hwnd).refresh_counters.count_row_build(view_id);
            recompute_find_for_view(state_mut(hwnd), view_id, &rows);
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
            let (rows, member_rows, image_requests, reply_target_requests) = {
                let state = state_ref(hwnd);
                let room_view = state
                    .model
                    .room_views
                    .get(room_id)
                    .expect("thread view must target an existing room view");
                let timestamps = state.timestamps();
                let mut rows = build_thread_timeline_rows(
                    &state.model,
                    room_id,
                    thread_root_event_id,
                    timestamps,
                );
                rows.extend(armed_compose_target_row(state, view_id));
                let thread_activities = || {
                    room_view.activities.iter().filter(|activity| {
                        room_activity_belongs_to_thread(activity, thread_root_event_id)
                    })
                };
                let image_requests =
                    collect_lazy_room_preview_media_requests(room_view, thread_activities());
                let reply_target_requests =
                    collect_reply_target_requests(room_view, thread_activities());
                let member_rows = room_member_view_rows(room_view.members.as_slice());
                (rows, member_rows, image_requests, reply_target_requests)
            };
            dispatch_lazy_room_preview_media_requests(state_mut(hwnd), room_id, image_requests)?;
            dispatch_reply_target_fetches(state_mut(hwnd), room_id, reply_target_requests)?;
            validate_selected_timeline_event(state_mut(hwnd), view_id, &rows);
            let timeline = room_content_timeline(content);
            set_timeline_rows(timeline, &rows);
            state_mut(hwnd).refresh_counters.count_row_build(view_id);
            recompute_find_for_view(state_mut(hwnd), view_id, &rows);
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

pub(super) unsafe fn refresh_every_view_content(hwnd: HWND) -> Result<(), String> {
    let view_ids = state_ref(hwnd).views.keys().cloned().collect::<Vec<_>>();
    for view_id in &view_ids {
        refresh_view_content(hwnd, view_id)?;
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
    let rebuild_switchbar_after_activation = !state_ref(hwnd).switchbar_views.contains(view_id);
    let child = match view_id {
        ViewId::Status => {
            state_mut(hwnd).model.selected_room = None;
            ensure_status_view(hwnd)?
        }
        ViewId::RoomList => {
            state_mut(hwnd).model.selected_room = None;
            ensure_room_list_view(hwnd)?
        }
        ViewId::Room(room_id) => {
            let state = state_mut(hwnd);
            select_room(state, room_id)?;
            // Activating a closed query reopens its window.
            state.closed_queries.remove(room_id);
            ensure_room_view(hwnd, room_id)?
        }
        ViewId::Thread {
            room_id,
            thread_root_event_id,
        } => {
            select_room(state_mut(hwnd), room_id)?;
            ensure_thread_view(hwnd, room_id, thread_root_event_id)?
        }
    };
    let cleared_view_activity = handle_view_activation(state_mut(hwnd), view_id)?;

    let newly_active = {
        let state = state_mut(hwnd);
        let newly_active = state.active_view != *view_id;
        if newly_active {
            state.selected_timeline_event = None;
            hide_color_index(state);
        }
        state.active_view = view_id.clone();
        newly_active
    };

    refresh_tree_activity_colors(hwnd)?;
    refresh_view_content(hwnd, view_id)?;
    // A newly active window takes the treebar caret and unfolds its folder.
    // Re-applying the active one leaves a caret that the treebar keys put on
    // a space folder, which activates nothing.
    let caret_on_space = {
        let state = state_ref(hwnd);
        selected_tree_item(state.controls.tree)
            .is_some_and(|item| matches!(state.tree_items[&item].node, NavNode::Space { .. }))
    };
    if newly_active || !caret_on_space {
        select_tree_item(hwnd, view_id, preferred_tree_item, newly_active);
    }

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
    Ok(())
}

pub(super) unsafe fn activate_view(
    hwnd: HWND,
    view_id: &ViewId,
    preferred_tree_item: Option<isize>,
) -> Result<(), String> {
    let child = show_view(hwnd, view_id, preferred_tree_item)?;
    SetFocus(mdi_child_focus(child));
    Ok(())
}

/// Activation without the focus, for re-applying the active window after
/// a model change: the focus stays in the treebar, nick list or editbox
/// the user left it in. Returns the view's MDI child.
pub(super) unsafe fn show_view(
    hwnd: HWND,
    view_id: &ViewId,
    preferred_tree_item: Option<isize>,
) -> Result<HWND, String> {
    let applied = apply_view_activation_state(hwnd, view_id, preferred_tree_item)?;

    let mdi_client = state_ref(hwnd).controls.mdi_client;
    SendMessageW(mdi_client, WM_MDIACTIVATE, applied.child as WPARAM, 0);
    // Activating a minimized window must show it, as in mIRC. MDI itself
    // keeps a maximized window's successor maximized.
    if IsIconic(applied.child) != 0 {
        SendMessageW(mdi_client, WM_MDIRESTORE, applied.child as WPARAM, 0);
    }
    finish_view_activation_chrome(hwnd, &applied)?;
    Ok(applied.child)
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
    finish_view_activation_chrome(frame, &applied)?;
    SetFocus(mdi_child_focus(applied.child));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::super::test_fixtures::*;
    use super::*;

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
                &BTreeSet::new(),
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
            resolve_active_view(
                &ViewId::Room("!gone:example".to_owned()),
                None,
                &model,
                &BTreeSet::new(),
            ),
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
            resolve_active_view(
                &ViewId::Room("!gone:example".to_owned()),
                None,
                &model,
                &BTreeSet::new(),
            ),
            ViewId::Room("!alpha:example".to_owned())
        );
    }

    #[test]
    fn resolve_active_view_falls_back_to_status_when_no_rooms_remain() {
        let model = UiModel::default();
        assert_eq!(
            resolve_active_view(
                &ViewId::Room("!gone:example".to_owned()),
                None,
                &model,
                &BTreeSet::new(),
            ),
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
                &BTreeSet::new(),
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
                &BTreeSet::new(),
            ),
            ViewId::Room("!alpha:example".to_owned())
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
    fn close_action_follows_mirc_per_window_kind() {
        let mut state = test_state();
        state.model.rooms = vec![
            room("!ops:example", "Ops", false),
            query("!dm:example", "Alice", "@alice:example"),
            invited_room("!invite:example", "Invite"),
        ];

        assert_eq!(
            close_action(&state.model, &ViewId::Status),
            CloseAction::Refuse
        );
        assert_eq!(
            close_action(&state.model, &thread_view_id("!ops:example", "$root")),
            CloseAction::Destroy
        );
        assert_eq!(
            close_action(&state.model, &room_view_id("!ops:example")),
            CloseAction::Confirm {
                room_id: "!ops:example".to_owned(),
                leave: CloseLeave::Part,
            }
        );
        assert_eq!(
            close_action(&state.model, &room_view_id("!dm:example")),
            CloseAction::HideQuery("!dm:example".to_owned())
        );
        assert_eq!(
            close_action(&state.model, &room_view_id("!invite:example")),
            CloseAction::Confirm {
                room_id: "!invite:example".to_owned(),
                leave: CloseLeave::RejectInvite,
            }
        );
    }

    #[test]
    fn close_confirmation_text_names_the_room() {
        assert_eq!(
            close_confirmation_text(CloseLeave::Part, &room("!ops:example", "Ops", false)),
            "Leave Ops?\n\nYou may need a new invite to rejoin."
        );
        assert_eq!(
            close_confirmation_text(
                CloseLeave::RejectInvite,
                &invited_room("!invite:example", "Invite")
            ),
            "Reject the invite to Invite?"
        );
    }

    #[test]
    fn resolve_active_view_skips_closed_queries_when_falling_back() {
        let model = UiModel {
            rooms: vec![
                query("!dm:example", "Alice", "@alice:example"),
                room("!ops:example", "Ops", false),
            ],
            ..UiModel::default()
        };
        let closed = BTreeSet::from(["!dm:example".to_owned()]);

        assert_eq!(
            resolve_active_view(&room_view_id("!dm:example"), None, &model, &closed),
            room_view_id("!ops:example")
        );
        assert_eq!(
            resolve_active_view(
                &room_view_id("!gone:example"),
                Some("!dm:example"),
                &model,
                &closed
            ),
            room_view_id("!ops:example")
        );
        assert_eq!(
            resolve_active_view(&room_view_id("!dm:example"), None, &model, &BTreeSet::new()),
            room_view_id("!dm:example")
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
}
