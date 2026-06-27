#![cfg(windows)]

use std::env;
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use mmatrix_core::{Event, RoomSummary, StoredSession as CoreStoredSession, TimelineText};
use mmatrix_testkit::{
    LaunchOptions, MdiView, MmatrixApp, NavigationNode, Room, StoredSession, TestkitError,
    TimelineLine, UiError, UiState, Verification, ViewIdentity,
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

fn unique_local_app_data_path() -> PathBuf {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock must be after unix epoch")
        .as_nanos();
    env::temp_dir().join(format!("mmatrix-ui-local-app-data-{unique}"))
}

fn session_file_path(local_app_data: &Path) -> PathBuf {
    local_app_data
        .join("mmatrix")
        .join("ui-shell")
        .join("session.json")
}

fn launch_options(local_app_data: Option<PathBuf>) -> LaunchOptions {
    LaunchOptions {
        startup_timeout: Duration::from_secs(20),
        local_app_data,
    }
}

fn stored_session() -> StoredSession {
    StoredSession {
        user_id: "@alice:example".to_owned(),
        device_id: "DEVICE".to_owned(),
        access_token: "secret-token".to_owned(),
        refresh_token: Some("refresh-token".to_owned()),
    }
}

fn core_stored_session() -> CoreStoredSession {
    CoreStoredSession {
        user_id: "@alice:example".to_owned(),
        device_id: "DEVICE".to_owned(),
        access_token: "secret-token".to_owned(),
        refresh_token: Some("refresh-token".to_owned()),
    }
}

fn write_saved_session(local_app_data: &Path) -> Result<PathBuf, Box<dyn std::error::Error>> {
    let path = session_file_path(local_app_data);
    let parent = path
        .parent()
        .ok_or_else(|| format!("missing parent for {}", path.display()))?;
    fs::create_dir_all(parent)?;
    fs::write(
        &path,
        serde_json::to_vec(&serde_json::json!({
            "version": 1,
            "homeserver_url": "https://matrix.example",
            "session": {
                "user_id": "@alice:example",
                "device_id": "DEVICE",
                "access_token": "secret-token",
                "refresh_token": "refresh-token"
            }
        }))?,
    )?;
    Ok(path)
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

fn base_events() -> Vec<Event> {
    vec![
        Event::Started,
        Event::SyncedRooms(vec![
            RoomSummary {
                room_id: "!beta:example".to_owned(),
                display_name: "Beta".to_owned(),
                encrypted: true,
            },
            RoomSummary {
                room_id: "!alpha:example".to_owned(),
                display_name: "Alpha".to_owned(),
                encrypted: false,
            },
        ]),
        Event::TimelineText(TimelineText {
            room_id: "!alpha:example".to_owned(),
            body: "alpha first".to_owned(),
        }),
        Event::TimelineText(TimelineText {
            room_id: "!beta:example".to_owned(),
            body: "beta first".to_owned(),
        }),
        Event::TimelineText(TimelineText {
            room_id: "!beta:example".to_owned(),
            body: "beta second".to_owned(),
        }),
    ]
}

fn replacement_events() -> Vec<Event> {
    vec![
        Event::Started,
        Event::SyncedRooms(vec![
            RoomSummary {
                room_id: "!gamma:example".to_owned(),
                display_name: "Gamma".to_owned(),
                encrypted: false,
            },
            RoomSummary {
                room_id: "!delta:example".to_owned(),
                display_name: "Delta".to_owned(),
                encrypted: false,
            },
        ]),
        Event::TimelineText(TimelineText {
            room_id: "!gamma:example".to_owned(),
            body: "gamma only".to_owned(),
        }),
        Event::TimelineText(TimelineText {
            room_id: "!delta:example".to_owned(),
            body: "delta first".to_owned(),
        }),
        Event::TimelineText(TimelineText {
            room_id: "!delta:example".to_owned(),
            body: "delta second".to_owned(),
        }),
    ]
}

fn room_identity(room_id: &str) -> ViewIdentity {
    ViewIdentity::Room {
        room_id: room_id.to_owned(),
    }
}

fn find_view<'a>(ui_state: &'a UiState, identity: &ViewIdentity) -> &'a MdiView {
    ui_state
        .mdi
        .views
        .iter()
        .find(|view| &view.identity == identity)
        .unwrap_or_else(|| panic!("missing MDI view for {identity:?}"))
}

fn view_contains(view: &MdiView, substring: &str) -> bool {
    view.lines.iter().any(|line| line.contains(substring))
}

fn navigation_contains(node: &NavigationNode, substring: &str) -> bool {
    node.text.contains(substring)
        || node
            .children
            .iter()
            .any(|child| navigation_contains(child, substring))
}

fn assert_navigation_not_contains(node: &NavigationNode, substring: &str) {
    assert!(
        !navigation_contains(node, substring),
        "navigation unexpectedly contained {substring:?}"
    );
}

#[test]
fn automation_driver_launches_real_ui_and_reads_event_state()
-> Result<(), Box<dyn std::error::Error>> {
    let exe_path = mmatrix_ui_exe_path()
        .map_err(|error| format!("failed to locate mmatrix-ui.exe: {error}"))?;
    let state_path = write_state_file(&base_events())?;

    let args = [
        OsString::from("--render-state"),
        state_path.as_os_str().to_os_string(),
    ];
    let app = MmatrixApp::launch(&exe_path, &args)?;
    app.wait_idle(Duration::from_secs(5))?;

    let model = app.dump_model()?;
    assert_eq!(model.lifecycle_state, "ready");
    assert_eq!(
        model.rooms,
        vec![
            Room {
                room_id: "!beta:example".to_owned(),
                display_name: "Beta".to_owned(),
                encrypted: true,
            },
            Room {
                room_id: "!alpha:example".to_owned(),
                display_name: "Alpha".to_owned(),
                encrypted: false,
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
    let ui_state = app.dump_ui_state()?;
    assert_eq!(ui_state.mdi.active, room_identity("!beta:example"));

    let beta_view = find_view(&ui_state, &room_identity("!beta:example"));
    assert!(beta_view.active);
    assert!(beta_view.title.contains("Beta"));
    assert!(view_contains(beta_view, "beta first"));
    assert!(view_contains(beta_view, "beta second"));
    assert!(navigation_contains(&ui_state.navigation, "status"));
    assert!(navigation_contains(&ui_state.navigation, "rooms: 2"));
    assert!(navigation_contains(&ui_state.navigation, "Beta [e2ee]"));
    assert!(navigation_contains(&ui_state.navigation, "Alpha"));
    assert_navigation_not_contains(&ui_state.navigation, "beta first");
    assert_navigation_not_contains(&ui_state.navigation, "beta second");

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
    let state_path = write_state_file(&base_events())?;

    let args = [
        OsString::from("--render-state"),
        state_path.as_os_str().to_os_string(),
    ];
    let app = MmatrixApp::launch(&exe_path, &args)?;
    app.wait_idle(Duration::from_secs(5))?;

    let attached = MmatrixApp::find(Duration::from_secs(5))?;
    attached.ping()?;

    let version = attached.get_version()?;
    assert_eq!(version.protocol_version, 2);
    assert_eq!(version.app_version, env!("CARGO_PKG_VERSION"));

    assert_eq!(
        attached.list_rooms()?,
        vec![
            Room {
                room_id: "!beta:example".to_owned(),
                display_name: "Beta".to_owned(),
                encrypted: true,
            },
            Room {
                room_id: "!alpha:example".to_owned(),
                display_name: "Alpha".to_owned(),
                encrypted: false,
            },
        ]
    );

    attached.select_room("!alpha:example")?;
    assert_eq!(attached.timeline(None)?, vec!["alpha first".to_owned()]);
    let alpha_state = attached.dump_ui_state()?;
    assert_eq!(alpha_state.mdi.active, room_identity("!alpha:example"));
    let alpha_view = find_view(&alpha_state, &room_identity("!alpha:example"));
    assert!(alpha_view.active);
    assert!(view_contains(alpha_view, "alpha first"));

    attached.load_events(&serde_json::to_string(&replacement_events())?)?;
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
    assert_eq!(attached.dump_model()?.lifecycle_state, "ready");
    let replacement_state = attached.dump_ui_state()?;
    assert_eq!(
        replacement_state.mdi.active,
        room_identity("!gamma:example")
    );
    let gamma_view = find_view(&replacement_state, &room_identity("!gamma:example"));
    assert!(gamma_view.active);
    assert!(view_contains(gamma_view, "gamma only"));
    assert!(
        replacement_state
            .mdi
            .views
            .iter()
            .all(|view| view.identity != room_identity("!alpha:example"))
    );
    assert!(
        replacement_state
            .mdi
            .views
            .iter()
            .all(|view| view.identity != room_identity("!beta:example"))
    );
    assert_navigation_not_contains(&replacement_state.navigation, "alpha first");
    assert_navigation_not_contains(&replacement_state.navigation, "beta first");
    assert_navigation_not_contains(&replacement_state.navigation, "beta second");

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

#[test]
fn automation_driver_lifecycle_commands_fail_cleanly_in_render_state_mode()
-> Result<(), Box<dyn std::error::Error>> {
    let exe_path = mmatrix_ui_exe_path()
        .map_err(|error| format!("failed to locate mmatrix-ui.exe: {error}"))?;
    let state_path = write_state_file(&base_events())?;
    let local_app_data = unique_local_app_data_path();
    let _session_path = write_saved_session(&local_app_data)?;

    let args = [
        OsString::from("--render-state"),
        state_path.as_os_str().to_os_string(),
    ];
    let app = MmatrixApp::launch_with_options(
        &exe_path,
        &args,
        launch_options(Some(local_app_data.clone())),
    )?;
    app.wait_idle(Duration::from_secs(5))?;

    for (result, expected_code) in [
        (
            app.login("https://matrix.example", "@alice:example", "secret", None),
            "login_failed",
        ),
        (
            app.restore_session("https://matrix.example", stored_session()),
            "restore_failed",
        ),
        (app.restore_saved_session(), "restore_saved_failed"),
        (app.start_sync(), "start_sync_failed"),
        (app.logout(), "logout_failed"),
    ] {
        match result {
            Err(TestkitError::Protocol { code, message }) => {
                assert_eq!(code, expected_code);
                assert!(message.contains("core command sender is not available"));
            }
            other => panic!("expected protocol error {expected_code}, got {other:?}"),
        }
    }

    let _ = fs::remove_file(&state_path);
    let _ = fs::remove_dir_all(&local_app_data);
    Ok(())
}

#[test]
fn automation_driver_updates_lifecycle_from_synthetic_events_without_leaking_tokens()
-> Result<(), Box<dyn std::error::Error>> {
    let exe_path = mmatrix_ui_exe_path()
        .map_err(|error| format!("failed to locate mmatrix-ui.exe: {error}"))?;
    let state_path = write_state_file(&base_events())?;
    let local_app_data = unique_local_app_data_path();
    let session_path = session_file_path(&local_app_data);

    let args = [
        OsString::from("--render-state"),
        state_path.as_os_str().to_os_string(),
    ];
    let app = MmatrixApp::launch_with_options(
        &exe_path,
        &args,
        launch_options(Some(local_app_data.clone())),
    )?;
    app.wait_idle(Duration::from_secs(5))?;

    app.load_events(&serde_json::to_string(&vec![
        Event::Started,
        Event::LoggedIn {
            session: core_stored_session(),
        },
    ])?)?;
    app.wait_for_lifecycle_state("authenticated", Duration::from_secs(5))?;
    let model = app.dump_model()?;
    assert_eq!(model.status, "authenticated");
    assert!(!serde_json::to_string(&model)?.contains("secret-token"));
    assert!(!session_path.exists());

    app.load_events(&serde_json::to_string(&vec![
        Event::Started,
        Event::RestoredSession,
    ])?)?;
    app.wait_for_lifecycle_state("authenticated", Duration::from_secs(5))?;
    assert_eq!(app.dump_model()?.status, "restored saved session");

    app.load_events(&serde_json::to_string(&vec![
        Event::Started,
        Event::RestoredSession,
        Event::SyncStarted,
    ])?)?;
    app.wait_for_lifecycle_state("syncing", Duration::from_secs(5))?;
    assert_eq!(app.dump_model()?.status, "continuous sync started");

    app.load_events(&serde_json::to_string(&vec![
        Event::Started,
        Event::RestoredSession,
        Event::SyncedRooms(vec![RoomSummary {
            room_id: "!synthetic:example".to_owned(),
            display_name: "Synthetic".to_owned(),
            encrypted: false,
        }]),
        Event::LoggedOut,
    ])?)?;
    app.wait_for_lifecycle_state("logged_out", Duration::from_secs(5))?;
    let logged_out_model = app.dump_model()?;
    assert_eq!(logged_out_model.status, "logged out");
    assert!(logged_out_model.rooms.is_empty());
    assert!(logged_out_model.selected_room.is_none());

    let _ = fs::remove_file(&state_path);
    let _ = fs::remove_dir_all(&local_app_data);
    Ok(())
}

#[test]
fn automation_driver_loads_checked_in_event_fixture_via_file_api()
-> Result<(), Box<dyn std::error::Error>> {
    let exe_path = mmatrix_ui_exe_path()
        .map_err(|error| format!("failed to locate mmatrix-ui.exe: {error}"))?;
    let state_path = write_state_file(&base_events())?;

    let args = [
        OsString::from("--render-state"),
        state_path.as_os_str().to_os_string(),
    ];
    let app = MmatrixApp::launch(&exe_path, &args)?;
    app.wait_idle(Duration::from_secs(5))?;

    let attached = MmatrixApp::find(Duration::from_secs(5))?;
    let fixture_path = fixture_state_path("verification-error.json");
    attached.load_events_from_file(&fixture_path)?;
    attached.wait_idle(Duration::from_secs(5))?;

    assert_eq!(
        attached.list_rooms()?,
        vec![
            Room {
                room_id: "!verify:example".to_owned(),
                display_name: "Verify".to_owned(),
                encrypted: true,
            },
            Room {
                room_id: "!support:example".to_owned(),
                display_name: "Support".to_owned(),
                encrypted: false,
            },
        ]
    );
    assert_eq!(
        attached.dump_model()?.selected_room.as_deref(),
        Some("!verify:example")
    );
    assert_eq!(
        attached.timeline(None)?,
        vec![
            "verification requested by @alice:example".to_owned(),
            "sas emoji ready".to_owned(),
            "waiting for confirmation".to_owned(),
        ]
    );

    let model = attached.dump_model()?;
    assert_eq!(model.lifecycle_state, "error");
    assert_eq!(
        model.last_verification,
        Some(Verification {
            flow_id: "flow-verify-123".to_owned(),
            user_id: "@alice:example".to_owned(),
        })
    );
    assert_eq!(
        model.last_error,
        Some(UiError {
            operation: "send_text".to_owned(),
            message: "message send failed for event $event123".to_owned(),
        })
    );
    assert_eq!(
        model.last_timeline,
        Some(TimelineLine {
            room_id: "!verify:example".to_owned(),
            body: "waiting for confirmation".to_owned(),
        })
    );
    let ui_state = attached.dump_ui_state()?;
    let verify_view = find_view(&ui_state, &room_identity("!verify:example"));
    assert!(view_contains(verify_view, "verification requested"));
    assert!(view_contains(verify_view, "sas emoji ready"));
    assert!(view_contains(verify_view, "waiting for confirmation"));
    let status_view = find_view(&ui_state, &ViewIdentity::Status);
    assert!(view_contains(status_view, "flow-verify-123"));
    assert!(view_contains(status_view, "message send failed"));
    assert_navigation_not_contains(&ui_state.navigation, "verification requested");
    assert_navigation_not_contains(&ui_state.navigation, "sas emoji ready");
    assert_navigation_not_contains(&ui_state.navigation, "waiting for confirmation");
    assert_navigation_not_contains(&ui_state.navigation, "flow-verify-123");
    assert_navigation_not_contains(&ui_state.navigation, "message send failed");

    let _ = fs::remove_file(&state_path);
    Ok(())
}

#[test]
fn automation_driver_can_activate_status_view_and_return_to_room()
-> Result<(), Box<dyn std::error::Error>> {
    let exe_path = mmatrix_ui_exe_path()
        .map_err(|error| format!("failed to locate mmatrix-ui.exe: {error}"))?;
    let fixture_path = fixture_state_path("verification-error.json");

    let args = [
        OsString::from("--render-state"),
        fixture_path.as_os_str().to_os_string(),
    ];
    let app = MmatrixApp::launch(&exe_path, &args)?;
    app.wait_idle(Duration::from_secs(5))?;

    app.select_status_view()?;
    let status_state = app.dump_ui_state()?;
    assert_eq!(status_state.mdi.active, ViewIdentity::Status);
    assert_eq!(app.dump_model()?.selected_room, None);

    app.select_room("!verify:example")?;
    let room_state = app.dump_ui_state()?;
    assert_eq!(room_state.mdi.active, room_identity("!verify:example"));
    assert_eq!(
        app.dump_model()?.selected_room.as_deref(),
        Some("!verify:example")
    );

    Ok(())
}

#[test]
fn live_matrix_ui_session() -> Result<(), Box<dyn std::error::Error>> {
    let Some(homeserver_url) = env::var("MMATRIX_HS")
        .ok()
        .filter(|value| !value.is_empty())
    else {
        return Ok(());
    };
    let Some(username) = env::var("MMATRIX_USER")
        .ok()
        .filter(|value| !value.is_empty())
    else {
        return Ok(());
    };
    let Some(password) = env::var("MMATRIX_PASS")
        .ok()
        .filter(|value| !value.is_empty())
    else {
        return Ok(());
    };
    let room_id = env::var("MMATRIX_ROOM")
        .ok()
        .filter(|value| !value.is_empty());

    let exe_path = mmatrix_ui_exe_path()
        .map_err(|error| format!("failed to locate mmatrix-ui.exe: {error}"))?;
    let local_app_data = unique_local_app_data_path();
    let session_path = session_file_path(&local_app_data);
    let options = launch_options(Some(local_app_data.clone()));
    let args = [OsString::from("--automation")];

    let app = MmatrixApp::launch_with_options(&exe_path, &args, options.clone())?;
    app.wait_for_lifecycle_state("core_started", Duration::from_secs(30))?;
    app.login(&homeserver_url, &username, &password, None)?;
    app.wait_for_lifecycle_state("authenticated", Duration::from_secs(120))?;
    app.wait_for_lifecycle_state("ready", Duration::from_secs(120))?;
    app.wait_for_min_rooms(1, Duration::from_secs(120))?;
    assert!(session_path.is_file());

    app.start_sync()?;
    app.wait_for_lifecycle_state("syncing", Duration::from_secs(60))?;

    if let Some(room_id) = room_id.as_deref() {
        let unique_body = format!(
            "mmatrix ui live {}",
            SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos()
        );
        app.select_room(room_id)?;
        app.send_text(Some(room_id), &unique_body)?;
        app.wait_for_timeline_contains(Some(room_id), &unique_body, Duration::from_secs(120))?;
    }

    drop(app);

    let restored = MmatrixApp::launch_with_options(&exe_path, &args, options)?;
    restored.wait_for_lifecycle_state("authenticated", Duration::from_secs(120))?;
    restored.wait_for_lifecycle_state("ready", Duration::from_secs(120))?;
    restored.wait_for_min_rooms(1, Duration::from_secs(120))?;
    restored.logout()?;
    restored.wait_for_lifecycle_state("logged_out", Duration::from_secs(120))?;
    assert!(!session_path.exists());

    let _ = fs::remove_dir_all(&local_app_data);
    Ok(())
}
