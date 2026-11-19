#![cfg(windows)]

use std::env;
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use mirx_core::{
    Event, RoomActivity, RoomActivityKind, RoomMembership, RoomMessageActivity,
    RoomMessagePresentationKind, RoomMessageSendState, RoomNotificationMode, RoomSummary,
};
use mirx_testkit::{LaunchOptions, MirxApp, ViewIdentity};
use windows_sys::Win32::Foundation::{HWND, WPARAM};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    GW_CHILD, GW_HWNDNEXT, GetClassNameW, GetDlgItem, GetWindow, SendMessageW, WM_MDIDESTROY,
    WM_MDIGETACTIVE,
};

fn user_profile(user_id: &str) -> mirx_core::RoomUserProfile {
    mirx_core::RoomUserProfile {
        user_id: user_id.to_owned(),
        display_name: None,
        display_name_ambiguous: false,
    }
}

const MDI_ID: i32 = 1002;
const MDI_CHILD_CLASS_NAME: &str = "mirxWin32MdiChild";
const POLL_INTERVAL: Duration = Duration::from_millis(50);

fn launch_options() -> LaunchOptions {
    LaunchOptions {
        startup_timeout: Duration::from_secs(20),
        local_app_data: None,
        frame_size: None,
    }
}

fn mirx_ui_exe_path() -> Result<PathBuf, String> {
    if let Some(path) = option_env!("CARGO_BIN_EXE_mirx-ui") {
        return Ok(PathBuf::from(path));
    }

    let current_exe = env::current_exe().map_err(|error| format!("current_exe failed: {error}"))?;
    let profile_dir = current_exe.parent().and_then(Path::parent).ok_or_else(|| {
        format!(
            "failed to derive profile dir from {}",
            current_exe.display()
        )
    })?;
    let candidate = profile_dir.join("mirx-ui.exe");
    if candidate.is_file() {
        Ok(candidate)
    } else {
        Err(format!(
            "mirx-ui.exe was not found; checked CARGO_BIN_EXE_mirx-ui and {}",
            candidate.display()
        ))
    }
}

fn unique_state_path() -> PathBuf {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock must be after unix epoch")
        .as_nanos();
    env::temp_dir().join(format!("mirx-ui-mdi-reactivation-{unique}.json"))
}

fn fixture_state_path(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/ui-states")
        .join(name)
}

fn write_state_file(events: &[Event]) -> Result<PathBuf, Box<dyn std::error::Error>> {
    let state_path = unique_state_path();
    fs::write(&state_path, serde_json::to_vec(events)?)?;
    Ok(state_path)
}

fn room_summary(room_id: &str, display_name: &str, encrypted: bool) -> RoomSummary {
    RoomSummary {
        room_id: room_id.to_owned(),
        display_name: display_name.to_owned(),
        topic: None,
        membership: RoomMembership::Join,
        encrypted,
        is_space: false,
        is_direct: false,
        direct_user_ids: Vec::new(),
        tags: Vec::new(),
        space_children: Vec::new(),
        notification_mode: Some(RoomNotificationMode::AllMessages),
        canonical_alias: None,
        alt_aliases: Vec::new(),
    }
}

fn sent_message_activity(
    room_id: &str,
    event_id: &str,
    sender: &str,
    timestamp_ms: u64,
    body: &str,
) -> RoomActivity {
    RoomActivity {
        room_id: room_id.to_owned(),
        activity_id: format!("event:{event_id}"),
        timestamp_ms,
        highlight: false,
        relation: None,
        kind: RoomActivityKind::Message(RoomMessageActivity {
            event_id: Some(event_id.to_owned()),
            transaction_id: None,
            sender: user_profile(sender),
            presentation: RoomMessagePresentationKind::Text,
            body: body.to_owned(),
            send_state: RoomMessageSendState::Sent,
        }),
    }
}

fn base_events() -> Vec<Event> {
    vec![
        Event::Started,
        Event::SyncedRooms(vec![
            room_summary("!beta:example", "Beta", true),
            room_summary("!alpha:example", "Alpha", false),
        ]),
        Event::RoomActivity(Box::new(sent_message_activity(
            "!alpha:example",
            "$alpha-first",
            "@alice:example",
            1_700_000_000_000,
            "alpha first",
        ))),
        Event::RoomActivity(Box::new(sent_message_activity(
            "!beta:example",
            "$beta-first",
            "@bob:example",
            1_700_000_060_000,
            "beta first",
        ))),
    ]
}

#[test]
fn select_room_recreates_room_mdi_child_after_external_destroy()
-> Result<(), Box<dyn std::error::Error>> {
    let exe_path =
        mirx_ui_exe_path().map_err(|error| format!("failed to locate mirx-ui.exe: {error}"))?;
    let state_path = write_state_file(&base_events())?;

    let args = [
        OsString::from("--render-state"),
        state_path.as_os_str().to_os_string(),
    ];
    let app = MirxApp::launch_with_options(&exe_path, &args, launch_options())?;
    app.wait_idle(Duration::from_secs(5))?;

    let frame = app.automation_frame_hwnd() as HWND;
    let mdi_client = wait_for_mdi_client(frame, Duration::from_secs(5))?;
    let baseline_count = wait_for_nonzero_child_count(frame, Duration::from_secs(5))?;
    let beta_child = unsafe { SendMessageW(mdi_client, WM_MDIGETACTIVE, 0, 0) as HWND };
    if beta_child.is_null() {
        return Err("active MDI child was not found".into());
    }

    unsafe {
        SendMessageW(mdi_client, WM_MDIDESTROY, beta_child as WPARAM, 0);
    }

    wait_for_child_count(
        frame,
        baseline_count.saturating_sub(1),
        Duration::from_secs(5),
    )?;

    app.select_room("!beta:example")?;
    wait_for_child_count(frame, baseline_count, Duration::from_secs(5))?;

    let _ = fs::remove_file(&state_path);
    Ok(())
}

#[test]
fn select_thread_recreates_thread_mdi_child_after_external_destroy()
-> Result<(), Box<dyn std::error::Error>> {
    let exe_path =
        mirx_ui_exe_path().map_err(|error| format!("failed to locate mirx-ui.exe: {error}"))?;
    let fixture_path = fixture_state_path("thread-hierarchy.json");
    let args = [
        OsString::from("--render-state"),
        fixture_path.as_os_str().to_os_string(),
    ];
    let app = MirxApp::launch_with_options(&exe_path, &args, launch_options())?;
    app.wait_idle(Duration::from_secs(5))?;

    let thread_view = ViewIdentity::Thread {
        room_id: "!ops:example".to_owned(),
        thread_root_event_id: "$thread-root".to_owned(),
    };
    app.select_view(thread_view)?;
    app.wait_idle(Duration::from_secs(5))?;

    let frame = app.automation_frame_hwnd() as HWND;
    let mdi_client = wait_for_mdi_client(frame, Duration::from_secs(5))?;
    let baseline_count = wait_for_nonzero_child_count(frame, Duration::from_secs(5))?;
    let thread_child = unsafe { SendMessageW(mdi_client, WM_MDIGETACTIVE, 0, 0) as HWND };
    if thread_child.is_null() {
        return Err("active thread MDI child was not found".into());
    }

    unsafe {
        SendMessageW(mdi_client, WM_MDIDESTROY, thread_child as WPARAM, 0);
    }

    wait_for_child_count(
        frame,
        baseline_count.saturating_sub(1),
        Duration::from_secs(5),
    )?;

    app.select_view(ViewIdentity::Thread {
        room_id: "!ops:example".to_owned(),
        thread_root_event_id: "$thread-root".to_owned(),
    })?;
    wait_for_child_count(frame, baseline_count, Duration::from_secs(5))?;

    Ok(())
}

fn wait_for_nonzero_child_count(frame: HWND, timeout: Duration) -> Result<usize, String> {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        let count = mdi_child_count(frame);
        if count > 0 {
            return Ok(count);
        }
        thread::sleep(POLL_INTERVAL);
    }

    Err(format!(
        "timed out waiting for nonzero MDI child count; actual {}",
        mdi_child_count(frame)
    ))
}

fn wait_for_mdi_client(frame: HWND, timeout: Duration) -> Result<HWND, String> {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if let Some(mdi_client) = mdi_client(frame) {
            return Ok(mdi_client);
        }
        thread::sleep(POLL_INTERVAL);
    }

    Err("timed out waiting for MDI client child window".to_owned())
}

fn wait_for_child_count(
    frame: HWND,
    expected_count: usize,
    timeout: Duration,
) -> Result<(), String> {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if mdi_child_count(frame) == expected_count {
            return Ok(());
        }
        thread::sleep(POLL_INTERVAL);
    }

    Err(format!(
        "timed out waiting for {} MDI child window(s); actual {}",
        expected_count,
        mdi_child_count(frame)
    ))
}

fn mdi_client(frame: HWND) -> Option<HWND> {
    let hwnd = unsafe { GetDlgItem(frame, MDI_ID) };
    (!hwnd.is_null()).then_some(hwnd)
}

fn mdi_child_count(frame: HWND) -> usize {
    let Some(mdi_client) = mdi_client(frame) else {
        return 0;
    };

    let mut count = 0;
    let mut child = unsafe { GetWindow(mdi_client, GW_CHILD) };
    while !child.is_null() {
        if class_name(child).as_deref() == Some(MDI_CHILD_CLASS_NAME) {
            count += 1;
        }
        child = unsafe { GetWindow(child, GW_HWNDNEXT) };
    }

    count
}

fn class_name(hwnd: HWND) -> Option<String> {
    let mut buffer = [0u16; 128];
    let len = unsafe { GetClassNameW(hwnd, buffer.as_mut_ptr(), buffer.len() as i32) };
    if len <= 0 {
        return None;
    }
    Some(String::from_utf16_lossy(&buffer[..len as usize]))
}
