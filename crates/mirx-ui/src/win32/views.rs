//! The MDI view windows: creating, refreshing, activating and closing them.

use windows_sys::Win32::Graphics::Gdi::{HDC, SetBkColor};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    EN_CHANGE, WM_CTLCOLOREDIT, WM_CTLCOLORLISTBOX, WM_ERASEBKGND, WS_CLIPSIBLINGS,
};

use super::nick_colors::{NickColorSetting, NickColorer};
use super::settings::Settings;
use super::timeline_rows::color_row_senders;
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
/// mIRC's window menu Buffer: Clear (`/clear`) and Save... (`/savebuf`).
const SYSMENU_BUFFER_LABEL: &str = "&Buffer";
const SYSMENU_ID_BUFFER_CLEAR: usize = 0x0150;
const SYSMENU_BUFFER_CLEAR_LABEL: &str = "&Clear";
const SYSMENU_ID_BUFFER_SAVE: usize = 0x0160;
const SYSMENU_BUFFER_SAVE_LABEL: &str = "&Save...";

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

/// Writes the frame's `mirx - [caption]` title again from the maximized
/// `child`, now active. Wine's MDI writes it when a child maximizes, from
/// the child active then: switching to a maximized window, or creating one
/// maximized, titles the frame with the previous window, or none. A
/// maximized child's `WM_SETTEXT` writes it from the active child.
unsafe fn retitle_frame(child: HWND) {
    let caption = wide_null(&read_window_text(child));
    SendMessageW(child, WM_SETTEXT, 0, caption.as_ptr() as LPARAM);
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
        // Its one editbox changed, however: typed, pasted, set.
        WM_COMMAND if (wparam >> 16) as u32 == EN_CHANGE => {
            assert_eq!(
                lparam as HWND,
                mdi_child_edit(hwnd),
                "an MDI child's only edit control is its editbox"
            );
            if let Err(message) = typing::editbox_changed(frame_window_for(hwnd), hwnd) {
                eprintln!("{message}");
            }
            0
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
            if lparam as HWND == hwnd {
                if let Err(message) = handle_native_mdi_child_activation(hwnd) {
                    eprintln!("{message}");
                }
                if IsZoomed(hwnd) != 0 {
                    retitle_frame(hwnd);
                }
            }
            DefMDIChildProcW(hwnd, message, wparam, lparam)
        }
        WM_SYSCOMMAND if wparam & 0xFFF0 == SYSMENU_ID_FIND => {
            let frame = frame_window_for(hwnd);
            let view_id = state_ref(frame).hwnd_views[&(hwnd as isize)].clone();
            if let Err(message) = begin_find_dialog(frame, &view_id) {
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
        WM_SYSCOMMAND if wparam & 0xFFF0 == SYSMENU_ID_BUFFER_CLEAR => {
            let frame = frame_window_for(hwnd);
            let view_id = state_ref(frame).hwnd_views[&(hwnd as isize)].clone();
            if let Err(message) = clear_window(frame, &view_id)
                .and_then(|status| set_local_command_status(frame, status))
            {
                show_error_message(frame, "Clear Buffer", &message);
            }
            0
        }
        WM_SYSCOMMAND if wparam & 0xFFF0 == SYSMENU_ID_BUFFER_SAVE => {
            let frame = frame_window_for(hwnd);
            let view_id = state_ref(frame).hwnd_views[&(hwnd as isize)].clone();
            let saved = run_save_buffer_dialog(frame, &view_id).and_then(|status| match status {
                Some(status) => set_local_command_status(frame, status),
                None => Ok(()),
            });
            if let Err(message) = saved {
                show_error_message(frame, "Save Buffer", &message);
            }
            0
        }
        WM_SYSCOMMAND if wparam & 0xFFF0 == SYSMENU_ID_FONT => {
            let frame = frame_window_for(hwnd);
            let view_id = state_ref(frame).hwnd_views[&(hwnd as isize)].clone();
            if let Err(message) =
                run_local_command(frame, &view_id, SlashCommand::Font(FontCommand::Dialog))
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
                    CloseLeave::Part => submit_part_slash_command(state, &view_id, Some(room_id)),
                    CloseLeave::RejectInvite => {
                        submit_reject_slash_command(state, &view_id, Some(room_id))
                    }
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
        // The nick list's notifications (`LBS_NOTIFY`); only the
        // double-click does something.
        WM_COMMAND if (wparam >> 16) as u32 == LBN_DBLCLK => {
            assert_eq!(
                lparam as HWND,
                room_content_member_list(hwnd),
                "only the nick list notifies"
            );
            if let Err(message) = query_double_clicked_member(hwnd) {
                show_error_message(frame_window_for(hwnd), "Member", &message);
            }
            0
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
    // The two clip each other. When the window widens, the text area grows
    // over the nick list's old place before the list moves right. Without
    // clipping, the text area paints there, and the moving list copies
    // those pixels as its own still-valid bits.
    let timeline = CreateWindowExW(
        WS_EX_CLIENTEDGE,
        wide_null(TIMELINE_CLASS_NAME).as_ptr(),
        wide_null("").as_ptr(),
        WS_CHILD | WS_VISIBLE | WS_VSCROLL | WS_CLIPSIBLINGS,
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
            | WS_CLIPSIBLINGS
            | (LBS_NOINTEGRALHEIGHT
                | LBS_OWNERDRAWFIXED
                | LBS_HASSTRINGS
                | LBS_EXTENDEDSEL
                | LBS_NOTIFY) as u32,
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
    let layout = mdi::room_content_layout(rect.right - rect.left, Dpi::of_window(hwnd));
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

/// mirx's choice of the window that follows a gone one: the active one if
/// it still has a window, else its room, the selected room, the first room
/// or Status. A closed query has no window, so it is never picked.
/// `destroy_stale_room_views` activates it when the active window goes.
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

/// Takes the windows whose room, thread or listing went out of the
/// registry, and returns them for destroying. The active view may be one
/// of them: `destroy_stale_room_views` activates its successor.
pub(super) fn take_stale_room_views(state: &mut WindowState) -> Vec<MdiViewState> {
    let stale_view_ids = state
        .views
        .keys()
        .filter(|view_id| !view_id_is_valid(&state.model, view_id))
        .cloned()
        .collect::<Vec<_>>();

    for view_id in &stale_view_ids {
        state.find_states.remove(view_id);
        cancel_jump_for_view(state, view_id);
    }

    stale_view_ids
        .into_iter()
        .filter_map(|view_id| {
            let view = state.views.remove(&view_id)?;
            state.hwnd_views.remove(&(view.child as isize));
            room_window_gone(state, &view_id);
            Some(view)
        })
        .collect::<Vec<_>>()
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
        // Clipping its children: its background erase must not paint over
        // the text area's and the nick list's borders, which they draw
        // when they move, before the erase comes.
        ViewId::Room(_) | ViewId::Thread { .. } => (
            ROOM_CONTENT_CLASS_NAME,
            WS_CHILD | WS_VISIBLE | WS_CLIPCHILDREN,
            0,
        ),
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
    let buffer_menu = if view_has_editbox(view_id) {
        Some(create_buffer_menu(view_id)?)
    } else {
        None
    };
    let buffer_label = wide_null(SYSMENU_BUFFER_LABEL);
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
        || buffer_menu.is_some_and(|menu| {
            AppendMenuW(system_menu, MF_POPUP, menu as usize, buffer_label.as_ptr()) == 0
        })
        || AppendMenuW(system_menu, MF_STRING, SYSMENU_ID_FONT, font_label.as_ptr()) == 0
    {
        return Err(last_os_error(
            "AppendMenuW window menu Find Text..., Send File..., Logging, Buffer and Font...",
        ));
    }

    layout_mdi_child(child);
    Ok(())
}

/// A window's Buffer submenu. A thread window has nothing of its own to
/// clear, as `/clear` there says.
unsafe fn create_buffer_menu(view_id: &ViewId) -> Result<HMENU, String> {
    let menu = CreatePopupMenu();
    if menu.is_null() {
        return Err(last_os_error("CreatePopupMenu window menu Buffer"));
    }
    let clear_flags = match view_id {
        ViewId::Thread { .. } => MF_STRING | MF_GRAYED,
        ViewId::Status | ViewId::Room(_) => MF_STRING,
        ViewId::RoomList => unreachable!("the Rooms List has no buffer"),
    };
    append_menu_text(
        menu,
        clear_flags,
        SYSMENU_ID_BUFFER_CLEAR,
        SYSMENU_BUFFER_CLEAR_LABEL,
    )?;
    append_menu_text(
        menu,
        MF_STRING,
        SYSMENU_ID_BUFFER_SAVE,
        SYSMENU_BUFFER_SAVE_LABEL,
    )?;
    Ok(menu)
}

/// The window menu's Logging: turns the log of the window that owns the
/// menu on or off, and says so there. That window need not be the active
/// one.
unsafe fn toggle_window_logging(frame: HWND, child: HWND) -> Result<(), String> {
    let window = state_ref(frame).hwnd_views[&(child as isize)].clone();
    let view_id = log_view(&window)?;
    let on = view_log_path(state_ref(frame), &view_id).is_none();
    let line = set_view_logging(state_mut(frame), &view_id, on)?;
    echo_in_window(frame, &window, [line])
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

/// The window whose text `timeline` shows.
pub(super) unsafe fn timeline_view(state: &WindowState, timeline: HWND) -> ViewId {
    state
        .views
        .iter()
        .find(|(view_id, view)| {
            **view_id != ViewId::RoomList && view_timeline(view_id, view.content) == timeline
        })
        .map(|(view_id, _)| view_id.clone())
        .expect("a timeline is a window's")
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
    cancel_jump_for_view(state, &view_id);
    room_window_gone(state, &view_id);
    assert_eq!(
        removed.child, child,
        "tracked child HWND must match registry"
    );
    // The active window is still the active view only at frame teardown,
    // where nothing follows it: closing it activated MDI's next child
    // before WM_NCDESTROY, and a window whose room went had its successor
    // activated before it was destroyed (`destroy_stale_room_views`).
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
        rows_key: None,
        nick_list: Vec::new(),
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

/// Destroys the windows whose room, thread or listing went. If the active
/// window is one of them, mirx's choice of the next one
/// (`resolve_active_view`) is activated first, so MDI never picks one when
/// the active child goes. The gone window is not left: its room has no
/// marker to move.
pub(super) unsafe fn destroy_stale_room_views(hwnd: HWND) -> Result<(), String> {
    let (mdi_client, stale_views) = {
        let state = state_mut(hwnd);
        (state.controls.mdi_client, take_stale_room_views(state))
    };
    let successor = {
        let state = state_ref(hwnd);
        (!state.views.contains_key(&state.active_view)).then(|| {
            resolve_active_view(
                &state.active_view,
                state.model.selected_room.as_deref(),
                &state.model,
                &state.closed_queries,
            )
        })
    };
    if let Some(successor) = successor {
        // `activate_view`: the focus was in the window going.
        activate_view(hwnd, &successor, None)?;
    }

    // WM_MDIDESTROY, not WM_CLOSE: closing a room window asks to leave it.
    for view in stale_views {
        SendMessageW(mdi_client, WM_MDIDESTROY, view.child as WPARAM, 0);
    }
    Ok(())
}

/// Everything a window's rows are built from. A window rebuilds its rows
/// in `sync_view_content` only when this differs from the key it last built
/// them from, so it must hold every input of the row builders
/// (`build_room_timeline_rows`, `build_thread_timeline_rows`,
/// `build_status_timeline_rows`, `armed_compose_target_row`) and of the
/// refresh's fetches.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ViewRowsKey {
    /// A room or thread window's room, and its view model's revision.
    room: Option<(RoomSummary, Option<u64>)>,
    /// The Status window's log.
    status_revision: Option<u64>,
    timestamp: TimestampSetting,
    own_user_id: Option<String>,
    /// Whether the refresh may ask for preview media and reply targets: a
    /// window whose requests were skipped asks once it may. The price is
    /// one rebuild of every window when it flips, as on (re)connecting.
    can_fetch_media: bool,
    can_fetch_events: bool,
    compose_target: Option<ComposeTarget>,
    /// A room window's line marker: the read marker's event while the
    /// marker is shown. Not the whole read state: its unread counts change
    /// with every receipt and draw nothing.
    line_marker: Option<String>,
    /// A room or thread window's nick colours while some nick can take
    /// one: the list, and the notify list while an entry asks for it. The
    /// ranks they read are the room's members, in its revision. The scheme
    /// is not here: auto colours pick from it as they are drawn.
    nick_colors: Option<(NickColorSetting, Vec<String>)>,
    /// A room or thread window's Show mode prefix (Options > IRC). The
    /// ranks it reads are the room's members, in its revision.
    mode_prefix: bool,
}

/// The inputs of `view_id`'s rows now; the Rooms List has none.
pub(super) fn view_rows_key(state: &WindowState, view_id: &ViewId) -> ViewRowsKey {
    let room = view_id.room_id().map(|room_id| {
        (
            room_by_id(&state.model.rooms, room_id)
                .expect("a room or thread window's room exists")
                .clone(),
            state.model.room_views.revision(room_id),
        )
    });
    ViewRowsKey {
        room,
        status_revision: match view_id {
            ViewId::Status => Some(state.model.status_log.revision()),
            ViewId::Room(_) | ViewId::Thread { .. } => None,
            ViewId::RoomList => unreachable!("the Rooms List has no rows to gate"),
        },
        timestamp: state.settings.settings().timestamp.clone(),
        own_user_id: state.model.own_user_id().map(str::to_owned),
        can_fetch_media: can_fetch_room_media(state),
        can_fetch_events: can_fetch_room_events(state),
        compose_target: state.views[view_id].compose().target.clone(),
        line_marker: match view_id {
            ViewId::Room(room_id) => room_line_marker(state, room_id).map(str::to_owned),
            ViewId::Status | ViewId::Thread { .. } => None,
            ViewId::RoomList => unreachable!("the Rooms List has no rows to gate"),
        },
        nick_colors: view_nick_colors(state.settings.settings(), view_id),
        mode_prefix: view_id.room_id().is_some() && state.settings.settings().irc.mode_prefix,
    }
}

/// The nick colour inputs of `view_id`'s rows: none for Status, which
/// shows no senders, and none while no nick takes a colour.
fn view_nick_colors(
    settings: &Settings,
    view_id: &ViewId,
) -> Option<(NickColorSetting, Vec<String>)> {
    let nick_colors = &settings.nick_colors;
    if view_id.room_id().is_none() || !nick_colors.colors_nicks() {
        return None;
    }
    let notify = if nick_colors.list.entries().iter().any(|entry| entry.notify) {
        settings
            .notify
            .list
            .entries()
            .iter()
            .map(|entry| entry.user_id.clone())
            .collect()
    } else {
        Vec::new()
    };
    Some((nick_colors.clone(), notify))
}

/// Brings a window up to date with the model: its caption, and its rows
/// if their inputs changed since they were built. Rebuilding is the costly
/// part of a refresh, which runs after every core event batch.
pub(super) unsafe fn sync_view_content(hwnd: HWND, view_id: &ViewId) -> Result<(), String> {
    if *view_id == ViewId::RoomList {
        // Owner data: a refresh only sets the item count and repaints.
        return refresh_view_content(hwnd, view_id);
    }
    let state = state_ref(hwnd);
    if state.views[view_id].rows_key.as_ref() == Some(&view_rows_key(state, view_id)) {
        return Ok(());
    }
    refresh_view_content(hwnd, view_id)
}

/// Sets the window's caption, unless it already reads so: setting a
/// maximized child's caption repaints the frame's.
unsafe fn sync_view_caption(hwnd: HWND, view_id: &ViewId) {
    let state = state_ref(hwnd);
    let child = state.views[view_id].child;
    let caption = view_caption(&state.model, view_id);
    if read_window_text(child) != caption {
        let caption = wide_null(&caption);
        SendMessageW(child, WM_SETTEXT, 0, caption.as_ptr() as LPARAM);
    }
}

/// Rebuilds the window's rows from the model, whatever changed.
pub(super) unsafe fn refresh_view_content(hwnd: HWND, view_id: &ViewId) -> Result<(), String> {
    let content = state_ref(hwnd)
        .views
        .get(view_id)
        .ok_or_else(|| "view state was not tracked".to_owned())?
        .content;
    sync_view_caption(hwnd, view_id);
    if *view_id == ViewId::RoomList {
        // No text and no editbox: only the list's rows change.
        refresh_room_list_rows(state_ref(hwnd), content);
        return Ok(());
    }
    disarm_vanished_compose_target(state_mut(hwnd), view_id);
    // Logging first: a log that fails prints why in the window, which the
    // rows built next then show.
    if let ViewId::Room(_) = view_id {
        decide_auto_log(state_mut(hwnd), view_id);
    }
    if let ViewId::Status | ViewId::Room(_) = view_id {
        write_view_log(state_mut(hwnd), view_id);
    }
    // Fetching marks what it asks for as pending in the room's view model,
    // so it goes before the key: the rows are built from that state.
    if let Some(room_id) = view_id.room_id() {
        dispatch_view_fetches(hwnd, view_id, room_id)?;
    }
    let key = view_rows_key(state_ref(hwnd), view_id);
    let rows = {
        let state = state_ref(hwnd);
        let timestamps = state.timestamps();
        let mut rows = match view_id {
            ViewId::Status => build_status_timeline_rows(&state.model.status_log, timestamps),
            ViewId::Room(room_id) => build_room_timeline_rows(
                &state.model,
                room_id,
                timestamps,
                room_line_marker(state, room_id),
                state.settings.settings().irc.mode_prefix,
            ),
            ViewId::Thread {
                room_id,
                thread_root_event_id,
            } => build_thread_timeline_rows(
                &state.model,
                room_id,
                thread_root_event_id,
                timestamps,
                state.settings.settings().irc.mode_prefix,
            ),
            ViewId::RoomList => unreachable!("the Rooms List returned above"),
        };
        if let Some(room_id) = view_id.room_id() {
            let members = state
                .model
                .room_views
                .get(room_id)
                .map(|room_view| room_view.members.as_slice())
                .unwrap_or_default();
            color_row_senders(
                &mut rows,
                &NickColorer::new(state.settings.settings(), members),
            );
        }
        rows.extend(armed_compose_target_row(state, view_id));
        rows
    };
    validate_selected_timeline_event(state_mut(hwnd), view_id, &rows);
    let timeline = view_timeline(view_id, content);
    set_timeline_rows(timeline, &rows);
    recompute_find_for_view(state_mut(hwnd), view_id, &rows);
    apply_find_to_active_view(hwnd, view_id)?;
    if let Some(room_id) = view_id.room_id() {
        let items = {
            let state = state_ref(hwnd);
            let room = room_by_id(&state.model.rooms, room_id)
                .expect("a room or thread window's room exists");
            // An invite's window lists nobody: its room is not joined yet.
            if is_sendable_room(room) {
                let members = state
                    .model
                    .room_views
                    .get(room_id)
                    .map(|room_view| room_view.members.as_slice())
                    .unwrap_or_default();
                nick_list_items(members)
            } else {
                Vec::new()
            }
        };
        let list = room_content_member_list(content);
        let view = state_mut(hwnd)
            .views
            .get_mut(view_id)
            .expect("a refreshed view has a window");
        sync_nick_list(list, &mut view.nick_list, items);
        // Colours and presence draw from the model.
        InvalidateRect(list, null(), 1);
    }
    InvalidateRect(content, null(), 1);
    InvalidateRect(timeline, null(), 1);
    let state = state_mut(hwnd);
    state
        .views
        .get_mut(view_id)
        .expect("a refreshed view has a window")
        .rows_key = Some(key);
    state.refresh_counters.count_row_build(view_id);
    Ok(())
}

/// Asks for the preview media and reply targets the window's messages need
/// and its room does not have yet.
unsafe fn dispatch_view_fetches(hwnd: HWND, view_id: &ViewId, room_id: &str) -> Result<(), String> {
    let (image_requests, reply_target_requests) = {
        let state = state_ref(hwnd);
        let room =
            room_by_id(&state.model.rooms, room_id).expect("a room or thread window's room exists");
        match state.model.room_views.get(room_id) {
            Some(room_view) if is_sendable_room(room) => {
                let activities = || {
                    room_view
                        .activities
                        .iter()
                        .filter(|activity| match view_id {
                            ViewId::Thread {
                                thread_root_event_id,
                                ..
                            } => room_activity_belongs_to_thread(activity, thread_root_event_id),
                            _ => true,
                        })
                };
                (
                    collect_lazy_room_preview_media_requests(room_view, activities()),
                    collect_reply_target_requests(room_view, activities()),
                )
            }
            _ => return Ok(()),
        }
    };
    dispatch_lazy_room_preview_media_requests(state_mut(hwnd), room_id, image_requests)?;
    dispatch_reply_target_fetches(state_mut(hwnd), room_id, reply_target_requests)
}

pub(super) unsafe fn refresh_every_view_content(hwnd: HWND) -> Result<(), String> {
    let view_ids = state_ref(hwnd).views.keys().cloned().collect::<Vec<_>>();
    for view_id in &view_ids {
        refresh_view_content(hwnd, view_id)?;
    }
    Ok(())
}

unsafe fn apply_view_activation_state(
    hwnd: HWND,
    view_id: &ViewId,
    preferred_tree_item: Option<isize>,
) -> Result<HWND, String> {
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
    let previous_active = state_ref(hwnd).active_view.clone();
    let newly_active = previous_active != *view_id;
    if newly_active {
        handle_view_activation(state_mut(hwnd), view_id)?;
    } else {
        // Re-applying the active window after a model change reads nothing
        // again. A load or a join that made it the active one may have
        // left it activity from before, which the active window never
        // shows.
        state_mut(hwnd).room_activity.remove(view_id);
    }
    // The window left, if it is still there: closing the active window
    // left it already, and a window whose room went has no marker to move.
    if newly_active && state_ref(hwnd).views.contains_key(&previous_active) {
        line_marker::leave_window(hwnd, &previous_active)?;
    }
    {
        let state = state_mut(hwnd);
        if newly_active {
            state.selected_timeline_event = None;
            hide_color_index(state);
        }
        state.active_view = view_id.clone();
    }
    if newly_active {
        // Before the refresh below repaints it.
        line_marker::enter_window(hwnd, view_id);
    }

    refresh_tree_activity_colors(hwnd)?;
    if newly_active {
        // Find marks its match in the active window only. The Rooms List
        // has no timeline, and the previous window is gone if its room went.
        if previous_active != ViewId::RoomList
            && state_ref(hwnd).views.contains_key(&previous_active)
        {
            apply_find_to_active_view(hwnd, &previous_active)?;
        }
        // Its find projection and selection are the active window's.
        refresh_view_content(hwnd, view_id)?;
    } else {
        sync_view_content(hwnd, view_id)?;
    }
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

    Ok(child)
}

/// The switchbar and status bar after an activation: a window new to the
/// switchbar adds its button, and the checked button and activity colours
/// follow.
unsafe fn finish_view_activation_chrome(hwnd: HWND) -> Result<(), String> {
    sync_switchbar(hwnd)?;
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
    let child = apply_view_activation_state(hwnd, view_id, preferred_tree_item)?;

    let mdi_client = state_ref(hwnd).controls.mdi_client;
    SendMessageW(mdi_client, WM_MDIACTIVATE, child as WPARAM, 0);
    // Activating a minimized window must show it, as in mIRC. MDI itself
    // keeps a maximized window's successor maximized.
    if IsIconic(child) != 0 {
        SendMessageW(mdi_client, WM_MDIRESTORE, child as WPARAM, 0);
    }
    finish_view_activation_chrome(hwnd)?;
    Ok(child)
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

    let child = apply_view_activation_state(frame, &view_id, None)?;
    finish_view_activation_chrome(frame)?;
    SetFocus(mdi_child_focus(child));
    Ok(())
}

#[cfg(test)]
mod tests {
    use windows_sys::Win32::UI::WindowsAndMessaging::IsChild;

    use super::super::test_fixtures::*;
    use super::*;

    /// Ops with its room window and Status, stand-in windows only.
    fn keyed_state() -> WindowState {
        let mut state = compose_target_state();
        state.model.room_read_states.insert(
            "!ops:example".to_owned(),
            RoomReadState {
                room_id: "!ops:example".to_owned(),
                latest_active_event_id: None,
                fully_read_event_id: Some("$q".to_owned()),
                fully_read_timestamp_ms: None,
                marked_unread: false,
                num_unread: 1,
                num_notifications: 0,
                num_mentions: 0,
            },
        );
        state.views.insert(ViewId::Status, tracked_view(100));
        state
            .views
            .insert(room_view_id("!ops:example"), tracked_view(200));
        state
    }

    #[test]
    fn view_rows_key_changes_with_each_input() {
        let ops = room_view_id("!ops:example");
        type Change = fn(&mut WindowState);
        let changes: [(&str, Change, bool, bool); 16] = [
            // (what, change, moves the room's key, moves Status's key)
            (
                "a rename",
                |state| state.model.rooms[0].display_name = "Renamed".to_owned(),
                true,
                false,
            ),
            (
                "a room view mutation",
                |state| {
                    state.model.room_views.get_mut("!ops:example").unwrap();
                },
                true,
                false,
            ),
            (
                "a Status line",
                |state| log_status(state, StatusLineKind::Info, "* hello".to_owned()),
                false,
                true,
            ),
            (
                "the own user",
                |state| {
                    state.model.own_account = Some(OwnAccount::matrix("@other:example".to_owned()))
                },
                true,
                true,
            ),
            (
                "fetching becomes possible",
                |state| state.model.connection = ConnectionState::Syncing,
                true,
                true,
            ),
            (
                "a synthetic load",
                |state| state.synthetic_load_in_progress = true,
                true,
                true,
            ),
            (
                "an armed editbox",
                |state| {
                    state
                        .views
                        .get_mut(&room_view_id("!ops:example"))
                        .unwrap()
                        .compose_mut()
                        .target = Some(ComposeTarget::Reply {
                        event_id: "$q".to_owned(),
                    })
                },
                true,
                false,
            ),
            (
                "the timestamp format",
                |state| {
                    state
                        .settings
                        .update(|settings| settings.timestamp.show = false)
                        .unwrap()
                },
                true,
                true,
            ),
            (
                "a read marker move",
                |state| {
                    state
                        .model
                        .room_read_states
                        .get_mut("!ops:example")
                        .unwrap()
                        .fully_read_event_id = Some("$moved".to_owned())
                },
                true,
                false,
            ),
            (
                "an unread count",
                |state| {
                    let read_state = state
                        .model
                        .room_read_states
                        .get_mut("!ops:example")
                        .unwrap();
                    read_state.num_unread += 1;
                    read_state.num_notifications += 1;
                },
                false,
                false,
            ),
            (
                "Show mode prefix",
                |state| {
                    state
                        .settings
                        .update(|settings| settings.irc.mode_prefix = true)
                        .unwrap()
                },
                true,
                false,
            ),
            (
                "hiding the line marker",
                |state| {
                    state
                        .settings
                        .update(|settings| settings.marker.show = false)
                        .unwrap()
                },
                true,
                false,
            ),
            (
                // Paint takes the colour; the rows stay.
                "the line marker's colour",
                |state| {
                    state
                        .settings
                        .update(|settings| {
                            settings.marker.color = crate::win32::colors::PaletteIndex::new(12)
                                .expect("a palette index")
                        })
                        .unwrap()
                },
                false,
                false,
            ),
            (
                "a nick colour entry",
                |state| {
                    state
                        .settings
                        .update(|settings| settings.nick_colors.list.push(nick_color_entry("* 4")))
                        .unwrap()
                },
                true,
                false,
            ),
            (
                // No entry asks for the notify list.
                "a notify list change",
                |state| {
                    state
                        .settings
                        .update(|settings| {
                            settings
                                .notify
                                .list
                                .add("@bob:example".to_owned(), None, false);
                        })
                        .unwrap()
                },
                false,
                false,
            ),
            (
                // Auto colours pick from it as they are drawn.
                "the colour scheme",
                |state| {
                    state
                        .settings
                        .update(|settings| {
                            settings.colors.set(
                                ColorItem::Background,
                                crate::win32::colors::PaletteIndex::new(1)
                                    .expect("a palette index"),
                            )
                        })
                        .unwrap()
                },
                false,
                false,
            ),
        ];
        for (what, change, moves_room, moves_status) in changes {
            let mut state = keyed_state();
            let room_before = view_rows_key(&state, &ops);
            let status_before = view_rows_key(&state, &ViewId::Status);
            assert_eq!(view_rows_key(&state, &ops), room_before, "{what}");
            change(&mut state);
            assert_eq!(
                view_rows_key(&state, &ops) != room_before,
                moves_room,
                "{what} (room)"
            );
            assert_eq!(
                view_rows_key(&state, &ViewId::Status) != status_before,
                moves_status,
                "{what} (Status)"
            );
        }
    }

    fn nick_color_entry(value: &str) -> crate::win32::nick_colors::NickColorEntry {
        crate::win32::nick_colors::NickColorEntry::parse_ini_value(value).expect("a valid entry")
    }

    /// The notify list is an input of the rows only while an entry asks
    /// for it, and the nick colours only while some nick can take one.
    #[test]
    fn nick_colour_inputs_key_the_rows_only_while_they_colour() {
        let ops = room_view_id("!ops:example");
        let notify_moves = |entry: &str, active: bool| {
            let mut state = keyed_state();
            state
                .settings
                .update(|settings| {
                    settings.nick_colors.list.push(nick_color_entry(entry));
                    settings.nick_colors.active = active;
                })
                .unwrap();
            let before = view_rows_key(&state, &ops);
            state
                .settings
                .update(|settings| {
                    settings
                        .notify
                        .list
                        .add("@bob:example".to_owned(), None, false);
                })
                .unwrap();
            view_rows_key(&state, &ops) != before
        };
        assert!(!notify_moves("* 4", true));
        assert!(notify_moves("* 4 notify", true));
        assert!(!notify_moves("* 4 notify", false));
        // Turning colours off moves the key; while off, the list does not.
        let mut state = keyed_state();
        state
            .settings
            .update(|settings| settings.nick_colors.list.push(nick_color_entry("* 4")))
            .unwrap();
        let on = view_rows_key(&state, &ops);
        state
            .settings
            .update(|settings| settings.nick_colors.active = false)
            .unwrap();
        let off = view_rows_key(&state, &ops);
        assert_ne!(off, on);
        state
            .settings
            .update(|settings| settings.nick_colors.list.push(nick_color_entry("* 5")))
            .unwrap();
        assert_eq!(view_rows_key(&state, &ops), off);
    }

    #[test]
    fn sync_views_rebuilds_only_the_changed_room() {
        unsafe {
            with_test_frame(600, 400, |hwnd| {
                state_mut(hwnd).model.rooms = vec![
                    room("!a:example", "A", false),
                    room("!b:example", "B", false),
                ];
                sync_views_and_tree(hwnd).unwrap();
                let builds = |room_id| state_ref(hwnd).refresh_counters.room_row_builds(room_id);
                let (a, b) = (builds("!a:example"), builds("!b:example"));
                assert!(a > 0 && b > 0, "every window is built once");
                let total = state_ref(hwnd).refresh_counters.timeline_row_builds();

                sync_views_and_tree(hwnd).unwrap();
                assert_eq!(
                    state_ref(hwnd).refresh_counters.timeline_row_builds(),
                    total
                );

                apply_core_event(state_mut(hwnd), timeline("!b:example", "hello")).unwrap();
                sync_views_and_tree(hwnd).unwrap();
                assert_eq!(builds("!a:example"), a);
                assert_eq!(builds("!b:example"), b + 1);
                assert_eq!(
                    state_ref(hwnd).refresh_counters.timeline_row_builds(),
                    total + 1
                );
            });
        }
    }

    #[test]
    fn a_command_ack_is_counted_and_rebuilds_nothing() {
        unsafe {
            with_test_frame(600, 400, |hwnd| {
                state_mut(hwnd).model.rooms = vec![room("!a:example", "A", false)];
                sync_views_and_tree(hwnd).unwrap();
                let counters = |hwnd| {
                    let counters = &state_ref(hwnd).refresh_counters;
                    (
                        counters.timeline_row_builds(),
                        counters.rows_wrapped(),
                        counters.treebar_rebuilds(),
                        counters.switchbar_rebuilds(),
                    )
                };
                let before = counters(hwnd);
                let generation = state_ref(hwnd).event_generation;

                let (sender, _receiver) = inert_command_sender();
                let id = sender.send(Command::StartSync).unwrap();
                apply_events_and_refresh(hwnd, vec![Event::CommandHandled(id)]).unwrap();
                assert!(state_ref(hwnd).core_answers.is_answered(id));
                assert_eq!(state_ref(hwnd).core_answers.count(), 1);
                assert_eq!(state_ref(hwnd).event_generation, generation + 1);
                assert_eq!(counters(hwnd), before);
            });
        }
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
            room_views: RoomViews::from_iter([("!alpha:example".to_owned(), room_view)]),
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
    fn stale_thread_cleanup_takes_only_the_gone_thread() {
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
        assert_eq!(removed[0].child, 30 as HWND);
        // The successor is activated by the caller, not guessed here.
        assert_eq!(
            state.active_view,
            thread_view_id("!alpha:example", "$thread-root")
        );
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
        assert_eq!(removed[0].child, 20 as HWND);
        assert_eq!(
            state.active_view,
            thread_view_id("!alpha:example", "$thread-root")
        );
        assert!(state.views.contains_key(&ViewId::Status));
        assert!(!state.hwnd_views.contains_key(&20));
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
        // Only at frame teardown is the active window unregistered, and
        // nothing follows it there.
        assert_eq!(state.active_view, ViewId::Room("!beta:example".to_owned()));
    }

    fn ops_bob_dev() -> Vec<RoomSummary> {
        vec![
            room("!ops:example", "Ops", false),
            query("!bob:example", "Bob", "@bob:example"),
            room("!dev:example", "Dev", false),
        ]
    }

    /// A connected frame with Ops, a query with Bob and Dev, a line each
    /// and unread, with a sender whose commands wait. Ops was entered,
    /// then Dev, so MDI's next child after Dev is Ops; no commands are
    /// recorded yet.
    unsafe fn with_rooms_frame(test: impl FnOnce(HWND)) {
        with_test_frame(600, 400, |hwnd| {
            let (sender, commands) = inert_command_sender();
            std::mem::forget(commands);
            let state = state_mut(hwnd);
            state.command_sender = Some(sender);
            state.model.connection = ConnectionState::Syncing;
            state.model.rooms = ops_bob_dev();
            sync_views_and_tree(hwnd).unwrap();
            let mut events = Vec::new();
            for (index, room_id) in ["!ops:example", "!bob:example", "!dev:example"]
                .into_iter()
                .enumerate()
            {
                events.push(timeline_with_transaction_id(
                    room_id,
                    &format!("${index}"),
                    None,
                    "hi",
                    1_700_000_000_000 + index as u64 * 1_000,
                ));
                events.push(room_read_state(room_id, None, None, false, 1, 0, 0));
            }
            apply_events_and_refresh(hwnd, events).unwrap();
            activate_view(hwnd, &room_view_id("!ops:example"), None).unwrap();
            activate_view(hwnd, &room_view_id("!dev:example"), None).unwrap();
            state_mut(hwnd).recorded_commands.clear();
            test(hwnd);
        });
    }

    fn recorded_read_commands(state: &WindowState) -> Vec<String> {
        state
            .recorded_commands
            .iter()
            .filter_map(|command| match command {
                Command::MarkRoomRead(request) => {
                    Some(format!("receipt {} {}", request.room_id, request.event_id))
                }
                Command::SetReadMarker(request) => {
                    Some(format!("marker {} {}", request.room_id, request.event_id))
                }
                _ => None,
            })
            .collect()
    }

    /// Whether `view_id`'s window was entered and not left since.
    unsafe fn line_marker_sight(hwnd: HWND, view_id: &ViewId) -> line_marker::MarkerSight {
        let content = state_ref(hwnd).views[view_id].content;
        timeline_state_ref(view_timeline(view_id, content)).line_marker_sight
    }

    unsafe fn has_focus(hwnd: HWND, view_id: &ViewId) -> bool {
        IsChild(state_ref(hwnd).views[view_id].child, GetFocus()) != 0
    }

    #[test]
    fn the_window_after_a_gone_active_room_is_entered_and_left() {
        unsafe {
            with_rooms_frame(|hwnd| {
                let ops = room_view_id("!ops:example");
                // Ops has news since it was left.
                apply_events_and_refresh(
                    hwnd,
                    vec![
                        timeline_with_transaction_id(
                            "!ops:example",
                            "$ops-news",
                            None,
                            "news",
                            1_700_000_010_000,
                        ),
                        room_read_state("!ops:example", None, None, false, 1, 0, 0),
                    ],
                )
                .unwrap();

                let mut rooms = ops_bob_dev();
                rooms.pop();
                apply_events_and_refresh(hwnd, vec![Event::SyncedRooms(rooms)]).unwrap();

                let state = state_ref(hwnd);
                assert!(!state.views.contains_key(&room_view_id("!dev:example")));
                assert_eq!(
                    state.active_view,
                    resolve_active_view(
                        &room_view_id("!dev:example"),
                        None,
                        &state.model,
                        &state.closed_queries
                    )
                );
                assert_eq!(state.active_view, ops);
                assert_eq!(
                    line_marker_sight(hwnd, &ops),
                    line_marker::MarkerSight::Unseen
                );
                assert_eq!(
                    recorded_read_commands(state),
                    vec!["receipt !ops:example $ops-news"]
                );
                assert!(has_focus(hwnd, &ops));

                // Ops had no rule: leaving it moves its marker, once.
                activate_view(hwnd, &ViewId::Status, None).unwrap();
                assert_eq!(
                    recorded_read_commands(state_ref(hwnd)),
                    vec![
                        "receipt !ops:example $ops-news",
                        "marker !ops:example $ops-news"
                    ]
                );
            });
        }
    }

    #[test]
    fn a_gone_thread_window_activates_its_room() {
        unsafe {
            with_rooms_frame(|hwnd| {
                let ops = room_view_id("!ops:example");
                let thread = thread_view_id("!ops:example", "$0");
                apply_events_and_refresh(
                    hwnd,
                    vec![thread_timeline("!ops:example", "$reply", "$0", "re")],
                )
                .unwrap();
                // MDI's next child after the thread window is Dev.
                activate_view(hwnd, &thread, None).unwrap();
                state_mut(hwnd).recorded_commands.clear();

                state_mut(hwnd)
                    .model
                    .room_views
                    .get_mut("!ops:example")
                    .unwrap()
                    .threads
                    .clear();
                sync_views_and_tree(hwnd).unwrap();

                assert!(!state_ref(hwnd).views.contains_key(&thread));
                assert_eq!(state_ref(hwnd).active_view, ops);
                assert_eq!(
                    line_marker_sight(hwnd, &ops),
                    line_marker::MarkerSight::Unseen
                );
                assert!(has_focus(hwnd, &ops));
            });
        }
    }

    #[test]
    fn a_joined_room_is_entered_and_the_window_before_it_left() {
        unsafe {
            with_rooms_frame(|hwnd| {
                let ops = room_view_id("!ops:example");
                let new = room_view_id("!new:example");
                activate_view(hwnd, &ops, None).unwrap();
                state_mut(hwnd).recorded_commands.clear();

                let mut rooms = ops_bob_dev();
                rooms.push(room("!new:example", "New", false));
                apply_events_and_refresh(
                    hwnd,
                    vec![
                        Event::SyncedRooms(rooms),
                        Event::RoomJoined(mirx_core::RoomJoined {
                            room_id: "!new:example".to_owned(),
                            alias: None,
                        }),
                    ],
                )
                .unwrap();

                assert_eq!(state_ref(hwnd).active_view, new);
                // Ops had no rule: leaving it moved its marker.
                assert_eq!(
                    recorded_read_commands(state_ref(hwnd)),
                    vec!["marker !ops:example $0"]
                );
                assert_eq!(
                    line_marker_sight(hwnd, &new),
                    line_marker::MarkerSight::Unseen
                );
                assert_eq!(
                    line_marker_sight(hwnd, &ops),
                    line_marker::MarkerSight::Away
                );
                assert!(has_focus(hwnd, &new));
            });
        }
    }

    #[test]
    fn a_join_and_its_leave_in_one_batch_keep_the_active_window() {
        unsafe {
            with_rooms_frame(|hwnd| {
                let dev = room_view_id("!dev:example");
                let mut rooms = ops_bob_dev();
                rooms.push(room("!x:example", "X", false));
                apply_events_and_refresh(
                    hwnd,
                    vec![
                        Event::SyncedRooms(rooms),
                        Event::RoomJoined(mirx_core::RoomJoined {
                            room_id: "!x:example".to_owned(),
                            alias: None,
                        }),
                        Event::SyncedRooms(ops_bob_dev()),
                    ],
                )
                .unwrap();

                assert_eq!(state_ref(hwnd).active_view, dev);
                assert!(
                    !state_ref(hwnd)
                        .views
                        .contains_key(&room_view_id("!x:example"))
                );
                assert_eq!(
                    recorded_read_commands(state_ref(hwnd)),
                    Vec::<String>::new()
                );
                assert_eq!(
                    line_marker_sight(hwnd, &dev),
                    line_marker::MarkerSight::Unseen
                );
            });
        }
    }
}
