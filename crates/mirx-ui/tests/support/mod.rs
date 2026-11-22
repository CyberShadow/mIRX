//! GUI helpers the Wine suites share: each declares `mod support;`.

use std::env;
use std::ffi::OsString;
use std::fs;
use std::ops::Deref;
use std::path::{Path, PathBuf};
use std::process::ExitStatus;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, MutexGuard, OnceLock};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use mirx_testkit::{
    ComposeKey, LaunchOptions, MdiView, MirxApp, TestkitError, UiState, ViewIdentity,
};
use windows_sys::Win32::Foundation::{HWND, WPARAM};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    FindWindowW, GetDlgItem, IsWindow, IsWindowVisible, PostMessageW, SendMessageW, SetWindowTextW,
    WM_COMMAND, WM_GETTEXT, WM_GETTEXTLENGTH,
};

pub const POLL_INTERVAL: Duration = Duration::from_millis(25);

static TEMP_PATH_COUNTER: AtomicU64 = AtomicU64::new(1);

pub fn unique_temp_path(prefix: &str, extension: Option<&str>) -> PathBuf {
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock must be after unix epoch")
        .as_nanos();
    let counter = TEMP_PATH_COUNTER.fetch_add(1, Ordering::Relaxed);
    let stem = format!("{prefix}-{}-{timestamp}-{counter}", std::process::id());
    match extension {
        Some(extension) => env::temp_dir().join(format!("{stem}.{extension}")),
        None => env::temp_dir().join(stem),
    }
}

pub fn unique_local_app_data_path() -> PathBuf {
    unique_temp_path("mirx-ui-local-app-data", None)
}

pub fn session_file_path(local_app_data: &Path) -> PathBuf {
    local_app_data
        .join("mirx")
        .join("ui-shell")
        .join("session.json")
}

pub fn settings_file_path(local_app_data: &Path) -> PathBuf {
    local_app_data
        .join("mirx")
        .join("ui-shell")
        .join("mirx.ini")
}

/// Writes `ini` as the whole settings file.
pub fn write_settings(local_app_data: &Path, ini: &str) -> Result<(), Box<dyn std::error::Error>> {
    let path = settings_file_path(local_app_data);
    fs::create_dir_all(path.parent().ok_or("the settings file has a folder")?)?;
    fs::write(path, ini)?;
    Ok(())
}

/// `connect` is the `[connect]` section's keys after `startup`.
pub fn write_connect_settings(
    local_app_data: &Path,
    connect: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    write_settings(
        local_app_data,
        &format!("[connect]\r\nstartup=on\r\n{connect}"),
    )
}

pub fn launch_options(local_app_data: Option<PathBuf>) -> LaunchOptions {
    LaunchOptions {
        startup_timeout: Duration::from_secs(20),
        local_app_data,
        frame_size: None,
    }
}

pub struct GuiTestApp {
    app: MirxApp,
    _gui_test_guard: MutexGuard<'static, ()>,
}

impl Deref for GuiTestApp {
    type Target = MirxApp;

    fn deref(&self) -> &Self::Target {
        &self.app
    }
}

impl GuiTestApp {
    /// Ends mirx with `end` (closing it, or waiting for it to exit), still
    /// holding the GUI lock.
    pub fn end<T>(self, end: impl FnOnce(MirxApp) -> T) -> T {
        end(self.app)
    }

    /// Closes mirx and waits for it to exit, still holding the GUI lock.
    pub fn close(self, timeout: Duration) -> Result<ExitStatus, TestkitError> {
        self.end(|app| app.close(timeout))
    }
}

fn gui_test_lock() -> &'static Mutex<()> {
    static GUI_TEST_LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    GUI_TEST_LOCK.get_or_init(|| Mutex::new(()))
}

fn lock_gui_test() -> MutexGuard<'static, ()> {
    let lock = gui_test_lock();
    match lock.lock() {
        Ok(guard) => guard,
        Err(poisoned) => {
            lock.clear_poison();
            poisoned.into_inner()
        }
    }
}

pub fn launch_app_with_options(
    exe_path: &Path,
    args: &[OsString],
    options: LaunchOptions,
) -> Result<GuiTestApp, Box<dyn std::error::Error>> {
    let guard = lock_gui_test();
    let app = mirx_testkit::MirxApp::launch_with_options(exe_path, args, options)?;
    Ok(GuiTestApp {
        app,
        _gui_test_guard: guard,
    })
}

pub fn assert_bytes_do_not_contain(bytes: &[u8], needle: &str) {
    assert!(
        !bytes
            .windows(needle.len())
            .any(|window| window == needle.as_bytes()),
        "bytes unexpectedly contained {needle:?}"
    );
}

pub fn room_identity(room_id: &str) -> ViewIdentity {
    ViewIdentity::Room {
        room_id: room_id.to_owned(),
    }
}

pub fn find_view<'a>(ui_state: &'a UiState, identity: &ViewIdentity) -> &'a MdiView {
    ui_state
        .mdi
        .views
        .iter()
        .find(|view| &view.identity == identity)
        .unwrap_or_else(|| panic!("missing MDI view for {identity:?}"))
}

pub fn view_contains(view: &MdiView, substring: &str) -> bool {
    view.lines.iter().any(|line| line.contains(substring))
}

pub fn invoke_account_menu_command(
    app: &MirxApp,
    command_id: usize,
) -> Result<(), Box<dyn std::error::Error>> {
    let frame = app.automation_frame_hwnd() as HWND;
    unsafe {
        SendMessageW(frame, WM_COMMAND, command_id as WPARAM, 0);
    }
    app.wait_idle(Duration::from_secs(5))?;
    Ok(())
}

pub fn wide_null(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}

pub const MENU_ID_FILE_CONNECT: usize = 40030;
pub const MENU_ID_FILE_DISCONNECT: usize = 40031;
pub const MENU_ID_FILE_SELECT_SERVER: usize = 40032;
pub const MENU_ID_FILE_LOG_OUT: usize = 40004;
pub const LOGIN_DIALOG_SERVER_ID: i32 = 5001;
pub const LOGIN_DIALOG_USER_ID: i32 = 5002;
pub const LOGIN_DIALOG_PASSWORD_ID: i32 = 5003;
pub const LOGIN_DIALOG_OK_ID: usize = 5005;

/// Launches `exe` (mirx-ui.exe) for automation on `local_app_data` and
/// waits for its core to start.
pub fn launch_automation_app(
    exe: &Path,
    local_app_data: &Path,
) -> Result<GuiTestApp, Box<dyn std::error::Error>> {
    let app = launch_app_with_options(
        exe,
        &[OsString::from("--automation")],
        launch_options(Some(local_app_data.to_path_buf())),
    )?;
    app.wait_for_lifecycle_state("core_started", Duration::from_secs(30))?;
    Ok(app)
}

pub fn status_view_contains(app: &MirxApp, line: &str) -> Result<bool, Box<dyn std::error::Error>> {
    Ok(view_contains(
        find_view(&app.dump_ui_state()?, &ViewIdentity::Status),
        line,
    ))
}

/// The SDK's stores of the session mirx signed in.
pub fn store_dir_path(local_app_data: &Path) -> PathBuf {
    local_app_data
        .join("mirx")
        .join("ui-shell")
        .join("matrix-store")
}

/// Signs in through Select Server and waits for the sync.
pub fn sign_in_with_select_server(
    app: &MirxApp,
    homeserver_url: &str,
    user: &str,
    password: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let (dialog, menu_thread) = open_select_server(app, [homeserver_url, user, password])?;
    submit_select_server(dialog, menu_thread)?;
    app.wait_for_connection("syncing", Duration::from_secs(30))?;
    Ok(())
}

/// The Log Out dialog, which opens when logging out would lose room keys.
pub const LOG_OUT_DIALOG_CLASS: &str = "mirxWin32LogOutDialog";
/// Its Log Out button.
pub const LOG_OUT_DIALOG_LOG_OUT_ID: usize = 5020;

/// File > Log Out of a session whose key backup a new session can open:
/// it logs out without a word.
#[allow(
    dead_code,
    reason = "only the live suite's server sets up recovery, which this needs"
)]
pub fn log_out_with_the_menu(
    app: &MirxApp,
    local_app_data: &Path,
) -> Result<(), Box<dyn std::error::Error>> {
    invoke_account_menu_command(app, MENU_ID_FILE_LOG_OUT)?;
    let class = wide_null(LOG_OUT_DIALOG_CLASS);
    let deadline = Instant::now() + Duration::from_secs(30);
    while app.dump_model()?.connection != "signed_out" {
        let dialog = unsafe { FindWindowW(class.as_ptr(), std::ptr::null()) };
        if !dialog.is_null() {
            return Err("Log Out warned that room keys would be lost".into());
        }
        if Instant::now() >= deadline {
            return Err("Log Out did not log out within 30 seconds".into());
        }
        std::thread::sleep(POLL_INTERVAL);
    }
    assert_logged_out(app, local_app_data)
}

/// File > Log Out, and Log Out in the dialog that warns that room keys
/// would be lost.
pub fn log_out_anyway_with_the_menu(
    app: &MirxApp,
    local_app_data: &Path,
) -> Result<(), Box<dyn std::error::Error>> {
    start_log_out_anyway_with_the_menu(app)?;
    app.wait_for_connection("signed_out", Duration::from_secs(30))?;
    assert_logged_out(app, local_app_data)
}

/// The same, up to the dialog's close: the Log Out is under way.
pub fn start_log_out_anyway_with_the_menu(app: &MirxApp) -> Result<(), Box<dyn std::error::Error>> {
    invoke_account_menu_command(app, MENU_ID_FILE_LOG_OUT)?;
    let dialog = app.wait_for_modal_dialog(Duration::from_secs(30))?;
    assert_eq!(
        (dialog.class.as_str(), dialog.title.as_str()),
        (LOG_OUT_DIALOG_CLASS, "Log Out")
    );
    let dialog = dialog.hwnd as HWND;
    assert_ne!(
        unsafe { PostMessageW(dialog, WM_COMMAND, LOG_OUT_DIALOG_LOG_OUT_ID as WPARAM, 0) },
        0
    );
    wait_for_window_gone(dialog, "Log Out dialog")
}

/// What Log Out leaves: no session file, no store.
fn assert_logged_out(
    app: &MirxApp,
    local_app_data: &Path,
) -> Result<(), Box<dyn std::error::Error>> {
    wait_for_status_line(app, "* Logged out", Duration::from_secs(10))?;
    app.wait_idle(Duration::from_secs(5))?;
    assert_eq!(app.dump_model()?.last_error, None);
    assert!(!session_file_path(local_app_data).exists());
    assert!(!store_dir_path(local_app_data).exists());
    Ok(())
}

/// The Status window's lines.
pub fn status_view_lines(app: &MirxApp) -> Result<Vec<String>, Box<dyn std::error::Error>> {
    Ok(find_view(&app.dump_ui_state()?, &ViewIdentity::Status)
        .lines
        .clone())
}

pub fn wait_for_status_line(
    app: &MirxApp,
    line: &str,
    timeout: Duration,
) -> Result<(), Box<dyn std::error::Error>> {
    let deadline = Instant::now() + timeout;
    loop {
        let lines = status_view_lines(app)?;
        if lines.iter().any(|status_line| status_line.contains(line)) {
            return Ok(());
        }
        if Instant::now() >= deadline {
            return Err(format!("Status never showed {line:?}: {lines:?}").into());
        }
        std::thread::sleep(POLL_INTERVAL);
    }
}

/// Opens File > Select Server..., which runs modally on the thread this
/// returns, and fills in its fields; the dialog is returned open.
pub fn open_select_server(
    app: &MirxApp,
    fields: [&str; 3],
) -> Result<(HWND, std::thread::JoinHandle<()>), Box<dyn std::error::Error>> {
    let frame_value = app.automation_frame_hwnd();
    let menu_thread = std::thread::spawn(move || unsafe {
        SendMessageW(
            frame_value as HWND,
            WM_COMMAND,
            MENU_ID_FILE_SELECT_SERVER as WPARAM,
            0,
        );
    });
    let class_name = wide_null("mirxWin32LoginDialog");
    let dialog = wait_for_window(&class_name, "Select Server dialog")?;
    assert_eq!(unsafe { read_window_text(dialog) }, "Select Server");
    for (control_id, text) in [
        LOGIN_DIALOG_SERVER_ID,
        LOGIN_DIALOG_USER_ID,
        LOGIN_DIALOG_PASSWORD_ID,
    ]
    .into_iter()
    .zip(fields)
    {
        let edit = unsafe { GetDlgItem(dialog, control_id) };
        assert!(!edit.is_null(), "login dialog control {control_id}");
        let text = wide_null(text);
        assert_ne!(unsafe { SetWindowTextW(edit, text.as_ptr()) }, 0);
    }
    Ok((dialog, menu_thread))
}

/// Presses OK on a valid Select Server form, which signs in.
pub fn submit_select_server(
    dialog: HWND,
    menu_thread: std::thread::JoinHandle<()>,
) -> Result<(), Box<dyn std::error::Error>> {
    assert_ne!(
        unsafe { PostMessageW(dialog, WM_COMMAND, LOGIN_DIALOG_OK_ID as WPARAM, 0) },
        0
    );
    wait_for_window_gone(dialog, "Select Server dialog")?;
    menu_thread
        .join()
        .expect("Select Server menu command thread must not panic");
    Ok(())
}

/// Waits for a window of `class_name` to show. A dialog exists before its
/// controls do: it shows once they are there.
pub fn wait_for_window(class_name: &[u16], what: &str) -> Result<HWND, Box<dyn std::error::Error>> {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let window = unsafe { FindWindowW(class_name.as_ptr(), std::ptr::null()) };
        if !window.is_null() && unsafe { IsWindowVisible(window) } != 0 {
            return Ok(window);
        }
        if Instant::now() >= deadline {
            return Err(format!("{what} did not appear within five seconds").into());
        }
        std::thread::sleep(POLL_INTERVAL);
    }
}

pub fn wait_for_window_gone(window: HWND, what: &str) -> Result<(), Box<dyn std::error::Error>> {
    let deadline = Instant::now() + Duration::from_secs(5);
    while unsafe { IsWindow(window) } != 0 {
        if Instant::now() >= deadline {
            return Err(format!("{what} did not close within five seconds").into());
        }
        std::thread::sleep(POLL_INTERVAL);
    }
    Ok(())
}

pub unsafe fn read_window_text(hwnd: HWND) -> String {
    let len = unsafe { SendMessageW(hwnd, WM_GETTEXTLENGTH, 0, 0) };
    let mut buffer = vec![0u16; len as usize + 1];
    unsafe { SendMessageW(hwnd, WM_GETTEXT, buffer.len(), buffer.as_mut_ptr() as isize) };
    String::from_utf16_lossy(&buffer[..len as usize])
}

/// Types each command in the active window and waits for it to run.
pub fn type_commands(app: &MirxApp, inputs: &[&str]) -> Result<(), Box<dyn std::error::Error>> {
    for input in inputs {
        app.set_compose_text(input)?;
        app.send_compose_key(ComposeKey::Return)?;
        app.wait_idle(Duration::from_secs(5))?;
        assert_eq!(app.compose_text()?, "", "{input}");
    }
    Ok(())
}
