//! The Sessions dialog (Tools > Sessions...): your sessions in a list,
//! with Verify..., Rename..., Sign Out... for the selected one, Refresh and
//! Close. Its content is `sessions`'; this module shows it.
//!
//! It opens empty and asks the core for your sessions; the answer (or the
//! error) comes while its modal loop runs, through `take_core_answer`, as
//! do the errors of what its buttons send. A rename or sign-out the server
//! made reads the list again.

use std::ptr::{null, null_mut};

use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::Controls::{
    LVN_ITEMCHANGED, LVS_REPORT, LVS_SHOWSELALWAYS, LVS_SINGLESEL, NMHDR,
};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::EnableWindow;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    BS_DEFPUSHBUTTON, BS_PUSHBUTTON, DM_GETDEFID, DefWindowProcW, IDCANCEL, MoveWindow,
    SetWindowTextW, WM_CLOSE, WM_COMMAND, WM_NCCREATE, WM_NCDESTROY, WM_NOTIFY, WNDPROC, WS_CHILD,
    WS_EX_CLIENTEDGE, WS_TABSTOP, WS_VISIBLE,
};

use mirx_core::{Command, DeviceSummary, Event, Operation, UserDevices};

use super::command_runner::{own_user_id, sign_out_session};
use super::commands::SlashCommand;
use super::dialogs::{
    DialogMetrics, create_modal_dialog, default_button, dialog_text_size, end_modal_dialog,
};
use super::dpi::Dpi;
use super::list_view::{
    LIST_VIEW_CLASS_NAME, fill_list, select_row, selected_row, set_up_report_list,
};
use super::sessions::{
    EMPTY_NAME_TEXT, READING_TEXT, RenameOutcome, rename_outcome, session_actions, session_rows,
    status_text, verify_command,
};
use super::timestamp::LocalTimeFn;
use super::window_data::{self, WindowData};
use super::{
    BUTTON_CLASS_NAME, STATIC_CLASS_NAME, create_child_window, enqueue_command, run_local_command,
    run_modal_dialog, show_single_field_dialog, state_mut, state_ref, submit_device_rename,
    wide_null,
};

pub(super) const SESSIONS_DIALOG_CLASS_NAME: &str = "mirxWin32SessionsDialog";

const SESSIONS_LIST_ID: u16 = 7701;
const SESSIONS_VERIFY_ID: u16 = 7702;
const SESSIONS_RENAME_ID: u16 = 7703;
const SESSIONS_SIGN_OUT_ID: u16 = 7704;
const SESSIONS_REFRESH_ID: u16 = 7705;
const SESSIONS_STATUS_ID: u16 = 7706;

// The layout's lengths at 96 dpi.
const LIST_WIDTH: i32 = 420;
const LIST_HEIGHT: i32 = 140;
const SESSION_COLUMN_WIDTH: i32 = 100;
const NAME_COLUMN_WIDTH: i32 = 130;
const VERIFIED_COLUMN_WIDTH: i32 = 60;

struct Controls {
    list: HWND,
    verify: HWND,
    rename: HWND,
    sign_out: HWND,
    refresh: HWND,
    status: HWND,
}

/// The dialog's state, owned by `run_sessions_dialog` and lent to the
/// window procedure while the window exists.
struct DialogState {
    /// The frame, whose core the dialog asks.
    frame: HWND,
    own_user_id: String,
    local_time: LocalTimeFn,
    controls: Controls,
    /// The last list, which a selected row is a session of; `None` until
    /// the first.
    devices: Option<UserDevices>,
    /// What Verify... runs once the dialog is gone: the verification's
    /// emoji and answers are in Status and the menu bar, which the dialog
    /// would keep disabled.
    verify: Option<SlashCommand>,
}

unsafe fn dialog_state(hwnd: HWND) -> &'static mut DialogState {
    window_data::get(hwnd).expect("the sessions dialog has its state")
}

/// Shows your sessions until Close, or until Verify... closes it and
/// starts that verification.
pub(super) unsafe fn run_sessions_dialog(hwnd: HWND) -> Result<(), String> {
    // The window the dialog was opened from, where Verify's lines print.
    let window = state_ref(hwnd).active_view.clone();
    let state = state_mut(hwnd);
    let own_user_id = own_user_id(&state.model)?.to_owned();
    let caption = format!("Sessions - {own_user_id}");
    let local_time = state.local_time;
    enqueue_command(state, Command::ListUserDevices(own_user_id.clone()))?;
    let mut dialog_state = Box::new(WindowData::new(DialogState {
        frame: hwnd,
        own_user_id,
        local_time,
        controls: Controls {
            list: null_mut(),
            verify: null_mut(),
            rename: null_mut(),
            sign_out: null_mut(),
            refresh: null_mut(),
            status: null_mut(),
        },
        devices: None,
        verify: None,
    }));
    let dialog = create_modal_dialog(
        hwnd,
        SESSIONS_DIALOG_CLASS_NAME,
        &caption,
        (&mut *dialog_state as *mut WindowData<DialogState>).cast(),
        |dialog| create_controls(dialog),
    )?;
    state_mut(hwnd).sessions_dialog = Some(dialog);
    run_modal_dialog(hwnd, dialog);
    state_mut(hwnd).sessions_dialog = None;
    match dialog_state.verify.take() {
        Some(verify) => run_local_command(hwnd, &window, verify),
        None => Ok(()),
    }
}

unsafe fn create_controls(dialog: HWND) -> Result<(), String> {
    let instance = GetModuleHandleW(null());
    let dpi = Dpi::of_window(dialog);
    let DialogMetrics {
        margin,
        gap,
        button_width,
        button_height,
        ..
    } = DialogMetrics::at(dpi);
    let control = |class: &str, text: &str, style: u32, ex_style: u32, id: u16| {
        create_child_window(
            dialog,
            class,
            text,
            WS_CHILD | WS_VISIBLE | style,
            ex_style,
            id,
            instance,
            null_mut(),
        )
    };
    let place = |control: HWND, x: i32, y: i32, width: i32, height: i32| {
        MoveWindow(control, x, y, width, height, 0);
        control
    };
    let state = dialog_state(dialog);
    let (list_width, list_height) = (dpi.px(LIST_WIDTH), dpi.px(LIST_HEIGHT));
    let content_width = list_width + gap + button_width;

    state.controls.list = place(
        control(
            LIST_VIEW_CLASS_NAME,
            "",
            WS_TABSTOP | LVS_REPORT | LVS_SINGLESEL | LVS_SHOWSELALWAYS,
            WS_EX_CLIENTEDGE,
            SESSIONS_LIST_ID,
        )?,
        margin,
        margin,
        list_width,
        list_height,
    );
    // The list's inside, less its sunken edges and a scroll bar.
    let inner_width = list_width - dpi.px(24);
    let (session, name, verified) = (
        dpi.px(SESSION_COLUMN_WIDTH),
        dpi.px(NAME_COLUMN_WIDTH),
        dpi.px(VERIFIED_COLUMN_WIDTH),
    );
    set_up_report_list(
        state.controls.list,
        &[
            ("Session", session),
            ("Name", name),
            ("Verified", verified),
            ("Last seen", inner_width - session - name - verified),
        ],
    )?;

    // The row buttons beside the list, greyed until a row is selected;
    // Refresh until the first answer.
    let button_x = margin + list_width + gap;
    let mut button_y = margin;
    let mut button = |text: &str, id: u16| {
        let button = place(
            control(
                BUTTON_CLASS_NAME,
                text,
                WS_TABSTOP | BS_PUSHBUTTON as u32,
                0,
                id,
            )?,
            button_x,
            button_y,
            button_width,
            button_height,
        );
        EnableWindow(button, 0);
        button_y += button_height + gap;
        Ok::<_, String>(button)
    };
    state.controls.verify = button("&Verify...", SESSIONS_VERIFY_ID)?;
    state.controls.rename = button("&Rename...", SESSIONS_RENAME_ID)?;
    state.controls.sign_out = button("&Sign Out...", SESSIONS_SIGN_OUT_ID)?;
    state.controls.refresh = button("Re&fresh", SESSIONS_REFRESH_ID)?;

    // The status: two lines, as an error may need.
    let mut y = margin + list_height + gap;
    let (_, line_height) = dialog_text_size("Xg", content_width);
    state.controls.status = place(
        control(STATIC_CLASS_NAME, READING_TEXT, 0, 0, SESSIONS_STATUS_ID)?,
        margin,
        y,
        content_width,
        2 * line_height,
    );
    y += 2 * line_height + gap;

    // Close is `IDCANCEL`, which `IsDialogMessageW` sends for Esc, and the
    // default button.
    place(
        control(
            BUTTON_CLASS_NAME,
            "Close",
            WS_TABSTOP | BS_DEFPUSHBUTTON as u32,
            0,
            IDCANCEL as u16,
        )?,
        margin + content_width - button_width,
        y,
        button_width,
        button_height,
    );
    Ok(())
}

/// What the core says that is the open dialog's: your sessions fill the
/// list; the errors of its read and of its renames and sign-outs go in the
/// status line; a rename or sign-out the server made reads the list again.
pub(super) unsafe fn take_core_answer(dialog: HWND, event: &Event) -> Result<(), String> {
    let state = dialog_state(dialog);
    match event {
        Event::UserDevices(devices) if devices.user_id == state.own_user_id => {
            fill(state, devices.clone());
        }
        Event::Error(error) if error.operation == Operation::ListUserDevices => {
            set_text(state.controls.status, &error.message);
            // The user may try again, once connected.
            EnableWindow(state.controls.refresh, 1);
        }
        Event::Error(error)
            if matches!(
                error.operation,
                Operation::RenameDevice | Operation::SignOutDevice
            ) =>
        {
            set_text(state.controls.status, &error.message);
        }
        Event::DeviceRenamed { .. } | Event::DeviceSignedOut(_) => read_sessions(state)?,
        // The account page signs it out, and mirx is not told when.
        Event::DeviceSignOutApproval { device_id, .. } => set_text(
            state.controls.status,
            &format!("Sign out {device_id} on your account page, then Refresh"),
        ),
        _ => {}
    }
    Ok(())
}

/// Asks the core for the list again.
unsafe fn read_sessions(state: &DialogState) -> Result<(), String> {
    enqueue_command(
        state_mut(state.frame),
        Command::ListUserDevices(state.own_user_id.clone()),
    )?;
    set_text(state.controls.status, READING_TEXT);
    EnableWindow(state.controls.refresh, 0);
    Ok(())
}

/// Shows `devices`; the session that was selected stays selected while it
/// is listed.
unsafe fn fill(state: &mut DialogState, devices: UserDevices) {
    let selected = selected_device(state).map(|device| device.device_id.clone());
    let rows = session_rows(&devices.devices, state.local_time);
    let status = status_text(&devices);
    let reselected = selected.and_then(|device_id| {
        devices
            .devices
            .iter()
            .position(|device| device.device_id == device_id)
    });
    // Before the list changes: its notifications read the new sessions.
    state.devices = Some(devices);
    fill_list(state.controls.list, &rows);
    if let Some(row) = reselected {
        select_row(state.controls.list, row);
    }
    grey_row_buttons(state);
    set_text(state.controls.status, &status);
    EnableWindow(state.controls.refresh, 1);
}

unsafe fn selected_device(state: &DialogState) -> Option<&DeviceSummary> {
    selected_row(state.controls.list).map(|row| {
        &state
            .devices
            .as_ref()
            .expect("a row is a listed session")
            .devices[row]
    })
}

unsafe fn grey_row_buttons(state: &DialogState) {
    let actions = session_actions(selected_device(state));
    for (button, enabled) in [
        (state.controls.verify, actions.verify),
        (state.controls.rename, actions.rename),
        (state.controls.sign_out, actions.sign_out),
    ] {
        EnableWindow(button, enabled.into());
    }
}

unsafe fn handle_verify(dialog: HWND) {
    let state = dialog_state(dialog);
    let device = selected_device(state).expect("Verify is greyed without a selection");
    state.verify = Some(verify_command(&state.own_user_id, device));
    end_modal_dialog(dialog);
}

/// Rename... asks for the name in a dialog over this one.
unsafe fn handle_rename(dialog: HWND) -> Result<(), String> {
    let device = selected_device(dialog_state(dialog))
        .expect("Rename is greyed without a selection")
        .clone();
    let label = format!("Name for session {}:", device.device_id);
    let old_name = device.display_name.as_deref();
    let Some(typed) = show_single_field_dialog(
        dialog,
        "Rename Session",
        &label,
        old_name.unwrap_or_default(),
    )?
    else {
        return Ok(());
    };
    // Read again: answers came during the prompt's modal loop.
    let state = dialog_state(dialog);
    match rename_outcome(old_name, &typed) {
        RenameOutcome::Unchanged => {}
        RenameOutcome::Empty => set_text(state.controls.status, EMPTY_NAME_TEXT),
        RenameOutcome::Rename(name) => {
            let status = format!("Renaming {}...", device.device_id);
            submit_device_rename(state_mut(state.frame), device.device_id, name)?;
            set_text(state.controls.status, &status);
        }
    }
    Ok(())
}

/// Sign Out... confirms as `/devices delete` does, over this dialog.
unsafe fn handle_sign_out(dialog: HWND) -> Result<(), String> {
    let state = dialog_state(dialog);
    let frame = state.frame;
    let device_id = selected_device(state)
        .expect("Sign Out is greyed without a selection")
        .device_id
        .clone();
    let status = format!("Signing out {device_id}...");
    if sign_out_session(dialog, frame, device_id)? {
        set_text(dialog_state(dialog).controls.status, &status);
    }
    Ok(())
}

unsafe fn set_text(control: HWND, text: &str) {
    let text = wide_null(text);
    SetWindowTextW(control, text.as_ptr());
}

unsafe extern "system" fn window_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match message {
        WM_NCCREATE => {
            window_data::borrow_create_param::<DialogState>(hwnd, lparam);
            DefWindowProcW(hwnd, message, wparam, lparam)
        }
        WM_COMMAND => {
            let result = match (wparam & 0xffff) as u16 {
                id if id == IDCANCEL as u16 => {
                    end_modal_dialog(hwnd);
                    Ok(())
                }
                SESSIONS_VERIFY_ID => {
                    handle_verify(hwnd);
                    Ok(())
                }
                SESSIONS_RENAME_ID => handle_rename(hwnd),
                SESSIONS_SIGN_OUT_ID => handle_sign_out(hwnd),
                SESSIONS_REFRESH_ID => read_sessions(dialog_state(hwnd)),
                _ => return DefWindowProcW(hwnd, message, wparam, lparam),
            };
            // What could not be sent says so where the dialog's errors go.
            if let Err(error) = result {
                set_text(dialog_state(hwnd).controls.status, &error);
            }
            0
        }
        WM_NOTIFY => {
            let header = &*(lparam as *const NMHDR);
            if header.idFrom as u16 == SESSIONS_LIST_ID && header.code == LVN_ITEMCHANGED {
                grey_row_buttons(dialog_state(hwnd));
            }
            0
        }
        DM_GETDEFID => default_button(IDCANCEL as usize),
        // The close box: Close.
        WM_CLOSE => {
            end_modal_dialog(hwnd);
            0
        }
        WM_NCDESTROY => {
            window_data::detach::<DialogState>(hwnd);
            DefWindowProcW(hwnd, message, wparam, lparam)
        }
        _ => DefWindowProcW(hwnd, message, wparam, lparam),
    }
}

/// The dialog's class and procedure, registered with the other dialogs'.
pub(super) const DIALOG_CLASS: (&str, WNDPROC) = (SESSIONS_DIALOG_CLASS_NAME, Some(window_proc));

/// The automation harness's part of this module: what it reads and does
/// here for the protocol.
#[cfg(any(test, feature = "automation"))]
pub(super) mod automation {
    use super::*;
    use crate::automation::UiSessions;
    use crate::win32::list_view::automation::dump_list;
    use crate::win32::read_window_text;
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::IsWindowEnabled;

    /// The Sessions dialog as automation sees it, read back from its controls.
    pub(in crate::win32) unsafe fn dump(dialog: HWND) -> UiSessions {
        let state = dialog_state(dialog);
        let controls = &state.controls;
        let enabled = |control| IsWindowEnabled(control) != 0;
        let (rows, row_click_points) = dump_list(controls.list, 4);
        UiSessions {
            caption: read_window_text(dialog),
            status: read_window_text(controls.status),
            rows,
            row_click_points,
            selected: selected_row(controls.list),
            verify_enabled: enabled(controls.verify),
            rename_enabled: enabled(controls.rename),
            sign_out_enabled: enabled(controls.sign_out),
            refresh_enabled: enabled(controls.refresh),
        }
    }
}
