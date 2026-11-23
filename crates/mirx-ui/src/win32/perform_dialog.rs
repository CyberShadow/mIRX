//! Options > Connect > Options > Perform...: mIRC's Perform dialog, its
//! `Enable perform on connect` box over the `Perform commands:` editbox,
//! one command line per line. mIRC also picks a network; mirx has one
//! account, so one list.
//!
//! OK checks the lines with `mirx.ini`'s own parser and keeps the dialog
//! open on a wrong line, which it selects, as the Aliases dialog does. It
//! hands what it accepted to the Options dialog, whose OK saves it.

use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows_sys::Win32::UI::Controls::EM_SETSEL;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::SetFocus;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    DM_GETDEFID, DefWindowProcW, ES_AUTOHSCROLL, ES_AUTOVSCROLL, ES_MULTILINE, ES_WANTRETURN,
    GetDlgItem, IDCANCEL, MB_ICONWARNING, MB_OK, MessageBoxW, SendMessageW, WM_CLOSE, WM_COMMAND,
    WM_NCCREATE, WM_NCDESTROY, WM_NEXTDLGCTL, WNDPROC, WS_HSCROLL, WS_TABSTOP, WS_VSCROLL,
};

use super::dialogs::{
    DIALOG_EDIT_EX_STYLE, DialogColumn, create_modal_dialog, default_button, end_modal_dialog,
    is_checked, next_dialog_control, select_edit_line, set_check,
};
use super::dpi::Dpi;
use super::perform::PerformSetting;
use super::window_data::{self, WindowData};
use super::{EDIT_CLASS_NAME, read_window_text, run_modal_dialog, wide_null};

pub(super) const PERFORM_DIALOG_CLASS_NAME: &str = "mirxWin32PerformDialog";
const PERFORM_DIALOG_TITLE: &str = "Perform";
/// The title of the box that names a wrong line.
const PERFORM_ERROR_TITLE: &str = "mirx Perform";
pub(super) const PERFORM_DIALOG_ENABLED_ID: u16 = 6150;
pub(super) const PERFORM_DIALOG_EDIT_ID: u16 = 6151;
pub(super) const PERFORM_DIALOG_OK_ID: usize = 6152;
pub(super) const PERFORM_DIALOG_CANCEL_ID: usize = 6153;
/// The editbox's size at 96 dpi.
const EDIT_WIDTH: i32 = 400;
const EDIT_HEIGHT: i32 = 200;
const EDIT_STYLE: u32 = WS_TABSTOP
    | WS_VSCROLL
    | WS_HSCROLL
    | (ES_MULTILINE | ES_WANTRETURN | ES_AUTOVSCROLL | ES_AUTOHSCROLL) as u32;
/// What `IsDialogMessageW` sends for Esc on a button.
const ESC_CANCEL_ID: usize = IDCANCEL as usize;

/// The dialog's state, owned by `run_perform_dialog` and lent to the
/// window procedure while the window exists.
struct DialogState {
    /// What OK accepted.
    accepted: Option<PerformSetting>,
}

/// Shows `current` over `owner`, the Options dialog, until OK or Cancel:
/// what OK accepted, or `None`.
pub(super) unsafe fn run_perform_dialog(
    owner: HWND,
    current: &PerformSetting,
) -> Result<Option<PerformSetting>, String> {
    let text = current
        .lines()
        .iter()
        .map(|line| line.text())
        .collect::<Vec<_>>()
        .join("\r\n");
    let mut state = Box::new(WindowData::new(DialogState { accepted: None }));
    let dialog = create_modal_dialog(
        owner,
        PERFORM_DIALOG_CLASS_NAME,
        PERFORM_DIALOG_TITLE,
        (&mut *state as *mut WindowData<DialogState>).cast(),
        |dialog| {
            let dpi = Dpi::of_window(dialog);
            let mut column = DialogColumn::new(dialog, dpi.px(EDIT_WIDTH));
            let enabled =
                column.checkbox(PERFORM_DIALOG_ENABLED_ID, "Enable perform on connect")?;
            set_check(enabled, current.enabled);
            column.label("Perform commands:", 0, 0)?;
            let margin = column.metrics.margin;
            let width = column.width;
            column.add(
                (
                    EDIT_CLASS_NAME,
                    &text,
                    EDIT_STYLE,
                    DIALOG_EDIT_EX_STYLE,
                    PERFORM_DIALOG_EDIT_ID,
                ),
                (margin, width, dpi.px(EDIT_HEIGHT)),
                0,
            )?;
            column.buttons(&[
                (PERFORM_DIALOG_OK_ID, "OK"),
                (PERFORM_DIALOG_CANCEL_ID, "Cancel"),
            ])
        },
    )?;
    let edit = GetDlgItem(dialog, PERFORM_DIALOG_EDIT_ID.into());
    SetFocus(edit);
    // Focus selects all of a dialog's edit: the caret starts at the top.
    SendMessageW(edit, EM_SETSEL, 0, 0);
    run_modal_dialog(owner, dialog);
    Ok(state.accepted.take())
}

/// OK: the box and the lines, or the box naming the first wrong line,
/// which is selected.
unsafe fn accept(dialog: HWND) {
    let edit = GetDlgItem(dialog, PERFORM_DIALOG_EDIT_ID.into());
    let text = read_window_text(edit);
    match PerformSetting::parse_lines(text.lines()) {
        Ok(lines) => {
            let enabled = is_checked(GetDlgItem(dialog, PERFORM_DIALOG_ENABLED_ID.into()));
            window_data::get::<DialogState>(dialog)
                .expect("the perform dialog has its state")
                .accepted = Some(PerformSetting::new(enabled, lines));
            end_modal_dialog(dialog);
        }
        Err((line, reason)) => {
            let message = wide_null(&format!("Line {}: {reason}", line + 1));
            let title = wide_null(PERFORM_ERROR_TITLE);
            MessageBoxW(
                dialog,
                message.as_ptr(),
                title.as_ptr(),
                MB_OK | MB_ICONWARNING,
            );
            select_edit_line(edit, line);
        }
    }
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
        WM_COMMAND => match wparam & 0xffff {
            PERFORM_DIALOG_OK_ID => {
                accept(hwnd);
                0
            }
            PERFORM_DIALOG_CANCEL_ID | ESC_CANCEL_ID => {
                end_modal_dialog(hwnd);
                0
            }
            _ => DefWindowProcW(hwnd, message, wparam, lparam),
        },
        DM_GETDEFID => default_button(PERFORM_DIALOG_OK_ID),
        // The multi-line editbox takes every key and passes Tab on as a
        // dialog manager's WM_NEXTDLGCTL.
        WM_NEXTDLGCTL => next_dialog_control(hwnd, wparam, lparam),
        // Esc in the editbox, and the close box: Cancel.
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
pub(super) const DIALOG_CLASS: (&str, WNDPROC) = (PERFORM_DIALOG_CLASS_NAME, Some(window_proc));
