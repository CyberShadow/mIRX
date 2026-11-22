//! Tools > Aliases... (Alt+A): mIRC's Scripts Editor at its Aliases tab,
//! reduced to what mirx has. One editbox holds the alias lines of
//! `aliases.ini` in the text font, unwrapped as mIRC's editor is; OK
//! checks them with the file's own parser and keeps the dialog open on a
//! wrong line, which it selects.

use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows_sys::Win32::UI::Controls::{EM_LINEINDEX, EM_LINELENGTH, EM_SCROLLCARET, EM_SETSEL};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{GetFocus, SetFocus};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    DM_GETDEFID, DefWindowProcW, ES_AUTOHSCROLL, ES_AUTOVSCROLL, ES_MULTILINE, ES_WANTRETURN,
    GetDlgItem, GetNextDlgTabItem, IDCANCEL, MB_ICONWARNING, MB_OK, MessageBoxW, SendMessageW,
    WM_CLOSE, WM_COMMAND, WM_NCCREATE, WM_NCDESTROY, WM_NEXTDLGCTL, WM_SETFONT, WNDPROC,
    WS_HSCROLL, WS_TABSTOP, WS_VSCROLL,
};

use super::aliases::{Aliases, SourceError};
use super::dialogs::{
    DIALOG_EDIT_EX_STYLE, DialogColumn, create_modal_dialog, default_button, end_modal_dialog,
};
use super::dpi::Dpi;
use super::window_data::{self, WindowData};
use super::{
    EDIT_CLASS_NAME, read_window_text, run_modal_dialog, set_local_command_status, state_mut,
    state_ref, wide_null,
};

pub(super) const ALIASES_DIALOG_CLASS_NAME: &str = "mirxWin32AliasesDialog";
const ALIASES_DIALOG_TITLE: &str = "Aliases - aliases.ini";
/// The title of the box that names a wrong line.
const ALIASES_ERROR_TITLE: &str = "mirx Aliases";
pub(super) const ALIASES_DIALOG_EDIT_ID: u16 = 5030;
pub(super) const ALIASES_DIALOG_OK_ID: usize = 5031;
pub(super) const ALIASES_DIALOG_CANCEL_ID: usize = 5032;
/// The editbox's size at 96 dpi.
const EDIT_WIDTH: i32 = 560;
const EDIT_HEIGHT: i32 = 340;
const EDIT_STYLE: u32 = WS_TABSTOP
    | WS_VSCROLL
    | WS_HSCROLL
    | (ES_MULTILINE | ES_WANTRETURN | ES_AUTOVSCROLL | ES_AUTOHSCROLL) as u32;
/// What `IsDialogMessageW` sends for Esc on a button.
const ESC_CANCEL_ID: usize = IDCANCEL as usize;

/// The dialog's state, owned by `run_aliases_dialog` and lent to the
/// window procedure while the window exists.
struct DialogState {
    /// The aliases OK accepted.
    saved: Option<Aliases>,
}

/// Tools > Aliases...: shows the aliases until OK or Cancel; OK saves them
/// at once.
pub(super) unsafe fn run_aliases_dialog(hwnd: HWND) -> Result<(), String> {
    let text = state_ref(hwnd).aliases.aliases().source().join("\r\n");
    let mut state = Box::new(WindowData::new(DialogState { saved: None }));
    let dialog = create_modal_dialog(
        hwnd,
        ALIASES_DIALOG_CLASS_NAME,
        ALIASES_DIALOG_TITLE,
        (&mut *state as *mut WindowData<DialogState>).cast(),
        |dialog| {
            let dpi = Dpi::of_window(dialog);
            let mut column = DialogColumn::new(dialog, dpi.px(EDIT_WIDTH));
            let margin = column.metrics.margin;
            let width = column.width;
            column.add(
                (
                    EDIT_CLASS_NAME,
                    &text,
                    EDIT_STYLE,
                    DIALOG_EDIT_EX_STYLE,
                    ALIASES_DIALOG_EDIT_ID,
                ),
                (margin, width, dpi.px(EDIT_HEIGHT)),
                0,
            )?;
            column.buttons(&[
                (ALIASES_DIALOG_OK_ID, "OK"),
                (ALIASES_DIALOG_CANCEL_ID, "Cancel"),
            ])
        },
    )?;
    let edit = GetDlgItem(dialog, ALIASES_DIALOG_EDIT_ID.into());
    // mIRC's editor is in a fixed font: the text font, which the dialog
    // shares with the frame at the frame's DPI. The frame owns it.
    SendMessageW(
        edit,
        WM_SETFONT,
        state_ref(hwnd).text_font.hfont() as WPARAM,
        1,
    );
    SetFocus(edit);
    // Focus selects all of a dialog's edit: the caret starts at the top.
    SendMessageW(edit, EM_SETSEL, 0, 0);
    run_modal_dialog(hwnd, dialog);
    let Some(aliases) = state.saved.take() else {
        return Ok(());
    };
    state_mut(hwnd).aliases.replace(aliases)?;
    set_local_command_status(hwnd, "aliases saved".to_owned())
}

/// OK: the text's aliases, or the box naming its first wrong line, which
/// is selected.
unsafe fn accept(dialog: HWND) {
    let edit = GetDlgItem(dialog, ALIASES_DIALOG_EDIT_ID.into());
    let text = read_window_text(edit);
    match Aliases::from_source(text.lines().map(str::to_owned).collect()) {
        Ok(aliases) => {
            window_data::get::<DialogState>(dialog)
                .expect("the aliases dialog has its state")
                .saved = Some(aliases);
            end_modal_dialog(dialog);
        }
        Err(SourceError { line, reason }) => {
            let message = wide_null(&format!("Line {}: {reason}", line + 1));
            let title = wide_null(ALIASES_ERROR_TITLE);
            MessageBoxW(
                dialog,
                message.as_ptr(),
                title.as_ptr(),
                MB_OK | MB_ICONWARNING,
            );
            select_line(edit, line);
        }
    }
}

/// Selects the 0-based `line` of the editbox, scrolled into view.
unsafe fn select_line(edit: HWND, line: usize) {
    let start = SendMessageW(edit, EM_LINEINDEX, line, 0);
    let length = SendMessageW(edit, EM_LINELENGTH, start as WPARAM, 0);
    SetFocus(edit);
    SendMessageW(edit, EM_SETSEL, start as WPARAM, start + length);
    SendMessageW(edit, EM_SCROLLCARET, 0, 0);
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
            ALIASES_DIALOG_OK_ID => {
                accept(hwnd);
                0
            }
            ALIASES_DIALOG_CANCEL_ID | ESC_CANCEL_ID => {
                end_modal_dialog(hwnd);
                0
            }
            _ => DefWindowProcW(hwnd, message, wparam, lparam),
        },
        DM_GETDEFID => default_button(ALIASES_DIALOG_OK_ID),
        // The multi-line editbox takes every key and passes Tab on as a
        // dialog manager's WM_NEXTDLGCTL: to the control in `wparam`, or
        // to the previous (`wparam` set) or next tab stop.
        WM_NEXTDLGCTL => {
            let next = match lparam {
                0 => GetNextDlgTabItem(hwnd, GetFocus(), (wparam != 0).into()),
                _ => wparam as HWND,
            };
            SetFocus(next);
            0
        }
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
pub(super) const DIALOG_CLASS: (&str, WNDPROC) = (ALIASES_DIALOG_CLASS_NAME, Some(window_proc));
