#![cfg(windows)]

use std::env;
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use mmatrix_testkit::{
    MmatrixApp, Room, SyntheticRoom, SyntheticState, SyntheticTimeline, TestkitError, TimelineLine,
};

fn mmatrix_ui_exe_path() -> Result<PathBuf, String> {
    if let Some(path) = option_env!("CARGO_BIN_EXE_mmatrix-ui") {
        return Ok(PathBuf::from(path));
    }

    // Fallback for layouts where Cargo does not inject the binary path into this test.
    let current_exe = env::current_exe().map_err(|error| format!("current_exe failed: {error}"))?;
    let profile_dir = current_exe.parent().and_then(Path::parent).ok_or_else(|| {
        format!(
            "failed to derive profile dir from {}",
            current_exe.display()
        )
    })?;
    let candidate = profile_dir.join("mmatrix-ui.exe");
    if candidate.is_file() {
        Ok(candidate)
    } else {
        Err(format!(
            "mmatrix-ui.exe was not found; checked CARGO_BIN_EXE_mmatrix-ui and {}",
            candidate.display()
        ))
    }
}

fn unique_state_path() -> PathBuf {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock must be after unix epoch")
        .as_nanos();
    env::temp_dir().join(format!("mmatrix-ui-automation-driver-{unique}.json"))
}

fn unique_screenshot_path() -> PathBuf {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock must be after unix epoch")
        .as_nanos();
    env::temp_dir().join(format!("mmatrix-ui-automation-driver-{unique}.png"))
}

fn write_state_file(state: &SyntheticState) -> Result<PathBuf, Box<dyn std::error::Error>> {
    let state_path = unique_state_path();
    fs::write(&state_path, serde_json::to_vec(state)?)?;
    Ok(state_path)
}

fn base_state() -> SyntheticState {
    SyntheticState {
        rooms: vec![
            SyntheticRoom {
                room_id: "!alpha:example".to_owned(),
                display_name: "Alpha".to_owned(),
                encrypted: false,
            },
            SyntheticRoom {
                room_id: "!beta:example".to_owned(),
                display_name: "Beta".to_owned(),
                encrypted: true,
            },
        ],
        timelines: vec![
            SyntheticTimeline {
                room_id: "!alpha:example".to_owned(),
                lines: vec!["alpha first".to_owned()],
            },
            SyntheticTimeline {
                room_id: "!beta:example".to_owned(),
                lines: vec!["beta first".to_owned(), "beta second".to_owned()],
            },
        ],
        selected_room: Some("!beta:example".to_owned()),
        verification: None,
        error: None,
    }
}

fn replacement_state() -> SyntheticState {
    SyntheticState {
        rooms: vec![
            SyntheticRoom {
                room_id: "!gamma:example".to_owned(),
                display_name: "Gamma".to_owned(),
                encrypted: false,
            },
            SyntheticRoom {
                room_id: "!delta:example".to_owned(),
                display_name: "Delta".to_owned(),
                encrypted: false,
            },
        ],
        timelines: vec![
            SyntheticTimeline {
                room_id: "!gamma:example".to_owned(),
                lines: vec!["gamma only".to_owned()],
            },
            SyntheticTimeline {
                room_id: "!delta:example".to_owned(),
                lines: vec!["delta first".to_owned(), "delta second".to_owned()],
            },
        ],
        selected_room: Some("!gamma:example".to_owned()),
        verification: None,
        error: None,
    }
}

#[test]
fn automation_driver_launches_real_ui_and_reads_synthetic_state()
-> Result<(), Box<dyn std::error::Error>> {
    let exe_path = mmatrix_ui_exe_path()
        .map_err(|error| format!("failed to locate mmatrix-ui.exe: {error}"))?;
    let state = base_state();
    let state_path = write_state_file(&state)?;

    let args = [
        OsString::from("--render-state"),
        state_path.as_os_str().to_os_string(),
    ];
    let app = MmatrixApp::launch(&exe_path, &args)?;
    app.wait_idle(Duration::from_secs(5))?;

    let model = app.dump_model()?;
    assert_eq!(
        model.rooms,
        vec![
            Room {
                room_id: "!alpha:example".to_owned(),
                display_name: "Alpha".to_owned(),
                encrypted: false,
            },
            Room {
                room_id: "!beta:example".to_owned(),
                display_name: "Beta".to_owned(),
                encrypted: true,
            },
        ]
    );
    assert_eq!(model.selected_room.as_deref(), Some("!beta:example"));
    assert_eq!(
        model.last_timeline,
        Some(TimelineLine {
            room_id: "!beta:example".to_owned(),
            body: "beta second".to_owned(),
        })
    );

    let timeline = app.timeline(Some("!beta:example"))?;
    assert_eq!(
        timeline,
        vec!["beta first".to_owned(), "beta second".to_owned()]
    );

    app.wait_for_timeline_contains(Some("!beta:example"), "beta second", Duration::from_secs(5))?;
    let screenshot_path = unique_screenshot_path();
    app.screenshot(&screenshot_path)?;
    let screenshot_bytes = fs::read(&screenshot_path)?;
    assert!(screenshot_bytes.starts_with(b"\x89PNG\r\n\x1a\n"));

    let _ = fs::remove_file(&state_path);
    let _ = fs::remove_file(&screenshot_path);
    Ok(())
}

#[test]
fn automation_driver_can_attach_and_replace_state_via_testkit()
-> Result<(), Box<dyn std::error::Error>> {
    let exe_path = mmatrix_ui_exe_path()
        .map_err(|error| format!("failed to locate mmatrix-ui.exe: {error}"))?;
    let state_path = write_state_file(&base_state())?;

    let args = [
        OsString::from("--render-state"),
        state_path.as_os_str().to_os_string(),
    ];
    let app = MmatrixApp::launch(&exe_path, &args)?;
    app.wait_idle(Duration::from_secs(5))?;

    let attached = MmatrixApp::find(Duration::from_secs(5))?;
    attached.ping()?;

    let version = attached.get_version()?;
    assert_eq!(version.protocol_version, 1);
    assert_eq!(version.app_version, env!("CARGO_PKG_VERSION"));

    assert_eq!(
        attached.list_rooms()?,
        vec![
            Room {
                room_id: "!alpha:example".to_owned(),
                display_name: "Alpha".to_owned(),
                encrypted: false,
            },
            Room {
                room_id: "!beta:example".to_owned(),
                display_name: "Beta".to_owned(),
                encrypted: true,
            },
        ]
    );

    attached.select_room("!alpha:example")?;
    assert_eq!(attached.timeline(None)?, vec!["alpha first".to_owned()]);

    let replacement = replacement_state();
    attached.load_synthetic_state(&replacement)?;
    attached.wait_idle(Duration::from_secs(5))?;

    assert_eq!(
        attached.list_rooms()?,
        vec![
            Room {
                room_id: "!gamma:example".to_owned(),
                display_name: "Gamma".to_owned(),
                encrypted: false,
            },
            Room {
                room_id: "!delta:example".to_owned(),
                display_name: "Delta".to_owned(),
                encrypted: false,
            },
        ]
    );
    assert_eq!(attached.timeline(None)?, vec!["gamma only".to_owned()]);
    assert_eq!(
        attached.dump_model()?.last_timeline,
        Some(TimelineLine {
            room_id: "!delta:example".to_owned(),
            body: "delta second".to_owned(),
        })
    );

    match attached.select_room("!missing:example") {
        Err(TestkitError::Protocol { code, message }) => {
            assert_eq!(code, "unknown_room");
            assert_eq!(message, "unknown room id: !missing:example");
        }
        other => panic!("expected protocol error for unknown room, got {other:?}"),
    }

    let _ = fs::remove_file(&state_path);
    Ok(())
}
