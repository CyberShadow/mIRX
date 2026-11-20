#![cfg(windows)]
//! mirx with 200 rooms: what one change costs the refresh path. The UI
//! counts its costly work (rows built for a window, log rows wrapped,
//! treebar and switchbar rebuilds) and the automation dump reports it; these
//! tests assert those counts, never times.

use std::env;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard, OnceLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use mirx_core::{
    Event, RoomActivity, RoomActivityKind, RoomMemberRank, RoomMemberSummary, RoomMembers,
    RoomMembership, RoomMessageActivity, RoomMessagePresentationKind, RoomMessageSendState,
    RoomNotificationMode, RoomSummary, RoomUserProfile,
};
use mirx_testkit::{LaunchOptions, MirxApp, UiRefreshCounters, UiState};

const ROOMS: usize = 200;
const MESSAGES_PER_ROOM: usize = 50;
const BASE_TIMESTAMP_MS: u64 = 1_700_000_000_000;

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

fn unique_local_app_data_path() -> PathBuf {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock must be after unix epoch")
        .as_nanos();
    env::temp_dir().join(format!("mirx-ui-scale-{}-{unique}", std::process::id()))
}

/// One mirx at a time: Wine's GUI tests flake when processes overlap.
fn lock_gui_test() -> MutexGuard<'static, ()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    let lock = LOCK.get_or_init(|| Mutex::new(()));
    lock.lock().unwrap_or_else(|poisoned| {
        lock.clear_poison();
        poisoned.into_inner()
    })
}

fn room_id(index: usize) -> String {
    format!("!r{index:03}:example")
}

fn room_name(index: usize) -> String {
    format!("Room {index:03}")
}

fn room_summary(index: usize, display_name: &str) -> RoomSummary {
    RoomSummary {
        room_id: room_id(index),
        display_name: display_name.to_owned(),
        topic: None,
        membership: RoomMembership::Join,
        encrypted: false,
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

/// Every room, named `Room NNN` except where `renamed` says otherwise.
fn synced_rooms(renamed: Option<(usize, &str)>) -> Event {
    Event::SyncedRooms(
        (0..ROOMS)
            .map(|index| match renamed {
                Some((renamed, name)) if renamed == index => room_summary(index, name),
                _ => room_summary(index, &room_name(index)),
            })
            .collect(),
    )
}

const MEMBERS: [&str; 3] = ["@alice:example", "@bob:example", "@carol:example"];

fn room_members(index: usize) -> Event {
    Event::RoomMembers(RoomMembers {
        room_id: room_id(index),
        members: MEMBERS
            .iter()
            .map(|user_id| RoomMemberSummary {
                user_id: (*user_id).to_owned(),
                display_name: None,
                display_name_ambiguous: false,
                is_own_user: false,
                rank: RoomMemberRank::Member,
                can_kick: false,
                can_ban: false,
            })
            .collect(),
    })
}

fn text_event(index: usize, event_id: &str, sender: &str, timestamp_ms: u64, body: &str) -> Event {
    Event::RoomActivity(Box::new(RoomActivity {
        room_id: room_id(index),
        activity_id: format!("event:{event_id}"),
        timestamp_ms,
        highlight: false,
        relation: None,
        kind: RoomActivityKind::Message(RoomMessageActivity {
            event_id: Some(event_id.to_owned()),
            transaction_id: None,
            sender: RoomUserProfile {
                user_id: sender.to_owned(),
                display_name: None,
                display_name_ambiguous: false,
            },
            presentation: RoomMessagePresentationKind::Text,
            body: body.to_owned(),
            styles: Vec::new(),
            user_links: Vec::new(),
            send_state: RoomMessageSendState::Sent,
        }),
    }))
}

/// The `message`th message of a room: every fifth is long enough to wrap in
/// a maximized window, with commas.
fn message_body(index: usize, message: usize) -> String {
    if message.is_multiple_of(5) {
        format!(
            "room {index}, message {message}: a long line that wraps, at a space or after a \
             comma, as mIRC wraps its lines, so that the window has more paint lines than rows"
        )
    } else {
        format!("room {index} message {message}")
    }
}

fn two_hundred_rooms() -> Vec<Event> {
    let mut events = vec![Event::Started, synced_rooms(None)];
    for index in 0..ROOMS {
        events.push(room_members(index));
        for message in 0..MESSAGES_PER_ROOM {
            events.push(text_event(
                index,
                &format!("$r{index:03}m{message:02}:example"),
                MEMBERS[message % MEMBERS.len()],
                BASE_TIMESTAMP_MS + (message as u64) * 60_000,
                &message_body(index, message),
            ));
        }
    }
    events
}

struct ScaleApp {
    app: MirxApp,
    _guard: MutexGuard<'static, ()>,
}

impl std::ops::Deref for ScaleApp {
    type Target = MirxApp;

    fn deref(&self) -> &MirxApp {
        &self.app
    }
}

/// mirx with the 200 rooms loaded and settled.
fn launch_with_two_hundred_rooms() -> Result<ScaleApp, Box<dyn std::error::Error>> {
    let guard = lock_gui_test();
    let exe_path =
        mirx_ui_exe_path().map_err(|error| format!("failed to locate mirx-ui.exe: {error}"))?;
    let app = MirxApp::launch_with_options(
        &exe_path,
        &[OsString::from("--automation")],
        LaunchOptions {
            startup_timeout: Duration::from_secs(20),
            local_app_data: Some(unique_local_app_data_path()),
            frame_size: None,
        },
    )?;
    app.wait_for_lifecycle_state("core_started", Duration::from_secs(30))?;
    app.load_events(&serde_json::to_string(&two_hundred_rooms())?)?;
    app.wait_idle(Duration::from_secs(60))?;
    Ok(ScaleApp { app, _guard: guard })
}

fn append(app: &MirxApp, events: &[Event]) -> Result<(), Box<dyn std::error::Error>> {
    app.append_synthetic_events(&serde_json::to_string(events)?)?;
    app.wait_idle(Duration::from_secs(30))?;
    Ok(())
}

/// The counts between two dumps.
fn delta(before: &UiState, after: &UiState) -> UiRefreshCounters {
    let (before, after) = (before.refresh_counters, after.refresh_counters);
    UiRefreshCounters {
        timeline_row_builds: after.timeline_row_builds - before.timeline_row_builds,
        rows_wrapped: after.rows_wrapped - before.rows_wrapped,
        treebar_rebuilds: after.treebar_rebuilds - before.treebar_rebuilds,
        switchbar_rebuilds: after.switchbar_rebuilds - before.switchbar_rebuilds,
    }
}

mod with_two_hundred_rooms {
    use super::*;

    #[test]
    fn the_rooms_load_and_dump() -> Result<(), Box<dyn std::error::Error>> {
        let app = launch_with_two_hundred_rooms()?;
        let before = app.dump_ui_state()?;
        assert_eq!(before.rooms.len(), ROOMS);
        // The dump (about 1 MB) must stay well inside the 4 MiB response
        // mapping.
        let size = serde_json::to_vec(&before)?.len();
        eprintln!("a 200-room UI state dump is {size} bytes");
        assert!(size < 2 * 1024 * 1024, "{size} bytes");

        let idle = app.dump_ui_state()?;
        eprintln!("an idle dump: {:?}", delta(&before, &idle));
        append(
            &app,
            &[text_event(
                117,
                "$r117new:example",
                MEMBERS[1],
                BASE_TIMESTAMP_MS + 3_600_000,
                "hello",
            )],
        )?;
        eprintln!("one message: {:?}", delta(&idle, &app.dump_ui_state()?));
        Ok(())
    }
}
