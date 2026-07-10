#![cfg(windows)]

use std::env;
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use mmatrix_core::{Event, RoomMembership, RoomReadState, RoomSummary, SpaceChildSummary};
use mmatrix_testkit::{LaunchOptions, MmatrixApp, NavigationPathSegment};

fn mmatrix_ui_exe_path() -> Result<PathBuf, String> {
    if let Some(path) = option_env!("CARGO_BIN_EXE_mmatrix-ui") {
        return Ok(PathBuf::from(path));
    }

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
    env::temp_dir().join(format!("mmatrix-ui-verify-duplicate-labels-{unique}.json"))
}

fn write_state_file(events: &[Event]) -> Result<PathBuf, Box<dyn std::error::Error>> {
    let state_path = unique_state_path();
    fs::write(&state_path, serde_json::to_vec(events)?)?;
    Ok(state_path)
}

fn space(room_id: &str, display_name: &str, children: Vec<&str>) -> RoomSummary {
    RoomSummary {
        room_id: room_id.to_owned(),
        display_name: display_name.to_owned(),
            topic: None,
        membership: RoomMembership::Join,
        encrypted: false,
        is_space: true,
        is_direct: false,
        direct_user_ids: Vec::new(),
        tags: Vec::new(),
        space_children: children
            .into_iter()
            .map(|room_id| SpaceChildSummary {
                room_id: room_id.to_owned(),
                suggested: false,
                order: None,
            })
            .collect(),
    }
}

fn room(room_id: &str, display_name: &str) -> RoomSummary {
    RoomSummary {
        room_id: room_id.to_owned(),
        display_name: display_name.to_owned(),
            topic: None,
        membership: RoomMembership::Join,
        encrypted: false,
        is_space: false,
        is_direct: false,
        direct_user_ids: Vec::new(),
        tags: Vec::new(),
        space_children: Vec::new(),
    }
}

#[test]
fn selected_navigation_path_round_trips_for_duplicate_sibling_labels()
-> Result<(), Box<dyn std::error::Error>> {
    let exe_path = mmatrix_ui_exe_path()
        .map_err(|error| format!("failed to locate mmatrix-ui.exe: {error}"))?;
    let state_path = write_state_file(&[
        Event::Started,
        Event::SyncedRooms(vec![
            space(
                "!space:example",
                "Space",
                vec!["!room1:example", "!room2:example"],
            ),
            room("!room1:example", "Same"),
            room("!room2:example", "Same"),
        ]),
    ])?;

    let args = [
        OsString::from("--render-state"),
        state_path.as_os_str().to_os_string(),
    ];
    let app = MmatrixApp::launch_with_options(
        &exe_path,
        &args,
        LaunchOptions {
            startup_timeout: Duration::from_secs(20),
            local_app_data: None,
            frame_size: None,
        },
    )?;
    app.wait_idle(Duration::from_secs(5))?;

    app.select_room("!room2:example")?;
    let path = app.selected_navigation_path()?;
    assert_eq!(
        path,
        vec![
            NavigationPathSegment::first("mmatrix"),
            NavigationPathSegment::first("rooms: 2"),
            NavigationPathSegment::first("Space"),
            NavigationPathSegment {
                text: "Same".to_owned(),
                same_text_index: 1,
            },
        ]
    );

    app.select_navigation_path(&path)?;
    assert_eq!(
        app.dump_model()?.selected_room.as_deref(),
        Some("!room2:example"),
        "re-selecting the exported navigation path should keep the same room selected",
    );

    let _ = fs::remove_file(&state_path);
    Ok(())
}

#[test]
fn selected_navigation_path_round_trips_for_decorated_duplicate_sibling_labels()
-> Result<(), Box<dyn std::error::Error>> {
    let exe_path = mmatrix_ui_exe_path()
        .map_err(|error| format!("failed to locate mmatrix-ui.exe: {error}"))?;
    let state_path = write_state_file(&[
        Event::Started,
        Event::SyncedRooms(vec![
            space(
                "!space:example",
                "Space",
                vec!["!room1:example", "!room2:example"],
            ),
            room("!room1:example", "Same"),
            room("!room2:example", "Same"),
        ]),
        Event::RoomReadState(RoomReadState {
            room_id: "!room1:example".to_owned(),
            latest_active_event_id: Some("$latest-room1".to_owned()),
            fully_read_event_id: Some("$fully-read-room1".to_owned()),
            marked_unread: true,
            num_unread: 3,
            num_notifications: 2,
            num_mentions: 1,
        }),
        Event::RoomReadState(RoomReadState {
            room_id: "!room2:example".to_owned(),
            latest_active_event_id: Some("$latest-room2".to_owned()),
            fully_read_event_id: Some("$fully-read-room2".to_owned()),
            marked_unread: true,
            num_unread: 3,
            num_notifications: 2,
            num_mentions: 1,
        }),
    ])?;

    let args = [
        OsString::from("--render-state"),
        state_path.as_os_str().to_os_string(),
    ];
    let app = MmatrixApp::launch_with_options(
        &exe_path,
        &args,
        LaunchOptions {
            startup_timeout: Duration::from_secs(20),
            local_app_data: None,
            frame_size: None,
        },
    )?;
    app.wait_idle(Duration::from_secs(5))?;

    app.select_room("!room2:example")?;
    let path = app.selected_navigation_path()?;
    assert_eq!(
        path,
        vec![
            NavigationPathSegment::first("mmatrix"),
            NavigationPathSegment::first("rooms: 2"),
            NavigationPathSegment::first("Space"),
            NavigationPathSegment {
                text: "Same [@1 !2 *3 M]".to_owned(),
                same_text_index: 1,
            },
        ]
    );

    app.select_navigation_path(&path)?;
    assert_eq!(
        app.dump_model()?.selected_room.as_deref(),
        Some("!room2:example"),
        "re-selecting the exported navigation path should keep the same room selected",
    );

    let _ = fs::remove_file(&state_path);
    Ok(())
}
