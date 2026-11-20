//! The sign-in, E2EE and About modal dialogs, and the chrome every mirx
//! dialog shares, Options' too: the class, the font, the metrics, the
//! placement over the owner and the modal loop.

use super::*;

use windows_sys::Win32::Foundation::ERROR_CLASS_ALREADY_EXISTS;
use windows_sys::Win32::Graphics::Gdi::{
    COLOR_BTNFACE, DEFAULT_GUI_FONT, DT_CALCRECT, DT_WORDBREAK, DrawTextW, GetStockObject,
};
use windows_sys::Win32::System::SystemServices::SS_CENTER;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::GetFocus;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    AdjustWindowRectEx, DC_HASDEFID, DM_GETDEFID, EnumChildWindows, GW_OWNER, GetNextDlgTabItem,
    GetWindow, IDCANCEL, IsDialogMessageW, SWP_NOACTIVATE, SWP_NOZORDER, SetWindowPos,
    WINDOW_STYLE, WM_NEXTDLGCTL, WNDPROC,
};
use windows_sys::core::BOOL;

const LOGIN_DIALOG_CLASS_NAME: &str = "mirxWin32LoginDialog";
const RECOVER_E2EE_DIALOG_CLASS_NAME: &str = "mirxWin32RecoverE2eeDialog";
const SETUP_E2EE_DIALOG_CLASS_NAME: &str = "mirxWin32SetupE2eeDialog";
const RECOVERY_KEY_HANDOFF_DIALOG_CLASS_NAME: &str = "mirxWin32RecoveryKeyHandoffDialog";
pub(super) const SINGLE_FIELD_DIALOG_CLASS_NAME: &str = "mirxWin32SingleFieldDialog";
pub(super) const ABOUT_DIALOG_CLASS_NAME: &str = "mirxWin32AboutDialog";

const LOGIN_DIALOG_HOME_SERVER_ID: u16 = 5001;
const LOGIN_DIALOG_USERNAME_ID: u16 = 5002;
const LOGIN_DIALOG_PASSWORD_ID: u16 = 5003;
const LOGIN_DIALOG_DEVICE_ID: u16 = 5004;
const LOGIN_DIALOG_OK_ID: usize = 5005;
const LOGIN_DIALOG_CANCEL_ID: usize = 5006;
const RECOVER_E2EE_DIALOG_SECRET_ID: u16 = 5007;
const RECOVER_E2EE_DIALOG_OK_ID: usize = 5008;
const RECOVER_E2EE_DIALOG_CANCEL_ID: usize = 5009;
const SETUP_E2EE_DIALOG_PASSPHRASE_ID: u16 = 5013;
const SETUP_E2EE_DIALOG_WAIT_ID: u16 = 5014;
const SETUP_E2EE_DIALOG_OK_ID: usize = 5015;
const SETUP_E2EE_DIALOG_CANCEL_ID: usize = 5016;
const RECOVERY_KEY_HANDOFF_DIALOG_KEY_ID: u16 = 5017;
const RECOVERY_KEY_HANDOFF_DIALOG_ACK_ID: usize = 5018;
const SINGLE_FIELD_DIALOG_EDIT_ID: u16 = 5010;
const SINGLE_FIELD_DIALOG_OK_ID: usize = 5011;
const SINGLE_FIELD_DIALOG_CANCEL_ID: usize = 5012;
/// The About box's lines, top to bottom, with the control ids tests read.
const ABOUT_DIALOG_LINES: [(u16, &str); 5] = [
    (5019, concat!("mirx ", env!("CARGO_PKG_VERSION"))),
    (5020, "mIRC, but Matrix."),
    (5021, "A native Win32 Matrix client"),
    (5022, "End-to-end encryption by matrix-rust-sdk"),
    (5023, "Licensed under the Apache License 2.0"),
];
pub(super) const ABOUT_DIALOG_OK_ID: usize = 5024;
/// What `IsDialogMessageW` sends for Esc: every dialog takes it as its
/// Cancel button.
const ESC_CANCEL_ID: usize = IDCANCEL as usize;

/// The answer to `DM_GETDEFID`: the dialog's default button, which
/// `IsDialogMessageW` presses for Enter.
const fn default_button(id: usize) -> LRESULT {
    ((DC_HASDEFID as LRESULT) << 16) | id as LRESULT
}

/// The dialogs' content widths: what their labels and fields need.
const LOGIN_DIALOG_WIDTH: i32 = 280;
const SINGLE_FIELD_DIALOG_WIDTH: i32 = 260;
const RECOVERY_KEY_HANDOFF_DIALOG_WIDTH: i32 = 320;

// Windows' dialog metrics at 96 dpi, in pixels: what the dialog units of
// mIRC's MS Shell Dlg 8 resource dialogs come to.
/// Between the dialog's edge and its controls.
pub(super) const DIALOG_MARGIN: i32 = 11;
/// Between related controls.
pub(super) const DIALOG_GAP: i32 = 7;
/// Between a label and the field under it.
pub(super) const DIALOG_LABEL_GAP: i32 = 3;
pub(super) const DIALOG_BUTTON_WIDTH: i32 = 75;
pub(super) const DIALOG_BUTTON_HEIGHT: i32 = 23;
pub(super) const DIALOG_EDIT_HEIGHT: i32 = 21;
pub(super) const DIALOG_CHECKBOX_HEIGHT: i32 = 17;
/// An edit's style: a native dialog edit has the sunken client edge and no
/// flat border.
pub(super) const DIALOG_EDIT_STYLE: u32 = WS_TABSTOP | ES_AUTOHSCROLL as u32;
pub(super) const DIALOG_EDIT_EX_STYLE: u32 = WS_EX_CLIENTEDGE;

const DIALOG_STYLE: WINDOW_STYLE = WS_OVERLAPPED | WS_CAPTION | WS_SYSMENU;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct LoginForm {
    pub(super) server: String,
    pub(super) username: String,
    pub(super) password: String,
    pub(super) device_display_name: String,
}

struct LoginDialogState {
    result: Arc<Mutex<Option<LoginForm>>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct RecoverE2eeForm {
    pub(super) recovery_secret: String,
}

struct RecoverE2eeDialogState {
    result: Arc<Mutex<Option<RecoverE2eeForm>>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct SetupE2eeRecoveryForm {
    pub(super) passphrase: Option<String>,
    pub(super) wait_for_backups_to_upload: bool,
}

struct SetupE2eeRecoveryDialogState {
    result: Arc<Mutex<Option<SetupE2eeRecoveryForm>>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct SubmittedSetupE2eeRecovery {
    pub(super) passphrase_redacted: bool,
    pub(super) wait_for_backups_to_upload: bool,
}

pub(super) struct SingleFieldDialogState {
    pub(super) result: Arc<Mutex<Option<String>>>,
}

/// Every mirx dialog's class and window procedure.
const DIALOG_CLASSES: [(&str, WNDPROC); 8] = [
    (LOGIN_DIALOG_CLASS_NAME, Some(login_dialog_window_proc)),
    (
        RECOVER_E2EE_DIALOG_CLASS_NAME,
        Some(recover_e2ee_dialog_window_proc),
    ),
    (
        SETUP_E2EE_DIALOG_CLASS_NAME,
        Some(setup_e2ee_dialog_window_proc),
    ),
    (
        RECOVERY_KEY_HANDOFF_DIALOG_CLASS_NAME,
        Some(recovery_key_handoff_dialog_window_proc),
    ),
    (
        SINGLE_FIELD_DIALOG_CLASS_NAME,
        Some(single_field_dialog_window_proc),
    ),
    (ABOUT_DIALOG_CLASS_NAME, Some(about_dialog_window_proc)),
    (
        options_dialog::OPTIONS_DIALOG_CLASS_NAME,
        Some(options_dialog::window_proc),
    ),
    colors_dialog::DIALOG_CLASS,
];

/// Registers every dialog class on the button face, the colour labels and
/// buttons paint their own backgrounds in. A second registration, as each
/// unit test makes, finds the classes registered.
pub(super) fn register_dialog_classes(instance: *mut c_void) -> Result<(), String> {
    for (name, window_proc) in DIALOG_CLASSES {
        let class_name = wide_null(name);
        let window_class = WNDCLASSEXW {
            cbSize: size_of::<WNDCLASSEXW>() as u32,
            style: 0,
            lpfnWndProc: window_proc,
            cbClsExtra: 0,
            cbWndExtra: 0,
            hInstance: instance,
            hIcon: null_mut(),
            hCursor: unsafe { LoadCursorW(null_mut(), IDC_ARROW) },
            hbrBackground: (COLOR_BTNFACE as isize + 1) as HBRUSH,
            lpszMenuName: null(),
            lpszClassName: class_name.as_ptr(),
            hIconSm: null_mut(),
        };
        if unsafe { RegisterClassExW(&window_class) } == 0
            && unsafe { GetLastError() } != ERROR_CLASS_ALREADY_EXISTS
        {
            return Err(last_os_error(&format!("RegisterClassExW {name}")));
        }
    }
    colors_dialog::register_control_classes(instance)
}

/// Creates a dialog of `class` over `owner` and shows it, as a dialog
/// manager shows a resource dialog: `create_controls` fills it, every
/// control takes the dialog font, and the dialog fits its controls with the
/// margin on every side, centred on `owner`. The caller runs its modal loop.
pub(super) unsafe fn create_modal_dialog(
    owner: HWND,
    class: &str,
    title: &str,
    param: *const c_void,
    create_controls: impl FnOnce(HWND) -> Result<(), String>,
) -> Result<HWND, String> {
    let class_name = wide_null(class);
    let title = wide_null(title);
    let dialog = CreateWindowExW(
        0,
        class_name.as_ptr(),
        title.as_ptr(),
        DIALOG_STYLE,
        0,
        0,
        0,
        0,
        owner,
        null_mut(),
        GetModuleHandleW(null()),
        param,
    );
    if dialog.is_null() {
        return Err(last_os_error(&format!("CreateWindowExW {class}")));
    }
    if let Err(error) = create_controls(dialog) {
        DestroyWindow(dialog);
        return Err(error);
    }
    apply_dialog_font(dialog);

    let (right, bottom) = controls_extent(dialog);
    let mut frame = RECT {
        left: 0,
        top: 0,
        right: right + DIALOG_MARGIN,
        bottom: bottom + DIALOG_MARGIN,
    };
    if AdjustWindowRectEx(&mut frame, DIALOG_STYLE, 0, 0) == 0 {
        DestroyWindow(dialog);
        return Err(last_os_error("AdjustWindowRectEx dialog"));
    }
    let (width, height) = (frame.right - frame.left, frame.bottom - frame.top);
    let mut owner_rect = RECT::default();
    GetWindowRect(owner, &mut owner_rect);
    let (x, y) = centered_origin(&owner_rect, width, height);
    SetWindowPos(
        dialog,
        null_mut(),
        x,
        y,
        width,
        height,
        SWP_NOZORDER | SWP_NOACTIVATE,
    );
    ShowWindow(dialog, SW_SHOW);
    Ok(dialog)
}

/// Where a `width` × `height` window centred on `owner` goes.
fn centered_origin(owner: &RECT, width: i32, height: i32) -> (i32, i32) {
    (
        owner.left + (owner.right - owner.left - width) / 2,
        owner.top + (owner.bottom - owner.top - height) / 2,
    )
}

/// Gives every control of `dialog` the dialog font (MS Shell Dlg 8, which
/// mIRC's dialogs use); a control left alone draws in the bold System
/// font. A stock object, so nothing owns or deletes it.
unsafe fn apply_dialog_font(dialog: HWND) {
    unsafe extern "system" fn set_font(control: HWND, font: LPARAM) -> BOOL {
        SendMessageW(control, WM_SETFONT, font as WPARAM, 0);
        1
    }
    EnumChildWindows(
        dialog,
        Some(set_font),
        GetStockObject(DEFAULT_GUI_FONT) as LPARAM,
    );
}

/// The right and bottom edges of `dialog`'s controls, shown or not, in its
/// client coordinates.
pub(super) unsafe fn controls_extent(dialog: HWND) -> (i32, i32) {
    unsafe extern "system" fn extend(control: HWND, extent: LPARAM) -> BOOL {
        let (dialog, right, bottom) = &mut *(extent as *mut (HWND, i32, i32));
        let mut rect = RECT::default();
        GetWindowRect(control, &mut rect);
        MapWindowPoints(null_mut(), *dialog, (&mut rect as *mut RECT).cast(), 2);
        *right = (*right).max(rect.right);
        *bottom = (*bottom).max(rect.bottom);
        1
    }
    let mut extent = (dialog, 0, 0);
    EnumChildWindows(
        dialog,
        Some(extend),
        &mut extent as *mut (HWND, i32, i32) as LPARAM,
    );
    (extent.1, extent.2)
}

/// `text`'s size in the dialog font, wrapped as a label `wrap_width` wide
/// wraps it.
pub(super) unsafe fn dialog_text_size(text: &str, wrap_width: i32) -> (i32, i32) {
    let dc = GetDC(null_mut());
    let old_font = SelectObject(dc, GetStockObject(DEFAULT_GUI_FONT));
    let text = text.encode_utf16().collect::<Vec<_>>();
    let mut rect = RECT {
        left: 0,
        top: 0,
        right: wrap_width,
        bottom: 0,
    };
    DrawTextW(
        dc,
        text.as_ptr(),
        text.len() as i32,
        &mut rect,
        DT_CALCRECT | DT_WORDBREAK,
    );
    SelectObject(dc, old_font);
    ReleaseDC(null_mut(), dc);
    (rect.right, rect.bottom)
}

/// Stacks a simple dialog's controls in one column, a label over each
/// field and the buttons in a row under them, as mIRC's small dialogs do.
struct DialogColumn {
    dialog: HWND,
    width: i32,
    /// Where the next control goes.
    y: i32,
}

impl DialogColumn {
    fn new(dialog: HWND, width: i32) -> Self {
        Self {
            dialog,
            width,
            y: DIALOG_MARGIN,
        }
    }

    /// Adds a control at `x` on the next row, and leaves `gap` under it.
    unsafe fn add(
        &mut self,
        (class, text, style, ex_style, id): (&str, &str, u32, u32, u16),
        (x, width, height): (i32, i32, i32),
        gap: i32,
    ) -> Result<HWND, String> {
        let control = create_child_window(
            self.dialog,
            class,
            text,
            WS_CHILD | WS_VISIBLE | style,
            ex_style,
            id,
            GetModuleHandleW(null()),
            null_mut(),
        )?;
        MoveWindow(control, x, self.y, width, height, 0);
        self.y += height + gap;
        Ok(control)
    }

    /// A label as tall as its text wrapped to the column; `style` aligns it.
    unsafe fn label(&mut self, text: &str, id: u16, style: u32) -> Result<HWND, String> {
        let (_, height) = dialog_text_size(text, self.width);
        self.add(
            (STATIC_CLASS_NAME, text, style, 0, id),
            (DIALOG_MARGIN, self.width, height),
            DIALOG_LABEL_GAP,
        )
    }

    unsafe fn edit(&mut self, id: u16, text: &str, style: u32) -> Result<HWND, String> {
        self.add(
            (
                EDIT_CLASS_NAME,
                text,
                DIALOG_EDIT_STYLE | style,
                DIALOG_EDIT_EX_STYLE,
                id,
            ),
            (DIALOG_MARGIN, self.width, DIALOG_EDIT_HEIGHT),
            DIALOG_GAP,
        )
    }

    unsafe fn checkbox(&mut self, id: u16, text: &str) -> Result<HWND, String> {
        self.add(
            (
                BUTTON_CLASS_NAME,
                text,
                WS_TABSTOP | BS_AUTOCHECKBOX as u32,
                0,
                id,
            ),
            (DIALOG_MARGIN, self.width, DIALOG_CHECKBOX_HEIGHT),
            DIALOG_GAP,
        )
    }

    /// The buttons, right-aligned in a row a margin under the fields; the
    /// first is the default.
    unsafe fn buttons(self, buttons: &[(usize, &str)]) -> Result<(), String> {
        let row_width = buttons.len() as i32 * (DIALOG_BUTTON_WIDTH + DIALOG_GAP) - DIALOG_GAP;
        let x = DIALOG_MARGIN + self.width - row_width;
        self.button_row(x, buttons)
    }

    /// One default button, centred a margin under the fields.
    unsafe fn centred_button(self, id: usize, text: &str) -> Result<(), String> {
        let x = DIALOG_MARGIN + (self.width - DIALOG_BUTTON_WIDTH) / 2;
        self.button_row(x, &[(id, text)])
    }

    unsafe fn button_row(mut self, mut x: i32, buttons: &[(usize, &str)]) -> Result<(), String> {
        self.y += DIALOG_MARGIN - DIALOG_GAP;
        for (index, (id, text)) in buttons.iter().enumerate() {
            let style = match index {
                0 => BS_DEFPUSHBUTTON,
                _ => BS_PUSHBUTTON,
            };
            let button = create_child_window(
                self.dialog,
                BUTTON_CLASS_NAME,
                text,
                WS_CHILD | WS_VISIBLE | WS_TABSTOP | style as u32,
                0,
                *id as u16,
                GetModuleHandleW(null()),
                null_mut(),
            )?;
            MoveWindow(
                button,
                x,
                self.y,
                DIALOG_BUTTON_WIDTH,
                DIALOG_BUTTON_HEIGHT,
                0,
            );
            x += DIALOG_BUTTON_WIDTH + DIALOG_GAP;
        }
        Ok(())
    }
}

unsafe extern "system" fn login_dialog_window_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match message {
        WM_NCCREATE => {
            window_data::adopt_create_param::<LoginDialogState>(hwnd, lparam);
            // Sets the window text from the CreateWindowExW title.
            DefWindowProcW(hwnd, message, wparam, lparam)
        }
        WM_COMMAND => {
            let command_id = wparam & 0xffff;
            match command_id {
                LOGIN_DIALOG_OK_ID => {
                    let state = login_dialog_state(hwnd);
                    let field = |id: u16| read_window_text(GetDlgItem(hwnd, id.into()));
                    let form = LoginForm {
                        server: field(LOGIN_DIALOG_HOME_SERVER_ID),
                        username: field(LOGIN_DIALOG_USERNAME_ID),
                        password: field(LOGIN_DIALOG_PASSWORD_ID),
                        device_display_name: field(LOGIN_DIALOG_DEVICE_ID),
                    };
                    // Like mIRC's dialogs, a form that cannot work stays
                    // open with the reason.
                    if let Err(message) = login_server(&form.server, &form.username)
                        .and_then(|_| login_method(&form.username, &form.password))
                    {
                        show_error_message(hwnd, "Select Server", &message);
                        return 0;
                    }
                    *state
                        .result
                        .lock()
                        .expect("login dialog result mutex poisoned") = Some(form);
                    end_modal_dialog(hwnd);
                    0
                }
                LOGIN_DIALOG_CANCEL_ID | ESC_CANCEL_ID => {
                    end_modal_dialog(hwnd);
                    0
                }
                _ => DefWindowProcW(hwnd, message, wparam, lparam),
            }
        }
        DM_GETDEFID => default_button(LOGIN_DIALOG_OK_ID),
        WM_CLOSE => {
            end_modal_dialog(hwnd);
            0
        }
        WM_NCDESTROY => {
            window_data::drop_owned::<LoginDialogState>(hwnd);
            0
        }
        _ => DefWindowProcW(hwnd, message, wparam, lparam),
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
            window_data::adopt_create_param::<RecoverE2eeDialogState>(hwnd, lparam);
            // Sets the window text from the CreateWindowExW title.
            DefWindowProcW(hwnd, message, wparam, lparam)
        }
        WM_COMMAND => {
            let command_id = wparam & 0xffff;
            match command_id {
                RECOVER_E2EE_DIALOG_OK_ID => {
                    let state = recover_e2ee_dialog_state(hwnd);
                    let recovery_secret =
                        read_window_text(GetDlgItem(hwnd, RECOVER_E2EE_DIALOG_SECRET_ID.into()));
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
                    end_modal_dialog(hwnd);
                    0
                }
                RECOVER_E2EE_DIALOG_CANCEL_ID | ESC_CANCEL_ID => {
                    clear_recover_e2ee_dialog_secret(hwnd);
                    end_modal_dialog(hwnd);
                    0
                }
                _ => DefWindowProcW(hwnd, message, wparam, lparam),
            }
        }
        DM_GETDEFID => default_button(RECOVER_E2EE_DIALOG_OK_ID),
        WM_CLOSE => {
            clear_recover_e2ee_dialog_secret(hwnd);
            end_modal_dialog(hwnd);
            0
        }
        WM_NCDESTROY => {
            window_data::drop_owned::<RecoverE2eeDialogState>(hwnd);
            0
        }
        _ => DefWindowProcW(hwnd, message, wparam, lparam),
    }
}

unsafe extern "system" fn setup_e2ee_dialog_window_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match message {
        WM_NCCREATE => {
            window_data::adopt_create_param::<SetupE2eeRecoveryDialogState>(hwnd, lparam);
            // Sets the window text from the CreateWindowExW title.
            DefWindowProcW(hwnd, message, wparam, lparam)
        }
        WM_COMMAND => match wparam & 0xffff {
            SETUP_E2EE_DIALOG_OK_ID => {
                let state = setup_e2ee_dialog_state(hwnd);
                let passphrase_edit = GetDlgItem(hwnd, SETUP_E2EE_DIALOG_PASSPHRASE_ID.into());
                let passphrase = read_window_text(passphrase_edit);
                let wait_for_backups_to_upload = SendMessageW(
                    GetDlgItem(hwnd, SETUP_E2EE_DIALOG_WAIT_ID.into()),
                    BM_GETCHECK,
                    0,
                    0,
                ) == 1;
                set_window_text(passphrase_edit, "");
                *state
                    .result
                    .lock()
                    .expect("setup e2ee dialog result mutex poisoned") =
                    Some(SetupE2eeRecoveryForm {
                        passphrase: (!passphrase.is_empty()).then_some(passphrase),
                        wait_for_backups_to_upload,
                    });
                end_modal_dialog(hwnd);
                0
            }
            SETUP_E2EE_DIALOG_CANCEL_ID | ESC_CANCEL_ID => {
                clear_setup_e2ee_dialog_passphrase(hwnd);
                end_modal_dialog(hwnd);
                0
            }
            _ => DefWindowProcW(hwnd, message, wparam, lparam),
        },
        DM_GETDEFID => default_button(SETUP_E2EE_DIALOG_OK_ID),
        WM_CLOSE => {
            clear_setup_e2ee_dialog_passphrase(hwnd);
            end_modal_dialog(hwnd);
            0
        }
        WM_NCDESTROY => {
            window_data::drop_owned::<SetupE2eeRecoveryDialogState>(hwnd);
            0
        }
        _ => DefWindowProcW(hwnd, message, wparam, lparam),
    }
}

/// The recovery-key handoff keeps no state: the key is in its edit until
/// the dialog closes, which clears it.
unsafe extern "system" fn recovery_key_handoff_dialog_window_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match message {
        WM_COMMAND
            if matches!(
                wparam & 0xffff,
                RECOVERY_KEY_HANDOFF_DIALOG_ACK_ID | ESC_CANCEL_ID
            ) =>
        {
            clear_recovery_key_handoff_dialog_key(hwnd);
            end_modal_dialog(hwnd);
            0
        }
        DM_GETDEFID => default_button(RECOVERY_KEY_HANDOFF_DIALOG_ACK_ID),
        // The key's multi-line edit takes every key and passes Tab, and
        // Enter's move to the default button, on as a dialog manager's
        // WM_NEXTDLGCTL: to the control in `wparam`, or to the previous
        // (`wparam` set) or next tab stop.
        WM_NEXTDLGCTL => {
            let next = match lparam {
                0 => GetNextDlgTabItem(hwnd, GetFocus(), (wparam != 0).into()),
                _ => wparam as HWND,
            };
            SetFocus(next);
            0
        }
        WM_CLOSE => {
            clear_recovery_key_handoff_dialog_key(hwnd);
            end_modal_dialog(hwnd);
            0
        }
        _ => DefWindowProcW(hwnd, message, wparam, lparam),
    }
}

unsafe extern "system" fn single_field_dialog_window_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match message {
        WM_NCCREATE => {
            window_data::adopt_create_param::<SingleFieldDialogState>(hwnd, lparam);
            // Sets the window text from the CreateWindowExW title.
            DefWindowProcW(hwnd, message, wparam, lparam)
        }
        WM_COMMAND => {
            let command_id = wparam & 0xffff;
            match command_id {
                SINGLE_FIELD_DIALOG_OK_ID => {
                    let state = single_field_dialog_state(hwnd);
                    *state
                        .result
                        .lock()
                        .expect("single-field dialog result mutex poisoned") = Some(
                        read_window_text(GetDlgItem(hwnd, SINGLE_FIELD_DIALOG_EDIT_ID.into())),
                    );
                    end_modal_dialog(hwnd);
                    0
                }
                SINGLE_FIELD_DIALOG_CANCEL_ID | ESC_CANCEL_ID => {
                    end_modal_dialog(hwnd);
                    0
                }
                _ => DefWindowProcW(hwnd, message, wparam, lparam),
            }
        }
        DM_GETDEFID => default_button(SINGLE_FIELD_DIALOG_OK_ID),
        WM_CLOSE => {
            end_modal_dialog(hwnd);
            0
        }
        WM_NCDESTROY => {
            window_data::drop_owned::<SingleFieldDialogState>(hwnd);
            0
        }
        _ => DefWindowProcW(hwnd, message, wparam, lparam),
    }
}

/// The About box keeps no state: DefWindowProcW destroys it on WM_CLOSE.
/// Its OK is the default button and has the focus, so Enter presses it.
unsafe extern "system" fn about_dialog_window_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match message {
        WM_COMMAND if matches!(wparam & 0xffff, ABOUT_DIALOG_OK_ID | ESC_CANCEL_ID) => {
            end_modal_dialog(hwnd);
            0
        }
        _ => DefWindowProcW(hwnd, message, wparam, lparam),
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) unsafe fn create_child_window(
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

pub(super) const fn pack_size_lparam(width: i32, height: i32) -> LPARAM {
    (((height as u32) << 16) | (width as u32 & 0xffff)) as LPARAM
}

/// File > Select Server... while signed out: the sign-in form, with the
/// Server field filled from `/server <server>`.
unsafe fn create_login_dialog(
    owner: HWND,
    server: &str,
    result: Arc<Mutex<Option<LoginForm>>>,
) -> Result<HWND, String> {
    let mut state = window_data::CreateParam::new(LoginDialogState { result });
    create_modal_dialog(
        owner,
        LOGIN_DIALOG_CLASS_NAME,
        "Select Server",
        state.as_param(),
        |dialog| {
            let mut column = DialogColumn::new(dialog, LOGIN_DIALOG_WIDTH);
            column.label(
                "Server (matrix.org, or leave empty for a full user ID)",
                0,
                0,
            )?;
            column.edit(LOGIN_DIALOG_HOME_SERVER_ID, server, 0)?;
            column.label("User (alice or @alice:matrix.org)", 0, 0)?;
            column.edit(LOGIN_DIALOG_USERNAME_ID, "", 0)?;
            column.label("Password (leave empty to sign in with your browser)", 0, 0)?;
            column.edit(LOGIN_DIALOG_PASSWORD_ID, "", ES_PASSWORD as u32)?;
            column.label("Device name", 0, 0)?;
            column.edit(LOGIN_DIALOG_DEVICE_ID, DEFAULT_DEVICE_DISPLAY_NAME, 0)?;
            column.buttons(&[
                (LOGIN_DIALOG_OK_ID, "OK"),
                (LOGIN_DIALOG_CANCEL_ID, "Cancel"),
            ])
        },
    )
}

pub(super) unsafe fn show_login_dialog(
    owner: HWND,
    server: &str,
) -> Result<Option<LoginForm>, String> {
    let result = Arc::new(Mutex::new(None));
    let dialog = create_login_dialog(owner, server, Arc::clone(&result))?;
    SetFocus(GetDlgItem(dialog, LOGIN_DIALOG_HOME_SERVER_ID.into()));
    run_modal_dialog(owner, dialog);
    Ok(result
        .lock()
        .expect("login dialog result mutex poisoned")
        .clone())
}

unsafe fn create_recover_e2ee_dialog(
    owner: HWND,
    result: Arc<Mutex<Option<RecoverE2eeForm>>>,
) -> Result<HWND, String> {
    let mut state = window_data::CreateParam::new(RecoverE2eeDialogState { result });
    create_modal_dialog(
        owner,
        RECOVER_E2EE_DIALOG_CLASS_NAME,
        "Recover E2EE",
        state.as_param(),
        |dialog| {
            let mut column = DialogColumn::new(dialog, SINGLE_FIELD_DIALOG_WIDTH);
            column.label("Recovery key or passphrase", 0, 0)?;
            column.edit(RECOVER_E2EE_DIALOG_SECRET_ID, "", ES_PASSWORD as u32)?;
            column.buttons(&[
                (RECOVER_E2EE_DIALOG_OK_ID, "OK"),
                (RECOVER_E2EE_DIALOG_CANCEL_ID, "Cancel"),
            ])
        },
    )
}

pub(super) unsafe fn show_recover_e2ee_dialog(
    owner: HWND,
) -> Result<Option<RecoverE2eeForm>, String> {
    let result = Arc::new(Mutex::new(None));
    let dialog = create_recover_e2ee_dialog(owner, Arc::clone(&result))?;
    SetFocus(GetDlgItem(dialog, RECOVER_E2EE_DIALOG_SECRET_ID.into()));
    run_modal_dialog(owner, dialog);
    Ok(result
        .lock()
        .expect("recover e2ee dialog result mutex poisoned")
        .clone())
}

unsafe fn create_setup_e2ee_dialog(
    owner: HWND,
    result: Arc<Mutex<Option<SetupE2eeRecoveryForm>>>,
) -> Result<HWND, String> {
    let mut state = window_data::CreateParam::new(SetupE2eeRecoveryDialogState { result });
    create_modal_dialog(
        owner,
        SETUP_E2EE_DIALOG_CLASS_NAME,
        "Set Up E2EE Recovery",
        state.as_param(),
        |dialog| {
            let mut column = DialogColumn::new(dialog, SINGLE_FIELD_DIALOG_WIDTH);
            column.label("Passphrase (optional)", 0, 0)?;
            column.edit(SETUP_E2EE_DIALOG_PASSPHRASE_ID, "", ES_PASSWORD as u32)?;
            column.checkbox(SETUP_E2EE_DIALOG_WAIT_ID, "Wait for backups to upload")?;
            column.buttons(&[
                (SETUP_E2EE_DIALOG_OK_ID, "OK"),
                (SETUP_E2EE_DIALOG_CANCEL_ID, "Cancel"),
            ])
        },
    )
}

pub(super) unsafe fn show_setup_e2ee_dialog(
    owner: HWND,
) -> Result<Option<SetupE2eeRecoveryForm>, String> {
    let result = Arc::new(Mutex::new(None));
    let dialog = create_setup_e2ee_dialog(owner, Arc::clone(&result))?;
    SetFocus(GetDlgItem(dialog, SETUP_E2EE_DIALOG_PASSPHRASE_ID.into()));
    run_modal_dialog(owner, dialog);
    Ok(result
        .lock()
        .expect("setup e2ee dialog result mutex poisoned")
        .clone())
}

/// One labelled field with OK and Cancel: the Commands menu's prompts and
/// Find.
unsafe fn create_single_field_dialog(
    owner: HWND,
    title: &str,
    label: &str,
    initial_text: &str,
    result: Arc<Mutex<Option<String>>>,
) -> Result<HWND, String> {
    let mut state = window_data::CreateParam::new(SingleFieldDialogState { result });
    create_modal_dialog(
        owner,
        SINGLE_FIELD_DIALOG_CLASS_NAME,
        title,
        state.as_param(),
        |dialog| {
            let mut column = DialogColumn::new(dialog, SINGLE_FIELD_DIALOG_WIDTH);
            column.label(label, 0, 0)?;
            column.edit(SINGLE_FIELD_DIALOG_EDIT_ID, initial_text, 0)?;
            column.buttons(&[
                (SINGLE_FIELD_DIALOG_OK_ID, "OK"),
                (SINGLE_FIELD_DIALOG_CANCEL_ID, "Cancel"),
            ])
        },
    )
}

pub(super) unsafe fn show_single_field_dialog(
    owner: HWND,
    title: &str,
    label: &str,
    initial_text: &str,
) -> Result<Option<String>, String> {
    let result = Arc::new(Mutex::new(None));
    let dialog =
        create_single_field_dialog(owner, title, label, initial_text, Arc::clone(&result))?;
    let edit = GetDlgItem(dialog, SINGLE_FIELD_DIALOG_EDIT_ID.into());
    let len = initial_text.encode_utf16().count();
    SendMessageW(edit, EM_SETSEL, len, len as LPARAM);
    SetFocus(edit);

    run_modal_dialog(owner, dialog);
    Ok(result
        .lock()
        .expect("single-field dialog result mutex poisoned")
        .clone())
}

/// The recovery key that setting up recovery made, shown once.
unsafe fn create_recovery_key_handoff_dialog(
    owner: HWND,
    recovery_key: &str,
) -> Result<HWND, String> {
    create_modal_dialog(
        owner,
        RECOVERY_KEY_HANDOFF_DIALOG_CLASS_NAME,
        "E2EE Recovery Key",
        null(),
        |dialog| {
            let mut column = DialogColumn::new(dialog, RECOVERY_KEY_HANDOFF_DIALOG_WIDTH);
            column.label(
                "Save this recovery key now. It will not be shown again.",
                0,
                0,
            )?;
            // A single-line edit's height, plus the key's further lines
            // wrapped with room to spare for the edit's edges and margins.
            let wrap_width = RECOVERY_KEY_HANDOFF_DIALOG_WIDTH - 2 * DIALOG_MARGIN;
            let (_, line_height) = dialog_text_size("X", wrap_width);
            let (_, key_height) = dialog_text_size(recovery_key, wrap_width);
            column.add(
                (
                    EDIT_CLASS_NAME,
                    recovery_key,
                    WS_TABSTOP | ES_MULTILINE as u32 | ES_READONLY as u32 | ES_AUTOVSCROLL as u32,
                    DIALOG_EDIT_EX_STYLE,
                    RECOVERY_KEY_HANDOFF_DIALOG_KEY_ID,
                ),
                (
                    DIALOG_MARGIN,
                    RECOVERY_KEY_HANDOFF_DIALOG_WIDTH,
                    DIALOG_EDIT_HEIGHT + key_height - line_height,
                ),
                DIALOG_GAP,
            )?;
            column.buttons(&[(RECOVERY_KEY_HANDOFF_DIALOG_ACK_ID, "I Saved It")])
        },
    )
}

pub(super) unsafe fn show_e2ee_recovery_key(owner: HWND, recovery_key: &str) -> Result<(), String> {
    let dialog = create_recovery_key_handoff_dialog(owner, recovery_key)?;
    // Selected, so Ctrl+C copies the key.
    let key = GetDlgItem(dialog, RECOVERY_KEY_HANDOFF_DIALOG_KEY_ID.into());
    SendMessageW(key, EM_SETSEL, 0, -1);
    SetFocus(key);
    run_modal_dialog(owner, dialog);
    Ok(())
}

/// Help > About mirx: the lines centred over a centred OK, like mIRC's
/// About box.
pub(super) unsafe fn create_about_dialog(owner: HWND) -> Result<HWND, String> {
    create_modal_dialog(
        owner,
        ABOUT_DIALOG_CLASS_NAME,
        "About mirx",
        null(),
        |dialog| {
            let width = ABOUT_DIALOG_LINES
                .iter()
                .map(|(_, text)| dialog_text_size(text, i32::MAX).0)
                .max()
                .expect("the About box has lines");
            let mut column = DialogColumn::new(dialog, width);
            for (id, text) in ABOUT_DIALOG_LINES {
                column.label(text, id, SS_CENTER)?;
            }
            column.centred_button(ABOUT_DIALOG_OK_ID, "OK")
        },
    )
}

/// Tools > Options... (Alt+O): saves what OK saves in one write, then
/// applies only what changed, and sends the highlight words' changes to the
/// server, whose push rules echo back as the new list.
pub(super) unsafe fn run_options_dialog(hwnd: HWND) -> Result<(), String> {
    let state = state_ref(hwnd);
    let dialog = options_dialog::OptionsDialog::open(
        hwnd,
        state.settings.settings(),
        state.model.highlight_words.as_deref(),
    )?;
    state_mut(hwnd).options_dialog = Some(dialog.hwnd());
    let saved = dialog.run();
    state_mut(hwnd).options_dialog = None;
    let Some(options_dialog::OptionsResult {
        settings: new,
        highlight_changes,
    }) = saved
    else {
        return Ok(());
    };
    let old = state_ref(hwnd).settings.settings().clone();
    state_mut(hwnd)
        .settings
        .update(|settings| *settings = new.clone())?;
    if new.timestamp != old.timestamp {
        refresh_every_view_content(hwnd)?;
    }
    if new.switchbar.buttons != old.switchbar.buttons {
        set_switchbar_button_width(
            state_ref(hwnd).controls.switchbar,
            new.switchbar.buttons.pixels(),
        );
        layout_controls(hwnd);
    }
    if new.switchbar.activity != old.switchbar.activity {
        let controls = &state_ref(hwnd).controls;
        for bar in [controls.switchbar, controls.tree] {
            InvalidateRect(bar, null(), 1);
        }
    }
    let commands = highlight_changes
        .add
        .into_iter()
        .map(Command::AddHighlightWord)
        .chain(
            highlight_changes
                .remove
                .into_iter()
                .map(Command::RemoveHighlightWord),
        )
        .collect::<Vec<_>>();
    let status = if commands.is_empty() {
        "options saved"
    } else {
        "options saved; highlight words sent to the server"
    };
    for command in commands {
        enqueue_command(state_mut(hwnd), command)?;
    }
    set_local_command_status(hwnd, status.to_owned())
}

/// Tools > Colors... (Alt+K): a changed scheme OK saved is saved and
/// drawn at once in every window.
pub(super) unsafe fn run_colors_dialog(hwnd: HWND) -> Result<(), String> {
    let current = state_ref(hwnd).settings.settings().colors.clone();
    let dialog = colors_dialog::ColorsDialog::open(hwnd, &current)?;
    state_mut(hwnd).colors_dialog = Some(dialog.hwnd());
    let saved = dialog.run();
    state_mut(hwnd).colors_dialog = None;
    match saved {
        Some(scheme) if scheme != current => {
            apply_color_scheme(hwnd, scheme)?;
            set_local_command_status(hwnd, "colors saved".to_owned())
        }
        // Cancel, or OK with nothing changed.
        Some(_) | None => Ok(()),
    }
}

/// Help > About mirx: modal over `owner`; OK, Enter and Esc close it.
pub(super) unsafe fn show_about_dialog(owner: HWND) -> Result<(), String> {
    let dialog = create_about_dialog(owner)?;
    SetFocus(GetDlgItem(dialog, ABOUT_DIALOG_OK_ID as i32));
    run_modal_dialog(owner, dialog);
    Ok(())
}

/// Runs `dialog`'s message loop until it is destroyed, with `owner`
/// disabled: every key the user presses is the dialog's, so the frame's
/// accelerators do not apply. `IsDialogMessageW` gives the dialog a
/// dialog's keys: Tab and Shift+Tab move between its controls, Enter
/// presses the button its `DM_GETDEFID` names and Esc sends `IDCANCEL`.
/// The dialog closes with `end_modal_dialog`, which enables `owner` again.
pub(super) unsafe fn run_modal_dialog(owner: HWND, dialog: HWND) {
    EnableWindow(owner, 0);
    let mut message = MSG::default();
    while IsWindow(dialog) != 0 {
        if GetMessageW(&mut message, null_mut(), 0, 0) <= 0 {
            // The application is quitting: close the dialog, and leave the
            // quit for the frame's loop.
            end_modal_dialog(dialog);
            PostQuitMessage(message.wParam as i32);
            break;
        }
        if IsDialogMessageW(dialog, &message) == 0 {
            TranslateMessage(&message);
            DispatchMessageW(&message);
        }
    }
}

/// Closes a dialog that `run_modal_dialog` runs, as `EndDialog` does: the
/// owner is enabled *before* the dialog goes. Destroying the active window
/// activates another one, and a disabled owner cannot be it, so the
/// activation would leave the app (under Wine, nothing is active and keys
/// typed next are lost).
pub(super) unsafe fn end_modal_dialog(dialog: HWND) {
    EnableWindow(GetWindow(dialog, GW_OWNER), 1);
    DestroyWindow(dialog);
}

unsafe fn login_dialog_state(hwnd: HWND) -> &'static LoginDialogState {
    window_data::get(hwnd).expect("the login dialog has its state")
}

unsafe fn recover_e2ee_dialog_state(hwnd: HWND) -> &'static RecoverE2eeDialogState {
    window_data::get(hwnd).expect("the recover e2ee dialog has its state")
}

unsafe fn setup_e2ee_dialog_state(hwnd: HWND) -> &'static SetupE2eeRecoveryDialogState {
    window_data::get(hwnd).expect("the setup e2ee dialog has its state")
}

unsafe fn single_field_dialog_state(hwnd: HWND) -> &'static SingleFieldDialogState {
    window_data::get(hwnd).expect("the single-field dialog has its state")
}

unsafe fn clear_recovery_key_handoff_dialog_key(hwnd: HWND) {
    set_window_text(
        GetDlgItem(hwnd, RECOVERY_KEY_HANDOFF_DIALOG_KEY_ID.into()),
        "",
    );
}

unsafe fn clear_recover_e2ee_dialog_secret(hwnd: HWND) {
    set_window_text(GetDlgItem(hwnd, RECOVER_E2EE_DIALOG_SECRET_ID.into()), "");
}

unsafe fn clear_setup_e2ee_dialog_passphrase(hwnd: HWND) {
    set_window_text(GetDlgItem(hwnd, SETUP_E2EE_DIALOG_PASSPHRASE_ID.into()), "");
}

#[cfg(test)]
mod tests {
    use windows_sys::Win32::System::Threading::GetCurrentThreadId;
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::IsWindowEnabled;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        EnumThreadWindows, GCLP_HBRBACKGROUND, GW_OWNER, GetClassLongPtrW, GetDlgCtrlID, GetWindow,
        IsWindowVisible, WM_GETFONT,
    };

    use super::super::test_fixtures::*;
    use super::*;

    /// The open dialog of `class` that `frame` owns.
    unsafe fn owned_dialog(frame: HWND, class: &str) -> HWND {
        unsafe extern "system" fn find_owned_dialog(hwnd: HWND, found: LPARAM) -> BOOL {
            let found = &mut *(found as *mut (HWND, &str, HWND));
            if GetWindow(hwnd, GW_OWNER) == found.0 && window_class_is(hwnd, found.1) {
                found.2 = hwnd;
            }
            1
        }
        let mut found: (HWND, &str, HWND) = (frame, class, null_mut());
        EnumThreadWindows(
            GetCurrentThreadId(),
            Some(find_owned_dialog),
            &mut found as *mut (HWND, &str, HWND) as LPARAM,
        );
        assert!(!found.2.is_null(), "a {class} is open");
        found.2
    }

    /// Types `c` and Enter into the frame's single-field dialog once its
    /// modal loop runs. The keys are posted to the dialog's own edit control,
    /// so that loop handles messages for the dialog's children.
    unsafe extern "system" fn type_into_single_field_dialog(
        frame: HWND,
        _message: u32,
        timer: usize,
        _time: u32,
    ) {
        KillTimer(frame, timer);
        let dialog = owned_dialog(frame, SINGLE_FIELD_DIALOG_CLASS_NAME);
        let edit = GetDlgItem(dialog, SINGLE_FIELD_DIALOG_EDIT_ID.into());
        PostMessageW(edit, WM_CHAR, 'c' as WPARAM, 0);
        PostMessageW(edit, WM_KEYDOWN, VK_RETURN_KEY, 0);
    }

    /// Presses Enter in the recovery-key handoff's key. It does not assert
    /// the key has the focus: unit tests run in parallel, and the focus
    /// goes with whichever test's window is in the foreground.
    unsafe extern "system" fn press_enter_in_recovery_key(
        frame: HWND,
        _message: u32,
        timer: usize,
        _time: u32,
    ) {
        KillTimer(frame, timer);
        let dialog = owned_dialog(frame, RECOVERY_KEY_HANDOFF_DIALOG_CLASS_NAME);
        let key = GetDlgItem(dialog, RECOVERY_KEY_HANDOFF_DIALOG_KEY_ID.into());
        PostMessageW(key, WM_KEYDOWN, VK_RETURN_KEY, 0);
    }

    /// The key's multi-line edit takes every key itself; Enter in it still
    /// presses the default button.
    #[test]
    fn enter_in_the_recovery_key_presses_i_saved_it() {
        unsafe {
            with_test_frame(600, 400, |hwnd| {
                register_dialog_classes(GetModuleHandleW(null())).unwrap();
                SetTimer(hwnd, 1, 0, Some(press_enter_in_recovery_key));
                show_e2ee_recovery_key(hwnd, "EsTc abcd efgh").unwrap();
                assert_ne!(IsWindowEnabled(hwnd), 0, "closing it enables the frame");
            });
        }
    }

    #[test]
    fn a_single_field_dialog_takes_the_keys_typed_into_its_edit() {
        unsafe {
            with_test_frame(600, 400, |hwnd| {
                register_dialog_classes(GetModuleHandleW(null())).unwrap();
                SetTimer(hwnd, 1, 0, Some(type_into_single_field_dialog));
                assert_eq!(
                    show_single_field_dialog(hwnd, "Find", "Find", "ab").unwrap(),
                    Some("abc".to_owned())
                );
                assert_ne!(IsWindowEnabled(hwnd), 0, "closing it enables the frame");
            });
        }
    }

    #[test]
    fn a_dialog_centres_on_its_owner() {
        let owner = RECT {
            left: 100,
            top: 50,
            right: 901,
            bottom: 651,
        };
        assert_eq!(centered_origin(&owner, 301, 200), (350, 250));
        // Larger than its owner, it overhangs both sides alike.
        assert_eq!(centered_origin(&owner, 1001, 601), (0, 50));
    }

    unsafe fn children(dialog: HWND) -> Vec<HWND> {
        unsafe extern "system" fn collect(child: HWND, children: LPARAM) -> BOOL {
            (*(children as *mut Vec<HWND>)).push(child);
            1
        }
        let mut children = Vec::new();
        EnumChildWindows(
            dialog,
            Some(collect),
            &mut children as *mut Vec<HWND> as LPARAM,
        );
        children
    }

    /// What every dialog shows: the button face, the dialog font in every
    /// control, its controls fitted with the margin on every side, and the
    /// whole centred on `owner`.
    unsafe fn assert_dialog_chrome(owner: HWND, dialog: HWND) {
        let name = read_window_text(dialog);
        assert_ne!(IsWindowVisible(dialog), 0, "{name} is shown");
        assert_eq!(
            GetClassLongPtrW(dialog, GCLP_HBRBACKGROUND),
            COLOR_BTNFACE as usize + 1,
            "{name} is on the button face"
        );
        let font = GetStockObject(DEFAULT_GUI_FONT) as LRESULT;
        let controls = children(dialog);
        assert!(!controls.is_empty(), "{name} has controls");
        for control in controls {
            assert_eq!(
                SendMessageW(control, WM_GETFONT, 0, 0),
                font,
                "{name}'s control {} has the dialog font",
                GetDlgCtrlID(control)
            );
        }
        let mut client = RECT::default();
        GetClientRect(dialog, &mut client);
        assert_eq!(
            controls_extent(dialog),
            (client.right - DIALOG_MARGIN, client.bottom - DIALOG_MARGIN),
            "{name} fits its controls"
        );
        let (mut dialog_rect, mut owner_rect) = (RECT::default(), RECT::default());
        GetWindowRect(dialog, &mut dialog_rect);
        GetWindowRect(owner, &mut owner_rect);
        let centre = |rect: &RECT| (rect.left + rect.right, rect.top + rect.bottom);
        let (dialog_centre, owner_centre) = (centre(&dialog_rect), centre(&owner_rect));
        assert!(
            (dialog_centre.0 - owner_centre.0).abs() <= 2
                && (dialog_centre.1 - owner_centre.1).abs() <= 2,
            "{name} centred at {dialog_centre:?}, not on {owner_centre:?}"
        );
    }

    #[test]
    fn every_dialog_is_centred_on_its_owner_in_the_dialog_font() {
        unsafe {
            with_test_frame(800, 600, |frame| {
                register_dialog_classes(GetModuleHandleW(null())).unwrap();
                let options = options_dialog::OptionsDialog::open(
                    frame,
                    state_ref(frame).settings.settings(),
                    None,
                )
                .unwrap();
                let dialogs = [
                    create_login_dialog(frame, "matrix.org", Arc::default()).unwrap(),
                    create_recover_e2ee_dialog(frame, Arc::default()).unwrap(),
                    create_setup_e2ee_dialog(frame, Arc::default()).unwrap(),
                    create_single_field_dialog(frame, "Find", "Find", "", Arc::default()).unwrap(),
                    create_recovery_key_handoff_dialog(frame, "EsTc abcd efgh").unwrap(),
                    create_about_dialog(frame).unwrap(),
                    options.hwnd(),
                ];
                for dialog in dialogs {
                    assert_dialog_chrome(frame, dialog);
                    DestroyWindow(dialog);
                }
            });
        }
    }
}
