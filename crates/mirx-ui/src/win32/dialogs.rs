//! The sign-in, E2EE and About modal dialogs, and the modal dialog
//! plumbing they share.

use super::*;

const LOGIN_DIALOG_CLASS_NAME: &str = "mirxWin32LoginDialog";
const RECOVER_E2EE_DIALOG_CLASS_NAME: &str = "mirxWin32RecoverE2eeDialog";
const SETUP_E2EE_DIALOG_CLASS_NAME: &str = "mirxWin32SetupE2eeDialog";
const RECOVERY_KEY_HANDOFF_DIALOG_CLASS_NAME: &str = "mirxWin32RecoveryKeyHandoffDialog";
pub(super) const FIND_DIALOG_CLASS_NAME: &str = "mirxWin32FindDialog";
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
const FIND_DIALOG_QUERY_ID: u16 = 5010;
const FIND_DIALOG_OK_ID: usize = 5011;
const FIND_DIALOG_CANCEL_ID: usize = 5012;
/// The About box's lines, top to bottom, with the control ids tests read.
const ABOUT_DIALOG_LINES: [(u16, &str); 5] = [
    (5019, concat!("mirx ", env!("CARGO_PKG_VERSION"))),
    (5020, "mIRC, but Matrix."),
    (5021, "A native Win32 Matrix client"),
    (5022, "End-to-end encryption by matrix-rust-sdk"),
    (5023, "Licensed under the Apache License 2.0"),
];
pub(super) const ABOUT_DIALOG_OK_ID: usize = 5024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct LoginForm {
    pub(super) server: String,
    pub(super) username: String,
    pub(super) password: String,
    pub(super) device_display_name: String,
}

struct LoginDialogState {
    /// `/server <server>` fills the Server field.
    initial_server: String,
    result: Arc<Mutex<Option<LoginForm>>>,
    server_label: HWND,
    server_edit: HWND,
    username_label: HWND,
    username_edit: HWND,
    password_label: HWND,
    password_edit: HWND,
    device_display_name_label: HWND,
    device_display_name_edit: HWND,
    ok_button: HWND,
    cancel_button: HWND,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct RecoverE2eeForm {
    pub(super) recovery_secret: String,
}

struct RecoverE2eeDialogState {
    result: Arc<Mutex<Option<RecoverE2eeForm>>>,
    recovery_secret_label: HWND,
    recovery_secret_edit: HWND,
    ok_button: HWND,
    cancel_button: HWND,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct SetupE2eeRecoveryForm {
    pub(super) passphrase: Option<String>,
    pub(super) wait_for_backups_to_upload: bool,
}

struct SetupE2eeRecoveryDialogState {
    result: Arc<Mutex<Option<SetupE2eeRecoveryForm>>>,
    passphrase_label: HWND,
    passphrase_edit: HWND,
    wait_for_backups_to_upload_checkbox: HWND,
    ok_button: HWND,
    cancel_button: HWND,
}

struct RecoveryKeyHandoffDialogState {
    recovery_key: String,
    instruction_label: HWND,
    recovery_key_edit: HWND,
    acknowledge_button: HWND,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct SubmittedSetupE2eeRecovery {
    pub(super) passphrase_redacted: bool,
    pub(super) wait_for_backups_to_upload: bool,
}

pub(super) struct FindDialogState {
    pub(super) result: Arc<Mutex<Option<String>>>,
    pub(super) label: String,
    pub(super) query_label: HWND,
    pub(super) query_edit: HWND,
    pub(super) ok_button: HWND,
    pub(super) cancel_button: HWND,
}

pub(super) fn register_login_dialog_class(instance: *mut c_void) -> Result<(), String> {
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

pub(super) fn register_recover_e2ee_dialog_class(instance: *mut c_void) -> Result<(), String> {
    let class_name = wide_null(RECOVER_E2EE_DIALOG_CLASS_NAME);
    let window_class = WNDCLASSEXW {
        cbSize: size_of::<WNDCLASSEXW>() as u32,
        style: 0,
        lpfnWndProc: Some(recover_e2ee_dialog_window_proc),
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
            return Err(last_os_error("RegisterClassExW recover e2ee dialog"));
        }
    }

    Ok(())
}

pub(super) fn register_setup_e2ee_dialog_class(instance: *mut c_void) -> Result<(), String> {
    let class_name = wide_null(SETUP_E2EE_DIALOG_CLASS_NAME);
    let window_class = WNDCLASSEXW {
        cbSize: size_of::<WNDCLASSEXW>() as u32,
        style: 0,
        lpfnWndProc: Some(setup_e2ee_dialog_window_proc),
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
            return Err(last_os_error("RegisterClassExW setup e2ee dialog"));
        }
    }

    Ok(())
}

pub(super) fn register_recovery_key_handoff_dialog_class(
    instance: *mut c_void,
) -> Result<(), String> {
    let class_name = wide_null(RECOVERY_KEY_HANDOFF_DIALOG_CLASS_NAME);
    let window_class = WNDCLASSEXW {
        cbSize: size_of::<WNDCLASSEXW>() as u32,
        style: 0,
        lpfnWndProc: Some(recovery_key_handoff_dialog_window_proc),
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
            return Err(last_os_error(
                "RegisterClassExW recovery key handoff dialog",
            ));
        }
    }

    Ok(())
}

pub(super) fn register_find_dialog_class(instance: *mut c_void) -> Result<(), String> {
    let class_name = wide_null(FIND_DIALOG_CLASS_NAME);
    let window_class = WNDCLASSEXW {
        cbSize: size_of::<WNDCLASSEXW>() as u32,
        style: 0,
        lpfnWndProc: Some(find_dialog_window_proc),
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
            return Err(last_os_error("RegisterClassExW find dialog"));
        }
    }

    Ok(())
}

pub(super) fn register_about_dialog_class(instance: *mut c_void) -> Result<(), String> {
    let class_name = wide_null(ABOUT_DIALOG_CLASS_NAME);
    let window_class = WNDCLASSEXW {
        cbSize: size_of::<WNDCLASSEXW>() as u32,
        style: 0,
        lpfnWndProc: Some(about_dialog_window_proc),
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
            return Err(last_os_error("RegisterClassExW about dialog"));
        }
    }

    Ok(())
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
        WM_CREATE => match create_login_dialog_controls(hwnd) {
            Ok(()) => 0,
            Err(message) => {
                eprintln!("{message}");
                -1
            }
        },
        WM_COMMAND => {
            let command_id = wparam & 0xffff;
            match command_id {
                LOGIN_DIALOG_OK_ID => {
                    let Some(state) = login_dialog_state(hwnd) else {
                        return 0;
                    };
                    let form = LoginForm {
                        server: read_window_text(state.server_edit),
                        username: read_window_text(state.username_edit),
                        password: read_window_text(state.password_edit),
                        device_display_name: read_window_text(state.device_display_name_edit),
                    };
                    // Like mIRC's dialogs, a form that cannot work stays
                    // open with the reason.
                    if let Err(message) = login_server(&form.server, &form.username) {
                        show_error_message(hwnd, "Select Server", &message);
                        return 0;
                    }
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
                _ => DefWindowProcW(hwnd, message, wparam, lparam),
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
        WM_CREATE => match create_recover_e2ee_dialog_controls(hwnd) {
            Ok(()) => 0,
            Err(message) => {
                eprintln!("{message}");
                -1
            }
        },
        WM_COMMAND => {
            let command_id = wparam & 0xffff;
            match command_id {
                RECOVER_E2EE_DIALOG_OK_ID => {
                    let Some(state) = recover_e2ee_dialog_state(hwnd) else {
                        return 0;
                    };
                    let recovery_secret = read_window_text(state.recovery_secret_edit);
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
                    DestroyWindow(hwnd);
                    0
                }
                RECOVER_E2EE_DIALOG_CANCEL_ID => {
                    clear_recover_e2ee_dialog_secret(hwnd);
                    DestroyWindow(hwnd);
                    0
                }
                _ => DefWindowProcW(hwnd, message, wparam, lparam),
            }
        }
        WM_CLOSE => {
            clear_recover_e2ee_dialog_secret(hwnd);
            DestroyWindow(hwnd);
            0
        }
        WM_SIZE => {
            layout_recover_e2ee_dialog(hwnd);
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
        WM_CREATE => match create_setup_e2ee_dialog_controls(hwnd) {
            Ok(()) => 0,
            Err(message) => {
                eprintln!("{message}");
                -1
            }
        },
        WM_COMMAND => match wparam & 0xffff {
            SETUP_E2EE_DIALOG_OK_ID => {
                let Some(state) = setup_e2ee_dialog_state(hwnd) else {
                    return 0;
                };
                let passphrase = read_window_text(state.passphrase_edit);
                let wait_for_backups_to_upload =
                    SendMessageW(state.wait_for_backups_to_upload_checkbox, BM_GETCHECK, 0, 0) == 1;
                set_window_text(state.passphrase_edit, "");
                *state
                    .result
                    .lock()
                    .expect("setup e2ee dialog result mutex poisoned") =
                    Some(SetupE2eeRecoveryForm {
                        passphrase: (!passphrase.is_empty()).then_some(passphrase),
                        wait_for_backups_to_upload,
                    });
                DestroyWindow(hwnd);
                0
            }
            SETUP_E2EE_DIALOG_CANCEL_ID => {
                clear_setup_e2ee_dialog_passphrase(hwnd);
                DestroyWindow(hwnd);
                0
            }
            _ => DefWindowProcW(hwnd, message, wparam, lparam),
        },
        WM_CLOSE => {
            clear_setup_e2ee_dialog_passphrase(hwnd);
            DestroyWindow(hwnd);
            0
        }
        WM_SIZE => {
            layout_setup_e2ee_dialog(hwnd);
            0
        }
        WM_NCDESTROY => {
            window_data::drop_owned::<SetupE2eeRecoveryDialogState>(hwnd);
            0
        }
        _ => DefWindowProcW(hwnd, message, wparam, lparam),
    }
}

unsafe extern "system" fn recovery_key_handoff_dialog_window_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match message {
        WM_NCCREATE => {
            window_data::adopt_create_param::<RecoveryKeyHandoffDialogState>(hwnd, lparam);
            // Sets the window text from the CreateWindowExW title.
            DefWindowProcW(hwnd, message, wparam, lparam)
        }
        WM_CREATE => match create_recovery_key_handoff_dialog_controls(hwnd) {
            Ok(()) => 0,
            Err(message) => {
                eprintln!("{message}");
                -1
            }
        },
        WM_COMMAND if wparam & 0xffff == RECOVERY_KEY_HANDOFF_DIALOG_ACK_ID => {
            clear_recovery_key_handoff_dialog_key(hwnd);
            DestroyWindow(hwnd);
            0
        }
        WM_CLOSE => {
            clear_recovery_key_handoff_dialog_key(hwnd);
            DestroyWindow(hwnd);
            0
        }
        WM_SIZE => {
            layout_recovery_key_handoff_dialog(hwnd);
            0
        }
        WM_NCDESTROY => {
            window_data::drop_owned::<RecoveryKeyHandoffDialogState>(hwnd);
            0
        }
        _ => DefWindowProcW(hwnd, message, wparam, lparam),
    }
}

unsafe extern "system" fn find_dialog_window_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match message {
        WM_NCCREATE => {
            window_data::adopt_create_param::<FindDialogState>(hwnd, lparam);
            // Sets the window text from the CreateWindowExW title.
            DefWindowProcW(hwnd, message, wparam, lparam)
        }
        WM_CREATE => match create_find_dialog_controls(hwnd) {
            Ok(()) => 0,
            Err(message) => {
                eprintln!("{message}");
                -1
            }
        },
        WM_COMMAND => {
            let command_id = wparam & 0xffff;
            match command_id {
                FIND_DIALOG_OK_ID => {
                    let Some(state) = find_dialog_state(hwnd) else {
                        return 0;
                    };
                    *state
                        .result
                        .lock()
                        .expect("find dialog result mutex poisoned") =
                        Some(read_window_text(state.query_edit));
                    DestroyWindow(hwnd);
                    0
                }
                FIND_DIALOG_CANCEL_ID => {
                    DestroyWindow(hwnd);
                    0
                }
                _ => DefWindowProcW(hwnd, message, wparam, lparam),
            }
        }
        WM_CLOSE => {
            DestroyWindow(hwnd);
            0
        }
        WM_SIZE => {
            layout_find_dialog(hwnd);
            0
        }
        WM_NCDESTROY => {
            window_data::drop_owned::<FindDialogState>(hwnd);
            0
        }
        _ => DefWindowProcW(hwnd, message, wparam, lparam),
    }
}

/// The About box keeps no state: DefWindowProcW destroys it on WM_CLOSE.
unsafe extern "system" fn about_dialog_window_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match message {
        WM_CREATE => match create_about_dialog_controls(hwnd) {
            Ok(()) => 0,
            Err(message) => {
                eprintln!("{message}");
                -1
            }
        },
        WM_COMMAND if wparam & 0xffff == ABOUT_DIALOG_OK_ID => {
            DestroyWindow(hwnd);
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

unsafe fn create_login_dialog_controls(hwnd: HWND) -> Result<(), String> {
    let instance = GetModuleHandleW(null());
    let state = login_dialog_state_mut(hwnd).expect("login dialog state must exist");

    state.server_label = create_login_dialog_control(
        hwnd,
        STATIC_CLASS_NAME,
        "Server (matrix.org, or leave empty for a full user ID)",
        WS_CHILD | WS_VISIBLE,
        0,
        0,
        instance,
    )?;
    let initial_server = state.initial_server.clone();
    state.server_edit = create_login_dialog_control(
        hwnd,
        EDIT_CLASS_NAME,
        &initial_server,
        WS_CHILD | WS_VISIBLE | WS_BORDER | WS_TABSTOP | ES_AUTOHSCROLL as u32,
        WS_EX_CLIENTEDGE,
        LOGIN_DIALOG_HOME_SERVER_ID,
        instance,
    )?;

    state.username_label = create_login_dialog_control(
        hwnd,
        STATIC_CLASS_NAME,
        "User (alice or @alice:matrix.org)",
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
        "Device name",
        WS_CHILD | WS_VISIBLE,
        0,
        0,
        instance,
    )?;
    state.device_display_name_edit = create_login_dialog_control(
        hwnd,
        EDIT_CLASS_NAME,
        DEFAULT_DEVICE_DISPLAY_NAME,
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

unsafe fn create_recover_e2ee_dialog_controls(hwnd: HWND) -> Result<(), String> {
    let instance = GetModuleHandleW(null());
    let state = recover_e2ee_dialog_state_mut(hwnd).expect("recover e2ee dialog state must exist");

    state.recovery_secret_label = create_login_dialog_control(
        hwnd,
        STATIC_CLASS_NAME,
        "Recovery key or passphrase",
        WS_CHILD | WS_VISIBLE,
        0,
        0,
        instance,
    )?;
    state.recovery_secret_edit = create_login_dialog_control(
        hwnd,
        EDIT_CLASS_NAME,
        "",
        WS_CHILD | WS_VISIBLE | WS_BORDER | WS_TABSTOP | ES_AUTOHSCROLL as u32 | ES_PASSWORD as u32,
        WS_EX_CLIENTEDGE,
        RECOVER_E2EE_DIALOG_SECRET_ID,
        instance,
    )?;
    state.ok_button = create_login_dialog_control(
        hwnd,
        BUTTON_CLASS_NAME,
        "OK",
        WS_CHILD | WS_VISIBLE | WS_TABSTOP | BS_DEFPUSHBUTTON as u32,
        0,
        RECOVER_E2EE_DIALOG_OK_ID as u16,
        instance,
    )?;
    state.cancel_button = create_login_dialog_control(
        hwnd,
        BUTTON_CLASS_NAME,
        "Cancel",
        WS_CHILD | WS_VISIBLE | WS_TABSTOP | BS_PUSHBUTTON as u32,
        0,
        RECOVER_E2EE_DIALOG_CANCEL_ID as u16,
        instance,
    )?;

    layout_recover_e2ee_dialog(hwnd);
    Ok(())
}

unsafe fn create_setup_e2ee_dialog_controls(hwnd: HWND) -> Result<(), String> {
    let instance = GetModuleHandleW(null());
    let state = setup_e2ee_dialog_state_mut(hwnd).expect("setup e2ee dialog state must exist");

    state.passphrase_label = create_login_dialog_control(
        hwnd,
        STATIC_CLASS_NAME,
        "Passphrase (optional)",
        WS_CHILD | WS_VISIBLE,
        0,
        0,
        instance,
    )?;
    state.passphrase_edit = create_login_dialog_control(
        hwnd,
        EDIT_CLASS_NAME,
        "",
        WS_CHILD | WS_VISIBLE | WS_BORDER | WS_TABSTOP | ES_AUTOHSCROLL as u32 | ES_PASSWORD as u32,
        WS_EX_CLIENTEDGE,
        SETUP_E2EE_DIALOG_PASSPHRASE_ID,
        instance,
    )?;
    state.wait_for_backups_to_upload_checkbox = create_login_dialog_control(
        hwnd,
        BUTTON_CLASS_NAME,
        "Wait for backups to upload",
        WS_CHILD | WS_VISIBLE | WS_TABSTOP | BS_AUTOCHECKBOX as u32,
        0,
        SETUP_E2EE_DIALOG_WAIT_ID,
        instance,
    )?;
    state.ok_button = create_login_dialog_control(
        hwnd,
        BUTTON_CLASS_NAME,
        "OK",
        WS_CHILD | WS_VISIBLE | WS_TABSTOP | BS_DEFPUSHBUTTON as u32,
        0,
        SETUP_E2EE_DIALOG_OK_ID as u16,
        instance,
    )?;
    state.cancel_button = create_login_dialog_control(
        hwnd,
        BUTTON_CLASS_NAME,
        "Cancel",
        WS_CHILD | WS_VISIBLE | WS_TABSTOP | BS_PUSHBUTTON as u32,
        0,
        SETUP_E2EE_DIALOG_CANCEL_ID as u16,
        instance,
    )?;

    layout_setup_e2ee_dialog(hwnd);
    Ok(())
}

unsafe fn create_find_dialog_controls(hwnd: HWND) -> Result<(), String> {
    let instance = GetModuleHandleW(null());
    let state = find_dialog_state_mut(hwnd).expect("find dialog state must exist");

    state.query_label = create_login_dialog_control(
        hwnd,
        STATIC_CLASS_NAME,
        &state.label,
        WS_CHILD | WS_VISIBLE,
        0,
        0,
        instance,
    )?;
    state.query_edit = create_login_dialog_control(
        hwnd,
        EDIT_CLASS_NAME,
        "",
        WS_CHILD | WS_VISIBLE | WS_BORDER | WS_TABSTOP | ES_AUTOHSCROLL as u32,
        WS_EX_CLIENTEDGE,
        FIND_DIALOG_QUERY_ID,
        instance,
    )?;
    state.ok_button = create_login_dialog_control(
        hwnd,
        BUTTON_CLASS_NAME,
        "OK",
        WS_CHILD | WS_VISIBLE | WS_TABSTOP | BS_DEFPUSHBUTTON as u32,
        0,
        FIND_DIALOG_OK_ID as u16,
        instance,
    )?;
    state.cancel_button = create_login_dialog_control(
        hwnd,
        BUTTON_CLASS_NAME,
        "Cancel",
        WS_CHILD | WS_VISIBLE | WS_TABSTOP | BS_PUSHBUTTON as u32,
        0,
        FIND_DIALOG_CANCEL_ID as u16,
        instance,
    )?;

    layout_find_dialog(hwnd);
    Ok(())
}

unsafe fn create_recovery_key_handoff_dialog_controls(hwnd: HWND) -> Result<(), String> {
    let instance = GetModuleHandleW(null());
    let state = recovery_key_handoff_dialog_state_mut(hwnd)
        .expect("recovery key handoff dialog state must exist");

    state.instruction_label = create_login_dialog_control(
        hwnd,
        STATIC_CLASS_NAME,
        "Save this recovery key now. It will not be shown again.",
        WS_CHILD | WS_VISIBLE,
        0,
        0,
        instance,
    )?;
    state.recovery_key_edit = create_login_dialog_control(
        hwnd,
        EDIT_CLASS_NAME,
        "",
        WS_CHILD
            | WS_VISIBLE
            | WS_BORDER
            | WS_TABSTOP
            | ES_MULTILINE as u32
            | ES_READONLY as u32
            | ES_AUTOVSCROLL as u32,
        WS_EX_CLIENTEDGE,
        RECOVERY_KEY_HANDOFF_DIALOG_KEY_ID,
        instance,
    )?;
    set_window_text(state.recovery_key_edit, &state.recovery_key);
    state.acknowledge_button = create_login_dialog_control(
        hwnd,
        BUTTON_CLASS_NAME,
        "I Saved It",
        WS_CHILD | WS_VISIBLE | WS_TABSTOP | BS_DEFPUSHBUTTON as u32,
        0,
        RECOVERY_KEY_HANDOFF_DIALOG_ACK_ID as u16,
        instance,
    )?;

    layout_recovery_key_handoff_dialog(hwnd);
    Ok(())
}

unsafe fn create_about_dialog_controls(hwnd: HWND) -> Result<(), String> {
    let instance = GetModuleHandleW(null());
    let mut rect = RECT::default();
    GetClientRect(hwnd, &mut rect);
    let width = rect.right - rect.left;
    let line_height = 18;
    let line_spacing = 24;
    for (index, (control_id, text)) in ABOUT_DIALOG_LINES.into_iter().enumerate() {
        let line = create_login_dialog_control(
            hwnd,
            STATIC_CLASS_NAME,
            text,
            WS_CHILD | WS_VISIBLE,
            0,
            control_id,
            instance,
        )?;
        MoveWindow(
            line,
            16,
            16 + index as i32 * line_spacing,
            width - 32,
            line_height,
            1,
        );
    }
    let ok_button = create_login_dialog_control(
        hwnd,
        BUTTON_CLASS_NAME,
        "OK",
        WS_CHILD | WS_VISIBLE | WS_TABSTOP | BS_DEFPUSHBUTTON as u32,
        0,
        ABOUT_DIALOG_OK_ID as u16,
        instance,
    )?;
    let button_width = 96;
    let button_height = 28;
    MoveWindow(
        ok_button,
        (width - button_width) / 2,
        rect.bottom - button_height - 16,
        button_width,
        button_height,
        1,
    );
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
        state.server_label,
        state.username_label,
        state.password_label,
        state.device_display_name_label,
    ];
    for (index, hwnd_control) in labels.iter().enumerate() {
        let y = 16 + (index as i32 * block_height);
        MoveWindow(*hwnd_control, label_x, y, label_width, label_height, 1);
    }
    MoveWindow(
        state.server_edit,
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

pub(super) unsafe fn show_login_dialog(
    owner: HWND,
    server: &str,
) -> Result<Option<LoginForm>, String> {
    let instance = GetModuleHandleW(null());
    let result = Arc::new(Mutex::new(None));
    let mut dialog_state = window_data::CreateParam::new(LoginDialogState {
        initial_server: server.to_owned(),
        result: Arc::clone(&result),
        server_label: null_mut(),
        server_edit: null_mut(),
        username_label: null_mut(),
        username_edit: null_mut(),
        password_label: null_mut(),
        password_edit: null_mut(),
        device_display_name_label: null_mut(),
        device_display_name_edit: null_mut(),
        ok_button: null_mut(),
        cancel_button: null_mut(),
    });
    let class_name = wide_null(LOGIN_DIALOG_CLASS_NAME);
    let title = wide_null("Select Server");
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
        dialog_state.as_param(),
    );
    if dialog.is_null() {
        return Err(last_os_error("CreateWindowExW login dialog"));
    }

    run_modal_dialog(owner, dialog, |_| false);
    Ok(result
        .lock()
        .expect("login dialog result mutex poisoned")
        .clone())
}

unsafe fn layout_recover_e2ee_dialog(hwnd: HWND) {
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

    let Some(state) = recover_e2ee_dialog_state(hwnd) else {
        return;
    };

    MoveWindow(
        state.recovery_secret_label,
        label_x,
        16,
        label_width,
        label_height,
        1,
    );
    MoveWindow(
        state.recovery_secret_edit,
        input_x,
        16 + label_height + spacing,
        input_width,
        input_height,
        1,
    );

    let button_y = 16 + block_height;
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

unsafe fn layout_setup_e2ee_dialog(hwnd: HWND) {
    let mut rect = RECT::default();
    GetClientRect(hwnd, &mut rect);
    let width = rect.right - rect.left;
    let input_width = (width - 32).max(0);
    let label_height = 18;
    let input_height = 24;
    let checkbox_height = 24;
    let spacing = 8;
    let button_width = 96;
    let button_height = 28;

    let Some(state) = setup_e2ee_dialog_state(hwnd) else {
        return;
    };

    MoveWindow(state.passphrase_label, 16, 16, input_width, label_height, 1);
    MoveWindow(
        state.passphrase_edit,
        16,
        16 + label_height + spacing,
        input_width,
        input_height,
        1,
    );
    MoveWindow(
        state.wait_for_backups_to_upload_checkbox,
        16,
        16 + label_height + spacing + input_height + spacing,
        input_width,
        checkbox_height,
        1,
    );

    let button_y = 16 + label_height + spacing + input_height + spacing + checkbox_height + 12;
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

unsafe fn layout_find_dialog(hwnd: HWND) {
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

    let Some(state) = find_dialog_state(hwnd) else {
        return;
    };

    MoveWindow(state.query_label, label_x, 16, label_width, label_height, 1);
    MoveWindow(
        state.query_edit,
        input_x,
        16 + label_height + spacing,
        input_width,
        input_height,
        1,
    );

    let button_y = 16 + block_height;
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

unsafe fn layout_recovery_key_handoff_dialog(hwnd: HWND) {
    let mut rect = RECT::default();
    GetClientRect(hwnd, &mut rect);
    let width = rect.right - rect.left;
    let input_width = (width - 32).max(0);
    let label_height = 18;
    let input_height = 72;
    let spacing = 8;
    let button_width = 96;
    let button_height = 28;

    let Some(state) = recovery_key_handoff_dialog_state(hwnd) else {
        return;
    };

    MoveWindow(
        state.instruction_label,
        16,
        16,
        input_width,
        label_height,
        1,
    );
    MoveWindow(
        state.recovery_key_edit,
        16,
        16 + label_height + spacing,
        input_width,
        input_height,
        1,
    );
    MoveWindow(
        state.acknowledge_button,
        width - 16 - button_width,
        16 + label_height + spacing + input_height + 12,
        button_width,
        button_height,
        1,
    );
}

pub(super) unsafe fn show_recover_e2ee_dialog(
    owner: HWND,
) -> Result<Option<RecoverE2eeForm>, String> {
    let instance = GetModuleHandleW(null());
    let result = Arc::new(Mutex::new(None));
    let mut dialog_state = window_data::CreateParam::new(RecoverE2eeDialogState {
        result: Arc::clone(&result),
        recovery_secret_label: null_mut(),
        recovery_secret_edit: null_mut(),
        ok_button: null_mut(),
        cancel_button: null_mut(),
    });
    let class_name = wide_null(RECOVER_E2EE_DIALOG_CLASS_NAME);
    let title = wide_null("Recover E2EE");
    let dialog = CreateWindowExW(
        0,
        class_name.as_ptr(),
        title.as_ptr(),
        WS_OVERLAPPED | WS_CAPTION | WS_SYSMENU | WS_VISIBLE,
        CW_USEDEFAULT,
        CW_USEDEFAULT,
        420,
        170,
        owner,
        null_mut(),
        instance,
        dialog_state.as_param(),
    );
    if dialog.is_null() {
        return Err(last_os_error("CreateWindowExW recover e2ee dialog"));
    }

    run_modal_dialog(owner, dialog, |_| false);
    Ok(result
        .lock()
        .expect("recover e2ee dialog result mutex poisoned")
        .clone())
}

pub(super) unsafe fn show_setup_e2ee_dialog(
    owner: HWND,
) -> Result<Option<SetupE2eeRecoveryForm>, String> {
    let instance = GetModuleHandleW(null());
    let result = Arc::new(Mutex::new(None));
    let mut dialog_state = window_data::CreateParam::new(SetupE2eeRecoveryDialogState {
        result: Arc::clone(&result),
        passphrase_label: null_mut(),
        passphrase_edit: null_mut(),
        wait_for_backups_to_upload_checkbox: null_mut(),
        ok_button: null_mut(),
        cancel_button: null_mut(),
    });
    let class_name = wide_null(SETUP_E2EE_DIALOG_CLASS_NAME);
    let title = wide_null("Set Up E2EE Recovery");
    let dialog = CreateWindowExW(
        0,
        class_name.as_ptr(),
        title.as_ptr(),
        WS_OVERLAPPED | WS_CAPTION | WS_SYSMENU | WS_VISIBLE,
        CW_USEDEFAULT,
        CW_USEDEFAULT,
        420,
        210,
        owner,
        null_mut(),
        instance,
        dialog_state.as_param(),
    );
    if dialog.is_null() {
        return Err(last_os_error("CreateWindowExW setup e2ee dialog"));
    }

    run_modal_dialog(owner, dialog, |_| false);
    Ok(result
        .lock()
        .expect("setup e2ee dialog result mutex poisoned")
        .clone())
}

pub(super) unsafe fn show_single_field_dialog(
    owner: HWND,
    title: &str,
    label: &str,
    initial_text: &str,
) -> Result<Option<String>, String> {
    let instance = GetModuleHandleW(null());
    let result = Arc::new(Mutex::new(None));
    let mut dialog_state = window_data::CreateParam::new(FindDialogState {
        result: Arc::clone(&result),
        label: label.to_owned(),
        query_label: null_mut(),
        query_edit: null_mut(),
        ok_button: null_mut(),
        cancel_button: null_mut(),
    });
    let class_name = wide_null(FIND_DIALOG_CLASS_NAME);
    let title = wide_null(title);
    let dialog = CreateWindowExW(
        0,
        class_name.as_ptr(),
        title.as_ptr(),
        WS_OVERLAPPED | WS_CAPTION | WS_SYSMENU | WS_VISIBLE,
        CW_USEDEFAULT,
        CW_USEDEFAULT,
        420,
        170,
        owner,
        null_mut(),
        instance,
        dialog_state.as_param(),
    );
    if dialog.is_null() {
        return Err(last_os_error("CreateWindowExW single field dialog"));
    }

    if let Some(state) = find_dialog_state(dialog) {
        set_window_text(state.query_edit, initial_text);
        let len = initial_text.encode_utf16().count();
        SendMessageW(state.query_edit, EM_SETSEL, len, len as LPARAM);
        SetFocus(state.query_edit);
    }

    run_modal_dialog(owner, dialog, |message| {
        find_dialog_handles_key(dialog, message)
    });
    Ok(result
        .lock()
        .expect("find dialog result mutex poisoned")
        .clone())
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

/// Help > About mirx: modal over `owner`; OK, Enter and Esc close it.
pub(super) unsafe fn show_about_dialog(owner: HWND) -> Result<(), String> {
    let class_name = wide_null(ABOUT_DIALOG_CLASS_NAME);
    let title = wide_null("About mirx");
    let dialog = CreateWindowExW(
        0,
        class_name.as_ptr(),
        title.as_ptr(),
        WS_OVERLAPPED | WS_CAPTION | WS_SYSMENU | WS_VISIBLE,
        CW_USEDEFAULT,
        CW_USEDEFAULT,
        380,
        230,
        owner,
        null_mut(),
        GetModuleHandleW(null()),
        null(),
    );
    if dialog.is_null() {
        return Err(last_os_error("CreateWindowExW about dialog"));
    }

    SetFocus(GetDlgItem(dialog, ABOUT_DIALOG_OK_ID as i32));
    run_modal_dialog(owner, dialog, |message| {
        let closes = message.message == WM_KEYDOWN
            && matches!(message.wParam, VK_RETURN_KEY | VK_ESCAPE_KEY)
            && (message.hwnd == dialog || IsChild(dialog, message.hwnd) != 0);
        if closes {
            DestroyWindow(dialog);
        }
        closes
    });
    Ok(())
}

/// Runs `dialog`'s message loop until it is destroyed, with `owner`
/// disabled: every key the user presses is the dialog's, so the frame's
/// accelerators do not apply. `handles_key` may take a message before it is
/// dispatched.
pub(super) unsafe fn run_modal_dialog(
    owner: HWND,
    dialog: HWND,
    mut handles_key: impl FnMut(&MSG) -> bool,
) {
    EnableWindow(owner, 0);
    let mut message = MSG::default();
    while IsWindow(dialog) != 0 {
        if GetMessageW(&mut message, null_mut(), 0, 0) <= 0 {
            // The application is quitting: close the dialog, and leave the
            // quit for the frame's loop.
            DestroyWindow(dialog);
            PostQuitMessage(message.wParam as i32);
            break;
        }
        if handles_key(&message) {
            continue;
        }
        TranslateMessage(&message);
        DispatchMessageW(&message);
    }
    EnableWindow(owner, 1);
}

unsafe fn login_dialog_state(hwnd: HWND) -> Option<&'static LoginDialogState> {
    window_data::get(hwnd).map(|state| &*state)
}

unsafe fn login_dialog_state_mut(hwnd: HWND) -> Option<&'static mut LoginDialogState> {
    window_data::get(hwnd)
}

unsafe fn recover_e2ee_dialog_state(hwnd: HWND) -> Option<&'static RecoverE2eeDialogState> {
    window_data::get(hwnd).map(|state| &*state)
}

unsafe fn recover_e2ee_dialog_state_mut(hwnd: HWND) -> Option<&'static mut RecoverE2eeDialogState> {
    window_data::get(hwnd)
}

unsafe fn setup_e2ee_dialog_state(hwnd: HWND) -> Option<&'static SetupE2eeRecoveryDialogState> {
    window_data::get(hwnd).map(|state| &*state)
}

unsafe fn setup_e2ee_dialog_state_mut(
    hwnd: HWND,
) -> Option<&'static mut SetupE2eeRecoveryDialogState> {
    window_data::get(hwnd)
}

unsafe fn recovery_key_handoff_dialog_state(
    hwnd: HWND,
) -> Option<&'static RecoveryKeyHandoffDialogState> {
    window_data::get(hwnd).map(|state| &*state)
}

unsafe fn recovery_key_handoff_dialog_state_mut(
    hwnd: HWND,
) -> Option<&'static mut RecoveryKeyHandoffDialogState> {
    window_data::get(hwnd)
}

unsafe fn clear_recovery_key_handoff_dialog_key(hwnd: HWND) {
    if let Some(state) = recovery_key_handoff_dialog_state(hwnd) {
        set_window_text(state.recovery_key_edit, "");
    }
}

unsafe fn find_dialog_state(hwnd: HWND) -> Option<&'static FindDialogState> {
    window_data::get(hwnd).map(|state| &*state)
}

unsafe fn find_dialog_state_mut(hwnd: HWND) -> Option<&'static mut FindDialogState> {
    window_data::get(hwnd)
}

unsafe fn find_dialog_handles_key(dialog: HWND, message: &MSG) -> bool {
    if message.message != WM_KEYDOWN
        || (message.hwnd != dialog && IsChild(dialog, message.hwnd) == 0)
    {
        return false;
    }

    match message.wParam {
        VK_RETURN_KEY => {
            SendMessageW(dialog, WM_COMMAND, FIND_DIALOG_OK_ID, 0);
            true
        }
        VK_ESCAPE_KEY => {
            DestroyWindow(dialog);
            true
        }
        _ => false,
    }
}

unsafe fn clear_recover_e2ee_dialog_secret(hwnd: HWND) {
    if let Some(state) = recover_e2ee_dialog_state(hwnd) {
        set_window_text(state.recovery_secret_edit, "");
    }
}

unsafe fn clear_setup_e2ee_dialog_passphrase(hwnd: HWND) {
    if let Some(state) = setup_e2ee_dialog_state(hwnd) {
        set_window_text(state.passphrase_edit, "");
    }
}

pub(super) unsafe fn show_e2ee_recovery_key(owner: HWND, recovery_key: &str) -> Result<(), String> {
    let instance = GetModuleHandleW(null());
    let mut dialog_state = window_data::CreateParam::new(RecoveryKeyHandoffDialogState {
        recovery_key: recovery_key.to_owned(),
        instruction_label: null_mut(),
        recovery_key_edit: null_mut(),
        acknowledge_button: null_mut(),
    });
    let class_name = wide_null(RECOVERY_KEY_HANDOFF_DIALOG_CLASS_NAME);
    let title = wide_null("E2EE Recovery Key");
    let dialog = CreateWindowExW(
        0,
        class_name.as_ptr(),
        title.as_ptr(),
        WS_OVERLAPPED | WS_CAPTION | WS_SYSMENU | WS_VISIBLE,
        CW_USEDEFAULT,
        CW_USEDEFAULT,
        460,
        210,
        owner,
        null_mut(),
        instance,
        dialog_state.as_param(),
    );
    if dialog.is_null() {
        return Err(last_os_error("CreateWindowExW recovery key handoff dialog"));
    }

    run_modal_dialog(owner, dialog, |_| false);
    Ok(())
}

#[cfg(test)]
mod tests {
    use windows_sys::Win32::System::Threading::GetCurrentThreadId;
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::IsWindowEnabled;
    use windows_sys::Win32::UI::WindowsAndMessaging::{EnumThreadWindows, GW_OWNER, GetWindow};
    use windows_sys::core::BOOL;

    use super::super::test_fixtures::*;
    use super::*;

    /// Types `c` and Enter into the frame's single-field dialog once its
    /// modal loop runs. The keys are posted to the dialog's own edit control,
    /// so that loop handles messages for the dialog's children.
    unsafe extern "system" fn type_into_single_field_dialog(
        frame: HWND,
        _message: u32,
        timer: usize,
        _time: u32,
    ) {
        unsafe extern "system" fn find_owned_dialog(hwnd: HWND, found: LPARAM) -> BOOL {
            let found = &mut *(found as *mut (HWND, HWND));
            if GetWindow(hwnd, GW_OWNER) == found.0 && window_class_is(hwnd, FIND_DIALOG_CLASS_NAME)
            {
                found.1 = hwnd;
            }
            1
        }
        KillTimer(frame, timer);
        let mut found = (frame, null_mut());
        EnumThreadWindows(
            GetCurrentThreadId(),
            Some(find_owned_dialog),
            &mut found as *mut (HWND, HWND) as LPARAM,
        );
        let edit = find_dialog_state(found.1)
            .expect("the single-field dialog is open")
            .query_edit;
        PostMessageW(edit, WM_CHAR, 'c' as WPARAM, 0);
        PostMessageW(edit, WM_KEYDOWN, VK_RETURN_KEY, 0);
    }

    #[test]
    fn a_single_field_dialog_takes_the_keys_typed_into_its_edit() {
        unsafe {
            with_test_frame(600, 400, |hwnd| {
                register_find_dialog_class(GetModuleHandleW(null())).unwrap();
                SetTimer(hwnd, 1, 0, Some(type_into_single_field_dialog));
                assert_eq!(
                    show_single_field_dialog(hwnd, "Find", "Find", "ab").unwrap(),
                    Some("abc".to_owned())
                );
                assert_ne!(IsWindowEnabled(hwnd), 0, "closing it enables the frame");
            });
        }
    }
}
