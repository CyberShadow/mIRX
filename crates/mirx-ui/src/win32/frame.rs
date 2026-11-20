//! The frame window: its class, controls, layout, status bar and message
//! loop.

use super::*;

const FRAME_CLASS_NAME: &str = "mirxWin32Frame";

const MDI_CLASS_NAME: &str = "MDICLIENT";

const TREE_ID: u16 = 1001;
const MDI_ID: u16 = 1002;
const STATUS_BAR_ID: u16 = 1004;
const SWITCHBAR_ID: u16 = 1005;

pub(super) fn init_common_controls() -> Result<(), String> {
    let controls = INITCOMMONCONTROLSEX {
        dwSize: size_of::<INITCOMMONCONTROLSEX>() as u32,
        dwICC: ICC_TREEVIEW_CLASSES | ICC_BAR_CLASSES | ICC_LISTVIEW_CLASSES,
    };
    let success = unsafe { InitCommonControlsEx(&controls) };
    if success == 0 {
        return Err(last_os_error("InitCommonControlsEx"));
    }
    Ok(())
}

pub(super) fn register_frame_class(instance: *mut c_void) -> Result<(), String> {
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

pub(super) fn create_main_window(
    instance: *mut c_void,
    state: Box<WindowState>,
    frame_size: FrameSize,
) -> Result<HWND, String> {
    let class_name = wide_null(FRAME_CLASS_NAME);
    let title = wide_null(FRAME_TITLE);
    let mut state = window_data::CreateParam::new(*state);

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
            state.as_param(),
        )
    };

    if hwnd.is_null() {
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
            window_data::adopt_create_param::<WindowState>(hwnd, lparam);
            // Sets the window text from the CreateWindowExW title.
            DefWindowProcW(hwnd, message, wparam, lparam)
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
            if handle_switchbar_command(hwnd, wparam)
                || handle_window_key_command(hwnd, wparam)
                || handle_menu_command(hwnd, wparam).is_some()
            {
                0
            } else {
                default_window_proc(hwnd, message, wparam, lparam)
            }
        }
        WM_INITMENUPOPUP => {
            if let Err(message) = refresh_menu_popup(hwnd, wparam as HMENU) {
                show_error_message(hwnd, "Menu", &message);
            }
            0
        }
        WM_CONTEXTMENU if wparam as HWND == state_ref(hwnd).controls.tree => {
            if let Err(message) = show_navigation_room_context_menu(hwnd, lparam, None) {
                show_error_message(hwnd, "Room", &message);
            }
            0
        }
        WM_CONTEXTMENU => {
            let member_list = state_ref(hwnd)
                .views
                .get(&state_ref(hwnd).active_view)
                .map(|view| room_content_member_list(view.content));
            if member_list == Some(wparam as HWND) {
                if let Err(message) = show_member_context_menu(hwnd, wparam as HWND, lparam, None) {
                    show_error_message(hwnd, "Member", &message);
                }
                0
            } else {
                default_window_proc(hwnd, message, wparam, lparam)
            }
        }
        WM_CORE_EVENT => {
            handle_core_events(hwnd);
            0
        }
        WM_SYSCOMMAND if (wparam & 0xfff0) as u32 == SC_MINIMIZE => {
            minimize_frame(hwnd, message, wparam, lparam).unwrap_or_else(|message| {
                show_error_message(hwnd, "Tray", &message);
                0
            })
        }
        WM_TRAY_ICON => {
            if let Err(message) = on_tray_icon(hwnd, lparam) {
                show_error_message(hwnd, "Tray", &message);
            }
            0
        }
        message if message == taskbar_created_message() => {
            if let Err(message) = on_taskbar_created(hwnd) {
                show_error_message(hwnd, "Tray", &message);
            }
            0
        }
        WM_CONNECT_AT_STARTUP => {
            if let Err(message) = connect(hwnd, None) {
                show_error_message(hwnd, "Connect", &message);
            }
            0
        }
        WM_COPYDATA => handle_copydata(hwnd, lparam),
        WM_NOTIFY => {
            let header = &*(lparam as *const NMHDR);
            let control_id =
                u16::try_from(header.idFrom).expect("frame child control ids are 16-bit");
            match (control_id, header.code) {
                (TREE_ID, TVN_SELCHANGEDW) => {
                    handle_tree_selection(hwnd, &*(lparam as *const NMTREEVIEWW));
                    0
                }
                (TREE_ID, TVN_ITEMEXPANDEDW) => {
                    tree_expansion_changed(hwnd).expect("the treebar notifies");
                    0
                }
                (TREE_ID, NM_CUSTOMDRAW) => {
                    tree_custom_draw(state_ref(hwnd), &mut *(lparam as *mut NMTVCUSTOMDRAW))
                }
                (SWITCHBAR_ID, NM_CUSTOMDRAW) => {
                    switchbar_custom_draw(state_ref(hwnd), &mut *(lparam as *mut NMTBCUSTOMDRAW))
                }
                _ => default_window_proc(hwnd, message, wparam, lparam),
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
            remove_tray_icon(hwnd);
            let controls = &mut state_mut(hwnd).controls;
            // The menubar goes with the window; these hang in no menubar.
            for menu in [
                &mut controls.room_menu,
                &mut controls.message_menu,
                &mut controls.member_menu,
            ] {
                DestroyMenu(*menu);
                *menu = null_mut();
            }
            controls.room_notifications_menu = null_mut();
            DestroyAcceleratorTable(controls.accelerators);
            controls.accelerators = null_mut();
            if let Some(sender) = state_mut(hwnd).command_sender.as_ref() {
                let _ = sender.send(Command::Shutdown);
            }
            PostQuitMessage(0);
            0
        }
        WM_NCDESTROY => {
            if let Some(state) = window_data::get::<WindowState>(hwnd) {
                destroy_automation_state(state);
            }
            window_data::drop_owned::<WindowState>(hwnd);
            0
        }
        _ => default_window_proc(hwnd, message, wparam, lparam),
    }
}

unsafe fn create_controls(hwnd: HWND) -> Result<(), String> {
    let instance = GetModuleHandleW(null());
    let state = state_mut(hwnd);

    create_frame_menu(hwnd)?;

    state.controls.switchbar = CreateWindowExW(
        0,
        TOOLBARCLASSNAMEW,
        null(),
        // The frame lays the switchbar out. Without CCS_NORESIZE the toolbar
        // sizes itself to its ideal height and overlaps the MDI client's top
        // edge, painting over it whenever the switchbar repaints. Its
        // buttons wrap onto more rows on its own WM_SIZE (TBSTYLE_WRAPABLE);
        // `layout_controls` shows it as the settings say and sizes it to
        // its rows.
        WS_CHILD | TBSTYLE_FLAT | TBSTYLE_WRAPABLE | CCS_NORESIZE as u32 | CCS_NOPARENTALIGN as u32,
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
    set_switchbar_button_width(
        state.controls.switchbar,
        state.settings.settings().switchbar.buttons.pixels(),
    );
    // One line per label, cut with "..." when it is wider than its button.
    SendMessageW(state.controls.switchbar, TB_SETMAXTEXTROWS, 1, 0);
    let text_flags = DT_CENTER | DT_SINGLELINE | DT_END_ELLIPSIS;
    SendMessageW(
        state.controls.switchbar,
        TB_SETDRAWTEXTFLAGS,
        text_flags as WPARAM,
        text_flags as LPARAM,
    );

    // `layout_controls` shows it, as the settings say.
    state.controls.tree = create_child_window(
        hwnd,
        TREE_CLASS_NAME,
        "",
        // Buttons on the items with children, as mIRC's treebar; no lines,
        // and none on the root (Status).
        WS_CHILD | WS_BORDER | TVS_HASBUTTONS,
        0,
        TREE_ID,
        instance,
        null_mut(),
    )?;
    set_treebar_colors(state.controls.tree, &state.settings.settings().colors);
    state.controls.color_index = create_color_index(hwnd)?;

    let mut mdi_client: CLIENTCREATESTRUCT = zeroed();
    // No MDI window list in the Window menu: the switchbar is the window
    // list, as in mIRC.
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

/// Makes `scheme` the colours of every window and saves it. The new
/// brushes are in place before anything repaints; the old ones are
/// selected nowhere (they are only handed out while painting), so they go
/// at once.
pub(super) unsafe fn apply_color_scheme(hwnd: HWND, scheme: ColorScheme) -> Result<(), String> {
    use windows_sys::Win32::Graphics::Gdi::{
        RDW_ALLCHILDREN, RDW_ERASE, RDW_INVALIDATE, RedrawWindow,
    };
    let brushes = SchemeBrushes::create(&scheme);
    let state = state_mut(hwnd);
    state
        .settings
        .update(|settings| settings.colors = scheme.clone())?;
    drop(std::mem::replace(&mut state.scheme_brushes, brushes));
    set_treebar_colors(state.controls.tree, &scheme);
    // Every window's text area, editbox and nick list. Hidden and
    // minimized ones paint when they show.
    RedrawWindow(
        state.controls.mdi_client,
        null(),
        null_mut(),
        RDW_INVALIDATE | RDW_ERASE | RDW_ALLCHILDREN,
    );
    Ok(())
}

/// The treebar in the scheme's Treebar colours. Its selected item keeps
/// the system highlight, and activity colours its items' text over these.
pub(super) unsafe fn set_treebar_colors(tree: HWND, colors: &ColorScheme) {
    use windows_sys::Win32::UI::Controls::{TVM_SETBKCOLOR, TVM_SETTEXTCOLOR};
    SendMessageW(
        tree,
        TVM_SETBKCOLOR,
        0,
        colors.colorref(ColorItem::Treebar) as LPARAM,
    );
    SendMessageW(
        tree,
        TVM_SETTEXTCOLOR,
        0,
        colors.colorref(ColorItem::TreebarText) as LPARAM,
    );
}

/// Gives every switchbar button `width` pixels, whatever its label.
pub(super) unsafe fn set_switchbar_button_width(switchbar: HWND, width: i32) {
    // The minimum and the maximum width.
    SendMessageW(
        switchbar,
        TB_SETBUTTONWIDTH,
        0,
        pack_size_lparam(width, width),
    );
    // A wrapping toolbar starts a new row only once a button's bitmap would
    // pass the edge (comctl32's `TOOLBAR_WrapToolbar`), and the buttons have
    // no bitmap: a bitmap as wide as the button wraps the button itself, so
    // no row ends in a clipped button. 16 is the default bitmap height.
    SendMessageW(switchbar, TB_SETBITMAPSIZE, 0, pack_size_lparam(width, 16));
}

pub(super) unsafe fn layout_controls(hwnd: HWND) {
    let state = state_mut(hwnd);
    if state.controls.tree.is_null()
        || state.controls.mdi_client.is_null()
        || state.controls.status_bar.is_null()
        || state.controls.switchbar.is_null()
    {
        return;
    }

    let mut rect = RECT::default();
    GetClientRect(hwnd, &mut rect);

    let width = rect.right - rect.left;
    let height = rect.bottom - rect.top;
    // The bars are shown as the settings say. A hidden one is laid out
    // with no size, and the MDI client takes its place.
    let settings = state.settings.settings();
    let (show_switchbar, show_treebar) = (settings.switchbar.show, settings.treebar.show);
    ShowWindow(state.controls.switchbar, show_or_hide(show_switchbar));
    ShowWindow(state.controls.tree, show_or_hide(show_treebar));
    let tree_width = if show_treebar {
        TREE_WIDTH.min(width)
    } else {
        0
    };
    let switchbar_height = if show_switchbar {
        // The switchbar wraps its buttons on its own WM_SIZE, so it gets
        // the frame's width first and its height from the rows that gives.
        let mut switchbar_rect = RECT::default();
        GetWindowRect(state.controls.switchbar, &mut switchbar_rect);
        MoveWindow(
            state.controls.switchbar,
            0,
            0,
            width,
            switchbar_rect.bottom - switchbar_rect.top,
            1,
        );
        let rows = u32::try_from(SendMessageW(state.controls.switchbar, TB_GETROWS, 0, 0))
            .expect("a toolbar's row count is positive");
        let button_height =
            (SendMessageW(state.controls.switchbar, TB_GETBUTTONSIZE, 0, 0) as u32 >> 16) as i32;
        switchbar::switchbar_height(rows, button_height).min(height)
    } else {
        0
    };
    MoveWindow(state.controls.switchbar, 0, 0, width, switchbar_height, 1);
    // A status bar sizes and places itself at the bottom of its parent, so
    // the controls above it are laid out against its real height. Sizing it
    // to a guessed height instead left it overlapping them by a few pixels,
    // and which one painted last decided the snapshot.
    SendMessageW(state.controls.status_bar, WM_SIZE, 0, 0);
    let mut status_bar_rect = RECT::default();
    GetWindowRect(state.controls.status_bar, &mut status_bar_rect);
    let status_bar_height =
        (status_bar_rect.bottom - status_bar_rect.top).min((height - switchbar_height).max(0));
    let mdi_width = (width - tree_width).max(0);
    let mdi_height = (height - switchbar_height - status_bar_height).max(0);

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
}

fn show_or_hide(show: bool) -> SHOW_WINDOW_CMD {
    if show { SW_SHOW } else { SW_HIDE }
}

pub(super) fn frame_status_text(state: &WindowState) -> String {
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
        text.push_str(", ");
        text.push_str(e2ee_backup_health_summary(status));
        text.push_str(", cross-signing ");
        text.push_str(e2ee_cross_signing_label(status.cross_signing.as_ref()));
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

pub(super) unsafe fn refresh_status_bar(hwnd: HWND) -> Result<(), String> {
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

pub(super) fn message_loop() {
    let mut message = MSG::default();
    loop {
        let result = unsafe { GetMessageW(&mut message, null_mut(), 0, 0) };
        if result <= 0 {
            break;
        }

        unsafe {
            if let Some(frame) = message_frame(&message) {
                if TranslateAcceleratorW(frame, state_ref(frame).controls.accelerators, &message)
                    != 0
                {
                    continue;
                }
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

/// The frame that `hwnd`, one of its descendants, belongs to.
pub(super) unsafe fn frame_window_for(hwnd: HWND) -> HWND {
    let frame = GetAncestor(hwnd, GA_ROOT);
    assert!(
        window_data::get::<WindowState>(frame).is_some(),
        "window {hwnd:?} is in a frame"
    );
    frame
}

/// The frame whose accelerators apply to `message`: the frame or one of its
/// descendants is its target. Other windows of the thread (dialogs, menus,
/// the system's own) have none, nor have thread messages.
unsafe fn message_frame(message: &MSG) -> Option<HWND> {
    let root = GetAncestor(message.hwnd, GA_ROOT);
    window_class_is(root, FRAME_CLASS_NAME).then_some(root)
}

/// Whether `hwnd` is a window of class `class_name`; false for no window.
pub(super) unsafe fn window_class_is(hwnd: HWND, class_name: &str) -> bool {
    let mut buffer = [0u16; 256];
    let len = GetClassNameW(hwnd, buffer.as_mut_ptr(), buffer.len() as i32);
    buffer[..len as usize]
        .iter()
        .copied()
        .eq(class_name.encode_utf16())
}

pub(super) unsafe fn default_window_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    // Messages before WM_NCCREATE and after WM_NCDESTROY.
    let Some(state) = window_data::get::<WindowState>(hwnd) else {
        return DefWindowProcW(hwnd, message, wparam, lparam);
    };

    let mdi_client = state.controls.mdi_client;
    if mdi_client.is_null() {
        DefWindowProcW(hwnd, message, wparam, lparam)
    } else {
        DefFrameProcW(hwnd, mdi_client, message, wparam, lparam)
    }
}

#[cfg(test)]
mod tests {
    use windows_sys::Win32::UI::Controls::{TB_GETSTATE, TBSTATE_WRAP};
    use windows_sys::Win32::UI::WindowsAndMessaging::IsWindowVisible;

    use super::super::test_fixtures::*;
    use super::*;

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
            "e2ee status updated | e2ee: recovery incomplete, backup healthy, cross-signing complete"
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
            room_id: Some("!room:example".to_owned()),
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
            room_id: Some("!room:example".to_owned()),
            message: "message send failed".to_owned(),
        });

        assert_eq!(
            frame_status_text(&state),
            "verification accepted with @alice:example | verification: accepted flow-verify-123 from @alice:example | error: send text: message send failed"
        );
    }

    #[test]
    fn message_frame_is_the_frame_of_its_descendants_only() {
        unsafe {
            with_test_frame(600, 400, |hwnd| {
                let message_to = |target: HWND| MSG {
                    hwnd: target,
                    ..MSG::default()
                };
                let compose = state_ref(hwnd).views[&ViewId::Status].compose().edit;
                assert_eq!(message_frame(&message_to(compose)), Some(hwnd));
                assert_eq!(message_frame(&message_to(hwnd)), Some(hwnd));
                assert_eq!(message_frame(&message_to(null_mut())), None);

                register_dialog_classes(GetModuleHandleW(null())).unwrap();
                let dialog = create_about_dialog(hwnd).unwrap();
                let ok = GetDlgItem(dialog, ABOUT_DIALOG_OK_ID as i32);
                assert!(!ok.is_null());
                assert_eq!(message_frame(&message_to(ok)), None);
                assert_eq!(message_frame(&message_to(dialog)), None);
                DestroyWindow(dialog);
            });
        }
    }

    /// A frame child's rectangle in the frame's client coordinates.
    unsafe fn frame_child_layout(hwnd: HWND, child: HWND) -> UiRect {
        frame_child_rect(hwnd, child).expect("the child window is shown")
    }

    unsafe fn switchbar_button_height(switchbar: HWND) -> i32 {
        (SendMessageW(switchbar, TB_GETBUTTONSIZE, 0, 0) as u32 >> 16) as i32
    }

    #[test]
    fn the_switchbar_wraps_onto_rows_that_the_frame_lays_out() {
        unsafe {
            with_test_frame(600, 400, |hwnd| {
                let room_ids = (0..8)
                    .map(|index| format!("!room{index}:example"))
                    .collect::<Vec<_>>();
                for (index, room_id) in room_ids.iter().enumerate() {
                    state_mut(hwnd).model.rooms.push(room(
                        room_id,
                        &format!("Room {index}"),
                        false,
                    ));
                }
                sync_views_and_tree(hwnd).unwrap();

                let controls = &state_ref(hwnd).controls;
                let switchbar = controls.switchbar;
                let rows = SendMessageW(switchbar, TB_GETROWS, 0, 0) as u32;
                assert!(rows >= 2, "9 buttons of 120 px wrap in 600 px: {rows}");
                let bar = frame_child_layout(hwnd, switchbar);
                assert_eq!(
                    bar.height,
                    switchbar::switchbar_height(rows, switchbar_button_height(switchbar))
                );
                let mdi_client = frame_child_layout(hwnd, controls.mdi_client);
                assert_eq!(mdi_client.y, bar.y + bar.height);
                assert_eq!(frame_child_layout(hwnd, controls.tree).y, mdi_client.y);
                for index in 0..room_ids.len() + 1 {
                    let mut button = RECT::default();
                    SendMessageW(
                        switchbar,
                        TB_GETITEMRECT,
                        index,
                        &mut button as *mut RECT as LPARAM,
                    );
                    assert_eq!(
                        button.right - button.left,
                        state_ref(hwnd)
                            .settings
                            .settings()
                            .switchbar
                            .buttons
                            .pixels()
                    );
                    assert!(button.right <= bar.width, "button {index} is clipped");
                }

                // Activating a view checks its button and keeps the wrap
                // the layout set on the last button of each row.
                let wrapped = |hwnd: HWND| {
                    (0..room_ids.len() + 1)
                        .filter(|index| {
                            let command_id = SWITCHBAR_COMMAND_FIRST + index;
                            SendMessageW(
                                state_ref(hwnd).controls.switchbar,
                                TB_GETSTATE,
                                command_id,
                                0,
                            ) as u32
                                & TBSTATE_WRAP
                                != 0
                        })
                        .collect::<Vec<_>>()
                };
                let wrapped_before = wrapped(hwnd);
                assert_eq!(wrapped_before.len(), rows as usize - 1);
                activate_view(hwnd, &room_view_id(&room_ids[7]), None).unwrap();
                assert_eq!(wrapped(hwnd), wrapped_before);

                state_mut(hwnd).model.rooms.truncate(1);
                sync_views_and_tree(hwnd).unwrap();
                let switchbar = state_ref(hwnd).controls.switchbar;
                assert_eq!(SendMessageW(switchbar, TB_GETROWS, 0, 0), 1);
                let bar = frame_child_layout(hwnd, switchbar);
                assert_eq!(bar.height, switchbar::SWITCHBAR_ROW_HEIGHT);
                assert_eq!(
                    frame_child_layout(hwnd, state_ref(hwnd).controls.mdi_client).y,
                    bar.height
                );
            });
        }
    }

    #[test]
    fn hidden_bars_give_their_place_to_the_mdi_client() {
        unsafe {
            with_test_frame(800, 600, |hwnd| {
                let bar_command = |bar, command| {
                    run_slash_command(hwnd, SlashCommand::Bar { bar, command }).unwrap()
                };
                let mut client = RECT::default();
                GetClientRect(hwnd, &mut client);
                let controls = &state_ref(hwnd).controls;
                let (switchbar, tree, mdi_client) =
                    (controls.switchbar, controls.tree, controls.mdi_client);
                let shown = dump_frame_layout(hwnd);

                assert_eq!(
                    bar_command(Bar::Switchbar, BarCommand::Off),
                    Some("switchbar: off".to_owned())
                );
                assert!(!state_ref(hwnd).settings.settings().switchbar.show);
                let layout = dump_frame_layout(hwnd);
                assert_eq!(layout.switchbar, None);
                assert_eq!(layout.mdi_client.y, 0);
                assert_eq!(layout.treebar.unwrap().y, 0);

                // A hidden switchbar still mirrors the views.
                state_mut(hwnd)
                    .model
                    .rooms
                    .push(room("!room:example", "Room", false));
                sync_views_and_tree(hwnd).unwrap();
                assert_eq!(
                    SendMessageW(switchbar, TB_BUTTONCOUNT, 0, 0) as usize,
                    state_ref(hwnd).switchbar_views.len()
                );
                assert_eq!(state_ref(hwnd).switchbar_views.len(), 2);

                assert_eq!(
                    bar_command(Bar::Treebar, BarCommand::Off),
                    Some("treebar: off".to_owned())
                );
                let layout = dump_frame_layout(hwnd);
                assert_eq!(layout.treebar, None);
                assert_eq!(
                    layout.mdi_client,
                    UiRect {
                        x: 0,
                        y: 0,
                        width: client.right,
                        height: layout.mdi_client.height,
                    }
                );
                assert_eq!(IsWindowVisible(tree), 0);

                bar_command(Bar::Switchbar, BarCommand::On);
                bar_command(Bar::Treebar, BarCommand::On);
                let layout = dump_frame_layout(hwnd);
                assert_eq!(layout, shown);
                assert_eq!(
                    frame_child_layout(hwnd, mdi_client).y,
                    layout.switchbar.unwrap().height
                );
            });
        }
    }
}
