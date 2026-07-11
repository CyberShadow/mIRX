#![cfg(windows)]

use std::env;
use std::ffi::OsString;
use std::fs;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::ops::Deref;
use std::path::{Path, PathBuf};
use std::ptr::{copy_nonoverlapping, null_mut};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, MutexGuard, OnceLock};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use mmatrix_core::{
    E2eeBackupState, E2eeBackupStatus, E2eeCrossSigningStatus, E2eeRecoveryState, E2eeStatus,
    Event, RoomActivity as CoreRoomActivity, RoomActivityKind as CoreRoomActivityKind,
    RoomActivityRelation as CoreRoomActivityRelation, RoomActivityUpdate, RoomActivityUpdateKind,
    RoomActivityUpdateTarget, RoomHistoryBackfill, RoomHistoryBackfillEvent,
    RoomMedia as CoreRoomMedia, RoomMediaFormat as CoreRoomMediaFormat,
    RoomMediaKind as CoreRoomMediaKind, RoomMemberSummary, RoomMembers, RoomMembership,
    RoomMessageActivity as CoreRoomMessageActivity,
    RoomMessagePresentationKind as CoreRoomMessagePresentationKind,
    RoomMessageSendState as CoreRoomMessageSendState, RoomSummary, RoomTyping, SpaceChildSummary,
    StoredSession as CoreStoredSession, VerificationDecimals as CoreVerificationDecimals,
    VerificationEmoji as CoreVerificationEmoji, VerificationState as CoreVerificationState,
    VerificationStateEvent as CoreVerificationStateEvent,
};
use mmatrix_testkit::{
    ComposeKey, ContextMenuCommand, E2eeRecoverySetupResult, FindActiveViewResult, FrameSize,
    LaunchOptions, MdiView, MembershipCommandAction, MmatrixApp, Model, NavigationNode,
    NavigationPathSegment, Room, RoomActivity, RoomActivityKind, RoomActivityRelation,
    RoomAudioActivity, RoomAudioInfo, RoomEncryptedFileHashes, RoomEncryptedFileKey,
    RoomFileActivity, RoomFileInfo, RoomImageActivity, RoomImageInfo, RoomImageMediaState,
    RoomMediaSource, RoomMessageActivity, RoomMessageActivityLine, RoomMessagePresentationKind,
    RoomMessageSendState, RoomReactionSummary, RoomRedactedActivity, RoomStateNoticeActivity,
    RoomStateNoticeContent, RoomTag, SpaceChild, StoredSession, SubmittedDirectRoomCommand,
    SubmittedEditCommand, SubmittedHistoryCommand, SubmittedMembershipCommand,
    SubmittedReactionCommand, SubmittedRecoveryCommand, SubmittedRedactionCommand,
    SubmittedReplyCommand, SubmittedReplyThreadTarget, SubmittedRoomTopicCommand,
    SubmittedSetupRecoveryCommand, SubmittedVerificationAction, SwitchbarItem, TestkitError,
    UiError, UiFindHistorySearchState, UiFindState, UiRoomHistoryBackfillState, UiRoomReadState,
    UiRoomState, UiState, UnsupportedRoomMessageActivity, Verification, VerificationActionKind,
    VerificationState, ViewIdentity,
};
use windows_sys::Win32::Foundation::{GetLastError, HWND, RECT, WPARAM};
use windows_sys::Win32::System::DataExchange::{
    CloseClipboard, EmptyClipboard, OpenClipboard, SetClipboardData,
};
use windows_sys::Win32::System::Memory::{GMEM_MOVEABLE, GlobalAlloc, GlobalLock, GlobalUnlock};
use windows_sys::Win32::System::Threading::{AttachThreadInput, GetCurrentThreadId};
use windows_sys::Win32::UI::Controls::{TB_GETSTATE, TBSTATE_CHECKED, TBSTATE_MARKED};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::GetFocus;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    EnumThreadWindows, FindWindowExW, FindWindowW, GWL_STYLE, GetClassNameW, GetClientRect,
    GetDlgItem, GetLastActivePopup, GetScrollInfo, GetWindowLongPtrW, GetWindowRect,
    GetWindowThreadProcessId, IsWindow, PostMessageW, SB_PAGEUP, SB_TOP, SB_VERT, SCROLLINFO,
    SIF_ALL, SendMessageW, SetWindowTextW, WM_CHAR, WM_COMMAND, WM_GETTEXT, WM_GETTEXTLENGTH,
    WM_MDIACTIVATE, WM_MDIGETACTIVE, WM_PASTE, WM_VSCROLL, WS_VSCROLL,
};

#[allow(dead_code)]
#[path = "../src/dpapi.rs"]
mod dpapi;

const PROTECTED_SESSION_MAGIC: &[u8] = b"mmatrix-ui-session:v1\0";
const SESSION_DPAPI_ENTROPY: &[u8] = b"mmatrix-ui/session/v1";
const CF_UNICODETEXT: u32 = 13;
const EDIT_ID: i32 = 1003;
const MDI_ID: i32 = 1002;
const SWITCHBAR_ID: i32 = 1005;
const VIEW_CONTENT_ID: i32 = 2001;
const WM_TIMELINE_GET_TOP_LINE: u32 = 0x8000 + 100;
// Intentionally duplicated from production to exercise the real switchbar toolbar command range.
const SWITCHBAR_COMMAND_FIRST: usize = 41001;
const MENU_ID_VERIFY_ACCEPT: usize = 40007;
const MENU_ID_VERIFY_CANCEL: usize = 40008;
const MENU_ID_VERIFY_CONFIRM_SAS: usize = 40009;
const MENU_ID_VERIFY_REJECT_SAS: usize = 40010;
const MENU_ID_RECOVER_E2EE: usize = 40005;
const MENU_ID_SETUP_E2EE_RECOVERY: usize = 40017;
const RECOVER_E2EE_DIALOG_SECRET_ID: i32 = 5007;
const RECOVER_E2EE_DIALOG_OK_ID: usize = 5008;
const SETUP_E2EE_DIALOG_PASSPHRASE_ID: i32 = 5013;
const SETUP_E2EE_DIALOG_OK_ID: usize = 5015;
const RECOVERY_KEY_HANDOFF_DIALOG_KEY_ID: i32 = 5017;
const RECOVERY_KEY_HANDOFF_DIALOG_ACK_ID: usize = 5018;
const POLL_INTERVAL: Duration = Duration::from_millis(25);
static TEMP_PATH_COUNTER: AtomicU64 = AtomicU64::new(1);

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

fn unique_temp_path(prefix: &str, extension: Option<&str>) -> PathBuf {
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

fn unique_state_path() -> PathBuf {
    unique_temp_path("mmatrix-ui-automation-driver", Some("json"))
}

fn unique_screenshot_path() -> PathBuf {
    unique_temp_path("mmatrix-ui-automation-driver", Some("png"))
}

fn unique_local_app_data_path() -> PathBuf {
    unique_temp_path("mmatrix-ui-local-app-data", None)
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
        frame_size: None,
    }
}

struct GuiTestApp {
    app: MmatrixApp,
    _gui_test_guard: MutexGuard<'static, ()>,
}

impl Deref for GuiTestApp {
    type Target = MmatrixApp;

    fn deref(&self) -> &Self::Target {
        &self.app
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

fn launch_app(
    exe_path: &Path,
    args: &[OsString],
) -> Result<GuiTestApp, Box<dyn std::error::Error>> {
    let guard = lock_gui_test();
    let app = mmatrix_testkit::MmatrixApp::launch(exe_path, args)?;
    Ok(GuiTestApp {
        app,
        _gui_test_guard: guard,
    })
}

fn launch_app_with_options(
    exe_path: &Path,
    args: &[OsString],
    options: LaunchOptions,
) -> Result<GuiTestApp, Box<dyn std::error::Error>> {
    let guard = lock_gui_test();
    let app = mmatrix_testkit::MmatrixApp::launch_with_options(exe_path, args, options)?;
    Ok(GuiTestApp {
        app,
        _gui_test_guard: guard,
    })
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
    let payload = serde_json::to_vec(&serde_json::json!({
        "version": 1,
        "homeserver_url": "https://matrix.example",
        "session": {
            "user_id": "@alice:example",
            "device_id": "DEVICE",
            "access_token": "secret-token",
            "refresh_token": "refresh-token"
        }
    }))?;
    let protected = dpapi::protect(
        &payload,
        "mmatrix saved session test",
        SESSION_DPAPI_ENTROPY,
    )?;
    let mut bytes = Vec::with_capacity(PROTECTED_SESSION_MAGIC.len() + protected.len());
    bytes.extend_from_slice(PROTECTED_SESSION_MAGIC);
    bytes.extend_from_slice(&protected);
    fs::write(&path, bytes)?;
    Ok(path)
}

fn assert_bytes_do_not_contain(bytes: &[u8], needle: &str) {
    assert!(
        !bytes
            .windows(needle.len())
            .any(|window| window == needle.as_bytes()),
        "bytes unexpectedly contained {needle:?}"
    );
}

fn fixture_state_path(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/ui-states")
        .join(name)
}

fn fixture_update_path(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/ui-state-updates")
        .join(name)
}

fn write_state_file(events: &[Event]) -> Result<PathBuf, Box<dyn std::error::Error>> {
    let state_path = unique_state_path();
    fs::write(&state_path, serde_json::to_vec(events)?)?;
    Ok(state_path)
}

struct MockBackfillHomeserver {
    base_url: String,
}

fn spawn_mock_backfill_homeserver() -> Result<MockBackfillHomeserver, Box<dyn std::error::Error>> {
    let listener = TcpListener::bind("127.0.0.1:0")?;
    let address = listener.local_addr()?;
    std::thread::spawn(move || {
        for mut stream in listener.incoming().flatten() {
            let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
            let mut request = Vec::new();
            let mut buffer = [0u8; 4096];
            while let Ok(read) = stream.read(&mut buffer) {
                if read == 0 {
                    break;
                }
                request.extend_from_slice(&buffer[..read]);
                if request.windows(4).any(|window| window == b"\r\n\r\n") {
                    break;
                }
            }

            let request = String::from_utf8_lossy(&request);
            let request_line = request.lines().next().unwrap_or_default();
            let path = request_line.split_whitespace().nth(1).unwrap_or("/");

            if path == "/_matrix/client/versions" {
                respond_json(
                    &mut stream,
                    200,
                    &serde_json::json!({
                        "versions": ["r0.6.1", "v1.1"]
                    }),
                );
                continue;
            }

            if path.contains("/sync") {
                respond_json(
                    &mut stream,
                    200,
                    &serde_json::json!({
                        "next_batch": "s-sync",
                        "device_one_time_keys_count": {
                            "signed_curve25519": 0
                        },
                        "device_lists": {
                            "changed": [],
                            "left": []
                        },
                        "rooms": {
                            "join": {
                                "!beta:example": {
                                    "summary": {
                                        "m.joined_member_count": 1,
                                        "m.invited_member_count": 0
                                    },
                                    "state": {
                                        "events": [
                                            {
                                                "content": {
                                                    "creator": "@alice:example",
                                                    "room_version": "10"
                                                },
                                                "event_id": "$create",
                                                "origin_server_ts": 1700000000000u64,
                                                "sender": "@alice:example",
                                                "state_key": "",
                                                "type": "m.room.create"
                                            },
                                            {
                                                "content": {
                                                    "membership": "join",
                                                    "displayname": "Alice"
                                                },
                                                "event_id": "$member",
                                                "origin_server_ts": 1700000000001u64,
                                                "sender": "@alice:example",
                                                "state_key": "@alice:example",
                                                "type": "m.room.member"
                                            },
                                            {
                                                "content": {
                                                    "name": "Beta"
                                                },
                                                "event_id": "$name",
                                                "origin_server_ts": 1700000000002u64,
                                                "sender": "@alice:example",
                                                "state_key": "",
                                                "type": "m.room.name"
                                            }
                                        ]
                                    },
                                    "timeline": {
                                        "events": [],
                                        "limited": false,
                                        "prev_batch": "s-current"
                                    },
                                    "ephemeral": {
                                        "events": []
                                    },
                                    "account_data": {
                                        "events": []
                                    },
                                    "unread_notifications": {
                                        "notification_count": 0,
                                        "highlight_count": 0
                                    }
                                }
                            }
                        }
                    }),
                );
                continue;
            }

            if path.contains("/room_keys/version") {
                respond_json(
                    &mut stream,
                    200,
                    &serde_json::json!({
                        "algorithm": "m.megolm_backup.v1.curve25519-aes-sha2",
                        "auth_data": {
                            "public_key": "AQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQE"
                        },
                        "count": 0,
                        "etag": "1",
                        "version": "1"
                    }),
                );
                continue;
            }

            if path.contains("/keys/upload") {
                respond_json(
                    &mut stream,
                    200,
                    &serde_json::json!({
                        "one_time_key_counts": {}
                    }),
                );
                continue;
            }

            if path.contains("/keys/query") {
                respond_json(
                    &mut stream,
                    200,
                    &serde_json::json!({
                        "device_keys": {},
                        "failures": {},
                        "master_keys": {},
                        "self_signing_keys": {},
                        "user_signing_keys": {}
                    }),
                );
                continue;
            }

            if path.contains("/account_data/m.secret_storage.default_key") {
                respond_json(
                    &mut stream,
                    404,
                    &serde_json::json!({
                        "errcode": "M_NOT_FOUND",
                        "error": "not found"
                    }),
                );
                continue;
            }

            if path.contains("/rooms/!beta:example/state/m.room.encryption/")
                || path.contains("/rooms/%21beta%3Aexample/state/m.room.encryption/")
            {
                respond_json(
                    &mut stream,
                    200,
                    &serde_json::json!({
                        "content": {
                            "algorithm": "m.megolm.v1.aes-sha2"
                        },
                        "event_id": "$encryption",
                        "origin_server_ts": 1700000000003u64,
                        "sender": "@alice:example",
                        "state_key": "",
                        "type": "m.room.encryption"
                    }),
                );
                continue;
            }

            if path.contains("/rooms/%21beta%3Aexample/messages")
                || path.contains("/rooms/!beta:example/messages")
            {
                std::thread::sleep(Duration::from_secs(60));
                continue;
            }

            respond_json(
                &mut stream,
                404,
                &serde_json::json!({
                    "errcode": "M_UNRECOGNIZED",
                    "error": format!("unhandled mock homeserver path: {path}")
                }),
            );
        }
    });

    Ok(MockBackfillHomeserver {
        base_url: format!("http://{address}"),
    })
}

fn respond_json(stream: &mut std::net::TcpStream, status: u16, body: &serde_json::Value) {
    let reason = match status {
        200 => "OK",
        404 => "Not Found",
        _ => "Error",
    };
    let body = serde_json::to_vec(body).expect("mock homeserver response must serialize");
    let response = format!(
        "HTTP/1.1 {status} {reason}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    );
    let _ = stream.write_all(response.as_bytes());
    let _ = stream.write_all(&body);
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
    }
}

fn invited_room_summary(room_id: &str, display_name: &str, encrypted: bool) -> RoomSummary {
    RoomSummary {
        membership: RoomMembership::Invite,
        ..room_summary(room_id, display_name, encrypted)
    }
}

fn joined_space_summary(
    room_id: &str,
    display_name: &str,
    children: Vec<SpaceChildSummary>,
) -> RoomSummary {
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
        space_children: children,
    }
}

fn invited_space_summary(
    room_id: &str,
    display_name: &str,
    children: Vec<SpaceChildSummary>,
) -> RoomSummary {
    RoomSummary {
        membership: RoomMembership::Invite,
        ..joined_space_summary(room_id, display_name, children)
    }
}

fn listed_room(room_id: &str, display_name: &str, encrypted: bool) -> Room {
    listed_room_with_metadata(
        room_id,
        display_name,
        encrypted,
        false,
        Vec::new(),
        Vec::new(),
    )
}

fn listed_invited_room(room_id: &str, display_name: &str, encrypted: bool) -> Room {
    listed_room_with_membership(
        room_id,
        display_name,
        encrypted,
        RoomMembership::Invite,
        false,
        Vec::new(),
        Vec::new(),
    )
}

fn listed_room_with_metadata(
    room_id: &str,
    display_name: &str,
    encrypted: bool,
    is_direct: bool,
    direct_user_ids: Vec<&str>,
    tags: Vec<RoomTag>,
) -> Room {
    listed_room_with_membership(
        room_id,
        display_name,
        encrypted,
        RoomMembership::Join,
        is_direct,
        direct_user_ids,
        tags,
    )
}

fn listed_room_with_membership(
    room_id: &str,
    display_name: &str,
    encrypted: bool,
    membership: RoomMembership,
    is_direct: bool,
    direct_user_ids: Vec<&str>,
    tags: Vec<RoomTag>,
) -> Room {
    Room {
        room_id: room_id.to_owned(),
        display_name: display_name.to_owned(),
        topic: None,
        membership,
        encrypted,
        is_space: false,
        is_direct,
        direct_user_ids: direct_user_ids.into_iter().map(str::to_owned).collect(),
        tags,
        space_children: Vec::new(),
    }
}

fn listed_space(room_id: &str, display_name: &str, children: Vec<SpaceChild>) -> Room {
    Room {
        room_id: room_id.to_owned(),
        display_name: display_name.to_owned(),
        topic: None,
        membership: RoomMembership::Join,
        encrypted: false,
        is_space: true,
        is_direct: false,
        direct_user_ids: Vec::new(),
        tags: Vec::new(),
        space_children: children,
    }
}

fn room_tag(tag: &str, order: Option<&str>) -> RoomTag {
    RoomTag {
        tag: tag.to_owned(),
        order: order.map(str::to_owned),
    }
}

fn base_events() -> Vec<Event> {
    vec![
        Event::Started,
        Event::SyncedRooms(vec![
            room_summary("!beta:example", "Beta", true),
            room_summary("!alpha:example", "Alpha", false),
        ]),
        text_event(
            "!alpha:example",
            "@alice:example",
            1_700_000_000_000,
            "alpha first",
        ),
        text_event(
            "!beta:example",
            "@bob:example",
            1_700_000_060_000,
            "beta first",
        ),
        text_event(
            "!beta:example",
            "@bob:example",
            1_700_000_120_000,
            "beta second",
        ),
    ]
}

fn invite_events() -> Vec<Event> {
    vec![
        Event::Started,
        Event::SyncedRooms(vec![
            room_summary("!beta:example", "Beta", false),
            invited_room_summary("!invite:example", "Lobby", false),
        ]),
        text_event(
            "!beta:example",
            "@bob:example",
            1_700_000_120_000,
            "beta latest",
        ),
    ]
}

fn invite_target_events() -> Vec<Event> {
    vec![
        Event::Started,
        Event::SyncedRooms(vec![
            room_summary("!beta:example", "Beta", false),
            invited_room_summary("!invite-target:example", "Lobby", false),
            invited_room_summary("!shared-one:example", "Shared", false),
            invited_room_summary("!shared-two:example", "Shared", false),
        ]),
        text_event(
            "!beta:example",
            "@bob:example",
            1_700_000_120_000,
            "beta latest",
        ),
    ]
}

fn hierarchical_invite_space_events() -> Vec<Event> {
    vec![
        Event::Started,
        Event::SyncedRooms(vec![
            joined_space_summary(
                "!space:example",
                "Space",
                vec![SpaceChildSummary {
                    room_id: "!beta:example".to_owned(),
                    suggested: false,
                    order: None,
                }],
            ),
            room_summary("!beta:example", "Beta", false),
            invited_space_summary("!invite-space:example", "Invite Space", Vec::new()),
        ]),
        text_event(
            "!beta:example",
            "@bob:example",
            1_700_000_120_000,
            "beta latest",
        ),
    ]
}

fn sample_e2ee_status() -> E2eeStatus {
    E2eeStatus {
        recovery: E2eeRecoveryState::Incomplete,
        backup: E2eeBackupStatus {
            state: E2eeBackupState::Enabled,
            enabled_locally: true,
            exists_on_server: Some(true),
        },
        cross_signing: Some(E2eeCrossSigningStatus {
            has_master: true,
            has_self_signing: true,
            has_user_signing: false,
            complete: false,
        }),
    }
}

fn verification_state_event(flow_id: &str, user_id: &str, state: CoreVerificationState) -> Event {
    Event::VerificationState(CoreVerificationStateEvent {
        flow_id: flow_id.to_owned(),
        user_id: user_id.to_owned(),
        state,
    })
}

fn replacement_events() -> Vec<Event> {
    vec![
        Event::Started,
        Event::SyncedRooms(vec![
            room_summary("!gamma:example", "Gamma", false),
            room_summary("!delta:example", "Delta", false),
        ]),
        text_event(
            "!gamma:example",
            "@alice:example",
            1_700_000_000_000,
            "gamma only",
        ),
        text_event(
            "!delta:example",
            "@bob:example",
            1_700_000_060_000,
            "delta first",
        ),
        text_event(
            "!delta:example",
            "@bob:example",
            1_700_000_120_000,
            "delta second",
        ),
    ]
}

fn generated_event_id(room_id: &str, body: &str) -> String {
    format!("${room_id}:{}", body.replace(' ', "_"))
}

fn message_event(
    room_id: &str,
    sender: &str,
    timestamp_ms: u64,
    presentation: CoreRoomMessagePresentationKind,
    body: &str,
) -> Event {
    Event::RoomActivity(Box::new(CoreRoomActivity {
        room_id: room_id.to_owned(),
        activity_id: format!("event:{}", generated_event_id(room_id, body)),
        timestamp_ms,
        relation: None,
        kind: CoreRoomActivityKind::Message(CoreRoomMessageActivity {
            event_id: Some(generated_event_id(room_id, body)),
            transaction_id: None,
            sender: sender.to_owned(),
            presentation,
            body: body.to_owned(),
            send_state: CoreRoomMessageSendState::Sent,
        }),
    }))
}

fn text_event(room_id: &str, sender: &str, timestamp_ms: u64, body: &str) -> Event {
    message_event(
        room_id,
        sender,
        timestamp_ms,
        CoreRoomMessagePresentationKind::Text,
        body,
    )
}

fn message_event_with_event_id(
    room_id: &str,
    event_id: &str,
    sender: &str,
    timestamp_ms: u64,
    presentation: CoreRoomMessagePresentationKind,
    body: &str,
) -> Event {
    message_event_with_event_id_and_relation(
        room_id,
        event_id,
        sender,
        timestamp_ms,
        None,
        presentation,
        body,
    )
}

fn message_event_with_event_id_and_relation(
    room_id: &str,
    event_id: &str,
    sender: &str,
    timestamp_ms: u64,
    relation: Option<CoreRoomActivityRelation>,
    presentation: CoreRoomMessagePresentationKind,
    body: &str,
) -> Event {
    Event::RoomActivity(Box::new(CoreRoomActivity {
        room_id: room_id.to_owned(),
        activity_id: format!("event:{event_id}"),
        timestamp_ms,
        relation,
        kind: CoreRoomActivityKind::Message(CoreRoomMessageActivity {
            event_id: Some(event_id.to_owned()),
            transaction_id: None,
            sender: sender.to_owned(),
            presentation,
            body: body.to_owned(),
            send_state: CoreRoomMessageSendState::Sent,
        }),
    }))
}

fn text_event_with_event_id(
    room_id: &str,
    event_id: &str,
    sender: &str,
    timestamp_ms: u64,
    body: &str,
) -> Event {
    message_event_with_event_id(
        room_id,
        event_id,
        sender,
        timestamp_ms,
        CoreRoomMessagePresentationKind::Text,
        body,
    )
}

fn history_backfill_event_from_event(event: Event) -> RoomHistoryBackfillEvent {
    match event {
        Event::RoomActivity(activity) => RoomHistoryBackfillEvent::Activity(activity),
        Event::RoomActivityUpdate(update) => RoomHistoryBackfillEvent::ActivityUpdate(update),
        other => panic!("expected room history backfill event payload, got {other:?}"),
    }
}

fn room_history_backfill_event(
    room_id: &str,
    start: &str,
    end: Option<&str>,
    events: Vec<RoomHistoryBackfillEvent>,
) -> Event {
    Event::RoomHistoryBackfill(RoomHistoryBackfill {
        room_id: room_id.to_owned(),
        start: start.to_owned(),
        end: end.map(str::to_owned),
        events,
    })
}

fn pending_message_event_with_presentation(
    room_id: &str,
    transaction_id: &str,
    sender: &str,
    timestamp_ms: u64,
    presentation: CoreRoomMessagePresentationKind,
    body: &str,
) -> Event {
    Event::RoomActivity(Box::new(CoreRoomActivity {
        room_id: room_id.to_owned(),
        activity_id: format!("txn:{transaction_id}"),
        timestamp_ms,
        relation: None,
        kind: CoreRoomActivityKind::Message(CoreRoomMessageActivity {
            event_id: None,
            transaction_id: Some(transaction_id.to_owned()),
            sender: sender.to_owned(),
            presentation,
            body: body.to_owned(),
            send_state: CoreRoomMessageSendState::Pending,
        }),
    }))
}

fn pending_message_event(
    room_id: &str,
    transaction_id: &str,
    sender: &str,
    timestamp_ms: u64,
    body: &str,
) -> Event {
    pending_message_event_with_presentation(
        room_id,
        transaction_id,
        sender,
        timestamp_ms,
        CoreRoomMessagePresentationKind::Text,
        body,
    )
}

fn message_sent_update_event(room_id: &str, transaction_id: &str, event_id: &str) -> Event {
    Event::RoomActivityUpdate(RoomActivityUpdate {
        room_id: room_id.to_owned(),
        target: RoomActivityUpdateTarget::Transaction {
            transaction_id: transaction_id.to_owned(),
        },
        kind: RoomActivityUpdateKind::MessageSent {
            event_id: event_id.to_owned(),
        },
    })
}

fn message_failed_update_event(
    room_id: &str,
    transaction_id: &str,
    message: &str,
    recoverable: bool,
) -> Event {
    Event::RoomActivityUpdate(RoomActivityUpdate {
        room_id: room_id.to_owned(),
        target: RoomActivityUpdateTarget::Transaction {
            transaction_id: transaction_id.to_owned(),
        },
        kind: RoomActivityUpdateKind::MessageFailed {
            message: message.to_owned(),
            recoverable,
        },
    })
}

fn message_replaced_update_event(
    room_id: &str,
    event_id: &str,
    replacement_event_id: &str,
    sender: &str,
    timestamp_ms: u64,
    presentation: CoreRoomMessagePresentationKind,
    body: &str,
) -> Event {
    Event::RoomActivityUpdate(RoomActivityUpdate {
        room_id: room_id.to_owned(),
        target: RoomActivityUpdateTarget::Event {
            event_id: event_id.to_owned(),
        },
        kind: RoomActivityUpdateKind::MessageReplaced {
            replacement_event_id: replacement_event_id.to_owned(),
            sender: sender.to_owned(),
            timestamp_ms,
            presentation,
            body: body.to_owned(),
        },
    })
}

fn text_message_replaced_update_event(
    room_id: &str,
    event_id: &str,
    replacement_event_id: &str,
    sender: &str,
    timestamp_ms: u64,
    body: &str,
) -> Event {
    message_replaced_update_event(
        room_id,
        event_id,
        replacement_event_id,
        sender,
        timestamp_ms,
        CoreRoomMessagePresentationKind::Text,
        body,
    )
}

fn typing_event(room_id: &str, user_ids: &[&str]) -> Event {
    Event::RoomTyping(RoomTyping {
        room_id: room_id.to_owned(),
        user_ids: user_ids
            .iter()
            .map(|user_id| (*user_id).to_owned())
            .collect(),
    })
}

fn room_members_event(room_id: &str, members: &[(&str, Option<&str>)]) -> Event {
    Event::RoomMembers(RoomMembers {
        room_id: room_id.to_owned(),
        members: members
            .iter()
            .map(|(user_id, display_name)| RoomMemberSummary {
                user_id: (*user_id).to_owned(),
                display_name: display_name.map(str::to_owned),
            })
            .collect(),
    })
}

fn timeline_line(
    room_id: &str,
    sender: &str,
    timestamp_ms: u64,
    body: &str,
) -> RoomMessageActivityLine {
    RoomMessageActivityLine {
        room_id: room_id.to_owned(),
        activity_id: format!("event:{}", generated_event_id(room_id, body)),
        timestamp_ms,
        event_id: Some(generated_event_id(room_id, body)),
        transaction_id: None,
        sender: sender.to_owned(),
        presentation: RoomMessagePresentationKind::Text,
        body: body.to_owned(),
        send_state: RoomMessageSendState::Sent,
    }
}

fn timeline_line_with_event_id(
    room_id: &str,
    event_id: &str,
    sender: &str,
    timestamp_ms: u64,
    body: &str,
) -> RoomMessageActivityLine {
    RoomMessageActivityLine {
        room_id: room_id.to_owned(),
        activity_id: format!("event:{event_id}"),
        timestamp_ms,
        event_id: Some(event_id.to_owned()),
        transaction_id: None,
        sender: sender.to_owned(),
        presentation: RoomMessagePresentationKind::Text,
        body: body.to_owned(),
        send_state: RoomMessageSendState::Sent,
    }
}

fn message_entry(
    room_id: &str,
    sender: &str,
    timestamp_ms: u64,
    presentation: RoomMessagePresentationKind,
    body: &str,
) -> RoomActivity {
    RoomActivity {
        room_id: room_id.to_owned(),
        activity_id: format!("event:{}", generated_event_id(room_id, body)),
        timestamp_ms,
        relation: None,
        reactions: Vec::new(),
        kind: RoomActivityKind::Message(RoomMessageActivity {
            event_id: Some(generated_event_id(room_id, body)),
            transaction_id: None,
            sender: sender.to_owned(),
            presentation,
            body: body.to_owned(),
            send_state: RoomMessageSendState::Sent,
        }),
    }
}

fn text_entry(room_id: &str, sender: &str, timestamp_ms: u64, body: &str) -> RoomActivity {
    message_entry(
        room_id,
        sender,
        timestamp_ms,
        RoomMessagePresentationKind::Text,
        body,
    )
}

fn message_entry_with_event_id(
    room_id: &str,
    event_id: &str,
    sender: &str,
    timestamp_ms: u64,
    presentation: RoomMessagePresentationKind,
    body: &str,
) -> RoomActivity {
    message_entry_with_event_id_and_relation(
        room_id,
        event_id,
        sender,
        timestamp_ms,
        None,
        presentation,
        body,
    )
}

fn message_entry_with_event_id_and_relation(
    room_id: &str,
    event_id: &str,
    sender: &str,
    timestamp_ms: u64,
    relation: Option<RoomActivityRelation>,
    presentation: RoomMessagePresentationKind,
    body: &str,
) -> RoomActivity {
    RoomActivity {
        room_id: room_id.to_owned(),
        activity_id: format!("event:{event_id}"),
        timestamp_ms,
        relation,
        reactions: Vec::new(),
        kind: RoomActivityKind::Message(RoomMessageActivity {
            event_id: Some(event_id.to_owned()),
            transaction_id: None,
            sender: sender.to_owned(),
            presentation,
            body: body.to_owned(),
            send_state: RoomMessageSendState::Sent,
        }),
    }
}

fn text_entry_with_event_id(
    room_id: &str,
    event_id: &str,
    sender: &str,
    timestamp_ms: u64,
    body: &str,
) -> RoomActivity {
    message_entry_with_event_id(
        room_id,
        event_id,
        sender,
        timestamp_ms,
        RoomMessagePresentationKind::Text,
        body,
    )
}

fn text_entry_with_event_id_and_relation(
    room_id: &str,
    event_id: &str,
    sender: &str,
    timestamp_ms: u64,
    relation: Option<RoomActivityRelation>,
    body: &str,
) -> RoomActivity {
    message_entry_with_event_id_and_relation(
        room_id,
        event_id,
        sender,
        timestamp_ms,
        relation,
        RoomMessagePresentationKind::Text,
        body,
    )
}

fn with_reactions(mut activity: RoomActivity, reactions: Vec<RoomReactionSummary>) -> RoomActivity {
    activity.reactions = reactions;
    activity
}

fn reaction_summary(
    key: &str,
    count: u64,
    senders: &[&str],
    reaction_event_ids: &[&str],
) -> RoomReactionSummary {
    RoomReactionSummary {
        key: key.to_owned(),
        count,
        senders: senders.iter().map(|sender| (*sender).to_owned()).collect(),
        reaction_event_ids: reaction_event_ids
            .iter()
            .map(|event_id| (*event_id).to_owned())
            .collect(),
    }
}

fn pending_entry_with_presentation(
    room_id: &str,
    transaction_id: &str,
    sender: &str,
    timestamp_ms: u64,
    presentation: RoomMessagePresentationKind,
    body: &str,
) -> RoomActivity {
    RoomActivity {
        room_id: room_id.to_owned(),
        activity_id: format!("txn:{transaction_id}"),
        timestamp_ms,
        relation: None,
        reactions: Vec::new(),
        kind: RoomActivityKind::Message(RoomMessageActivity {
            event_id: None,
            transaction_id: Some(transaction_id.to_owned()),
            sender: sender.to_owned(),
            presentation,
            body: body.to_owned(),
            send_state: RoomMessageSendState::Pending,
        }),
    }
}

fn pending_entry(
    room_id: &str,
    transaction_id: &str,
    sender: &str,
    timestamp_ms: u64,
    body: &str,
) -> RoomActivity {
    pending_entry_with_presentation(
        room_id,
        transaction_id,
        sender,
        timestamp_ms,
        RoomMessagePresentationKind::Text,
        body,
    )
}

fn failed_entry(
    room_id: &str,
    sender: &str,
    timestamp_ms: u64,
    transaction_id: &str,
    body: &str,
    message: &str,
    recoverable: bool,
) -> RoomActivity {
    RoomActivity {
        room_id: room_id.to_owned(),
        activity_id: format!("txn:{transaction_id}"),
        timestamp_ms,
        relation: None,
        reactions: Vec::new(),
        kind: RoomActivityKind::Message(RoomMessageActivity {
            event_id: None,
            transaction_id: Some(transaction_id.to_owned()),
            sender: sender.to_owned(),
            presentation: RoomMessagePresentationKind::Text,
            body: body.to_owned(),
            send_state: RoomMessageSendState::Failed {
                message: message.to_owned(),
                recoverable,
            },
        }),
    }
}

fn topic_entry_with_event_id(
    room_id: &str,
    event_id: &str,
    sender: &str,
    timestamp_ms: u64,
    topic: &str,
) -> RoomActivity {
    RoomActivity {
        room_id: room_id.to_owned(),
        activity_id: format!("state:{event_id}"),
        timestamp_ms,
        relation: None,
        reactions: Vec::new(),
        kind: RoomActivityKind::StateNotice(RoomStateNoticeActivity {
            event_id: event_id.to_owned(),
            sender: sender.to_owned(),
            state_key: String::new(),
            content: RoomStateNoticeContent::Topic {
                topic: topic.to_owned(),
            },
        }),
    }
}

fn topic_event_with_event_id(
    room_id: &str,
    event_id: &str,
    sender: &str,
    timestamp_ms: u64,
    topic: &str,
) -> Event {
    Event::RoomActivity(Box::new(CoreRoomActivity {
        room_id: room_id.to_owned(),
        activity_id: format!("state:{event_id}"),
        timestamp_ms,
        relation: None,
        kind: CoreRoomActivityKind::StateNotice(mmatrix_core::RoomStateNoticeActivity {
            event_id: event_id.to_owned(),
            sender: sender.to_owned(),
            state_key: String::new(),
            content: mmatrix_core::RoomStateNoticeContent::Topic {
                topic: topic.to_owned(),
            },
        }),
    }))
}

fn member_entry_with_event_id(
    room_id: &str,
    event_id: &str,
    sender: &str,
    timestamp_ms: u64,
    state_key: &str,
    membership: &str,
    displayname: Option<&str>,
) -> RoomActivity {
    RoomActivity {
        room_id: room_id.to_owned(),
        activity_id: format!("state:{event_id}"),
        timestamp_ms,
        relation: None,
        reactions: Vec::new(),
        kind: RoomActivityKind::StateNotice(RoomStateNoticeActivity {
            event_id: event_id.to_owned(),
            sender: sender.to_owned(),
            state_key: state_key.to_owned(),
            content: RoomStateNoticeContent::Member {
                membership: membership.to_owned(),
                displayname: displayname.map(str::to_owned),
            },
        }),
    }
}

fn unsupported_entry_with_event_id(
    room_id: &str,
    event_id: &str,
    sender: &str,
    timestamp_ms: u64,
    msgtype: &str,
    body: &str,
) -> RoomActivity {
    RoomActivity {
        room_id: room_id.to_owned(),
        activity_id: format!("event:{event_id}"),
        timestamp_ms,
        relation: None,
        reactions: Vec::new(),
        kind: RoomActivityKind::UnsupportedMessage(UnsupportedRoomMessageActivity {
            event_id: event_id.to_owned(),
            sender: sender.to_owned(),
            msgtype: msgtype.to_owned(),
            body: body.to_owned(),
        }),
    }
}

fn image_entry_with_event_id(
    room_id: &str,
    timestamp_ms: u64,
    activity: RoomImageActivity,
) -> RoomActivity {
    RoomActivity {
        room_id: room_id.to_owned(),
        activity_id: format!("event:{}", activity.event_id),
        timestamp_ms,
        relation: None,
        reactions: Vec::new(),
        kind: RoomActivityKind::Image(Box::new(activity)),
    }
}

fn file_entry_with_event_id(
    room_id: &str,
    timestamp_ms: u64,
    activity: RoomFileActivity,
) -> RoomActivity {
    RoomActivity {
        room_id: room_id.to_owned(),
        activity_id: format!("event:{}", activity.event_id),
        timestamp_ms,
        relation: None,
        reactions: Vec::new(),
        kind: RoomActivityKind::File(Box::new(activity)),
    }
}

fn audio_entry_with_event_id(
    room_id: &str,
    timestamp_ms: u64,
    activity: RoomAudioActivity,
) -> RoomActivity {
    RoomActivity {
        room_id: room_id.to_owned(),
        activity_id: format!("event:{}", activity.event_id),
        timestamp_ms,
        relation: None,
        reactions: Vec::new(),
        kind: RoomActivityKind::Audio(Box::new(activity)),
    }
}

fn redacted_entry(
    room_id: &str,
    activity_id: &str,
    timestamp_ms: u64,
    relation: Option<RoomActivityRelation>,
    event_id: &str,
) -> RoomActivity {
    RoomActivity {
        room_id: room_id.to_owned(),
        activity_id: activity_id.to_owned(),
        timestamp_ms,
        relation,
        reactions: Vec::new(),
        kind: RoomActivityKind::Redacted(RoomRedactedActivity {
            event_id: event_id.to_owned(),
        }),
    }
}

fn fixture_encrypted_file_key() -> RoomEncryptedFileKey {
    RoomEncryptedFileKey {
        kty: "oct".to_owned(),
        key_ops: vec!["encrypt".to_owned(), "decrypt".to_owned()],
        alg: "A256CTR".to_owned(),
        k: "base64url-key".to_owned(),
        ext: true,
    }
}

fn fixture_encrypted_file_hashes() -> RoomEncryptedFileHashes {
    RoomEncryptedFileHashes {
        sha256: "base64-sha256".to_owned(),
    }
}

fn fixture_encrypted_media_source(url: &str) -> RoomMediaSource {
    RoomMediaSource::Encrypted {
        url: url.to_owned(),
        key: fixture_encrypted_file_key(),
        iv: "base64-iv".to_owned(),
        hashes: fixture_encrypted_file_hashes(),
        v: "v2".to_owned(),
    }
}

fn fixture_image_activity_with_sources(
    source: RoomMediaSource,
    thumbnail_source: Option<RoomMediaSource>,
    media_state: Option<RoomImageMediaState>,
) -> RoomImageActivity {
    RoomImageActivity {
        event_id: "$image-typed:example.org".to_owned(),
        sender: "@alice:example.org".to_owned(),
        body: "diagram.png".to_owned(),
        filename: Some("diagram.png".to_owned()),
        source,
        info: Some(RoomImageInfo {
            mimetype: Some("image/png".to_owned()),
            size: Some(12_345),
            width: Some(640),
            height: Some(480),
            is_animated: Some(false),
            thumbnail_source,
        }),
        media_state,
    }
}

fn fixture_image_activity(media_state: Option<RoomImageMediaState>) -> RoomImageActivity {
    fixture_image_activity_with_sources(
        RoomMediaSource::Plain {
            url: "mxc://example.org/diagram".to_owned(),
        },
        None,
        media_state,
    )
}

fn encrypted_fixture_image_activity(media_state: Option<RoomImageMediaState>) -> RoomImageActivity {
    fixture_image_activity_with_sources(
        fixture_encrypted_media_source("mxc://example.org/encrypted-diagram-full"),
        Some(fixture_encrypted_media_source(
            "mxc://example.org/encrypted-diagram-thumb",
        )),
        media_state,
    )
}

fn fixture_file_activity() -> RoomFileActivity {
    RoomFileActivity {
        event_id: "$file1:example.org".to_owned(),
        sender: "@alice:example.org".to_owned(),
        body: "Quarterly report".to_owned(),
        filename: Some("report.pdf".to_owned()),
        source: RoomMediaSource::Plain {
            url: "mxc://example.org/report".to_owned(),
        },
        info: Some(RoomFileInfo {
            mimetype: Some("application/pdf".to_owned()),
            size: Some(45_678),
        }),
    }
}

fn fixture_audio_activity() -> RoomAudioActivity {
    RoomAudioActivity {
        event_id: "$audio1:example.org".to_owned(),
        sender: "@alice:example.org".to_owned(),
        body: "voice-note.ogg".to_owned(),
        filename: Some("voice-note.ogg".to_owned()),
        source: RoomMediaSource::Plain {
            url: "mxc://example.org/audio".to_owned(),
        },
        info: Some(RoomAudioInfo {
            mimetype: Some("audio/ogg".to_owned()),
            size: Some(23_456),
            duration_ms: Some(12_000),
        }),
    }
}

fn room_identity(room_id: &str) -> ViewIdentity {
    ViewIdentity::Room {
        room_id: room_id.to_owned(),
    }
}

fn thread_identity(room_id: &str, thread_root_event_id: &str) -> ViewIdentity {
    ViewIdentity::Thread {
        room_id: room_id.to_owned(),
        thread_root_event_id: thread_root_event_id.to_owned(),
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

fn find_switchbar_item<'a>(ui_state: &'a UiState, identity: &ViewIdentity) -> &'a SwitchbarItem {
    ui_state
        .switchbar
        .items
        .iter()
        .find(|item| &item.identity == identity)
        .unwrap_or_else(|| panic!("missing switchbar item for {identity:?}"))
}

fn find_ui_room<'a>(ui_state: &'a UiState, room_id: &str) -> &'a UiRoomState {
    ui_state
        .rooms
        .iter()
        .find(|room| room.room_id == room_id)
        .unwrap_or_else(|| panic!("missing dumped room state for {room_id}"))
}

fn find_room_state_and_view<'a>(
    ui_state: &'a UiState,
    room_id: &str,
) -> (&'a UiRoomState, &'a MdiView) {
    (
        find_ui_room(ui_state, room_id),
        find_view(ui_state, &room_identity(room_id)),
    )
}

fn assert_active_switchbar_item(ui_state: &UiState, identity: &ViewIdentity) {
    let active_items = ui_state
        .switchbar
        .items
        .iter()
        .filter(|item| item.active)
        .collect::<Vec<_>>();
    assert_eq!(
        active_items.len(),
        1,
        "expected exactly one active switchbar item, got {active_items:?}"
    );
    assert_eq!(
        ui_state.switchbar.active_item().map(|item| &item.identity),
        Some(identity)
    );
    assert!(find_switchbar_item(ui_state, identity).active);
}

fn assert_switchbar_activity(ui_state: &UiState, identity: &ViewIdentity, expected: bool) {
    assert_eq!(
        find_switchbar_item(ui_state, identity).has_activity,
        expected,
        "unexpected switchbar activity for {identity:?}"
    );
}

fn view_contains(view: &MdiView, substring: &str) -> bool {
    view.lines.iter().any(|line| line.contains(substring))
}

fn view_lines_containing<'a>(view: &'a MdiView, substring: &str) -> Vec<&'a str> {
    view.lines
        .iter()
        .filter_map(|line| line.contains(substring).then_some(line.as_str()))
        .collect()
}

fn only_view_line_containing<'a>(view: &'a MdiView, substring: &str) -> &'a str {
    let matches = view_lines_containing(view, substring);
    assert_eq!(
        matches.len(),
        1,
        "expected exactly one view line containing {substring:?}, got {matches:?}"
    );
    matches[0]
}

fn view_member_labels(view: &MdiView) -> Vec<&str> {
    view.members
        .iter()
        .map(|member| member.label.as_str())
        .collect()
}

fn view_members_contain(view: &MdiView, needle: &str) -> bool {
    view.members
        .iter()
        .any(|member| member.label.contains(needle))
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

fn nav(text: &str, children: Vec<NavigationNode>) -> NavigationNode {
    nav_with_activity(text, false, children)
}

fn nav_with_activity(
    text: &str,
    has_activity: bool,
    children: Vec<NavigationNode>,
) -> NavigationNode {
    NavigationNode {
        text: text.to_owned(),
        has_activity,
        children,
    }
}

fn nav_path(labels: &[&str]) -> Vec<NavigationPathSegment> {
    labels
        .iter()
        .map(|label| NavigationPathSegment::first(*label))
        .collect()
}

fn nav_segment(text: &str, same_text_index: u32) -> NavigationPathSegment {
    NavigationPathSegment {
        text: text.to_owned(),
        same_text_index,
    }
}

fn navigation_path_by_substrings(
    root: &NavigationNode,
    path_substrings: &[&str],
) -> Vec<NavigationPathSegment> {
    fn find_text_path(node: &NavigationNode, path_substrings: &[&str]) -> Option<Vec<String>> {
        let (segment, remaining) = path_substrings.split_first()?;
        if !node.text.contains(segment) {
            return None;
        }
        if remaining.is_empty() {
            return Some(vec![node.text.clone()]);
        }

        node.children.iter().find_map(|child| {
            find_text_path(child, remaining).map(|mut path| {
                let mut with_self = Vec::with_capacity(path.len() + 1);
                with_self.push(node.text.clone());
                with_self.append(&mut path);
                with_self
            })
        })
    }

    find_text_path(root, path_substrings)
        .unwrap_or_else(|| panic!("missing navigation path containing {path_substrings:?}"))
        .into_iter()
        .map(NavigationPathSegment::first)
        .collect()
}

fn assert_navigation_activity(ui_state: &UiState, path_substrings: &[&str], expected: bool) {
    let node = ui_state
        .navigation
        .find_by_substring_path(path_substrings)
        .unwrap_or_else(|| panic!("missing unique navigation path containing {path_substrings:?}"));
    assert_eq!(
        node.has_activity, expected,
        "unexpected navigation activity for {path_substrings:?}"
    );
}

fn assert_no_submitted_commands(model: &Model) {
    assert_no_non_history_submitted_commands(model);
    assert!(model.submitted_history_commands.is_empty());
}

fn assert_no_non_history_submitted_commands(model: &Model) {
    assert!(model.submitted_membership_commands.is_empty());
    assert!(model.submitted_direct_room_commands.is_empty());
    assert!(model.submitted_room_topic_commands.is_empty());
    assert!(model.submitted_reaction_commands.is_empty());
    assert!(model.submitted_edit_commands.is_empty());
    assert!(model.submitted_reply_commands.is_empty());
    assert!(model.submitted_redaction_commands.is_empty());
    assert!(model.submitted_recovery_commands.is_empty());
    assert!(model.submitted_setup_recovery_commands.is_empty());
    assert!(model.submitted_verification_actions.is_empty());
}

fn assert_find_result(
    result: &FindActiveViewResult,
    active: &ViewIdentity,
    query: &str,
    matched: bool,
    match_count: usize,
    current_match_index: Option<usize>,
    selected_text: Option<&str>,
    history_search: UiFindHistorySearchState,
) {
    assert_eq!(result.active, active.clone());
    assert_eq!(result.query, query);
    assert_eq!(result.matched, matched);
    assert_eq!(result.match_count, match_count);
    assert_eq!(result.current_match_index, current_match_index);
    assert_eq!(result.selected_text.as_deref(), selected_text);
    assert_eq!(result.history_search, history_search);
}

fn assert_ui_active_find(
    ui_state: &UiState,
    active: &ViewIdentity,
    query: &str,
    matched: bool,
    match_count: usize,
    current_match_index: Option<usize>,
    selected_text: Option<&str>,
    history_search: UiFindHistorySearchState,
) {
    assert_eq!(
        ui_state.active_find,
        Some(UiFindState {
            active: active.clone(),
            query: query.to_owned(),
            matched,
            match_count,
            current_match_index,
            selected_text: selected_text.map(str::to_owned),
            history_search,
        })
    );
}

fn activate_switchbar_item_by_command(
    app: &MmatrixApp,
    identity: &ViewIdentity,
) -> Result<(), Box<dyn std::error::Error>> {
    let ui_state = app.dump_ui_state()?;
    let index = ui_state
        .switchbar
        .items
        .iter()
        .position(|item| &item.identity == identity)
        .unwrap_or_else(|| panic!("missing switchbar item for {identity:?}"));
    let frame = app.automation_frame_hwnd() as HWND;
    unsafe {
        SendMessageW(
            frame,
            WM_COMMAND,
            (SWITCHBAR_COMMAND_FIRST + index) as WPARAM,
            0,
        );
    }
    app.wait_idle(Duration::from_secs(5))?;
    Ok(())
}

fn assert_account_verification_menu_state(
    app: &MmatrixApp,
    accept: bool,
    cancel: bool,
    confirm: bool,
    reject: bool,
) {
    let state = app
        .dump_verification_menu_state()
        .expect("verification menu state must be available");
    assert_eq!(
        state.accept_enabled, accept,
        "unexpected enabled state for Accept Verification"
    );
    assert_eq!(
        state.cancel_enabled, cancel,
        "unexpected enabled state for Cancel Verification"
    );
    assert_eq!(
        state.confirm_sas_enabled, confirm,
        "unexpected enabled state for Confirm SAS"
    );
    assert_eq!(
        state.reject_sas_enabled, reject,
        "unexpected enabled state for Reject SAS"
    );
}

fn invoke_account_menu_command(
    app: &MmatrixApp,
    command_id: usize,
) -> Result<(), Box<dyn std::error::Error>> {
    let frame = app.automation_frame_hwnd() as HWND;
    unsafe {
        SendMessageW(frame, WM_COMMAND, command_id as WPARAM, 0);
    }
    app.wait_idle(Duration::from_secs(5))?;
    Ok(())
}

fn wide_null(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}

unsafe fn recover_e2ee_dialog_hwnd(frame: HWND, class_name_wide: *const u16) -> HWND {
    let dialog = unsafe { FindWindowW(class_name_wide, std::ptr::null()) };
    if !dialog.is_null() {
        return dialog;
    }

    let dialog = unsafe { GetLastActivePopup(frame) };
    if dialog != frame && unsafe { class_name(dialog) } == "mmatrixWin32RecoverE2eeDialog" {
        dialog
    } else {
        let thread_id = unsafe { GetWindowThreadProcessId(frame, std::ptr::null_mut()) };
        let mut dialog = 0 as HWND;
        unsafe {
            EnumThreadWindows(
                thread_id,
                Some(find_recover_e2ee_dialog_window),
                (&raw mut dialog) as isize,
            );
        }
        dialog
    }
}

unsafe extern "system" fn find_recover_e2ee_dialog_window(hwnd: HWND, lparam: isize) -> i32 {
    if unsafe { class_name(hwnd) } == "mmatrixWin32RecoverE2eeDialog" {
        unsafe { *(lparam as *mut HWND) = hwnd };
        0
    } else {
        1
    }
}

unsafe fn recovery_key_handoff_dialog_hwnd(frame: HWND, class_name_wide: *const u16) -> HWND {
    let dialog = unsafe { FindWindowW(class_name_wide, std::ptr::null()) };
    if !dialog.is_null() {
        return dialog;
    }

    let dialog = unsafe { GetLastActivePopup(frame) };
    if dialog != frame && unsafe { class_name(dialog) } == "mmatrixWin32RecoveryKeyHandoffDialog" {
        dialog
    } else {
        let thread_id = unsafe { GetWindowThreadProcessId(frame, std::ptr::null_mut()) };
        let mut dialog = 0 as HWND;
        unsafe {
            EnumThreadWindows(
                thread_id,
                Some(find_recovery_key_handoff_dialog_window),
                (&raw mut dialog) as isize,
            );
        }
        dialog
    }
}

unsafe extern "system" fn find_recovery_key_handoff_dialog_window(
    hwnd: HWND,
    lparam: isize,
) -> i32 {
    if unsafe { class_name(hwnd) } == "mmatrixWin32RecoveryKeyHandoffDialog" {
        unsafe { *(lparam as *mut HWND) = hwnd };
        0
    } else {
        1
    }
}

fn acknowledge_recovery_key_handoff(app: &MmatrixApp) -> Result<(), Box<dyn std::error::Error>> {
    let frame = app.automation_frame_hwnd() as HWND;
    let class_name_wide = wide_null("mmatrixWin32RecoveryKeyHandoffDialog");
    let deadline = Instant::now() + Duration::from_secs(5);
    let dialog = loop {
        let dialog = unsafe { recovery_key_handoff_dialog_hwnd(frame, class_name_wide.as_ptr()) };
        if !dialog.is_null() {
            break dialog;
        }
        if Instant::now() >= deadline {
            panic!("recovery key handoff dialog did not appear within five seconds");
        }
        std::thread::sleep(POLL_INTERVAL);
    };

    assert_eq!(
        unsafe { class_name(dialog) },
        "mmatrixWin32RecoveryKeyHandoffDialog"
    );
    assert!(
        !unsafe { GetDlgItem(dialog, RECOVERY_KEY_HANDOFF_DIALOG_KEY_ID) }.is_null(),
        "recovery key handoff dialog key edit control must exist"
    );
    let acknowledge_button =
        unsafe { GetDlgItem(dialog, RECOVERY_KEY_HANDOFF_DIALOG_ACK_ID as i32) };
    assert!(
        !acknowledge_button.is_null(),
        "recovery key handoff dialog acknowledgement button must exist"
    );
    assert_eq!(
        unsafe { read_window_text(acknowledge_button) },
        "I Saved It"
    );
    assert_ne!(
        unsafe {
            PostMessageW(
                dialog,
                WM_COMMAND,
                RECOVERY_KEY_HANDOFF_DIALOG_ACK_ID as WPARAM,
                0,
            )
        },
        0,
        "posting recovery key handoff acknowledgement must succeed"
    );

    let deadline = Instant::now() + Duration::from_secs(5);
    while unsafe { IsWindow(dialog) } != 0 {
        if Instant::now() >= deadline {
            panic!("recovery key handoff dialog did not close within five seconds");
        }
        std::thread::sleep(POLL_INTERVAL);
    }
    app.wait_idle(Duration::from_secs(5))?;
    Ok(())
}

fn submit_recover_e2ee_menu_dialog(
    app: &MmatrixApp,
    secret: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let frame = app.automation_frame_hwnd() as HWND;
    let frame_value = frame as isize;
    let menu_thread = std::thread::spawn(move || unsafe {
        SendMessageW(
            frame_value as HWND,
            WM_COMMAND,
            MENU_ID_RECOVER_E2EE as WPARAM,
            0,
        );
    });

    let class_name = wide_null("mmatrixWin32RecoverE2eeDialog");
    let deadline = Instant::now() + Duration::from_secs(5);
    let dialog = loop {
        let dialog = unsafe { recover_e2ee_dialog_hwnd(frame, class_name.as_ptr()) };
        if !dialog.is_null() {
            break dialog;
        }
        if Instant::now() >= deadline {
            panic!("Recover E2EE dialog did not appear within five seconds");
        }
        std::thread::sleep(POLL_INTERVAL);
    };

    let secret_edit = unsafe { GetDlgItem(dialog, RECOVER_E2EE_DIALOG_SECRET_ID) };
    assert!(
        !secret_edit.is_null(),
        "Recover E2EE dialog secret edit control must exist"
    );
    let secret = wide_null(secret);
    assert_ne!(
        unsafe { SetWindowTextW(secret_edit, secret.as_ptr()) },
        0,
        "setting Recover E2EE dialog secret must succeed"
    );
    assert_ne!(
        unsafe { PostMessageW(dialog, WM_COMMAND, RECOVER_E2EE_DIALOG_OK_ID as WPARAM, 0) },
        0,
        "posting Recover E2EE dialog OK command must succeed"
    );

    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if unsafe { IsWindow(dialog) } == 0 {
            break;
        }
        if Instant::now() >= deadline {
            panic!("Recover E2EE dialog did not close within five seconds");
        }
        std::thread::sleep(POLL_INTERVAL);
    }
    menu_thread
        .join()
        .expect("Recover E2EE menu command thread must not panic");
    app.wait_idle(Duration::from_secs(5))?;
    Ok(())
}

fn submit_setup_e2ee_recovery_menu_dialog(
    app: &MmatrixApp,
    passphrase: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let frame = app.automation_frame_hwnd() as HWND;
    let frame_value = frame as isize;
    let menu_thread = std::thread::spawn(move || unsafe {
        SendMessageW(
            frame_value as HWND,
            WM_COMMAND,
            MENU_ID_SETUP_E2EE_RECOVERY as WPARAM,
            0,
        );
    });

    let class_name = wide_null("mmatrixWin32SetupE2eeDialog");
    let deadline = Instant::now() + Duration::from_secs(5);
    let dialog = loop {
        let dialog = unsafe { FindWindowW(class_name.as_ptr(), std::ptr::null()) };
        if !dialog.is_null() {
            break dialog;
        }
        if Instant::now() >= deadline {
            panic!("Set Up E2EE Recovery dialog did not appear within five seconds");
        }
        std::thread::sleep(POLL_INTERVAL);
    };

    let passphrase_edit = unsafe { GetDlgItem(dialog, SETUP_E2EE_DIALOG_PASSPHRASE_ID) };
    assert!(
        !passphrase_edit.is_null(),
        "setup passphrase edit must exist"
    );
    let passphrase = wide_null(passphrase);
    assert_ne!(
        unsafe { SetWindowTextW(passphrase_edit, passphrase.as_ptr()) },
        0
    );
    assert_ne!(
        unsafe { PostMessageW(dialog, WM_COMMAND, SETUP_E2EE_DIALOG_OK_ID as WPARAM, 0) },
        0
    );

    let deadline = Instant::now() + Duration::from_secs(5);
    while unsafe { IsWindow(dialog) } != 0 {
        if Instant::now() >= deadline {
            panic!("Set Up E2EE Recovery dialog did not close within five seconds");
        }
        std::thread::sleep(POLL_INTERVAL);
    }
    menu_thread
        .join()
        .expect("setup E2EE menu command thread must not panic");
    app.wait_idle(Duration::from_secs(5))?;
    Ok(())
}

unsafe fn mdi_client_hwnd(app: &MmatrixApp) -> HWND {
    let frame = app.automation_frame_hwnd() as HWND;
    let mdi_client = unsafe { GetDlgItem(frame, MDI_ID) };
    assert!(!mdi_client.is_null(), "MDI client control must exist");
    mdi_client
}

unsafe fn mdi_child_hwnd_by_title(app: &MmatrixApp, title: &str) -> HWND {
    let mdi_client = unsafe { mdi_client_hwnd(app) };
    let mut child =
        unsafe { FindWindowExW(mdi_client, 0 as HWND, std::ptr::null(), std::ptr::null()) };
    while !child.is_null() {
        if unsafe { read_window_text(child) } == title {
            return child;
        }
        child = unsafe { FindWindowExW(mdi_client, child, std::ptr::null(), std::ptr::null()) };
    }
    panic!("missing MDI child window with title {title:?}");
}

fn native_activate_mdi_view(
    app: &MmatrixApp,
    identity: &ViewIdentity,
) -> Result<(), Box<dyn std::error::Error>> {
    let ui_state = app.dump_ui_state()?;
    let title = find_view(&ui_state, identity).title.clone();
    let mdi_client = unsafe { mdi_client_hwnd(app) };
    let child = unsafe { mdi_child_hwnd_by_title(app, &title) };
    unsafe {
        SendMessageW(mdi_client, WM_MDIACTIVATE, child as WPARAM, 0);
    }
    app.wait_idle(Duration::from_secs(5))?;
    Ok(())
}

fn assert_raw_switchbar_state(
    app: &MmatrixApp,
    ui_state: &UiState,
    identity: &ViewIdentity,
    checked: bool,
    marked: bool,
) {
    let index = ui_state
        .switchbar
        .items
        .iter()
        .position(|item| &item.identity == identity)
        .unwrap_or_else(|| panic!("missing switchbar item for {identity:?}"));
    let frame = app.automation_frame_hwnd() as HWND;
    let switchbar = unsafe { GetDlgItem(frame, SWITCHBAR_ID) };
    assert!(!switchbar.is_null(), "switchbar toolbar control must exist");
    let state = unsafe {
        SendMessageW(
            switchbar,
            TB_GETSTATE,
            (SWITCHBAR_COMMAND_FIRST + index) as WPARAM,
            0,
        )
    } as usize;
    assert_eq!(
        state & TBSTATE_CHECKED as usize != 0,
        checked,
        "unexpected raw checked state for {identity:?}"
    );
    assert_eq!(
        state & TBSTATE_MARKED as usize != 0,
        marked,
        "unexpected raw marked state for {identity:?}"
    );
}

unsafe fn compose_edit_hwnd(app: &MmatrixApp) -> HWND {
    let frame = app.automation_frame_hwnd() as HWND;
    let edit = unsafe { GetDlgItem(frame, EDIT_ID) };
    assert!(!edit.is_null(), "compose edit control must exist");
    edit
}

unsafe fn focused_hwnd(app: &MmatrixApp) -> HWND {
    let frame = app.automation_frame_hwnd() as HWND;
    let ui_thread = unsafe { GetWindowThreadProcessId(frame, null_mut()) };
    assert_ne!(ui_thread, 0, "UI thread ID must exist");
    let current_thread = unsafe { GetCurrentThreadId() };
    if current_thread != ui_thread {
        assert_ne!(
            unsafe { AttachThreadInput(current_thread, ui_thread, 1) },
            0,
            "AttachThreadInput must succeed"
        );
    }
    let focused = unsafe { GetFocus() };
    if current_thread != ui_thread {
        assert_ne!(
            unsafe { AttachThreadInput(current_thread, ui_thread, 0) },
            0,
            "AttachThreadInput detach must succeed"
        );
    }
    focused
}

fn assert_compose_has_focus(app: &MmatrixApp) {
    let compose = unsafe { compose_edit_hwnd(app) };
    let deadline = Instant::now() + Duration::from_secs(1);
    loop {
        let focused = unsafe { focused_hwnd(app) };
        if focused == compose {
            return;
        }
        if Instant::now() >= deadline {
            assert_eq!(focused, compose, "compose edit must have Win32 focus");
        }
        std::thread::sleep(POLL_INTERVAL);
    }
}

fn set_clipboard_text(owner: HWND, text: &str) -> Result<(), String> {
    let wide = text.encode_utf16().chain([0]).collect::<Vec<_>>();
    let bytes_len = std::mem::size_of_val(wide.as_slice());

    unsafe {
        if OpenClipboard(owner) == 0 {
            return Err(format!(
                "OpenClipboard failed: {}",
                std::io::Error::last_os_error()
            ));
        }
        if EmptyClipboard() == 0 {
            let _ = CloseClipboard();
            return Err(format!(
                "EmptyClipboard failed: {}",
                std::io::Error::last_os_error()
            ));
        }

        let memory = GlobalAlloc(GMEM_MOVEABLE, bytes_len);
        if memory.is_null() {
            let _ = CloseClipboard();
            return Err(format!(
                "GlobalAlloc failed: {}",
                std::io::Error::last_os_error()
            ));
        }

        let locked = GlobalLock(memory).cast::<u16>();
        if locked.is_null() {
            let _ = GlobalUnlock(memory);
            let _ = CloseClipboard();
            return Err(format!(
                "GlobalLock failed: {}",
                std::io::Error::last_os_error()
            ));
        }
        copy_nonoverlapping(wide.as_ptr(), locked, wide.len());
        let _ = GlobalUnlock(memory);

        if SetClipboardData(CF_UNICODETEXT, memory).is_null() {
            let _ = CloseClipboard();
            return Err(format!(
                "SetClipboardData failed: {}",
                std::io::Error::last_os_error()
            ));
        }
        if CloseClipboard() == 0 {
            return Err(format!(
                "CloseClipboard failed: {}",
                std::io::Error::last_os_error()
            ));
        }
    }

    Ok(())
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
    let app = launch_app(&exe_path, &args)?;
    app.wait_idle(Duration::from_secs(5))?;

    let model = app.dump_model()?;
    assert_eq!(model.lifecycle_state, "ready");
    assert_eq!(
        model.rooms,
        vec![
            listed_room("!beta:example", "Beta", true),
            listed_room("!alpha:example", "Alpha", false),
        ]
    );
    assert_eq!(model.selected_room.as_deref(), Some("!beta:example"));
    assert_eq!(
        model.last_activity,
        Some(timeline_line(
            "!beta:example",
            "@bob:example",
            1_700_000_120_000,
            "beta second",
        ))
    );

    let timeline = app.room_activity(Some("!beta:example"))?;
    assert_eq!(
        timeline,
        vec![
            text_entry(
                "!beta:example",
                "@bob:example",
                1_700_000_060_000,
                "beta first",
            ),
            text_entry(
                "!beta:example",
                "@bob:example",
                1_700_000_120_000,
                "beta second",
            ),
        ]
    );
    let ui_state = app.dump_ui_state()?;
    assert_eq!(ui_state.mdi.active, room_identity("!beta:example"));
    assert!(!ui_state.status_bar.text.is_empty());
    assert!(
        ui_state
            .status_bar
            .text
            .contains("timeline update from !beta:example")
    );
    assert_eq!(
        ui_state
            .switchbar
            .items
            .iter()
            .map(|item| &item.identity)
            .collect::<Vec<_>>(),
        vec![
            &ViewIdentity::Status,
            &room_identity("!beta:example"),
            &room_identity("!alpha:example"),
        ]
    );
    assert_eq!(
        ui_state
            .switchbar
            .items
            .iter()
            .map(|item| item.title.as_str())
            .collect::<Vec<_>>(),
        vec!["Status", "Beta [e2ee]", "Alpha"]
    );
    assert_active_switchbar_item(&ui_state, &ui_state.mdi.active);
    assert_switchbar_activity(&ui_state, &ViewIdentity::Status, false);
    assert_switchbar_activity(&ui_state, &room_identity("!beta:example"), false);
    assert_switchbar_activity(&ui_state, &room_identity("!alpha:example"), true);

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

    app.wait_for_room_activity_contains(
        Some("!beta:example"),
        "beta second",
        Duration::from_secs(5),
    )?;
    let screenshot_path = unique_screenshot_path();
    app.screenshot(&screenshot_path)?;
    let screenshot_bytes = fs::read(&screenshot_path)?;
    assert!(screenshot_bytes.starts_with(b"\x89PNG\r\n\x1a\n"));

    let _ = fs::remove_file(&state_path);
    let _ = fs::remove_file(&screenshot_path);
    Ok(())
}

#[test]
fn automation_driver_reports_failed_outgoing_text_state() -> Result<(), Box<dyn std::error::Error>>
{
    let exe_path = mmatrix_ui_exe_path()
        .map_err(|error| format!("failed to locate mmatrix-ui.exe: {error}"))?;
    let state_path = write_state_file(&[
        Event::Started,
        Event::SyncedRooms(vec![room_summary("!ops:example", "Ops", false)]),
        pending_message_event(
            "!ops:example",
            "txn-1",
            "@alice:example",
            1_700_000_000_000,
            "hello",
        ),
        message_failed_update_event("!ops:example", "txn-1", "send failed", true),
        Event::Error(mmatrix_core::CoreErrorEvent {
            operation: mmatrix_core::Operation::SendMessage,
            message: "send failed".to_owned(),
        }),
    ])?;

    let args = [
        OsString::from("--render-state"),
        state_path.as_os_str().to_os_string(),
    ];
    let app = launch_app(&exe_path, &args)?;
    app.wait_idle(Duration::from_secs(5))?;

    assert_eq!(
        app.room_activity(Some("!ops:example"))?,
        vec![failed_entry(
            "!ops:example",
            "@alice:example",
            1_700_000_000_000,
            "txn-1",
            "hello",
            "send failed",
            true,
        )]
    );

    let ui_state = app.dump_ui_state()?;
    assert!(view_contains(
        find_view(&ui_state, &room_identity("!ops:example")),
        "hello (failed)"
    ));

    let _ = fs::remove_file(&state_path);
    Ok(())
}

#[test]
fn automation_driver_reconciles_remote_echo_after_message_sent_update()
-> Result<(), Box<dyn std::error::Error>> {
    let exe_path = mmatrix_ui_exe_path()
        .map_err(|error| format!("failed to locate mmatrix-ui.exe: {error}"))?;
    let local_app_data = unique_local_app_data_path();
    let options = launch_options(Some(local_app_data.clone()));
    let args = [OsString::from("--automation")];

    let app = launch_app_with_options(&exe_path, &args, options)?;
    app.wait_for_lifecycle_state("core_started", Duration::from_secs(30))?;
    app.load_events(&serde_json::to_string(&vec![
        Event::Started,
        Event::SyncedRooms(vec![room_summary("!ops:example", "Ops", false)]),
        pending_message_event(
            "!ops:example",
            "txn-1",
            "@alice:example",
            1_700_000_000_000,
            "hello",
        ),
        text_event_with_event_id(
            "!ops:example",
            "$event:example.org",
            "@alice:example",
            1_700_000_000_100,
            "hello",
        ),
    ])?)?;
    app.wait_idle(Duration::from_secs(5))?;

    app.append_synthetic_events(&serde_json::to_string(&vec![message_sent_update_event(
        "!ops:example",
        "txn-1",
        "$event:example.org",
    )])?)?;
    app.wait_idle(Duration::from_secs(5))?;

    assert_eq!(
        app.room_activity(Some("!ops:example"))?,
        vec![RoomActivity {
            room_id: "!ops:example".to_owned(),
            activity_id: "event:$event:example.org".to_owned(),
            timestamp_ms: 1_700_000_000_100,
            relation: None,
            reactions: Vec::new(),
            kind: RoomActivityKind::Message(RoomMessageActivity {
                event_id: Some("$event:example.org".to_owned()),
                transaction_id: Some("txn-1".to_owned()),
                sender: "@alice:example".to_owned(),
                presentation: RoomMessagePresentationKind::Text,
                body: "hello".to_owned(),
                send_state: RoomMessageSendState::Sent,
            }),
        }]
    );

    drop(app);
    let _ = fs::remove_dir_all(&local_app_data);
    Ok(())
}

#[test]
fn automation_driver_rejects_invalid_append_message_sent_update_without_exiting()
-> Result<(), Box<dyn std::error::Error>> {
    let exe_path = mmatrix_ui_exe_path()
        .map_err(|error| format!("failed to locate mmatrix-ui.exe: {error}"))?;
    let local_app_data = unique_local_app_data_path();
    let options = launch_options(Some(local_app_data.clone()));
    let args = [OsString::from("--automation")];

    let app = launch_app_with_options(&exe_path, &args, options)?;
    app.wait_for_lifecycle_state("core_started", Duration::from_secs(30))?;
    app.load_events(&serde_json::to_string(&vec![
        Event::Started,
        Event::SyncedRooms(vec![room_summary("!ops:example", "Ops", false)]),
    ])?)?;
    app.wait_idle(Duration::from_secs(5))?;
    app.select_room("!ops:example")?;

    match app.append_synthetic_events(&serde_json::to_string(&vec![message_sent_update_event(
        "!ops:example",
        "txn-missing",
        "$event:example.org",
    )])?) {
        Err(TestkitError::Protocol { code, message }) => {
            assert_eq!(code, "invalid_synthetic_state");
            assert_eq!(
                message,
                "outgoing sent requires an existing queued timeline row"
            );
        }
        other => panic!("expected invalid_synthetic_state for append, got {other:?}"),
    }

    app.ping()?;
    assert_eq!(
        app.room_activity(Some("!ops:example"))?,
        Vec::<RoomActivity>::new()
    );
    assert_eq!(
        app.dump_model()?.selected_room.as_deref(),
        Some("!ops:example")
    );

    drop(app);
    let _ = fs::remove_dir_all(&local_app_data);
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
    let app = launch_app(&exe_path, &args)?;
    app.wait_idle(Duration::from_secs(5))?;

    let attached = MmatrixApp::find_by_pid(app.pid(), Duration::from_secs(5))?;
    attached.ping()?;

    let version = attached.get_version()?;
    assert_eq!(version.protocol_version, 25);
    assert_eq!(version.app_version, env!("CARGO_PKG_VERSION"));

    assert_eq!(
        attached.list_rooms()?,
        vec![
            listed_room("!beta:example", "Beta", true),
            listed_room("!alpha:example", "Alpha", false),
        ]
    );

    attached.select_room("!alpha:example")?;
    assert_eq!(
        attached.room_activity(None)?,
        vec![text_entry(
            "!alpha:example",
            "@alice:example",
            1_700_000_000_000,
            "alpha first",
        )]
    );
    let alpha_state = attached.dump_ui_state()?;
    assert_eq!(alpha_state.mdi.active, room_identity("!alpha:example"));
    assert_active_switchbar_item(&alpha_state, &room_identity("!alpha:example"));
    let alpha_view = find_view(&alpha_state, &room_identity("!alpha:example"));
    assert!(alpha_view.active);
    assert!(view_contains(alpha_view, "alpha first"));

    attached.load_events(&serde_json::to_string(&replacement_events())?)?;
    attached.wait_idle(Duration::from_secs(5))?;

    assert_eq!(
        attached.list_rooms()?,
        vec![
            listed_room("!gamma:example", "Gamma", false),
            listed_room("!delta:example", "Delta", false),
        ]
    );
    assert_eq!(
        attached.room_activity(None)?,
        vec![text_entry(
            "!gamma:example",
            "@alice:example",
            1_700_000_000_000,
            "gamma only",
        )]
    );
    assert_eq!(
        attached.dump_model()?.last_activity,
        Some(timeline_line(
            "!delta:example",
            "@bob:example",
            1_700_000_120_000,
            "delta second",
        ))
    );
    assert_eq!(attached.dump_model()?.lifecycle_state, "ready");
    let replacement_state = attached.dump_ui_state()?;
    assert_eq!(
        replacement_state.mdi.active,
        room_identity("!gamma:example")
    );
    assert_active_switchbar_item(&replacement_state, &room_identity("!gamma:example"));
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
    assert!(
        replacement_state
            .switchbar
            .items
            .iter()
            .all(|item| item.identity != room_identity("!alpha:example"))
    );
    assert!(
        replacement_state
            .switchbar
            .items
            .iter()
            .all(|item| item.identity != room_identity("!beta:example"))
    );
    assert_eq!(
        replacement_state
            .switchbar
            .items
            .iter()
            .map(|item| &item.identity)
            .collect::<Vec<_>>(),
        vec![
            &ViewIdentity::Status,
            &room_identity("!gamma:example"),
            &room_identity("!delta:example"),
        ]
    );
    assert_switchbar_activity(&replacement_state, &room_identity("!delta:example"), true);
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
fn automation_driver_exposes_space_metadata_but_rejects_space_actions()
-> Result<(), Box<dyn std::error::Error>> {
    let exe_path = mmatrix_ui_exe_path()
        .map_err(|error| format!("failed to locate mmatrix-ui.exe: {error}"))?;
    let state_path = write_state_file(&[
        Event::Started,
        Event::SyncedRooms(vec![
            RoomSummary {
                room_id: "!space:example".to_owned(),
                display_name: "Space".to_owned(),
                topic: None,
                membership: RoomMembership::Join,
                encrypted: false,
                is_space: true,
                is_direct: false,
                direct_user_ids: Vec::new(),
                tags: Vec::new(),
                space_children: vec![SpaceChildSummary {
                    room_id: "!room:example".to_owned(),
                    suggested: true,
                    order: Some("aaa".to_owned()),
                }],
            },
            room_summary("!room:example", "Room", true),
        ]),
        text_event(
            "!room:example",
            "@alice:example",
            1_700_000_000_000,
            "hello",
        ),
    ])?;

    let args = [
        OsString::from("--render-state"),
        state_path.as_os_str().to_os_string(),
    ];
    let app = launch_app(&exe_path, &args)?;
    app.wait_idle(Duration::from_secs(5))?;

    assert_eq!(
        app.list_rooms()?,
        vec![
            listed_space(
                "!space:example",
                "Space",
                vec![SpaceChild {
                    room_id: "!room:example".to_owned(),
                    suggested: true,
                    order: Some("aaa".to_owned()),
                }],
            ),
            listed_room("!room:example", "Room", true),
        ]
    );
    assert_eq!(
        app.dump_model()?.selected_room.as_deref(),
        Some("!room:example")
    );

    match app.select_room("!space:example") {
        Err(TestkitError::Protocol { code, message }) => {
            assert_eq!(code, "unknown_room");
            assert_eq!(message, "unknown room id: !space:example");
        }
        other => panic!("expected select_room unknown_room for space, got {other:?}"),
    }
    match app.send_text(Some("!space:example"), "hello") {
        Err(TestkitError::Protocol { code, message }) => {
            assert_eq!(code, "unknown_room");
            assert_eq!(message, "unknown room id: !space:example");
        }
        other => panic!("expected send_text unknown_room for space, got {other:?}"),
    }
    match app.room_activity(Some("!space:example")) {
        Err(TestkitError::Protocol { code, message }) => {
            assert_eq!(code, "unknown_room");
            assert_eq!(message, "unknown room id: !space:example");
        }
        other => panic!("expected timeline unknown_room for space, got {other:?}"),
    }

    let _ = fs::remove_file(&state_path);
    Ok(())
}

#[test]
fn automation_driver_lifecycle_commands_are_accepted_in_render_state_mode()
-> Result<(), Box<dyn std::error::Error>> {
    let exe_path = mmatrix_ui_exe_path()
        .map_err(|error| format!("failed to locate mmatrix-ui.exe: {error}"))?;
    let state_path = write_state_file(&base_events())?;
    let local_app_data = unique_local_app_data_path();
    let session_path = write_saved_session(&local_app_data)?;
    let raw_session_bytes = fs::read(&session_path)?;
    assert_bytes_do_not_contain(&raw_session_bytes, "secret-token");
    assert_bytes_do_not_contain(&raw_session_bytes, "refresh-token");
    assert_bytes_do_not_contain(&raw_session_bytes, "\"access_token\"");

    let args = [
        OsString::from("--render-state"),
        state_path.as_os_str().to_os_string(),
    ];
    let app = launch_app_with_options(
        &exe_path,
        &args,
        launch_options(Some(local_app_data.clone())),
    )?;
    app.wait_idle(Duration::from_secs(5))?;

    for result in [
        app.login("https://matrix.example", "@alice:example", "secret", None),
        app.restore_session("https://matrix.example", stored_session()),
        app.restore_saved_session(),
        app.start_sync(),
        app.logout(),
    ] {
        result?;
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
    let app = launch_app_with_options(
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
    assert!(
        app.dump_ui_state()?
            .status_bar
            .text
            .contains("authenticated")
    );
    assert!(!serde_json::to_string(&model)?.contains("secret-token"));
    assert!(!session_path.exists());

    app.load_events(&serde_json::to_string(&vec![
        Event::Started,
        Event::RestoredSession,
    ])?)?;
    app.wait_for_lifecycle_state("authenticated", Duration::from_secs(5))?;
    assert_eq!(app.dump_model()?.status, "restored saved session");
    assert!(
        app.dump_ui_state()?
            .status_bar
            .text
            .contains("restored saved session")
    );

    app.load_events(&serde_json::to_string(&vec![
        Event::Started,
        Event::RestoredSession,
        Event::SyncStarted,
    ])?)?;
    app.wait_for_lifecycle_state("syncing", Duration::from_secs(5))?;
    assert_eq!(app.dump_model()?.status, "continuous sync started");
    assert!(
        app.dump_ui_state()?
            .status_bar
            .text
            .contains("continuous sync started")
    );

    app.load_events(&serde_json::to_string(&vec![
        Event::Started,
        Event::RestoredSession,
        Event::SyncedRooms(vec![room_summary("!synthetic:example", "Synthetic", false)]),
        Event::LoggedOut,
    ])?)?;
    app.wait_for_lifecycle_state("logged_out", Duration::from_secs(5))?;
    let logged_out_model = app.dump_model()?;
    assert_eq!(logged_out_model.status, "logged out");
    assert!(logged_out_model.rooms.is_empty());
    assert!(logged_out_model.selected_room.is_none());
    let logged_out_state = app.dump_ui_state()?;
    assert!(logged_out_state.status_bar.text.contains("logged out"));
    assert_eq!(
        logged_out_state
            .switchbar
            .items
            .iter()
            .map(|item| &item.identity)
            .collect::<Vec<_>>(),
        vec![&ViewIdentity::Status]
    );
    assert_active_switchbar_item(&logged_out_state, &ViewIdentity::Status);

    let _ = fs::remove_file(&state_path);
    let _ = fs::remove_dir_all(&local_app_data);
    Ok(())
}

#[test]
fn automation_driver_dumps_typed_e2ee_status_from_synthetic_events()
-> Result<(), Box<dyn std::error::Error>> {
    let exe_path = mmatrix_ui_exe_path()
        .map_err(|error| format!("failed to locate mmatrix-ui.exe: {error}"))?;
    let args = [OsString::from("--automation")];
    let app = launch_app(&exe_path, &args)?;
    app.wait_for_lifecycle_state("core_started", Duration::from_secs(30))?;

    app.load_events(&serde_json::to_string(&vec![
        Event::Started,
        Event::E2eeStatus(sample_e2ee_status()),
    ])?)?;
    app.wait_idle(Duration::from_secs(5))?;

    let model = app.dump_model()?;
    assert_eq!(model.selected_room, None);
    assert_eq!(model.e2ee_status, Some(sample_e2ee_status()));

    let ui_state = app.dump_ui_state()?;
    assert_eq!(ui_state.mdi.active, ViewIdentity::Status);
    assert_active_switchbar_item(&ui_state, &ViewIdentity::Status);
    let status_view = find_view(&ui_state, &ViewIdentity::Status);
    assert!(status_view.active);
    assert!(view_contains(status_view, "recovery: incomplete"));
    assert!(view_contains(status_view, "local backup: enabled"));
    assert!(view_contains(status_view, "server backup: exists"));
    assert!(view_contains(status_view, "cross-signing: incomplete"));
    assert!(view_contains(
        status_view,
        "actions: /e2ee refresh to check server backup, /e2ee setup to enable recovery, /e2ee recover to restore keys",
    ));
    assert!(
        ui_state
            .status_bar
            .text
            .contains("e2ee: recovery incomplete, backup healthy, cross-signing incomplete")
    );

    Ok(())
}

#[test]
fn automation_driver_submits_verification_actions_for_current_flow_without_local_state_mutation()
-> Result<(), Box<dyn std::error::Error>> {
    let exe_path = mmatrix_ui_exe_path()
        .map_err(|error| format!("failed to locate mmatrix-ui.exe: {error}"))?;
    let args = [OsString::from("--automation")];
    let app = launch_app(&exe_path, &args)?;
    app.wait_for_lifecycle_state("core_started", Duration::from_secs(30))?;

    app.load_events(&serde_json::to_string(&vec![
        Event::Started,
        verification_state_event(
            "flow-current",
            "@alice:example",
            CoreVerificationState::SasReady {
                emojis: vec![CoreVerificationEmoji {
                    symbol: "🐶".to_owned(),
                    description: "dog".to_owned(),
                }],
                decimals: CoreVerificationDecimals {
                    first: 1234,
                    second: 5678,
                    third: 9012,
                },
            },
        ),
    ])?)?;
    app.wait_idle(Duration::from_secs(5))?;

    let initial_model = app.dump_model()?;
    assert_eq!(
        initial_model.last_verification,
        Some(Verification {
            flow_id: "flow-current".to_owned(),
            user_id: "@alice:example".to_owned(),
            state: VerificationState::SasReady {
                emojis: vec![mmatrix_testkit::VerificationEmoji {
                    symbol: "🐶".to_owned(),
                    description: "dog".to_owned(),
                }],
                decimals: mmatrix_testkit::VerificationDecimals {
                    first: 1234,
                    second: 5678,
                    third: 9012,
                },
            },
        })
    );
    assert!(initial_model.submitted_verification_actions.is_empty());

    app.accept_verification()?;
    app.cancel_verification()?;
    app.confirm_verification_sas()?;
    app.reject_verification_sas()?;
    app.wait_idle(Duration::from_secs(5))?;

    let model = app.dump_model()?;
    assert_eq!(model.last_verification, initial_model.last_verification);
    assert_eq!(
        model.submitted_verification_actions,
        vec![
            SubmittedVerificationAction {
                action: VerificationActionKind::Accept,
                flow_id: "flow-current".to_owned(),
                user_id: "@alice:example".to_owned(),
            },
            SubmittedVerificationAction {
                action: VerificationActionKind::Cancel,
                flow_id: "flow-current".to_owned(),
                user_id: "@alice:example".to_owned(),
            },
            SubmittedVerificationAction {
                action: VerificationActionKind::ConfirmSas,
                flow_id: "flow-current".to_owned(),
                user_id: "@alice:example".to_owned(),
            },
            SubmittedVerificationAction {
                action: VerificationActionKind::RejectSas,
                flow_id: "flow-current".to_owned(),
                user_id: "@alice:example".to_owned(),
            },
        ]
    );

    Ok(())
}

#[test]
fn automation_driver_submits_verification_action_for_explicit_flow()
-> Result<(), Box<dyn std::error::Error>> {
    let exe_path = mmatrix_ui_exe_path()
        .map_err(|error| format!("failed to locate mmatrix-ui.exe: {error}"))?;
    let args = [OsString::from("--automation")];
    let app = launch_app(&exe_path, &args)?;
    app.wait_for_lifecycle_state("core_started", Duration::from_secs(30))?;

    app.load_events(&serde_json::to_string(&vec![
        Event::Started,
        verification_state_event(
            "flow-current",
            "@alice:example",
            CoreVerificationState::Requested,
        ),
    ])?)?;
    app.wait_idle(Duration::from_secs(5))?;

    let initial_model = app.dump_model()?;
    app.accept_verification_flow("flow-explicit", "@bob:example")?;
    app.wait_idle(Duration::from_secs(5))?;

    let model = app.dump_model()?;
    assert_eq!(model.last_verification, initial_model.last_verification);
    assert_eq!(
        model.submitted_verification_actions,
        vec![SubmittedVerificationAction {
            action: VerificationActionKind::Accept,
            flow_id: "flow-explicit".to_owned(),
            user_id: "@bob:example".to_owned(),
        }]
    );

    Ok(())
}

#[test]
fn automation_driver_requested_verification_menu_state_and_commands()
-> Result<(), Box<dyn std::error::Error>> {
    let exe_path = mmatrix_ui_exe_path()
        .map_err(|error| format!("failed to locate mmatrix-ui.exe: {error}"))?;
    let args = [OsString::from("--automation")];
    let app = launch_app(&exe_path, &args)?;
    app.wait_for_lifecycle_state("core_started", Duration::from_secs(30))?;

    app.load_events(&serde_json::to_string(&vec![
        Event::Started,
        verification_state_event(
            "flow-requested",
            "@alice:example",
            CoreVerificationState::Requested,
        ),
    ])?)?;
    app.wait_idle(Duration::from_secs(5))?;

    let initial_model = app.dump_model()?;
    assert_account_verification_menu_state(&app, true, true, false, false);

    invoke_account_menu_command(&app, MENU_ID_VERIFY_ACCEPT)?;
    invoke_account_menu_command(&app, MENU_ID_VERIFY_CANCEL)?;

    let model = app.dump_model()?;
    assert_eq!(model.last_verification, initial_model.last_verification);
    assert_eq!(
        model.submitted_verification_actions,
        vec![
            SubmittedVerificationAction {
                action: VerificationActionKind::Accept,
                flow_id: "flow-requested".to_owned(),
                user_id: "@alice:example".to_owned(),
            },
            SubmittedVerificationAction {
                action: VerificationActionKind::Cancel,
                flow_id: "flow-requested".to_owned(),
                user_id: "@alice:example".to_owned(),
            },
        ]
    );

    Ok(())
}

#[test]
fn automation_driver_sas_ready_verification_menu_state_and_commands()
-> Result<(), Box<dyn std::error::Error>> {
    let exe_path = mmatrix_ui_exe_path()
        .map_err(|error| format!("failed to locate mmatrix-ui.exe: {error}"))?;
    let args = [OsString::from("--automation")];
    let app = launch_app(&exe_path, &args)?;
    app.wait_for_lifecycle_state("core_started", Duration::from_secs(30))?;

    app.load_events(&serde_json::to_string(&vec![
        Event::Started,
        verification_state_event(
            "flow-sas-ready",
            "@alice:example",
            CoreVerificationState::SasReady {
                emojis: vec![CoreVerificationEmoji {
                    symbol: "🐶".to_owned(),
                    description: "dog".to_owned(),
                }],
                decimals: CoreVerificationDecimals {
                    first: 1234,
                    second: 5678,
                    third: 9012,
                },
            },
        ),
    ])?)?;
    app.wait_idle(Duration::from_secs(5))?;

    let initial_model = app.dump_model()?;
    assert_account_verification_menu_state(&app, false, true, true, true);

    invoke_account_menu_command(&app, MENU_ID_VERIFY_CONFIRM_SAS)?;
    invoke_account_menu_command(&app, MENU_ID_VERIFY_REJECT_SAS)?;
    invoke_account_menu_command(&app, MENU_ID_VERIFY_CANCEL)?;

    let model = app.dump_model()?;
    assert_eq!(model.last_verification, initial_model.last_verification);
    assert_eq!(
        model.submitted_verification_actions,
        vec![
            SubmittedVerificationAction {
                action: VerificationActionKind::ConfirmSas,
                flow_id: "flow-sas-ready".to_owned(),
                user_id: "@alice:example".to_owned(),
            },
            SubmittedVerificationAction {
                action: VerificationActionKind::RejectSas,
                flow_id: "flow-sas-ready".to_owned(),
                user_id: "@alice:example".to_owned(),
            },
            SubmittedVerificationAction {
                action: VerificationActionKind::Cancel,
                flow_id: "flow-sas-ready".to_owned(),
                user_id: "@alice:example".to_owned(),
            },
        ]
    );

    Ok(())
}

#[test]
fn automation_driver_done_and_cancelled_verification_menu_items_are_disabled()
-> Result<(), Box<dyn std::error::Error>> {
    let exe_path = mmatrix_ui_exe_path()
        .map_err(|error| format!("failed to locate mmatrix-ui.exe: {error}"))?;

    let done_args = [
        OsString::from("--render-state"),
        fixture_state_path("verification-done.json")
            .as_os_str()
            .to_os_string(),
    ];
    let done_app = launch_app(&exe_path, &done_args)?;
    done_app.wait_idle(Duration::from_secs(5))?;
    assert_account_verification_menu_state(&done_app, false, false, false, false);
    drop(done_app);

    let cancelled_args = [
        OsString::from("--render-state"),
        fixture_state_path("verification-cancelled.json")
            .as_os_str()
            .to_os_string(),
    ];
    let cancelled_app = launch_app(&exe_path, &cancelled_args)?;
    cancelled_app.wait_idle(Duration::from_secs(5))?;
    assert_account_verification_menu_state(&cancelled_app, false, false, false, false);

    Ok(())
}

#[test]
fn automation_driver_verification_error_keeps_requested_menu_actions_enabled()
-> Result<(), Box<dyn std::error::Error>> {
    let exe_path = mmatrix_ui_exe_path()
        .map_err(|error| format!("failed to locate mmatrix-ui.exe: {error}"))?;
    let fixture_path = fixture_state_path("verification-error.json");
    let args = [
        OsString::from("--render-state"),
        fixture_path.as_os_str().to_os_string(),
    ];
    let app = launch_app(&exe_path, &args)?;
    app.wait_idle(Duration::from_secs(5))?;

    assert_account_verification_menu_state(&app, true, true, false, false);

    Ok(())
}

#[test]
fn automation_driver_recover_e2ee_before_auth_never_echoes_secret()
-> Result<(), Box<dyn std::error::Error>> {
    let exe_path = mmatrix_ui_exe_path()
        .map_err(|error| format!("failed to locate mmatrix-ui.exe: {error}"))?;
    let args = [OsString::from("--automation")];
    let app = launch_app(&exe_path, &args)?;
    app.wait_for_lifecycle_state("core_started", Duration::from_secs(30))?;

    app.recover_e2ee("sample recovery secret")?;
    app.wait_idle(Duration::from_secs(5))?;
    app.select_status_view()?;
    app.wait_idle(Duration::from_secs(2))?;

    let model = app.dump_model()?;
    assert_eq!(
        model.last_error,
        Some(UiError {
            operation: "recover_e2ee".to_owned(),
            message: "E2EE recovery requires an authenticated session".to_owned(),
        })
    );
    assert!(!model.status.contains("sample recovery secret"));
    assert!(!serde_json::to_string(&model)?.contains("sample recovery secret"));

    let ui_state = app.dump_ui_state()?;
    let status_view = find_view(&ui_state, &ViewIdentity::Status);
    assert!(!view_contains(status_view, "sample recovery secret"));
    assert!(!ui_state.status_bar.text.contains("sample recovery secret"));
    assert_eq!(app.compose_text()?, "");

    Ok(())
}

#[test]
fn automation_driver_setup_e2ee_recovery_before_auth_never_echoes_passphrase()
-> Result<(), Box<dyn std::error::Error>> {
    let exe_path = mmatrix_ui_exe_path()
        .map_err(|error| format!("failed to locate mmatrix-ui.exe: {error}"))?;
    let app = launch_app(&exe_path, &[OsString::from("--automation")])?;
    app.wait_for_lifecycle_state("core_started", Duration::from_secs(30))?;

    app.setup_e2ee_recovery(Some("sample setup passphrase"), true)?;
    app.wait_idle(Duration::from_secs(5))?;
    let model = app.dump_model()?;
    assert_eq!(
        model.last_error,
        Some(UiError {
            operation: "setup_e2ee_recovery".to_owned(),
            message: "E2EE recovery setup requires an authenticated session".to_owned(),
        })
    );
    assert!(!serde_json::to_string(&model)?.contains("sample setup passphrase"));
    assert_eq!(app.compose_text()?, "");
    Ok(())
}

#[test]
fn automation_driver_recover_e2ee_menu_dialog_submits_once_and_never_echoes_secret()
-> Result<(), Box<dyn std::error::Error>> {
    let exe_path = mmatrix_ui_exe_path()
        .map_err(|error| format!("failed to locate mmatrix-ui.exe: {error}"))?;
    let fixture_path = fixture_state_path("e2ee-recovery-needed.json");
    let args = [
        OsString::from("--render-state"),
        fixture_path.as_os_str().to_os_string(),
    ];
    let app = launch_app(&exe_path, &args)?;
    app.wait_idle(Duration::from_secs(5))?;

    let initial_model = app.dump_model()?;
    let secret = "sample recovery secret";
    submit_recover_e2ee_menu_dialog(&app, secret)?;

    let model = app.dump_model()?;
    assert_eq!(model.status, "e2ee recovery submitted");
    assert_eq!(
        model.submitted_recovery_commands,
        vec![SubmittedRecoveryCommand {
            secret_redacted: true,
        }]
    );
    assert_eq!(model.last_error, initial_model.last_error);
    assert!(!serde_json::to_string(&model)?.contains(secret));

    app.select_status_view()?;
    app.wait_idle(Duration::from_secs(2))?;
    let ui_state = app.dump_ui_state()?;
    assert!(!serde_json::to_string(&ui_state)?.contains(secret));
    assert!(!ui_state.status_bar.text.contains(secret));
    assert!(!view_contains(
        find_view(&ui_state, &ViewIdentity::Status),
        secret
    ));
    assert_eq!(app.compose_text()?, "");

    Ok(())
}

#[test]
fn automation_driver_setup_e2ee_recovery_menu_dialog_submits_once_and_redacts_passphrase()
-> Result<(), Box<dyn std::error::Error>> {
    let exe_path = mmatrix_ui_exe_path()
        .map_err(|error| format!("failed to locate mmatrix-ui.exe: {error}"))?;
    let fixture_path = fixture_state_path("e2ee-recovery-needed.json");
    let args = [
        OsString::from("--render-state"),
        fixture_path.as_os_str().to_os_string(),
    ];
    let app = launch_app(&exe_path, &args)?;
    app.wait_idle(Duration::from_secs(5))?;

    let passphrase = "sample setup passphrase";
    submit_setup_e2ee_recovery_menu_dialog(&app, passphrase)?;
    let model = app.dump_model()?;
    assert_eq!(model.status, "e2ee recovery setup submitted");
    assert_eq!(
        model.submitted_setup_recovery_commands,
        vec![SubmittedSetupRecoveryCommand {
            passphrase_redacted: true,
            wait_for_backups_to_upload: false,
        }]
    );
    assert!(!serde_json::to_string(&model)?.contains(passphrase));
    Ok(())
}

#[test]
fn automation_driver_e2ee_recovery_setup_success_handoff_acknowledges_and_never_persists_key()
-> Result<(), Box<dyn std::error::Error>> {
    let exe_path = mmatrix_ui_exe_path()
        .map_err(|error| format!("failed to locate mmatrix-ui.exe: {error}"))?;
    let recovery_key = "automation recovery key must not persist";
    let state_path = write_state_file(&[
        Event::Started,
        Event::E2eeRecoverySetupSucceeded(mmatrix_core::E2eeRecoverySetupSucceeded {
            recovery_key: recovery_key.to_owned(),
        }),
    ])?;
    let args = [
        OsString::from("--render-state"),
        state_path.as_os_str().to_os_string(),
    ];
    let app = launch_app(&exe_path, &args)?;

    acknowledge_recovery_key_handoff(&app)?;

    let model = app.dump_model()?;
    assert_eq!(model.status, "e2ee recovery setup complete");
    assert_eq!(
        model.last_e2ee_recovery_setup_result,
        Some(E2eeRecoverySetupResult {
            recovery_key_present: true,
        })
    );
    assert!(!serde_json::to_string(&model)?.contains(recovery_key));

    app.select_status_view()?;
    app.wait_idle(Duration::from_secs(2))?;
    let ui_state = app.dump_ui_state()?;
    assert!(!view_contains(
        find_view(&ui_state, &ViewIdentity::Status),
        recovery_key
    ));
    assert!(!ui_state.status_bar.text.contains(recovery_key));
    assert!(!serde_json::to_string(&ui_state)?.contains(recovery_key));
    assert_eq!(app.compose_text()?, "");

    let _ = fs::remove_file(&state_path);
    Ok(())
}

#[test]
fn automation_driver_slash_setup_e2ee_recovery_submits_once_without_history()
-> Result<(), Box<dyn std::error::Error>> {
    let exe_path = mmatrix_ui_exe_path()
        .map_err(|error| format!("failed to locate mmatrix-ui.exe: {error}"))?;
    let fixture_path = fixture_state_path("e2ee-recovery-needed.json");
    let args = [
        OsString::from("--render-state"),
        fixture_path.as_os_str().to_os_string(),
    ];
    let app = launch_app(&exe_path, &args)?;
    app.wait_idle(Duration::from_secs(5))?;

    let passphrase = "sample slash setup passphrase";
    app.set_compose_text(&format!("/e2ee setup wait passphrase {passphrase}"))?;
    app.send_compose_key(ComposeKey::Return)?;
    app.wait_idle(Duration::from_secs(5))?;
    assert_eq!(app.compose_text()?, "");
    let model = app.dump_model()?;
    assert_eq!(
        model.submitted_setup_recovery_commands,
        vec![SubmittedSetupRecoveryCommand {
            passphrase_redacted: true,
            wait_for_backups_to_upload: true,
        }]
    );
    app.send_compose_key(ComposeKey::Up)?;
    assert_ne!(
        app.compose_text()?,
        format!("/e2ee setup wait passphrase {passphrase}")
    );
    assert!(!serde_json::to_string(&model)?.contains(passphrase));
    Ok(())
}

#[test]
fn room_pane_renders_header_metadata_and_system_state() -> Result<(), Box<dyn std::error::Error>> {
    let exe_path = mmatrix_ui_exe_path()
        .map_err(|error| format!("failed to locate mmatrix-ui.exe: {error}"))?;
    let fixture_path = fixture_state_path("multi-room.json");

    let args = [
        OsString::from("--render-state"),
        fixture_path.as_os_str().to_os_string(),
    ];
    let app = launch_app(&exe_path, &args)?;
    app.wait_idle(Duration::from_secs(5))?;

    let model = app.dump_model()?;
    assert_eq!(model.selected_room.as_deref(), Some("!ops:example"));
    assert_eq!(
        model.last_activity,
        Some(timeline_line_with_event_id(
            "!random:example",
            "$random-1",
            "@dave:example",
            1_700_000_300_000,
            "random: lunch votes are in",
        ))
    );
    assert_eq!(
        app.room_activity(Some("!ops:example"))?,
        vec![
            topic_entry_with_event_id(
                "!ops:example",
                "$ops-topic-1",
                "@alice:example",
                1_700_000_000_000,
                "incident review",
            ),
            member_entry_with_event_id(
                "!ops:example",
                "$ops-member-1",
                "@carol:example",
                1_700_000_060_000,
                "@carol:example",
                "join",
                Some("carol"),
            ),
            text_entry_with_event_id(
                "!ops:example",
                "$ops-1",
                "@alice:example",
                1_700_000_120_000,
                "ops topic: incident review",
            ),
            text_entry_with_event_id(
                "!ops:example",
                "$ops-2",
                "@bob:example",
                1_700_000_180_000,
                "encryption enabled for ops",
            ),
            text_entry_with_event_id(
                "!ops:example",
                "$ops-3",
                "@carol:example",
                1_700_000_240_000,
                "deploy window opens at 18:00 UTC",
            ),
        ]
    );

    let ui_state = app.dump_ui_state()?;
    assert_eq!(ui_state.mdi.active, room_identity("!ops:example"));
    let ops_view = find_view(&ui_state, &room_identity("!ops:example"));
    assert!(view_contains(ops_view, "Room: Ops [e2ee] (!ops:example)"));
    assert!(view_contains(ops_view, "Topic: incident review"));
    assert!(view_contains(
        ops_view,
        "[22:15] @alice:example: ops topic: incident review"
    ));
    assert!(view_contains(
        ops_view,
        "* [22:13] @alice:example set topic: incident review"
    ));
    assert!(view_contains(ops_view, "* [22:14] carol joined"));

    Ok(())
}

#[test]
fn automation_driver_loads_member_list_fixture() -> Result<(), Box<dyn std::error::Error>> {
    let exe_path = mmatrix_ui_exe_path()
        .map_err(|error| format!("failed to locate mmatrix-ui.exe: {error}"))?;
    let fixture_path = fixture_state_path("member-list.json");

    let args = [
        OsString::from("--render-state"),
        fixture_path.as_os_str().to_os_string(),
    ];
    let app = launch_app(&exe_path, &args)?;
    app.wait_idle(Duration::from_secs(5))?;

    app.select_room("!members:example")?;
    let ui_state = app.dump_ui_state()?;
    let members_view = find_view(&ui_state, &room_identity("!members:example"));
    assert_eq!(
        view_member_labels(members_view),
        vec![
            "Alice",
            "Casey",
            "Sam (@sam-one:example)",
            "Sam (@sam-two:example)",
            "@ghost:example",
            "@blank:example",
            "Zoe",
        ]
    );
    assert!(view_contains(members_view, "member list context line"));

    Ok(())
}

fn launch_member_list_app() -> Result<GuiTestApp, Box<dyn std::error::Error>> {
    let exe_path = mmatrix_ui_exe_path()
        .map_err(|error| format!("failed to locate mmatrix-ui.exe: {error}"))?;
    let fixture_path = fixture_state_path("member-list.json");
    let app = launch_app(
        &exe_path,
        &[
            OsString::from("--render-state"),
            fixture_path.as_os_str().to_os_string(),
        ],
    )?;
    app.wait_idle(Duration::from_secs(5))?;
    app.select_room("!members:example")?;
    Ok(app)
}

#[test]
fn automation_driver_member_menu_state_tracks_selected_member()
-> Result<(), Box<dyn std::error::Error>> {
    let app = launch_member_list_app()?;
    assert_eq!(app.dump_member_menu_state()?.selected_user_id, None);
    assert!(!app.dump_member_menu_state()?.query_enabled);
    app.select_member("@casey:example")?;
    let state = app.dump_member_menu_state()?;
    assert!(state.query_enabled && state.mention_enabled);
    assert_eq!(state.selected_user_id.as_deref(), Some("@casey:example"));
    assert_eq!(state.selected_label.as_deref(), Some("Casey"));
    app.select_member("@sam-two:example")?;
    let state = app.dump_member_menu_state()?;
    assert_eq!(state.selected_user_id.as_deref(), Some("@sam-two:example"));
    assert_eq!(
        state.selected_label.as_deref(),
        Some("Sam (@sam-two:example)")
    );
    Ok(())
}

#[test]
fn automation_driver_member_menu_query_selected_member_records_direct_room()
-> Result<(), Box<dyn std::error::Error>> {
    let app = launch_member_list_app()?;
    app.select_member("@casey:example")?;
    app.query_selected_member_via_menu()?;
    assert_eq!(
        app.dump_model()?.submitted_direct_room_commands,
        vec![SubmittedDirectRoomCommand {
            user_id: "@casey:example".to_owned()
        }]
    );
    assert_eq!(app.compose_text()?, "");
    Ok(())
}

#[test]
fn automation_driver_member_context_menu_query_clicked_member_without_prior_selection()
-> Result<(), Box<dyn std::error::Error>> {
    let app = launch_member_list_app()?;
    app.open_member_context_menu("@alice:example", ContextMenuCommand::QueryMember)?;
    assert_eq!(
        app.dump_model()?.submitted_direct_room_commands,
        vec![SubmittedDirectRoomCommand {
            user_id: "@alice:example".to_owned()
        }]
    );
    assert_eq!(
        app.dump_member_menu_state()?.selected_user_id.as_deref(),
        Some("@alice:example")
    );
    Ok(())
}

#[test]
fn automation_driver_member_menu_mentions_display_name_or_user_id()
-> Result<(), Box<dyn std::error::Error>> {
    let app = launch_member_list_app()?;
    app.select_member("@alice:example")?;
    app.mention_selected_member_via_menu()?;
    assert_eq!(app.compose_text()?, "Alice ");
    app.set_compose_text("")?;
    app.select_member("@ghost:example")?;
    app.mention_selected_member_via_menu()?;
    assert_eq!(app.compose_text()?, "@ghost:example ");
    Ok(())
}

#[test]
fn automation_driver_selected_member_context_menu_uses_keyboard_selection()
-> Result<(), Box<dyn std::error::Error>> {
    let app = launch_member_list_app()?;
    app.select_member("@casey:example")?;
    app.open_selected_member_context_menu(ContextMenuCommand::MentionMember)?;
    assert_eq!(app.compose_text()?, "Casey ");
    Ok(())
}

#[test]
fn automation_driver_loads_member_list_fixture_in_narrow_frame()
-> Result<(), Box<dyn std::error::Error>> {
    let exe_path = mmatrix_ui_exe_path()
        .map_err(|error| format!("failed to locate mmatrix-ui.exe: {error}"))?;
    let fixture_path = fixture_state_path("member-list-narrow.json");
    assert_eq!(
        fixture_path.file_name().and_then(|name| name.to_str()),
        Some("member-list-narrow.json")
    );

    let frame_size = FrameSize {
        width: 640,
        height: 768,
    };
    let options = LaunchOptions {
        frame_size: Some(frame_size),
        ..launch_options(None)
    };
    assert_eq!(
        options.frame_size,
        Some(FrameSize {
            width: 640,
            height: 768,
        })
    );

    let args = [
        OsString::from("--render-state"),
        fixture_path.as_os_str().to_os_string(),
    ];
    let app = launch_app_with_options(&exe_path, &args, options)?;
    app.wait_idle(Duration::from_secs(5))?;
    let frame = app.automation_frame_hwnd() as HWND;
    let rect = unsafe { window_rect(frame) };
    assert_eq!(rect.right - rect.left, 640);
    assert_eq!(rect.bottom - rect.top, 768);

    app.select_room("!members:example")?;
    let ui_state = app.dump_ui_state()?;
    assert_eq!(ui_state.mdi.active, room_identity("!members:example"));
    assert_eq!(
        app.dump_model()?.selected_room.as_deref(),
        Some("!members:example")
    );
    assert_eq!(
        find_ui_room(&ui_state, "!members:example").room_id,
        "!members:example"
    );

    let members_view = find_view(&ui_state, &room_identity("!members:example"));
    assert_eq!(
        view_member_labels(members_view),
        vec![
            "Alice",
            "Casey",
            "Sam (@sam-one:example)",
            "Sam (@sam-two:example)",
            "@ghost:example",
            "@blank:example",
            "Zoe",
        ]
    );
    assert!(view_contains(members_view, "member list context line"));

    Ok(())
}

#[test]
fn automation_driver_replaces_room_members_on_append_and_state_reload()
-> Result<(), Box<dyn std::error::Error>> {
    let exe_path = mmatrix_ui_exe_path()
        .map_err(|error| format!("failed to locate mmatrix-ui.exe: {error}"))?;
    let state_path = write_state_file(&[
        Event::Started,
        Event::SyncedRooms(vec![room_summary("!members:example", "Members", false)]),
        text_event_with_event_id(
            "!members:example",
            "$members-context",
            "@alice:example",
            1_700_000_000_000,
            "room members context",
        ),
        room_members_event(
            "!members:example",
            &[
                ("@alice:example", Some("Alice")),
                ("@bob:example", Some("Bob")),
            ],
        ),
    ])?;

    let args = [
        OsString::from("--render-state"),
        state_path.as_os_str().to_os_string(),
    ];
    let app = launch_app(&exe_path, &args)?;
    app.wait_idle(Duration::from_secs(5))?;

    app.select_room("!members:example")?;
    let initial_state = app.dump_ui_state()?;
    let initial_view = find_view(&initial_state, &room_identity("!members:example"));
    assert_eq!(view_member_labels(initial_view), vec!["Alice", "Bob"]);

    app.append_synthetic_events(&serde_json::to_string(&vec![room_members_event(
        "!members:example",
        &[
            ("@alice:example", Some("Alice")),
            ("@carol:example", Some("Carol")),
        ],
    )])?)?;
    app.wait_idle(Duration::from_secs(5))?;

    let appended_state = app.dump_ui_state()?;
    let appended_view = find_view(&appended_state, &room_identity("!members:example"));
    assert_eq!(view_member_labels(appended_view), vec!["Alice", "Carol"]);
    assert!(!view_members_contain(appended_view, "Bob"));
    assert!(view_members_contain(appended_view, "Carol"));

    app.load_events(&serde_json::to_string(&vec![
        Event::Started,
        Event::SyncedRooms(vec![room_summary("!members:example", "Members", false)]),
        text_event_with_event_id(
            "!members:example",
            "$members-context",
            "@alice:example",
            1_700_000_000_000,
            "room members context",
        ),
        room_members_event(
            "!members:example",
            &[
                ("@alice:example", Some("Alice")),
                ("@dave:example", Some("Dave")),
            ],
        ),
    ])?)?;
    app.wait_idle(Duration::from_secs(5))?;
    app.select_room("!members:example")?;

    let reloaded_state = app.dump_ui_state()?;
    let reloaded_view = find_view(&reloaded_state, &room_identity("!members:example"));
    assert_eq!(view_member_labels(reloaded_view), vec!["Alice", "Dave"]);
    assert!(!view_members_contain(reloaded_view, "Carol"));
    assert!(view_members_contain(reloaded_view, "Dave"));

    let _ = fs::remove_file(&state_path);
    Ok(())
}

#[test]
fn custom_room_timeline_scrollbar_reports_visible_range_and_pages()
-> Result<(), Box<dyn std::error::Error>> {
    let exe_path = mmatrix_ui_exe_path()
        .map_err(|error| format!("failed to locate mmatrix-ui.exe: {error}"))?;
    let local_app_data = unique_local_app_data_path();
    let options = launch_options(Some(local_app_data.clone()));
    let args = [OsString::from("--automation")];

    let app = launch_app_with_options(&exe_path, &args, options)?;
    app.wait_for_lifecycle_state("core_started", Duration::from_secs(30))?;
    let mut events = base_events();
    for index in 0..80 {
        let body = format!("scroll row {index:02}");
        events.push(text_event(
            "!beta:example",
            "@bob:example",
            1_700_001_000_000 + index * 60_000,
            &body,
        ));
    }
    app.load_events(&serde_json::to_string(&events)?)?;
    app.wait_idle(Duration::from_secs(5))?;
    app.select_room("!beta:example")?;
    app.wait_for_room_activity_contains(
        Some("!beta:example"),
        "scroll row 79",
        Duration::from_secs(5),
    )?;

    let content_hwnd = unsafe { active_mdi_content_hwnd(&app) };
    assert_eq!(
        unsafe { class_name(content_hwnd) },
        "mmatrixWin32RoomContent"
    );
    let timeline_hwnd = unsafe { active_room_timeline_hwnd(&app) };
    let member_list_hwnd = unsafe { active_room_member_list_hwnd(&app) };
    assert_eq!(unsafe { class_name(timeline_hwnd) }, "mmatrixWin32Timeline");
    assert_eq!(unsafe { class_name(member_list_hwnd) }, "ListBox");
    let timeline_style = unsafe { window_style(timeline_hwnd) };
    let timeline_rect = unsafe { client_rect(timeline_hwnd) };
    assert!(
        timeline_style & WS_VSCROLL != 0,
        "timeline child must advertise WS_VSCROLL"
    );
    assert!(
        unsafe { read_window_text(timeline_hwnd) }.contains("Room: Beta [e2ee] (!beta:example)"),
        "timeline window text must expose the rendered room header via WM_GETTEXT"
    );
    assert!(
        unsafe { read_window_text(timeline_hwnd) }.contains("scroll row 79"),
        "timeline window text must contain the final rendered row via WM_GETTEXT"
    );

    let before = unsafe { scrollbar_info(timeline_hwnd) };
    let before_top_line = unsafe { timeline_top_line(timeline_hwnd) };
    if let Some(before) = before {
        assert!(
            before.nMax > 0,
            "vertical scrollbar must report a non-zero range: style=0x{:x} rect={}x{} nMin={} nMax={} nPage={} nPos={}",
            timeline_style,
            timeline_rect.right - timeline_rect.left,
            timeline_rect.bottom - timeline_rect.top,
            before.nMin,
            before.nMax,
            before.nPage,
            before.nPos
        );
        assert!(
            before.nPage > 0,
            "vertical scrollbar must report a visible page size"
        );
        assert_eq!(
            before.nPos,
            before.nMax - before.nPage as i32 + 1,
            "timeline should start at the bottom"
        );
    } else {
        // Wine can refuse cross-process GetScrollInfo on a custom WS_VSCROLL child even when
        // the in-process control state is valid. In that case verify size/style plus observable
        // page-down behavior using the control's internal top-line query.
        assert!(
            timeline_rect.right - timeline_rect.left > 0
                && timeline_rect.bottom - timeline_rect.top > 0,
            "timeline child must have a visible client rect"
        );
        assert!(before_top_line > 0, "timeline should start below the top");
    }

    app.append_synthetic_events(&serde_json::to_string(&vec![text_event(
        "!beta:example",
        "@bob:example",
        1_700_005_800_000,
        "scroll row 80",
    )])?)?;
    app.wait_for_room_activity_contains(
        Some("!beta:example"),
        "scroll row 80",
        Duration::from_secs(5),
    )?;
    let after_tail_append = unsafe { scrollbar_info(timeline_hwnd) };
    let after_tail_append_top_line = unsafe { timeline_top_line(timeline_hwnd) };
    if let Some(after_tail_append) = after_tail_append {
        assert_eq!(
            after_tail_append.nPos,
            after_tail_append.nMax - after_tail_append.nPage as i32 + 1,
            "appending at the tail must keep the timeline at the new bottom"
        );
    } else {
        assert!(
            after_tail_append_top_line > before_top_line,
            "appending at the tail must advance the timeline bottom"
        );
    }

    unsafe {
        SendMessageW(timeline_hwnd, WM_VSCROLL, SB_PAGEUP as WPARAM, 0);
    }

    let after_top_line = unsafe { timeline_top_line(timeline_hwnd) };
    let after_page_up = unsafe { scrollbar_info(timeline_hwnd) };
    if let Some(before) = before {
        let after = unsafe { scrollbar_info(timeline_hwnd) }
            .expect("GetScrollInfo should keep succeeding once it succeeds for the timeline child");
        assert!(
            after.nPos < before.nPos,
            "page-up must move scrollbar away from the tail: before={}, after={}",
            before.nPos,
            after.nPos
        );
    } else {
        assert!(
            after_top_line < before_top_line,
            "page-up must move the timeline away from the tail: before={}, after={}",
            before_top_line,
            after_top_line
        );
    }

    app.append_synthetic_events(&serde_json::to_string(&vec![text_event(
        "!beta:example",
        "@bob:example",
        1_700_005_860_000,
        "scroll row 81",
    )])?)?;
    app.wait_for_room_activity_contains(
        Some("!beta:example"),
        "scroll row 81",
        Duration::from_secs(5),
    )?;
    let after_scrolled_append = unsafe { scrollbar_info(timeline_hwnd) };
    let after_scrolled_append_top_line = unsafe { timeline_top_line(timeline_hwnd) };
    if let (Some(after_page_up), Some(after_scrolled_append)) =
        (after_page_up, after_scrolled_append)
    {
        assert_eq!(
            after_scrolled_append.nPos, after_page_up.nPos,
            "appending away from the tail must retain the current position"
        );
        assert!(
            after_scrolled_append.nPos
                < after_scrolled_append.nMax - after_scrolled_append.nPage as i32 + 1,
            "appending away from the tail must not jump to the bottom"
        );
    } else {
        assert_eq!(
            after_scrolled_append_top_line, after_top_line,
            "appending away from the tail must retain the current top line"
        );
    }

    let ui_state = app.dump_ui_state()?;
    assert_eq!(ui_state.mdi.active, room_identity("!beta:example"));
    assert_active_switchbar_item(&ui_state, &room_identity("!beta:example"));

    let _ = fs::remove_dir_all(&local_app_data);
    Ok(())
}

#[test]
fn custom_room_timeline_history_backfill_submits_once_while_loading()
-> Result<(), Box<dyn std::error::Error>> {
    let exe_path = mmatrix_ui_exe_path()
        .map_err(|error| format!("failed to locate mmatrix-ui.exe: {error}"))?;
    let local_app_data = unique_local_app_data_path();
    let server = spawn_mock_backfill_homeserver()?;
    let options = launch_options(Some(local_app_data.clone()));
    let args = [OsString::from("--automation")];

    let app = launch_app_with_options(&exe_path, &args, options)?;
    app.wait_for_lifecycle_state("core_started", Duration::from_secs(30))?;
    app.restore_session(&server.base_url, stored_session())?;
    app.wait_for_lifecycle_state("ready", Duration::from_secs(30))?;
    let mut events = base_events();
    for index in 0..80 {
        let body = format!("history row {index:02}");
        events.push(text_event(
            "!beta:example",
            "@bob:example",
            1_700_001_000_000 + index * 60_000,
            &body,
        ));
    }
    app.load_events(&serde_json::to_string(&events)?)?;
    app.wait_idle(Duration::from_secs(5))?;
    app.select_room("!beta:example")?;
    app.wait_for_room_activity_contains(
        Some("!beta:example"),
        "history row 79",
        Duration::from_secs(5),
    )?;

    let result = app.find_active_view("absent needle")?;
    assert_eq!(result.history_search, UiFindHistorySearchState::Loading);
    let model = app.dump_model()?;
    assert_eq!(model.submitted_history_commands.len(), 1);
    assert_eq!(
        model.submitted_history_commands.first(),
        Some(&SubmittedHistoryCommand {
            room_id: "!beta:example".to_owned(),
            from: None,
            limit: 50,
        })
    );
    let ui_state = app.dump_ui_state()?;
    let (room, view) = find_room_state_and_view(&ui_state, "!beta:example");
    assert_eq!(
        room.history_backfill,
        UiRoomHistoryBackfillState::Loading { from: None }
    );
    assert!(view_contains(view, "* Loading older history..."));

    app.trigger_active_timeline_history_backfill()?;
    assert_eq!(app.dump_model()?.submitted_history_commands.len(), 1);

    drop(app);
    let _ = fs::remove_dir_all(&local_app_data);
    Ok(())
}

#[test]
fn render_state_room_find_backfill_records_loading_and_matches_older_history()
-> Result<(), Box<dyn std::error::Error>> {
    let exe_path = mmatrix_ui_exe_path()
        .map_err(|error| format!("failed to locate mmatrix-ui.exe: {error}"))?;
    let fixture_path = fixture_state_path("history-backfill-loading.json");
    let args = [
        OsString::from("--render-state"),
        fixture_path.as_os_str().to_os_string(),
    ];

    let app = launch_app(&exe_path, &args)?;
    app.wait_idle(Duration::from_secs(5))?;
    app.select_room("!history:example")?;
    app.wait_for_room_activity_contains(
        Some("!history:example"),
        "scroll filler line 30",
        Duration::from_secs(5),
    )?;

    let result = app.find_active_view("older needle")?;
    assert_eq!(result.history_search, UiFindHistorySearchState::Loading);
    assert!(!result.matched);
    let model = app.dump_model()?;
    assert_eq!(model.submitted_history_commands.len(), 1);
    assert_eq!(
        model.submitted_history_commands.first(),
        Some(&SubmittedHistoryCommand {
            room_id: "!history:example".to_owned(),
            from: None,
            limit: 50,
        })
    );

    let ui_state = app.dump_ui_state()?;
    let (room, view) = find_room_state_and_view(&ui_state, "!history:example");
    assert_eq!(
        room.history_backfill,
        UiRoomHistoryBackfillState::Loading { from: None }
    );
    assert!(view_contains(view, "* Loading older history..."));
    assert_eq!(
        ui_state
            .active_find
            .as_ref()
            .map(|find| &find.history_search),
        Some(&UiFindHistorySearchState::Loading)
    );
    assert!(
        ui_state
            .status_bar
            .text
            .contains("find: loading older history for \"older needle\"")
    );

    app.append_synthetic_events(&serde_json::to_string(&vec![room_history_backfill_event(
        "!history:example",
        "start",
        Some("next"),
        vec![history_backfill_event_from_event(text_event_with_event_id(
            "!history:example",
            "$older-needle",
            "@alice:example",
            1_699_999_000_000,
            "older needle",
        ))],
    )])?)?;
    app.wait_idle(Duration::from_secs(5))?;

    let ui_state = app.dump_ui_state()?;
    let selected_text = only_view_line_containing(
        find_view(&ui_state, &room_identity("!history:example")),
        "older needle",
    );
    assert_ui_active_find(
        &ui_state,
        &room_identity("!history:example"),
        "older needle",
        true,
        1,
        Some(0),
        Some(selected_text),
        UiFindHistorySearchState::LocalOnly,
    );
    assert!(
        ui_state
            .status_bar
            .text
            .contains("find: 1/1 \"older needle\"")
    );
    Ok(())
}

#[test]
fn custom_room_timeline_history_backfill_failure_is_visible_and_retryable()
-> Result<(), Box<dyn std::error::Error>> {
    let exe_path = mmatrix_ui_exe_path()
        .map_err(|error| format!("failed to locate mmatrix-ui.exe: {error}"))?;
    let local_app_data = unique_local_app_data_path();
    let server = spawn_mock_backfill_homeserver()?;
    let options = launch_options(Some(local_app_data.clone()));
    let args = [OsString::from("--automation")];

    let app = launch_app_with_options(&exe_path, &args, options)?;
    app.wait_for_lifecycle_state("core_started", Duration::from_secs(30))?;
    app.restore_session(&server.base_url, stored_session())?;
    app.wait_for_lifecycle_state("ready", Duration::from_secs(30))?;
    let mut events = base_events();
    for index in 0..80 {
        let body = format!("history row {index:02}");
        events.push(text_event(
            "!beta:example",
            "@bob:example",
            1_700_001_000_000 + index * 60_000,
            &body,
        ));
    }
    app.load_events(&serde_json::to_string(&events)?)?;
    app.wait_idle(Duration::from_secs(5))?;
    app.select_room("!beta:example")?;
    app.wait_for_room_activity_contains(
        Some("!beta:example"),
        "history row 79",
        Duration::from_secs(5),
    )?;

    let result = app.find_active_view("absent needle")?;
    assert_eq!(result.history_search, UiFindHistorySearchState::Loading);
    let model = app.dump_model()?;
    assert_eq!(model.submitted_history_commands.len(), 1);
    assert_eq!(
        model.submitted_history_commands.last(),
        Some(&SubmittedHistoryCommand {
            room_id: "!beta:example".to_owned(),
            from: None,
            limit: 50,
        })
    );
    app.append_synthetic_events(&serde_json::to_string(&vec![room_history_backfill_event(
        "!beta:example",
        "s-current",
        Some("token-next"),
        vec![history_backfill_event_from_event(text_event_with_event_id(
            "!beta:example",
            "$older-seed",
            "@bob:example",
            1_699_999_880_000,
            "older seed row",
        ))],
    )])?)?;
    app.wait_idle(Duration::from_secs(5))?;

    app.trigger_active_timeline_history_backfill()?;
    let model = app.dump_model()?;
    assert_eq!(model.submitted_history_commands.len(), 2);
    assert_eq!(
        model.submitted_history_commands.last(),
        Some(&SubmittedHistoryCommand {
            room_id: "!beta:example".to_owned(),
            from: Some("token-next".to_owned()),
            limit: 50,
        })
    );
    app.append_synthetic_events(&serde_json::to_string(&vec![Event::Error(
        mmatrix_core::CoreErrorEvent {
            operation: mmatrix_core::Operation::BackfillRoomHistory,
            message: "homeserver unavailable".to_owned(),
        },
    )])?)?;
    app.wait_for_lifecycle_state("error", Duration::from_secs(30))?;
    app.wait_idle(Duration::from_secs(5))?;

    let ui_state = app.dump_ui_state()?;
    let (room, view) = find_room_state_and_view(&ui_state, "!beta:example");
    assert_eq!(
        room.history_backfill,
        UiRoomHistoryBackfillState::Failed {
            from: Some("token-next".to_owned()),
            message: "homeserver unavailable".to_owned(),
        }
    );
    assert!(view_contains(
        view,
        "* Unable to load older history: homeserver unavailable. Scroll to top to retry."
    ));
    assert_eq!(
        ui_state
            .active_find
            .as_ref()
            .map(|find| &find.history_search),
        Some(&UiFindHistorySearchState::Failed {
            message: "homeserver unavailable".to_owned(),
        })
    );
    assert!(ui_state.status_bar.text.contains(
        "find: unable to load older history for \"absent needle\": homeserver unavailable"
    ));
    assert_eq!(
        app.dump_model()?.last_error,
        Some(UiError {
            operation: "backfill_room_history".to_owned(),
            message: "homeserver unavailable".to_owned(),
        })
    );

    let result = app.find_active_view("absent needle")?;
    assert_eq!(result.history_search, UiFindHistorySearchState::Loading);
    let model = app.dump_model()?;
    assert_eq!(model.submitted_history_commands.len(), 3);
    assert_eq!(
        model.submitted_history_commands.last(),
        Some(&SubmittedHistoryCommand {
            room_id: "!beta:example".to_owned(),
            from: Some("token-next".to_owned()),
            limit: 50,
        })
    );
    let ui_state = app.dump_ui_state()?;
    let (room, _) = find_room_state_and_view(&ui_state, "!beta:example");
    assert_eq!(
        room.history_backfill,
        UiRoomHistoryBackfillState::Loading {
            from: Some("token-next".to_owned())
        }
    );

    drop(app);
    let _ = fs::remove_dir_all(&local_app_data);
    Ok(())
}

#[test]
fn custom_room_timeline_history_backfill_exhausted_is_visible_and_blocks_retry()
-> Result<(), Box<dyn std::error::Error>> {
    let exe_path = mmatrix_ui_exe_path()
        .map_err(|error| format!("failed to locate mmatrix-ui.exe: {error}"))?;
    let local_app_data = unique_local_app_data_path();
    let server = spawn_mock_backfill_homeserver()?;
    let options = launch_options(Some(local_app_data.clone()));
    let args = [OsString::from("--automation")];

    let app = launch_app_with_options(&exe_path, &args, options)?;
    app.wait_for_lifecycle_state("core_started", Duration::from_secs(30))?;
    app.restore_session(&server.base_url, stored_session())?;
    app.wait_for_lifecycle_state("ready", Duration::from_secs(30))?;
    let mut events = base_events();
    for index in 0..80 {
        let body = format!("history row {index:02}");
        events.push(text_event(
            "!beta:example",
            "@bob:example",
            1_700_001_000_000 + index * 60_000,
            &body,
        ));
    }
    app.load_events(&serde_json::to_string(&events)?)?;
    app.wait_idle(Duration::from_secs(5))?;
    app.select_room("!beta:example")?;
    app.wait_for_room_activity_contains(
        Some("!beta:example"),
        "history row 79",
        Duration::from_secs(5),
    )?;

    let result = app.find_active_view("absent needle")?;
    assert_eq!(result.history_search, UiFindHistorySearchState::Loading);
    let model = app.dump_model()?;
    assert_eq!(model.submitted_history_commands.len(), 1);
    app.append_synthetic_events(&serde_json::to_string(&vec![room_history_backfill_event(
        "!beta:example",
        "s-current",
        None,
        Vec::new(),
    )])?)?;
    app.wait_idle(Duration::from_secs(5))?;

    let ui_state = app.dump_ui_state()?;
    let (room, view) = find_room_state_and_view(&ui_state, "!beta:example");
    assert_eq!(room.history_backfill, UiRoomHistoryBackfillState::Exhausted);
    assert!(view_contains(view, "* No older history available."));
    assert_eq!(
        ui_state
            .active_find
            .as_ref()
            .map(|find| &find.history_search),
        Some(&UiFindHistorySearchState::Exhausted)
    );
    assert!(
        ui_state
            .status_bar
            .text
            .contains("find: no matches for \"absent needle\" in loaded or older history")
    );

    app.trigger_active_timeline_history_backfill()?;
    assert_eq!(app.dump_model()?.submitted_history_commands.len(), 1);

    drop(app);
    let _ = fs::remove_dir_all(&local_app_data);
    Ok(())
}

#[test]
fn trigger_active_timeline_history_backfill_submits_for_thread_parent_room()
-> Result<(), Box<dyn std::error::Error>> {
    let exe_path = mmatrix_ui_exe_path()
        .map_err(|error| format!("failed to locate mmatrix-ui.exe: {error}"))?;
    let fixture_path = fixture_state_path("message-edits.json");
    let local_app_data = unique_local_app_data_path();
    let options = launch_options(Some(local_app_data.clone()));
    let args = [OsString::from("--automation")];

    let app = launch_app_with_options(&exe_path, &args, options)?;
    app.wait_for_lifecycle_state("core_started", Duration::from_secs(30))?;
    app.load_events(&fs::read_to_string(&fixture_path)?)?;
    app.wait_idle(Duration::from_secs(5))?;
    app.select_view(thread_identity("!ops:example", "$thread-root"))?;

    app.trigger_active_timeline_history_backfill()?;
    assert_eq!(
        app.dump_model()?.submitted_history_commands,
        vec![SubmittedHistoryCommand {
            room_id: "!ops:example".to_owned(),
            from: None,
            limit: 50,
        }]
    );

    drop(app);
    let _ = fs::remove_dir_all(&local_app_data);
    Ok(())
}

#[test]
fn render_state_thread_find_backfill_matches_older_thread_reply()
-> Result<(), Box<dyn std::error::Error>> {
    let exe_path = mmatrix_ui_exe_path()
        .map_err(|error| format!("failed to locate mmatrix-ui.exe: {error}"))?;
    let fixture_path = fixture_state_path("find-thread.json");
    let args = [
        OsString::from("--render-state"),
        fixture_path.as_os_str().to_os_string(),
    ];
    let app = launch_app(&exe_path, &args)?;
    app.wait_idle(Duration::from_secs(5))?;
    let view_id = thread_identity("!find-thread:example", "$find-thread-root");
    app.select_view(view_id.clone())?;

    let result = app.find_active_view("older thread needle")?;
    assert_eq!(result.history_search, UiFindHistorySearchState::Loading);
    assert_eq!(app.dump_model()?.submitted_history_commands.len(), 1);

    app.append_synthetic_events(&serde_json::to_string(&vec![room_history_backfill_event(
        "!find-thread:example",
        "start",
        Some("next"),
        vec![
            history_backfill_event_from_event(message_event_with_event_id_and_relation(
                "!find-thread:example",
                "$find-thread-older",
                "@erin:example",
                1_699_999_000_000,
                Some(CoreRoomActivityRelation::Thread {
                    thread_root_event_id: "$find-thread-root".to_owned(),
                    in_reply_to_event_id: Some("$find-thread-root".to_owned()),
                    is_falling_back: false,
                }),
                CoreRoomMessagePresentationKind::Text,
                "older thread needle",
            )),
            history_backfill_event_from_event(text_event_with_event_id(
                "!find-thread:example",
                "$find-thread-older-room-only",
                "@frank:example",
                1_699_999_060_000,
                "older room-only needle",
            )),
        ],
    )])?)?;
    app.wait_idle(Duration::from_secs(5))?;

    let ui_state = app.dump_ui_state()?;
    let thread_view = find_view(&ui_state, &view_id);
    let selected_text = only_view_line_containing(thread_view, "older thread needle");
    assert_ui_active_find(
        &ui_state,
        &view_id,
        "older thread needle",
        true,
        1,
        Some(0),
        Some(selected_text),
        UiFindHistorySearchState::LocalOnly,
    );
    assert!(view_contains(thread_view, "older thread needle"));
    assert!(!view_contains(thread_view, "older room-only needle"));
    Ok(())
}

#[test]
fn automation_driver_history_backfill_prepends_older_messages_without_disturbing_active_room()
-> Result<(), Box<dyn std::error::Error>> {
    let exe_path = mmatrix_ui_exe_path()
        .map_err(|error| format!("failed to locate mmatrix-ui.exe: {error}"))?;
    let local_app_data = unique_local_app_data_path();
    let server = spawn_mock_backfill_homeserver()?;
    let options = launch_options(Some(local_app_data.clone()));
    let args = [OsString::from("--automation")];

    let app = launch_app_with_options(&exe_path, &args, options)?;
    app.wait_for_lifecycle_state("core_started", Duration::from_secs(30))?;
    app.restore_session(&server.base_url, stored_session())?;
    app.wait_for_lifecycle_state("ready", Duration::from_secs(30))?;
    let mut events = vec![
        Event::Started,
        Event::SyncedRooms(vec![room_summary("!beta:example", "Beta", false)]),
        text_event_with_event_id(
            "!beta:example",
            "$current-1",
            "@bob:example",
            1_700_000_060_000,
            "current row 1",
        ),
        text_event_with_event_id(
            "!beta:example",
            "$current-2",
            "@bob:example",
            1_700_000_120_000,
            "current row 2",
        ),
    ];
    for index in 0..80 {
        let event_id = format!("$current-extra-{index:02}");
        let body = format!("current extra row {index:02}");
        events.push(text_event_with_event_id(
            "!beta:example",
            &event_id,
            "@bob:example",
            1_700_000_180_000 + index * 60_000,
            &body,
        ));
    }
    app.load_events(&serde_json::to_string(&events)?)?;
    app.wait_idle(Duration::from_secs(5))?;
    app.select_room("!beta:example")?;
    app.wait_for_room_activity_contains(
        Some("!beta:example"),
        "current extra row 79",
        Duration::from_secs(5),
    )?;

    let expected_path = nav_path(&["mmatrix", "rooms: 1", "Beta"]);
    assert_eq!(
        app.dump_model()?.selected_room.as_deref(),
        Some("!beta:example")
    );
    assert_eq!(app.selected_navigation_path()?, expected_path);
    assert_compose_has_focus(&app);
    let timeline_hwnd = unsafe { active_room_timeline_hwnd(&app) };
    unsafe {
        SendMessageW(timeline_hwnd, WM_VSCROLL, SB_TOP as WPARAM, 0);
    }
    assert_eq!(
        unsafe { timeline_top_line(timeline_hwnd) },
        0,
        "backfill must start from the oldest visible current row"
    );
    app.trigger_active_timeline_history_backfill()?;

    app.append_synthetic_events(&serde_json::to_string(&vec![room_history_backfill_event(
        "!beta:example",
        "s-current",
        Some("token-next"),
        vec![
            history_backfill_event_from_event(text_event_with_event_id(
                "!beta:example",
                "$older-1",
                "@bob:example",
                1_699_999_940_000,
                "older row 1",
            )),
            history_backfill_event_from_event(text_event_with_event_id(
                "!beta:example",
                "$older-2",
                "@bob:example",
                1_700_000_000_000,
                "older row 2",
            )),
            history_backfill_event_from_event(text_event_with_event_id(
                "!beta:example",
                "$current-1",
                "@bob:example",
                1_700_000_060_000,
                "duplicate current row 1",
            )),
        ],
    )])?)?;
    app.wait_idle(Duration::from_secs(5))?;

    assert!(
        unsafe { timeline_top_line(timeline_hwnd) } > 0,
        "backfill must preserve the current row that was visible before prepending older rows"
    );

    let mut expected_activity = vec![
        text_entry_with_event_id(
            "!beta:example",
            "$older-1",
            "@bob:example",
            1_699_999_940_000,
            "older row 1",
        ),
        text_entry_with_event_id(
            "!beta:example",
            "$older-2",
            "@bob:example",
            1_700_000_000_000,
            "older row 2",
        ),
        text_entry_with_event_id(
            "!beta:example",
            "$current-1",
            "@bob:example",
            1_700_000_060_000,
            "current row 1",
        ),
        text_entry_with_event_id(
            "!beta:example",
            "$current-2",
            "@bob:example",
            1_700_000_120_000,
            "current row 2",
        ),
    ];
    for index in 0..80 {
        let event_id = format!("$current-extra-{index:02}");
        let body = format!("current extra row {index:02}");
        expected_activity.push(text_entry_with_event_id(
            "!beta:example",
            &event_id,
            "@bob:example",
            1_700_000_180_000 + index * 60_000,
            &body,
        ));
    }
    assert_eq!(app.room_activity(Some("!beta:example"))?, expected_activity);

    let ui_state = app.dump_ui_state()?;
    assert_eq!(
        app.dump_model()?.selected_room.as_deref(),
        Some("!beta:example")
    );
    assert_eq!(ui_state.mdi.active, room_identity("!beta:example"));
    assert_active_switchbar_item(&ui_state, &room_identity("!beta:example"));
    assert_eq!(app.selected_navigation_path()?, expected_path);
    assert_compose_has_focus(&app);
    assert_switchbar_activity(&ui_state, &room_identity("!beta:example"), false);
    assert_navigation_activity(&ui_state, &["mmatrix", "rooms: 1", "Beta"], false);
    assert!(!find_ui_room(&ui_state, "!beta:example").has_activity);
    assert_eq!(
        find_ui_room(&ui_state, "!beta:example").read_target_event_id,
        None
    );
    assert_eq!(
        find_ui_room(&ui_state, "!beta:example").history_backfill,
        UiRoomHistoryBackfillState::Idle
    );

    drop(app);
    let _ = fs::remove_dir_all(&local_app_data);
    Ok(())
}

#[test]
fn custom_room_timeline_gettext_normalizes_multiline_bodies()
-> Result<(), Box<dyn std::error::Error>> {
    let exe_path = mmatrix_ui_exe_path()
        .map_err(|error| format!("failed to locate mmatrix-ui.exe: {error}"))?;
    let local_app_data = unique_local_app_data_path();
    let options = launch_options(Some(local_app_data.clone()));
    let args = [OsString::from("--automation")];

    let app = launch_app_with_options(&exe_path, &args, options)?;
    app.wait_for_lifecycle_state("core_started", Duration::from_secs(30))?;
    let mut events = base_events();
    events.push(text_event(
        "!beta:example",
        "@bob:example",
        1_700_001_000_000,
        "multiline row 00 line a\nmultiline row 00 line b",
    ));
    app.load_events(&serde_json::to_string(&events)?)?;
    app.wait_idle(Duration::from_secs(5))?;
    app.select_room("!beta:example")?;
    app.wait_for_room_activity_contains(
        Some("!beta:example"),
        "multiline row 00 line a",
        Duration::from_secs(5),
    )?;

    let timeline_hwnd = unsafe { active_room_timeline_hwnd(&app) };
    let text = unsafe { read_window_text(timeline_hwnd) };
    assert!(
        text.contains("multiline row 00 line a\r\nmultiline row 00 line b"),
        "timeline WM_GETTEXT must normalize embedded multiline bodies to CRLF: {text:?}"
    );

    app.select_status_view()?;
    app.wait_idle(Duration::from_secs(2))?;
    app.select_room("!beta:example")?;
    app.wait_idle(Duration::from_secs(2))?;

    let text_after_switch = unsafe { read_window_text(active_room_timeline_hwnd(&app)) };
    assert!(
        text_after_switch.contains("multiline row 00 line a\r\nmultiline row 00 line b"),
        "timeline WM_GETTEXT must preserve CRLF normalization after view switches: {text_after_switch:?}"
    );

    let _ = fs::remove_dir_all(&local_app_data);
    Ok(())
}

#[test]
fn custom_room_timeline_gettext_preserves_trailing_blank_line()
-> Result<(), Box<dyn std::error::Error>> {
    let exe_path = mmatrix_ui_exe_path()
        .map_err(|error| format!("failed to locate mmatrix-ui.exe: {error}"))?;
    let local_app_data = unique_local_app_data_path();
    let options = launch_options(Some(local_app_data.clone()));
    let args = [OsString::from("--automation")];

    let app = launch_app_with_options(&exe_path, &args, options)?;
    app.wait_for_lifecycle_state("core_started", Duration::from_secs(30))?;
    let mut events = base_events();
    events.push(text_event(
        "!beta:example",
        "@bob:example",
        1_700_001_000_000,
        "trailing blank line a\n",
    ));
    app.load_events(&serde_json::to_string(&events)?)?;
    app.wait_idle(Duration::from_secs(5))?;
    app.select_room("!beta:example")?;
    app.wait_for_room_activity_contains(
        Some("!beta:example"),
        "trailing blank line a",
        Duration::from_secs(5),
    )?;

    let timeline_hwnd = unsafe { active_room_timeline_hwnd(&app) };
    let text = unsafe { read_window_text(timeline_hwnd) };
    assert!(
        text.contains("trailing blank line a\r\n"),
        "timeline WM_GETTEXT must preserve a trailing blank line as CRLF: {text:?}"
    );

    app.select_status_view()?;
    app.wait_idle(Duration::from_secs(2))?;
    app.select_room("!beta:example")?;
    app.wait_idle(Duration::from_secs(2))?;

    let text_after_switch = unsafe { read_window_text(active_room_timeline_hwnd(&app)) };
    assert!(
        text_after_switch.contains("trailing blank line a\r\n"),
        "timeline WM_GETTEXT must keep trailing blank lines after view switches: {text_after_switch:?}"
    );

    let _ = fs::remove_dir_all(&local_app_data);
    Ok(())
}

#[test]
fn custom_room_timeline_wrapped_rows_preserve_logical_gettext_and_scroll()
-> Result<(), Box<dyn std::error::Error>> {
    let exe_path = mmatrix_ui_exe_path()
        .map_err(|error| format!("failed to locate mmatrix-ui.exe: {error}"))?;
    let local_app_data = unique_local_app_data_path();
    let options = launch_options(Some(local_app_data.clone()));
    let args = [OsString::from("--automation")];

    let app = launch_app_with_options(&exe_path, &args, options)?;
    app.wait_for_lifecycle_state("core_started", Duration::from_secs(30))?;
    let mut events = base_events();
    let long_body =
        "dense wrap row 00 alpha beta gamma delta epsilon zeta eta theta iota kappa lambda mu";
    for index in 0..18 {
        let body = format!(
            "dense wrap row {index:02} alpha beta gamma delta epsilon zeta eta theta iota kappa lambda mu"
        );
        events.push(text_event(
            "!beta:example",
            "@bob:example",
            1_700_001_000_000 + index * 60_000,
            &body,
        ));
    }
    events.push(text_event(
        "!beta:example",
        "@bob:example",
        1_700_002_500_000,
        "dense multiline line a\ndense multiline line b",
    ));
    events.push(text_event(
        "!beta:example",
        "@bob:example",
        1_700_002_560_000,
        "dense trailing blank line\n",
    ));
    app.load_events(&serde_json::to_string(&events)?)?;
    app.wait_idle(Duration::from_secs(5))?;
    app.select_room("!beta:example")?;
    app.wait_for_room_activity_contains(Some("!beta:example"), long_body, Duration::from_secs(5))?;

    let timeline_hwnd = unsafe { active_room_timeline_hwnd(&app) };
    let timeline_style = unsafe { window_style(timeline_hwnd) };
    let timeline_rect = unsafe { client_rect(timeline_hwnd) };
    assert!(
        timeline_style & WS_VSCROLL != 0,
        "timeline child must advertise WS_VSCROLL"
    );

    let text = unsafe { read_window_text(timeline_hwnd) };
    assert!(
        text.contains(long_body),
        "timeline WM_GETTEXT must keep the full long logical row: {text:?}"
    );
    assert!(
        text.contains("dense multiline line a\r\ndense multiline line b"),
        "timeline WM_GETTEXT must keep multiline normalization for wrapped rows: {text:?}"
    );
    assert!(
        text.contains("dense trailing blank line\r\n"),
        "timeline WM_GETTEXT must keep trailing blank lines for wrapped rows: {text:?}"
    );

    let before = unsafe { scrollbar_info(timeline_hwnd) };
    let before_top_line = unsafe { timeline_top_line(timeline_hwnd) };
    if let Some(before) = before {
        assert!(
            before.nMax > 0,
            "vertical scrollbar must report a non-zero range: style=0x{:x} rect={}x{} nMin={} nMax={} nPage={} nPos={}",
            timeline_style,
            timeline_rect.right - timeline_rect.left,
            timeline_rect.bottom - timeline_rect.top,
            before.nMin,
            before.nMax,
            before.nPage,
            before.nPos
        );
        assert!(
            before.nPage > 0,
            "vertical scrollbar must report a visible page size"
        );
        assert_eq!(
            before.nPos,
            before.nMax - before.nPage as i32 + 1,
            "wrapped timeline should start at the bottom"
        );
    } else {
        assert!(
            timeline_rect.right - timeline_rect.left > 0
                && timeline_rect.bottom - timeline_rect.top > 0,
            "timeline child must have a visible client rect"
        );
        assert!(
            before_top_line > 0,
            "wrapped timeline should start below the top"
        );
    }

    unsafe {
        SendMessageW(timeline_hwnd, WM_VSCROLL, SB_PAGEUP as WPARAM, 0);
    }

    let after_top_line = unsafe { timeline_top_line(timeline_hwnd) };
    if let Some(before) = before {
        let after = unsafe { scrollbar_info(timeline_hwnd) }
            .expect("GetScrollInfo should keep succeeding once it succeeds for the timeline child");
        assert!(
            after.nPos < before.nPos,
            "page-up must move scrollbar position for wrapped rows: before={}, after={}",
            before.nPos,
            after.nPos
        );
    } else {
        assert!(
            after_top_line < before_top_line,
            "page-up must move the timeline top line for wrapped rows: before={}, after={}",
            before_top_line,
            after_top_line
        );
    }

    let _ = fs::remove_dir_all(&local_app_data);
    Ok(())
}

unsafe fn active_mdi_content_hwnd(app: &MmatrixApp) -> HWND {
    let frame = app.automation_frame_hwnd() as HWND;
    let mdi_client = unsafe { GetDlgItem(frame, MDI_ID) };
    assert!(!mdi_client.is_null(), "MDI client control must exist");
    let active_child = unsafe { SendMessageW(mdi_client, WM_MDIGETACTIVE, 0, 0) as HWND };
    assert!(!active_child.is_null(), "active MDI child must exist");
    let content = unsafe { GetDlgItem(active_child, VIEW_CONTENT_ID) };
    assert!(!content.is_null(), "active MDI child content must exist");
    content
}

unsafe fn active_room_timeline_hwnd(app: &MmatrixApp) -> HWND {
    unsafe { child_window_with_class(active_mdi_content_hwnd(app), "mmatrixWin32Timeline") }
}

unsafe fn active_room_member_list_hwnd(app: &MmatrixApp) -> HWND {
    unsafe { child_window_with_class(active_mdi_content_hwnd(app), "ListBox") }
}

unsafe fn child_window_with_class(parent: HWND, expected_class_name: &str) -> HWND {
    let mut child = unsafe { FindWindowExW(parent, 0 as HWND, std::ptr::null(), std::ptr::null()) };
    while !child.is_null() {
        if unsafe { class_name(child) } == expected_class_name {
            return child;
        }
        child = unsafe { FindWindowExW(parent, child, std::ptr::null(), std::ptr::null()) };
    }
    panic!(
        "child window with class {expected_class_name} was not found under {}",
        unsafe { class_name(parent) }
    );
}

unsafe fn class_name(hwnd: HWND) -> String {
    let mut buffer = vec![0u16; 256];
    let len = unsafe { GetClassNameW(hwnd, buffer.as_mut_ptr(), buffer.len() as i32) };
    String::from_utf16_lossy(&buffer[..len as usize])
}

unsafe fn client_rect(hwnd: HWND) -> RECT {
    let mut rect = RECT::default();
    unsafe { GetClientRect(hwnd, &mut rect) };
    rect
}

unsafe fn window_rect(hwnd: HWND) -> RECT {
    let mut rect = RECT::default();
    assert_ne!(
        unsafe { GetWindowRect(hwnd, &mut rect) },
        0,
        "GetWindowRect must succeed"
    );
    rect
}

unsafe fn window_style(hwnd: HWND) -> u32 {
    unsafe { GetWindowLongPtrW(hwnd, GWL_STYLE) as u32 }
}

unsafe fn read_window_text(hwnd: HWND) -> String {
    let len = unsafe { SendMessageW(hwnd, WM_GETTEXTLENGTH, 0, 0) };
    let mut buffer = vec![0u16; len as usize + 1];
    unsafe { SendMessageW(hwnd, WM_GETTEXT, buffer.len(), buffer.as_mut_ptr() as isize) };
    String::from_utf16_lossy(&buffer[..len as usize])
}

unsafe fn timeline_top_line(hwnd: HWND) -> i32 {
    unsafe { SendMessageW(hwnd, WM_TIMELINE_GET_TOP_LINE, 0, 0) as i32 }
}

unsafe fn scrollbar_info(hwnd: HWND) -> Option<SCROLLINFO> {
    let mut info = SCROLLINFO {
        cbSize: std::mem::size_of::<SCROLLINFO>() as u32,
        fMask: SIF_ALL,
        nMin: 0,
        nMax: 0,
        nPage: 0,
        nPos: 0,
        nTrackPos: 0,
    };
    let rect = unsafe { client_rect(hwnd) };
    let style = unsafe { window_style(hwnd) };
    let success = unsafe { GetScrollInfo(hwnd, SB_VERT, &mut info) };
    let error = unsafe { GetLastError() };
    if success == 0 {
        eprintln!(
            "cross-process GetScrollInfo failed for timeline child: style=0x{style:x} rect={}x{} last_error={error}",
            rect.right - rect.left,
            rect.bottom - rect.top
        );
        None
    } else {
        Some(info)
    }
}

#[test]
fn automation_driver_renders_space_hierarchy_and_preserves_duplicate_tree_selection()
-> Result<(), Box<dyn std::error::Error>> {
    let exe_path = mmatrix_ui_exe_path()
        .map_err(|error| format!("failed to locate mmatrix-ui.exe: {error}"))?;
    let local_app_data = unique_local_app_data_path();
    let options = launch_options(Some(local_app_data.clone()));
    let fixture_path = fixture_state_path("space-hierarchy.json");
    let args = [OsString::from("--automation")];

    let app = launch_app_with_options(&exe_path, &args, options)?;
    app.wait_for_lifecycle_state("core_started", Duration::from_secs(30))?;
    app.load_events_from_file(&fixture_path)?;
    app.wait_idle(Duration::from_secs(5))?;

    assert_eq!(
        app.dump_model()?.rooms,
        vec![
            listed_space(
                "!work:example",
                "Work",
                vec![
                    SpaceChild {
                        room_id: "!ops:example".to_owned(),
                        suggested: true,
                        order: Some("a".to_owned()),
                    },
                    SpaceChild {
                        room_id: "!shared:example".to_owned(),
                        suggested: false,
                        order: Some("b".to_owned()),
                    },
                ],
            ),
            listed_space(
                "!social:example",
                "Social",
                vec![
                    SpaceChild {
                        room_id: "!shared:example".to_owned(),
                        suggested: false,
                        order: Some("a".to_owned()),
                    },
                    SpaceChild {
                        room_id: "!random:example".to_owned(),
                        suggested: false,
                        order: Some("b".to_owned()),
                    },
                ],
            ),
            listed_room("!ops:example", "Ops", true),
            listed_room("!shared:example", "Shared", false),
            listed_room("!random:example", "Random", false),
            listed_room_with_metadata(
                "!direct:example",
                "Direct",
                true,
                true,
                vec!["@alice:example"],
                vec![
                    room_tag("m.favourite", Some("0.25")),
                    room_tag("u.work", None),
                ],
            ),
        ]
    );

    let initial_state = app.dump_ui_state()?;
    assert_eq!(
        initial_state.navigation,
        nav(
            "mmatrix",
            vec![
                nav("status", vec![]),
                nav(
                    "rooms: 4",
                    vec![
                        nav(
                            "Work",
                            vec![
                                nav("Ops [e2ee]", vec![]),
                                nav_with_activity("Shared", true, vec![]),
                            ],
                        ),
                        nav(
                            "Social",
                            vec![
                                nav_with_activity("Shared", true, vec![]),
                                nav_with_activity("Random", true, vec![]),
                            ],
                        ),
                        nav(
                            "unspaced rooms: 1",
                            vec![nav("Direct [query: @alice:example] [e2ee]", vec![])],
                        ),
                    ],
                ),
                nav("invites: 0", vec![]),
            ],
        )
    );
    assert_eq!(
        initial_state
            .mdi
            .views
            .iter()
            .map(|view| view.identity.clone())
            .collect::<Vec<_>>(),
        vec![
            ViewIdentity::Status,
            room_identity("!ops:example"),
            room_identity("!shared:example"),
            room_identity("!random:example"),
            room_identity("!direct:example"),
        ]
    );
    assert_eq!(
        initial_state
            .switchbar
            .items
            .iter()
            .map(|item| item.identity.clone())
            .collect::<Vec<_>>(),
        vec![
            ViewIdentity::Status,
            room_identity("!ops:example"),
            room_identity("!shared:example"),
            room_identity("!random:example"),
            room_identity("!direct:example"),
        ]
    );
    assert_eq!(initial_state.mdi.active, room_identity("!ops:example"));
    assert_eq!(
        app.dump_model()?.selected_room.as_deref(),
        Some("!ops:example")
    );
    assert_eq!(
        initial_state
            .rooms
            .iter()
            .map(|room| room.room_id.as_str())
            .collect::<Vec<_>>(),
        vec![
            "!work:example",
            "!social:example",
            "!ops:example",
            "!shared:example",
            "!random:example",
            "!direct:example",
        ]
    );
    assert_eq!(
        find_ui_room(&initial_state, "!direct:example"),
        &UiRoomState {
            room_id: "!direct:example".to_owned(),
            display_name: "Direct".to_owned(),
            membership: RoomMembership::Join,
            encrypted: true,
            is_space: false,
            is_direct: true,
            direct_user_ids: vec!["@alice:example".to_owned()],
            tags: vec![
                room_tag("m.favourite", Some("0.25")),
                room_tag("u.work", None),
            ],
            space_children: Vec::new(),
            has_activity: false,
            read_target_event_id: None,
            read_state: None,
            history_backfill: UiRoomHistoryBackfillState::Idle,
        }
    );
    assert_eq!(
        find_view(&initial_state, &room_identity("!direct:example")).title,
        "Direct [query: @alice:example] [e2ee]"
    );
    assert!(
        find_view(&initial_state, &room_identity("!direct:example"))
            .lines
            .contains(&"Room: Direct [query: @alice:example] [e2ee] (!direct:example)".to_owned())
    );
    assert_eq!(
        find_switchbar_item(&initial_state, &room_identity("!direct:example")).title,
        "Direct [query: @alice:example] [e2ee]"
    );
    assert_switchbar_activity(&initial_state, &room_identity("!ops:example"), false);
    assert_switchbar_activity(&initial_state, &room_identity("!shared:example"), true);
    assert_switchbar_activity(&initial_state, &room_identity("!random:example"), true);
    assert_switchbar_activity(&initial_state, &room_identity("!direct:example"), false);
    assert_eq!(
        app.selected_navigation_path()?,
        nav_path(&["mmatrix", "rooms: 4", "Work", "Ops [e2ee]"])
    );

    app.select_navigation_labels(&["mmatrix", "rooms: 4", "Work"])?;
    assert_eq!(
        app.selected_navigation_path()?,
        nav_path(&["mmatrix", "rooms: 4", "Work"])
    );
    let work_state = app.dump_ui_state()?;
    assert_eq!(work_state.mdi.active, room_identity("!ops:example"));
    assert_eq!(
        app.dump_model()?.selected_room.as_deref(),
        Some("!ops:example")
    );
    assert!(navigation_contains(
        &work_state.navigation,
        "Direct [query: @alice:example] [e2ee]"
    ));
    assert_eq!(
        find_view(&work_state, &room_identity("!direct:example")).title,
        "Direct [query: @alice:example] [e2ee]"
    );
    assert!(
        find_view(&work_state, &room_identity("!direct:example"))
            .lines
            .contains(&"Room: Direct [query: @alice:example] [e2ee] (!direct:example)".to_owned())
    );
    assert_eq!(
        find_switchbar_item(&work_state, &room_identity("!direct:example")).title,
        "Direct [query: @alice:example] [e2ee]"
    );

    app.select_navigation_labels(&["mmatrix", "rooms: 4", "Social", "Shared"])?;
    assert_eq!(
        app.selected_navigation_path()?,
        vec![
            nav_segment("mmatrix", 0),
            nav_segment("rooms: 4", 0),
            nav_segment("Social", 0),
            nav_segment("Shared", 0),
        ]
    );
    let shared_state = app.dump_ui_state()?;
    assert_eq!(shared_state.mdi.active, room_identity("!shared:example"));
    assert_eq!(
        app.dump_model()?.selected_room.as_deref(),
        Some("!shared:example")
    );
    assert_switchbar_activity(&shared_state, &room_identity("!shared:example"), false);

    app.select_room("!shared:example")?;
    assert_eq!(
        app.selected_navigation_path()?,
        nav_path(&["mmatrix", "rooms: 4", "Work", "Shared"])
    );
    let shared_primary_state = app.dump_ui_state()?;
    assert_eq!(
        shared_primary_state.mdi.active,
        room_identity("!shared:example")
    );
    assert_eq!(
        app.dump_model()?.selected_room.as_deref(),
        Some("!shared:example")
    );

    match app.select_room("!work:example") {
        Err(TestkitError::Protocol { code, message }) => {
            assert_eq!(code, "unknown_room");
            assert_eq!(message, "unknown room id: !work:example");
        }
        other => panic!("expected select_room unknown_room for space, got {other:?}"),
    }
    match app.send_text(Some("!work:example"), "hello") {
        Err(TestkitError::Protocol { code, message }) => {
            assert_eq!(code, "unknown_room");
            assert_eq!(message, "unknown room id: !work:example");
        }
        other => panic!("expected send_text unknown_room for space, got {other:?}"),
    }

    app.select_navigation_labels(&["mmatrix", "rooms: 4", "Work", "Ops [e2ee]"])?;
    assert_eq!(
        app.send_text_target(None, "queued for ops")?,
        "!ops:example"
    );
    app.wait_for_lifecycle_state("error", Duration::from_secs(30))?;
    assert_eq!(
        app.dump_model()?.last_error,
        Some(UiError {
            operation: "send_text".to_owned(),
            message: "send message requires an authenticated session".to_owned(),
        })
    );

    drop(app);
    let _ = fs::remove_dir_all(&local_app_data);
    Ok(())
}

#[test]
fn automation_driver_navigates_thread_hierarchy_fixture() -> Result<(), Box<dyn std::error::Error>>
{
    let exe_path = mmatrix_ui_exe_path()
        .map_err(|error| format!("failed to locate mmatrix-ui.exe: {error}"))?;
    let fixture_path = fixture_state_path("thread-hierarchy.json");
    let args = [
        OsString::from("--render-state"),
        fixture_path.as_os_str().to_os_string(),
    ];
    let app = launch_app(&exe_path, &args)?;
    app.wait_idle(Duration::from_secs(5))?;

    assert_eq!(
        app.room_activity(Some("!ops:example"))?,
        vec![
            text_entry_with_event_id(
                "!ops:example",
                "$ops-room-1",
                "@alice:example",
                1_700_000_000_000,
                "lobby",
            ),
            text_entry_with_event_id(
                "!ops:example",
                "$thread-root",
                "@bob:example",
                1_700_000_060_000,
                "root msg",
            ),
            text_entry_with_event_id_and_relation(
                "!ops:example",
                "$reply-1",
                "@carol:example",
                1_700_000_120_000,
                Some(RoomActivityRelation::Reply {
                    in_reply_to_event_id: "$ops-room-1".to_owned(),
                }),
                "plain",
            ),
            text_entry_with_event_id_and_relation(
                "!ops:example",
                "$thread-1",
                "@dave:example",
                1_700_000_180_000,
                Some(RoomActivityRelation::Thread {
                    thread_root_event_id: "$thread-root".to_owned(),
                    in_reply_to_event_id: Some("$thread-root".to_owned()),
                    is_falling_back: false,
                }),
                "thread rep",
            ),
            RoomActivity {
                room_id: "!ops:example".to_owned(),
                activity_id: "txn:thread-pending-1".to_owned(),
                timestamp_ms: 1_700_000_240_000,
                relation: Some(RoomActivityRelation::Thread {
                    thread_root_event_id: "$thread-root".to_owned(),
                    in_reply_to_event_id: Some("$thread-1".to_owned()),
                    is_falling_back: false,
                }),
                reactions: Vec::new(),
                kind: RoomActivityKind::Message(RoomMessageActivity {
                    event_id: None,
                    transaction_id: Some("thread-pending-1".to_owned()),
                    sender: "@alice:example".to_owned(),
                    presentation: RoomMessagePresentationKind::Text,
                    body: "pending threaded draft".to_owned(),
                    send_state: RoomMessageSendState::Pending,
                }),
            },
        ]
    );

    let initial_state = app.dump_ui_state()?;
    let room_path = navigation_path_by_substrings(
        &initial_state.navigation,
        &["mmatrix", "rooms: 1", "Work", "Ops"],
    );
    let thread_path = navigation_path_by_substrings(
        &initial_state.navigation,
        &["mmatrix", "rooms: 1", "Work", "Ops", "root msg"],
    );

    app.select_navigation_path(&room_path)?;
    assert_eq!(app.selected_navigation_path()?, room_path);
    let room_state = app.dump_ui_state()?;
    assert_eq!(room_state.mdi.active, room_identity("!ops:example"));
    let parent_view = find_view(&room_state, &room_identity("!ops:example"));
    let parent_lines = parent_view.lines.clone();
    assert!(view_contains(parent_view, "lobby"));
    assert!(view_contains(parent_view, "root msg"));
    assert!(view_contains(parent_view, "plain"));
    assert!(view_contains(parent_view, "thread rep"));
    assert!(view_contains(parent_view, "pending threaded draft"));

    app.select_navigation_path(&thread_path)?;
    assert_eq!(app.selected_navigation_path()?, thread_path);
    let thread_state = app.dump_ui_state()?;
    let thread_view_identity = thread_identity("!ops:example", "$thread-root");
    assert_eq!(thread_state.mdi.active, thread_view_identity);
    assert_eq!(
        thread_state
            .switchbar
            .items
            .iter()
            .map(|item| item.identity.clone())
            .collect::<Vec<_>>(),
        vec![
            ViewIdentity::Status,
            room_identity("!ops:example"),
            thread_view_identity.clone(),
        ]
    );
    assert_active_switchbar_item(&thread_state, &thread_view_identity);
    assert!(!find_switchbar_item(&thread_state, &room_identity("!ops:example")).active);

    let thread_view = find_view(&thread_state, &thread_view_identity);
    assert!(view_contains(thread_view, "root msg"));
    assert!(view_contains(thread_view, "thread rep"));
    assert!(view_contains(thread_view, "pending threaded draft"));
    assert!(!view_contains(thread_view, "lobby"));
    assert!(!view_contains(thread_view, "plain"));

    app.select_room("!ops:example")?;
    let reactivated_room_state = app.dump_ui_state()?;
    assert_eq!(
        reactivated_room_state.mdi.active,
        room_identity("!ops:example")
    );
    assert_eq!(
        find_view(&reactivated_room_state, &room_identity("!ops:example")).lines,
        parent_lines
    );

    Ok(())
}

#[test]
fn automation_driver_send_text_target_none_uses_active_thread_view()
-> Result<(), Box<dyn std::error::Error>> {
    let exe_path = mmatrix_ui_exe_path()
        .map_err(|error| format!("failed to locate mmatrix-ui.exe: {error}"))?;
    let fixture_path = fixture_state_path("thread-hierarchy.json");
    let args = [OsString::from("--automation")];
    let app = launch_app(&exe_path, &args)?;
    app.wait_for_lifecycle_state("core_started", Duration::from_secs(30))?;
    app.load_events(&fs::read_to_string(&fixture_path)?)?;
    app.wait_idle(Duration::from_secs(5))?;

    let thread_view_identity = thread_identity("!ops:example", "$thread-root");
    app.select_view(thread_view_identity.clone())?;
    app.wait_idle(Duration::from_secs(5))?;
    let thread_state = app.dump_ui_state()?;
    assert_eq!(thread_state.mdi.active, thread_view_identity);
    assert_switchbar_activity(&thread_state, &room_identity("!ops:example"), false);
    assert_switchbar_activity(&thread_state, &thread_view_identity, false);

    assert_eq!(
        app.send_text_target(None, "queued threaded automation send")?,
        "!ops:example"
    );
    app.wait_for_lifecycle_state("error", Duration::from_secs(30))?;

    let post_send_state = app.dump_ui_state()?;
    assert_eq!(post_send_state.mdi.active, thread_view_identity);
    assert_active_switchbar_item(&post_send_state, &thread_view_identity);
    assert_switchbar_activity(&post_send_state, &room_identity("!ops:example"), false);
    assert_switchbar_activity(&post_send_state, &thread_view_identity, false);
    let thread_view = find_view(&post_send_state, &thread_view_identity);
    assert!(view_contains(thread_view, "root msg"));
    assert!(view_contains(thread_view, "thread rep"));
    assert!(view_contains(thread_view, "pending threaded draft"));
    assert!(!view_contains(thread_view, "lobby"));
    assert!(!view_contains(thread_view, "plain"));

    Ok(())
}

#[test]
fn automation_driver_send_file_target_uses_explicit_joined_room()
-> Result<(), Box<dyn std::error::Error>> {
    let exe_path = mmatrix_ui_exe_path()
        .map_err(|error| format!("failed to locate mmatrix-ui.exe: {error}"))?;
    let fixture_path = fixture_state_path("thread-hierarchy.json");
    let app = launch_app(&exe_path, &[OsString::from("--automation")])?;
    app.wait_for_lifecycle_state("core_started", Duration::from_secs(30))?;
    app.load_events(&fs::read_to_string(&fixture_path)?)?;
    assert_eq!(
        app.send_file_target(
            Some("!ops:example"),
            "C:\\tmp\\report.txt",
            Some("report.txt"),
            Some("application/octet-stream"),
        )?,
        "!ops:example"
    );
    Ok(())
}

#[test]
fn automation_driver_sendfile_slash_rejects_active_thread() -> Result<(), Box<dyn std::error::Error>>
{
    let exe_path = mmatrix_ui_exe_path()
        .map_err(|error| format!("failed to locate mmatrix-ui.exe: {error}"))?;
    let fixture_path = fixture_state_path("thread-hierarchy.json");
    let app = launch_app(&exe_path, &[OsString::from("--automation")])?;
    app.wait_for_lifecycle_state("core_started", Duration::from_secs(30))?;
    app.load_events(&fs::read_to_string(&fixture_path)?)?;
    app.select_view(thread_identity("!ops:example", "$thread-root"))?;
    app.set_compose_text("/sendfile C:\\tmp\\report.txt")?;
    app.send_compose_key(ComposeKey::Return)?;
    app.wait_idle(Duration::from_secs(5))?;
    assert_eq!(
        app.dump_model()?.status,
        "file attachments cannot target threads yet"
    );
    Ok(())
}

#[test]
fn automation_driver_inactive_thread_activity_marks_only_thread_until_reactivated()
-> Result<(), Box<dyn std::error::Error>> {
    let exe_path = mmatrix_ui_exe_path()
        .map_err(|error| format!("failed to locate mmatrix-ui.exe: {error}"))?;
    let fixture_path = fixture_state_path("thread-hierarchy.json");
    let args = [OsString::from("--automation")];
    let app = launch_app(&exe_path, &args)?;
    app.wait_for_lifecycle_state("core_started", Duration::from_secs(30))?;
    app.load_events(&fs::read_to_string(&fixture_path)?)?;
    app.wait_idle(Duration::from_secs(5))?;

    let room_view_identity = room_identity("!ops:example");
    let thread_view_identity = thread_identity("!ops:example", "$thread-root");

    app.select_view(thread_view_identity.clone())?;
    app.select_room("!ops:example")?;

    app.append_synthetic_events(&serde_json::to_string(&vec![
        message_event_with_event_id_and_relation(
            "!ops:example",
            "$thread-activity-2",
            "@erin:example",
            1_700_000_300_000,
            Some(CoreRoomActivityRelation::Thread {
                thread_root_event_id: "$thread-root".to_owned(),
                in_reply_to_event_id: Some("$thread-1".to_owned()),
                is_falling_back: false,
            }),
            CoreRoomMessagePresentationKind::Text,
            "thread follow-up",
        ),
    ])?)?;
    app.wait_idle(Duration::from_secs(5))?;

    let inactive_thread_state = app.dump_ui_state()?;
    assert_eq!(inactive_thread_state.mdi.active, room_view_identity);
    assert_eq!(
        inactive_thread_state
            .switchbar
            .items
            .iter()
            .map(|item| item.identity.clone())
            .collect::<Vec<_>>(),
        vec![
            ViewIdentity::Status,
            room_view_identity.clone(),
            thread_view_identity.clone(),
        ]
    );
    assert_active_switchbar_item(&inactive_thread_state, &room_view_identity);
    assert_switchbar_activity(&inactive_thread_state, &room_view_identity, false);
    assert_switchbar_activity(&inactive_thread_state, &thread_view_identity, true);
    assert_eq!(
        find_switchbar_item(&inactive_thread_state, &thread_view_identity).title,
        "Thread: root msg"
    );

    app.select_view(thread_view_identity.clone())?;
    let reactivated_thread_state = app.dump_ui_state()?;
    assert_eq!(reactivated_thread_state.mdi.active, thread_view_identity);
    assert_active_switchbar_item(&reactivated_thread_state, &thread_view_identity);
    assert_switchbar_activity(&reactivated_thread_state, &room_view_identity, false);
    assert_switchbar_activity(&reactivated_thread_state, &thread_view_identity, false);

    Ok(())
}

#[test]
fn automation_driver_applies_receive_side_message_edits_in_place()
-> Result<(), Box<dyn std::error::Error>> {
    let exe_path = mmatrix_ui_exe_path()
        .map_err(|error| format!("failed to locate mmatrix-ui.exe: {error}"))?;
    let fixture_path = fixture_state_path("message-edits.json");
    let update_path = fixture_update_path("message-edits.json");
    let local_app_data = unique_local_app_data_path();
    let options = launch_options(Some(local_app_data.clone()));
    let args = [OsString::from("--automation")];
    let app = launch_app_with_options(&exe_path, &args, options)?;
    app.wait_for_lifecycle_state("core_started", Duration::from_secs(30))?;
    app.load_events(&fs::read_to_string(&fixture_path)?)?;
    app.wait_idle(Duration::from_secs(5))?;

    assert_eq!(
        app.room_activity(Some("!ops:example"))?,
        vec![
            text_entry_with_event_id(
                "!ops:example",
                "$ops-room-1",
                "@alice:example",
                1_700_000_000_000,
                "lobby draft",
            ),
            text_entry_with_event_id(
                "!ops:example",
                "$thread-root",
                "@bob:example",
                1_700_000_060_000,
                "root msg draft",
            ),
            text_entry_with_event_id_and_relation(
                "!ops:example",
                "$thread-1",
                "@carol:example",
                1_700_000_120_000,
                Some(RoomActivityRelation::Thread {
                    thread_root_event_id: "$thread-root".to_owned(),
                    in_reply_to_event_id: Some("$thread-root".to_owned()),
                    is_falling_back: false,
                }),
                "thread reply",
            ),
        ]
    );

    app.append_synthetic_events(&fs::read_to_string(&update_path)?)?;
    app.wait_idle(Duration::from_secs(5))?;

    assert_eq!(
        app.room_activity(Some("!ops:example"))?,
        vec![
            text_entry_with_event_id(
                "!ops:example",
                "$ops-room-1",
                "@alice:example",
                1_700_000_000_000,
                "lobby edited",
            ),
            text_entry_with_event_id(
                "!ops:example",
                "$thread-root",
                "@bob:example",
                1_700_000_060_000,
                "root msg edited",
            ),
            text_entry_with_event_id_and_relation(
                "!ops:example",
                "$thread-1",
                "@carol:example",
                1_700_000_120_000,
                Some(RoomActivityRelation::Thread {
                    thread_root_event_id: "$thread-root".to_owned(),
                    in_reply_to_event_id: Some("$thread-root".to_owned()),
                    is_falling_back: false,
                }),
                "thread reply",
            ),
        ]
    );

    let ui_state = app.dump_ui_state()?;
    let room_view = find_view(&ui_state, &room_identity("!ops:example"));
    assert!(view_contains(room_view, "lobby edited"));
    assert!(view_contains(room_view, "root msg edited"));
    assert!(!view_contains(room_view, "lobby draft"));
    assert!(!view_contains(room_view, "root msg draft"));

    drop(app);
    let _ = fs::remove_dir_all(&local_app_data);
    Ok(())
}

#[test]
fn automation_driver_loads_receive_side_reactions() -> Result<(), Box<dyn std::error::Error>> {
    let exe_path = mmatrix_ui_exe_path()
        .map_err(|error| format!("failed to locate mmatrix-ui.exe: {error}"))?;
    let fixture_path = fixture_state_path("reactions.json");
    let local_app_data = unique_local_app_data_path();
    let options = launch_options(Some(local_app_data.clone()));
    let args = [OsString::from("--automation")];
    let app = launch_app_with_options(&exe_path, &args, options)?;
    app.wait_for_lifecycle_state("core_started", Duration::from_secs(30))?;
    app.load_events(&fs::read_to_string(&fixture_path)?)?;
    app.wait_idle(Duration::from_secs(5))?;

    assert_eq!(
        app.room_activity(Some("!reactions:example"))?,
        vec![
            with_reactions(
                text_entry_with_event_id(
                    "!reactions:example",
                    "$react-normal",
                    "@normal:example",
                    1_700_200_000_000,
                    "normal reaction target",
                ),
                vec![
                    reaction_summary(
                        "+1",
                        2,
                        &["@alice:example", "@bob:example"],
                        &["$reaction-plus-one-a", "$reaction-plus-one-b"],
                    ),
                    reaction_summary("heart", 1, &["@carol:example"], &["$reaction-heart-a"],),
                ],
            ),
            text_entry_with_event_id(
                "!reactions:example",
                "$react-thread-root",
                "@root:example",
                1_700_200_060_000,
                "thread reaction root",
            ),
            with_reactions(
                text_entry_with_event_id_and_relation(
                    "!reactions:example",
                    "$react-thread-reply",
                    "@reply:example",
                    1_700_200_120_000,
                    Some(RoomActivityRelation::Thread {
                        thread_root_event_id: "$react-thread-root".to_owned(),
                        in_reply_to_event_id: Some("$react-thread-root".to_owned()),
                        is_falling_back: false,
                    }),
                    "thread reaction reply",
                ),
                vec![reaction_summary(
                    "eyes",
                    1,
                    &["@dave:example"],
                    &["$reaction-eyes-thread"],
                )],
            ),
        ]
    );

    let ui_state = app.dump_ui_state()?;
    let room_view = find_view(&ui_state, &room_identity("!reactions:example"));
    assert!(view_contains(room_view, "reactions: +1 x2  heart x1"));

    let thread_view_identity = thread_identity("!reactions:example", "$react-thread-root");
    app.select_view(thread_view_identity.clone())?;
    app.wait_idle(Duration::from_secs(5))?;
    let thread_state = app.dump_ui_state()?;
    let thread_view = find_view(&thread_state, &thread_view_identity);
    assert!(view_contains(thread_view, "reactions: eyes x1"));

    drop(app);
    let _ = fs::remove_dir_all(&local_app_data);
    Ok(())
}

#[test]
fn automation_driver_applies_receive_side_reactions_redactions()
-> Result<(), Box<dyn std::error::Error>> {
    let exe_path = mmatrix_ui_exe_path()
        .map_err(|error| format!("failed to locate mmatrix-ui.exe: {error}"))?;
    let fixture_path = fixture_state_path("reactions.json");
    let update_path = fixture_update_path("reactions.json");
    let local_app_data = unique_local_app_data_path();
    let options = launch_options(Some(local_app_data.clone()));
    let args = [OsString::from("--automation")];
    let app = launch_app_with_options(&exe_path, &args, options)?;
    app.wait_for_lifecycle_state("core_started", Duration::from_secs(30))?;
    app.load_events(&fs::read_to_string(&fixture_path)?)?;
    app.wait_idle(Duration::from_secs(5))?;

    app.append_synthetic_events(&fs::read_to_string(&update_path)?)?;
    app.wait_idle(Duration::from_secs(5))?;

    assert_eq!(
        app.room_activity(Some("!reactions:example"))?,
        vec![
            with_reactions(
                text_entry_with_event_id(
                    "!reactions:example",
                    "$react-normal",
                    "@normal:example",
                    1_700_200_000_000,
                    "normal reaction target",
                ),
                vec![reaction_summary(
                    "+1",
                    1,
                    &["@bob:example"],
                    &["$reaction-plus-one-b"],
                )],
            ),
            text_entry_with_event_id(
                "!reactions:example",
                "$react-thread-root",
                "@root:example",
                1_700_200_060_000,
                "thread reaction root",
            ),
            with_reactions(
                text_entry_with_event_id_and_relation(
                    "!reactions:example",
                    "$react-thread-reply",
                    "@reply:example",
                    1_700_200_120_000,
                    Some(RoomActivityRelation::Thread {
                        thread_root_event_id: "$react-thread-root".to_owned(),
                        in_reply_to_event_id: Some("$react-thread-root".to_owned()),
                        is_falling_back: false,
                    }),
                    "thread reaction reply",
                ),
                vec![reaction_summary(
                    "eyes",
                    1,
                    &["@dave:example"],
                    &["$reaction-eyes-thread"],
                )],
            ),
        ]
    );

    let ui_state = app.dump_ui_state()?;
    let room_view = find_view(&ui_state, &room_identity("!reactions:example"));
    assert!(view_contains(room_view, "reactions: +1 x1"));
    assert!(!view_contains(room_view, "heart x1"));

    let thread_view_identity = thread_identity("!reactions:example", "$react-thread-root");
    app.select_view(thread_view_identity.clone())?;
    app.wait_idle(Duration::from_secs(5))?;
    let thread_state = app.dump_ui_state()?;
    let thread_view = find_view(&thread_state, &thread_view_identity);
    assert!(view_contains(thread_view, "reactions: eyes x1"));

    drop(app);
    let _ = fs::remove_dir_all(&local_app_data);
    Ok(())
}

#[test]
fn automation_driver_applies_receive_side_redactions_without_leaking_payloads()
-> Result<(), Box<dyn std::error::Error>> {
    let exe_path = mmatrix_ui_exe_path()
        .map_err(|error| format!("failed to locate mmatrix-ui.exe: {error}"))?;
    let fixture_path = fixture_state_path("redactions.json");
    let update_path = fixture_update_path("redactions.json");
    let local_app_data = unique_local_app_data_path();
    let options = launch_options(Some(local_app_data.clone()));
    let args = [OsString::from("--automation")];
    let app = launch_app_with_options(&exe_path, &args, options)?;
    app.wait_for_lifecycle_state("core_started", Duration::from_secs(30))?;
    app.load_events(&fs::read_to_string(&fixture_path)?)?;
    app.wait_idle(Duration::from_secs(5))?;

    app.append_synthetic_events(&fs::read_to_string(&update_path)?)?;
    app.wait_idle(Duration::from_secs(5))?;

    let room_activity = app.room_activity(Some("!redactions:example"))?;
    assert_eq!(
        room_activity,
        vec![
            redacted_entry(
                "!redactions:example",
                "event:$redact-text",
                1_700_100_000_000,
                None,
                "$redact-text",
            ),
            redacted_entry(
                "!redactions:example",
                "event:$redact-edited",
                1_700_100_060_000,
                None,
                "$redact-edited",
            ),
            redacted_entry(
                "!redactions:example",
                "event:$redact-image",
                1_700_100_120_000,
                None,
                "$redact-image",
            ),
            redacted_entry(
                "!redactions:example",
                "event:$redact-unsupported",
                1_700_100_180_000,
                None,
                "$redact-unsupported",
            ),
            redacted_entry(
                "!redactions:example",
                "event:$redact-thread-root",
                1_700_100_240_000,
                None,
                "$redact-thread-root",
            ),
            redacted_entry(
                "!redactions:example",
                "event:$redact-thread-reply",
                1_700_100_300_000,
                Some(RoomActivityRelation::Thread {
                    thread_root_event_id: "$redact-thread-root".to_owned(),
                    in_reply_to_event_id: Some("$redact-thread-root".to_owned()),
                    is_falling_back: false,
                }),
                "$redact-thread-reply",
            ),
            redacted_entry(
                "!redactions:example",
                "state:$redact-topic-notice",
                1_700_100_360_000,
                None,
                "$redact-topic-notice",
            ),
        ]
    );

    let ui_state = app.dump_ui_state()?;
    let room_view = find_view(&ui_state, &room_identity("!redactions:example"));
    assert!(view_contains(room_view, "message redacted"));
    let room_view_text = room_view.lines.join("\n");
    let room_view_tombstones = room_view_text.matches("message redacted").count();
    assert!(
        room_view_tombstones >= room_activity.len(),
        "expected at least {} room-view tombstones, got {room_view_tombstones}: {room_view_text}",
        room_activity.len(),
    );
    assert!(navigation_contains(
        &ui_state.navigation,
        "Message redacted"
    ));

    let model = app.dump_model()?;
    assert_eq!(model.last_activity, None);

    let leak_strings = [
        "redaction text payload should disappear",
        "redaction edited original should disappear",
        "redaction edited replacement should disappear",
        "redaction-image-body.png",
        "redaction-image-file.png",
        "mxc://example.org/redaction-image-source",
        "com.example.custom",
        "redaction unsupported payload should disappear",
        "redaction thread root payload should disappear",
        "redaction thread reply payload should disappear",
    ];
    let redacted_topic_row = "@grace:example set topic: redaction topic should disappear";
    let room_activity_text = serde_json::to_string(&room_activity)?;
    let ui_state_text = serde_json::to_string(&ui_state)?;
    let model_text = serde_json::to_string(&model)?;
    for leak in leak_strings {
        assert!(
            !room_view_text.contains(leak),
            "room view unexpectedly leaked {leak:?}: {room_view_text}",
        );
        assert!(
            !room_activity_text.contains(leak),
            "room activity dump unexpectedly leaked {leak:?}: {room_activity_text}",
        );
        assert!(
            !ui_state_text.contains(leak),
            "ui state unexpectedly leaked {leak:?}: {ui_state_text}",
        );
        assert!(
            !model_text.contains(leak),
            "model dump unexpectedly leaked {leak:?}: {model_text}",
        );
    }
    assert!(!room_view_text.contains(redacted_topic_row));
    assert!(!ui_state_text.contains(redacted_topic_row));
    assert!(!room_activity_text.contains("redaction topic should disappear"));
    assert!(!model_text.contains("redaction topic should disappear"));

    let thread_view_identity = thread_identity("!redactions:example", "$redact-thread-root");
    let thread_path = navigation_path_by_substrings(
        &ui_state.navigation,
        &["mmatrix", "rooms: 1", "Redactions", "Message redacted"],
    );
    app.select_navigation_path(&thread_path)?;
    assert_eq!(app.selected_navigation_path()?, thread_path);
    let thread_state = app.dump_ui_state()?;
    let thread_view = find_view(&thread_state, &thread_view_identity);
    assert_eq!(
        find_switchbar_item(&thread_state, &thread_view_identity).title,
        "Thread: Message redacted"
    );
    assert_eq!(thread_view.title, "Thread: Message redacted");
    let thread_text = thread_view.lines.join("\n");
    assert!(thread_text.contains("message redacted"));
    assert!(thread_text.matches("message redacted").count() >= 2);
    assert!(!thread_text.contains("redaction thread root payload should disappear"));
    assert!(!thread_text.contains("redaction thread reply payload should disappear"));

    drop(app);
    let _ = fs::remove_dir_all(&local_app_data);
    Ok(())
}

#[test]
fn automation_driver_thread_root_message_edit_updates_thread_titles()
-> Result<(), Box<dyn std::error::Error>> {
    let exe_path = mmatrix_ui_exe_path()
        .map_err(|error| format!("failed to locate mmatrix-ui.exe: {error}"))?;
    let fixture_path = fixture_state_path("message-edits.json");
    let update_path = fixture_update_path("message-edits.json");
    let local_app_data = unique_local_app_data_path();
    let options = launch_options(Some(local_app_data.clone()));
    let args = [OsString::from("--automation")];
    let app = launch_app_with_options(&exe_path, &args, options)?;
    app.wait_for_lifecycle_state("core_started", Duration::from_secs(30))?;
    app.load_events(&fs::read_to_string(&fixture_path)?)?;
    app.wait_idle(Duration::from_secs(5))?;

    let thread_view_identity = thread_identity("!ops:example", "$thread-root");
    app.select_view(thread_view_identity.clone())?;

    let initial_state = app.dump_ui_state()?;
    assert_eq!(
        find_switchbar_item(&initial_state, &thread_view_identity).title,
        "Thread: root msg draft"
    );
    assert_eq!(
        find_view(&initial_state, &thread_view_identity).title,
        "Thread: root msg draft"
    );

    app.append_synthetic_events(&fs::read_to_string(&update_path)?)?;
    app.wait_idle(Duration::from_secs(5))?;

    let updated_state = app.dump_ui_state()?;
    assert_eq!(updated_state.mdi.active, thread_view_identity);
    assert_active_switchbar_item(&updated_state, &thread_view_identity);
    assert_eq!(
        find_switchbar_item(&updated_state, &thread_view_identity).title,
        "Thread: root msg edited"
    );
    let thread_view = find_view(&updated_state, &thread_view_identity);
    assert_eq!(thread_view.title, "Thread: root msg edited");
    assert!(view_contains(thread_view, "root msg edited"));
    assert!(!view_contains(thread_view, "root msg draft"));
    assert!(navigation_contains(
        &updated_state.navigation,
        "root msg edited"
    ));
    assert_navigation_not_contains(&updated_state.navigation, "root msg draft");

    let expected_path = navigation_path_by_substrings(
        &updated_state.navigation,
        &["mmatrix", "rooms: 1", "Work", "Ops", "root msg edited"],
    );
    assert_eq!(app.selected_navigation_path()?, expected_path);

    drop(app);
    let _ = fs::remove_dir_all(&local_app_data);
    Ok(())
}

#[test]
fn automation_driver_rejects_invalid_append_message_edit_update_without_exiting()
-> Result<(), Box<dyn std::error::Error>> {
    let exe_path = mmatrix_ui_exe_path()
        .map_err(|error| format!("failed to locate mmatrix-ui.exe: {error}"))?;
    let fixture_path = fixture_state_path("message-edits.json");
    let local_app_data = unique_local_app_data_path();
    let options = launch_options(Some(local_app_data.clone()));
    let args = [OsString::from("--automation")];
    let app = launch_app_with_options(&exe_path, &args, options)?;
    app.wait_for_lifecycle_state("core_started", Duration::from_secs(30))?;
    app.load_events(&fs::read_to_string(&fixture_path)?)?;
    app.wait_idle(Duration::from_secs(5))?;
    app.select_room("!ops:example")?;
    app.wait_idle(Duration::from_secs(5))?;

    match app.append_synthetic_events(&serde_json::to_string(&vec![
        text_message_replaced_update_event(
            "!ops:example",
            "$missing-event",
            "$edit-missing",
            "@alice:example",
            1_700_000_180_000,
            "ghost edit",
        ),
    ])?) {
        Err(TestkitError::Protocol { code, message }) => {
            assert_eq!(code, "invalid_synthetic_state");
            assert_eq!(
                message,
                "message edit requires an existing visible target event"
            );
        }
        other => panic!("expected invalid_synthetic_state for append, got {other:?}"),
    }

    app.ping()?;
    assert_eq!(
        app.room_activity(Some("!ops:example"))?,
        vec![
            text_entry_with_event_id(
                "!ops:example",
                "$ops-room-1",
                "@alice:example",
                1_700_000_000_000,
                "lobby draft",
            ),
            text_entry_with_event_id(
                "!ops:example",
                "$thread-root",
                "@bob:example",
                1_700_000_060_000,
                "root msg draft",
            ),
            text_entry_with_event_id_and_relation(
                "!ops:example",
                "$thread-1",
                "@carol:example",
                1_700_000_120_000,
                Some(RoomActivityRelation::Thread {
                    thread_root_event_id: "$thread-root".to_owned(),
                    in_reply_to_event_id: Some("$thread-root".to_owned()),
                    is_falling_back: false,
                }),
                "thread reply",
            ),
        ]
    );
    assert_eq!(
        app.dump_model()?.selected_room.as_deref(),
        Some("!ops:example")
    );

    drop(app);
    let _ = fs::remove_dir_all(&local_app_data);
    Ok(())
}

#[test]
fn automation_driver_thread_me_submission_preserves_active_thread_view()
-> Result<(), Box<dyn std::error::Error>> {
    let exe_path = mmatrix_ui_exe_path()
        .map_err(|error| format!("failed to locate mmatrix-ui.exe: {error}"))?;
    let fixture_path = fixture_state_path("thread-hierarchy.json");
    let args = [OsString::from("--automation")];
    let app = launch_app(&exe_path, &args)?;
    app.wait_for_lifecycle_state("core_started", Duration::from_secs(30))?;
    app.load_events(&fs::read_to_string(&fixture_path)?)?;
    app.wait_idle(Duration::from_secs(5))?;

    let thread_view_identity = thread_identity("!ops:example", "$thread-root");
    app.select_view(thread_view_identity.clone())?;

    app.set_compose_text("/me waves")?;
    app.send_compose_key(ComposeKey::Return)?;
    assert_eq!(app.compose_text()?, "");
    app.wait_for_lifecycle_state("error", Duration::from_secs(30))?;

    let post_send_state = app.dump_ui_state()?;
    assert_eq!(post_send_state.mdi.active, thread_view_identity);
    assert_active_switchbar_item(&post_send_state, &thread_view_identity);
    assert_switchbar_activity(&post_send_state, &room_identity("!ops:example"), false);
    assert_switchbar_activity(&post_send_state, &thread_view_identity, false);
    let thread_view = find_view(&post_send_state, &thread_view_identity);
    assert!(view_contains(thread_view, "root msg"));
    assert!(view_contains(thread_view, "thread rep"));
    assert!(view_contains(thread_view, "pending threaded draft"));
    assert!(!view_contains(thread_view, "lobby"));
    assert!(!view_contains(thread_view, "plain"));

    app.send_compose_key(ComposeKey::Up)?;
    assert_eq!(app.compose_text()?, "/me waves");

    Ok(())
}

#[test]
fn automation_driver_explicit_room_target_send_ignores_active_thread_view()
-> Result<(), Box<dyn std::error::Error>> {
    let exe_path = mmatrix_ui_exe_path()
        .map_err(|error| format!("failed to locate mmatrix-ui.exe: {error}"))?;
    let fixture_path = fixture_state_path("thread-hierarchy.json");
    let args = [OsString::from("--automation")];
    let app = launch_app(&exe_path, &args)?;
    app.wait_for_lifecycle_state("core_started", Duration::from_secs(30))?;
    app.load_events(&fs::read_to_string(&fixture_path)?)?;
    app.wait_idle(Duration::from_secs(5))?;

    let thread_view_identity = thread_identity("!ops:example", "$thread-root");
    app.select_view(thread_view_identity.clone())?;

    assert_eq!(
        app.send_text_target(Some("!ops:example"), "explicit parent room send")?,
        "!ops:example"
    );
    app.wait_for_lifecycle_state("error", Duration::from_secs(30))?;

    let post_send_state = app.dump_ui_state()?;
    assert_eq!(post_send_state.mdi.active, thread_view_identity);
    assert_active_switchbar_item(&post_send_state, &thread_view_identity);
    assert_switchbar_activity(&post_send_state, &room_identity("!ops:example"), false);
    assert_switchbar_activity(&post_send_state, &thread_view_identity, false);

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
    let app = launch_app(&exe_path, &args)?;
    app.wait_idle(Duration::from_secs(5))?;

    let attached = MmatrixApp::find_by_pid(app.pid(), Duration::from_secs(5))?;
    let fixture_path = fixture_state_path("verification-error.json");
    attached.load_events_from_file(&fixture_path)?;
    attached.wait_idle(Duration::from_secs(5))?;

    assert_eq!(
        attached.list_rooms()?,
        vec![
            listed_room("!verify:example", "Verify", true),
            listed_room("!support:example", "Support", false),
        ]
    );
    assert_eq!(
        attached.dump_model()?.selected_room.as_deref(),
        Some("!verify:example")
    );
    assert_eq!(
        attached.room_activity(None)?,
        vec![
            text_entry_with_event_id(
                "!verify:example",
                "$verify-1",
                "@alice:example",
                1_700_000_060_000,
                "verification requested by @alice:example",
            ),
            text_entry_with_event_id(
                "!verify:example",
                "$verify-2",
                "@alice:example",
                1_700_000_120_000,
                "sas emoji ready",
            ),
            text_entry_with_event_id(
                "!verify:example",
                "$verify-3",
                "@alice:example",
                1_700_000_180_000,
                "waiting for confirmation",
            ),
        ]
    );

    let model = attached.dump_model()?;
    assert_eq!(model.lifecycle_state, "error");
    assert_eq!(
        model.last_verification,
        Some(Verification {
            flow_id: "flow-verify-123".to_owned(),
            user_id: "@alice:example".to_owned(),
            state: VerificationState::Requested,
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
        model.last_activity,
        Some(timeline_line_with_event_id(
            "!verify:example",
            "$verify-3",
            "@alice:example",
            1_700_000_180_000,
            "waiting for confirmation",
        ))
    );
    let ui_state = attached.dump_ui_state()?;
    assert!(ui_state.status_bar.text.contains("error during send text"));
    assert!(ui_state.status_bar.text.contains("flow-verify-123"));
    assert!(ui_state.status_bar.text.contains("message send failed"));
    assert_eq!(
        ui_state
            .switchbar
            .items
            .iter()
            .map(|item| &item.identity)
            .collect::<Vec<_>>(),
        vec![
            &ViewIdentity::Status,
            &room_identity("!verify:example"),
            &room_identity("!support:example"),
        ]
    );
    assert_active_switchbar_item(&ui_state, &room_identity("!verify:example"));
    assert_switchbar_activity(&ui_state, &room_identity("!support:example"), true);
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
fn automation_driver_loads_mixed_room_activity_fixture_via_file_api()
-> Result<(), Box<dyn std::error::Error>> {
    let exe_path = mmatrix_ui_exe_path()
        .map_err(|error| format!("failed to locate mmatrix-ui.exe: {error}"))?;
    let state_path = write_state_file(&base_events())?;

    let args = [
        OsString::from("--render-state"),
        state_path.as_os_str().to_os_string(),
    ];
    let app = launch_app(&exe_path, &args)?;
    app.wait_idle(Duration::from_secs(5))?;

    let attached = MmatrixApp::find_by_pid(app.pid(), Duration::from_secs(5))?;
    let fixture_path = fixture_state_path("mixed-room-activity.json");
    attached.load_events_from_file(&fixture_path)?;
    attached.wait_idle(Duration::from_secs(5))?;

    assert_eq!(
        attached.list_rooms()?,
        vec![listed_room("!mixed:example", "Mixed", true)]
    );
    assert_eq!(
        attached.dump_model()?.selected_room.as_deref(),
        Some("!mixed:example")
    );
    assert_eq!(
        attached.room_activity(None)?,
        vec![
            text_entry_with_event_id(
                "!mixed:example",
                "$mixed-1",
                "@alice:example",
                1_700_000_000_000,
                "synced hello",
            ),
            topic_entry_with_event_id(
                "!mixed:example",
                "$mixed-topic-1",
                "@alice:example",
                1_700_000_060_000,
                "mixed room topic",
            ),
            member_entry_with_event_id(
                "!mixed:example",
                "$mixed-join-1",
                "@bob:example",
                1_700_000_120_000,
                "@bob:example",
                "join",
                Some("bob"),
            ),
            member_entry_with_event_id(
                "!mixed:example",
                "$mixed-leave-1",
                "@carol:example",
                1_700_000_180_000,
                "@carol:example",
                "leave",
                None,
            ),
            message_entry_with_event_id(
                "!mixed:example",
                "$mixed-notice-1",
                "@system:example",
                1_700_000_240_000,
                RoomMessagePresentationKind::Notice,
                "server maintenance",
            ),
            message_entry_with_event_id(
                "!mixed:example",
                "$mixed-emote-1",
                "@alice:example",
                1_700_000_300_000,
                RoomMessagePresentationKind::Emote,
                "waves hello",
            ),
            pending_entry_with_presentation(
                "!mixed:example",
                "txn-pending-1",
                "@alice:example",
                1_700_000_240_000,
                RoomMessagePresentationKind::Emote,
                "pending local echo",
            ),
            RoomActivity {
                room_id: "!mixed:example".to_owned(),
                activity_id: "txn:txn-failed-1".to_owned(),
                timestamp_ms: 1_700_000_300_000,
                relation: None,
                reactions: Vec::new(),
                kind: RoomActivityKind::Message(RoomMessageActivity {
                    event_id: None,
                    transaction_id: Some("txn-failed-1".to_owned()),
                    sender: "@alice:example".to_owned(),
                    presentation: RoomMessagePresentationKind::Emote,
                    body: "failed local echo".to_owned(),
                    send_state: RoomMessageSendState::Failed {
                        message: "send failed".to_owned(),
                        recoverable: true,
                    },
                }),
            },
        ]
    );

    let ui_state = attached.dump_ui_state()?;
    let mixed_view = find_view(&ui_state, &room_identity("!mixed:example"));
    assert!(view_contains(
        mixed_view,
        "[22:13] @alice:example: synced hello"
    ));
    assert!(view_contains(
        mixed_view,
        "* [22:14] @alice:example set topic: mixed room topic"
    ));
    assert!(view_contains(
        mixed_view,
        "[22:17] -@system:example- server maintenance"
    ));
    assert!(view_contains(
        mixed_view,
        "* [22:18] @alice:example waves hello"
    ));
    assert!(view_contains(
        mixed_view,
        "* [22:17] @alice:example pending local echo (sending)"
    ));
    assert!(view_contains(
        mixed_view,
        "* [22:18] @alice:example failed local echo (failed)"
    ));

    let _ = fs::remove_file(&state_path);
    Ok(())
}

#[test]
fn automation_driver_renders_room_typing_only_for_selected_room_view()
-> Result<(), Box<dyn std::error::Error>> {
    let exe_path = mmatrix_ui_exe_path()
        .map_err(|error| format!("failed to locate mmatrix-ui.exe: {error}"))?;
    let state_path = write_state_file(&[
        Event::Started,
        Event::SyncedRooms(vec![
            room_summary("!alpha:example", "Alpha", false),
            room_summary("!beta:example", "Beta", false),
        ]),
        text_event_with_event_id(
            "!alpha:example",
            "$alpha-context",
            "@carol:example.org",
            1_700_000_000_000,
            "alpha context",
        ),
        typing_event("!alpha:example", &["@alice:example.org"]),
    ])?;

    let args = [
        OsString::from("--render-state"),
        state_path.as_os_str().to_os_string(),
    ];
    let app = launch_app(&exe_path, &args)?;
    app.wait_idle(Duration::from_secs(5))?;

    let typing_line = "* @alice:example.org is typing";

    app.select_room("!beta:example")?;
    let beta_state = app.dump_ui_state()?;
    assert_eq!(beta_state.mdi.active, room_identity("!beta:example"));
    assert_active_switchbar_item(&beta_state, &room_identity("!beta:example"));
    assert!(!view_contains(
        find_view(&beta_state, &room_identity("!beta:example")),
        typing_line,
    ));

    app.select_room("!alpha:example")?;
    let alpha_state = app.dump_ui_state()?;
    assert_eq!(alpha_state.mdi.active, room_identity("!alpha:example"));
    assert_active_switchbar_item(&alpha_state, &room_identity("!alpha:example"));
    let alpha_view = find_view(&alpha_state, &room_identity("!alpha:example"));
    assert!(view_contains(alpha_view, "alpha context"));
    assert!(view_contains(alpha_view, typing_line));
    assert!(!view_contains(
        find_view(&alpha_state, &room_identity("!beta:example")),
        typing_line,
    ));
    assert_eq!(
        app.room_activity(Some("!alpha:example"))?,
        vec![text_entry_with_event_id(
            "!alpha:example",
            "$alpha-context",
            "@carol:example.org",
            1_700_000_000_000,
            "alpha context",
        )]
    );

    let _ = fs::remove_file(&state_path);
    Ok(())
}

#[test]
fn automation_driver_clears_room_typing_for_replacement_state_and_empty_user_list()
-> Result<(), Box<dyn std::error::Error>> {
    let exe_path = mmatrix_ui_exe_path()
        .map_err(|error| format!("failed to locate mmatrix-ui.exe: {error}"))?;
    let state_path = write_state_file(&[
        Event::Started,
        Event::SyncedRooms(vec![
            room_summary("!alpha:example", "Alpha", false),
            room_summary("!beta:example", "Beta", false),
        ]),
        text_event_with_event_id(
            "!alpha:example",
            "$alpha-context",
            "@carol:example.org",
            1_700_000_000_000,
            "alpha context",
        ),
        typing_event(
            "!alpha:example",
            &["@alice:example.org", "@bob:example.org"],
        ),
    ])?;

    let args = [
        OsString::from("--render-state"),
        state_path.as_os_str().to_os_string(),
    ];
    let app = launch_app(&exe_path, &args)?;
    app.wait_idle(Duration::from_secs(5))?;

    let typing_line = "* @alice:example.org, @bob:example.org are typing";

    app.select_room("!alpha:example")?;
    let initial_state = app.dump_ui_state()?;
    assert!(view_contains(
        find_view(&initial_state, &room_identity("!alpha:example")),
        typing_line,
    ));

    app.load_events(&serde_json::to_string(&vec![
        Event::Started,
        Event::SyncedRooms(vec![
            room_summary("!alpha:example", "Alpha", false),
            room_summary("!beta:example", "Beta", false),
        ]),
        text_event_with_event_id(
            "!alpha:example",
            "$alpha-context",
            "@carol:example.org",
            1_700_000_000_000,
            "alpha context",
        ),
    ])?)?;
    app.wait_idle(Duration::from_secs(5))?;
    app.select_room("!alpha:example")?;
    let replaced_state = app.dump_ui_state()?;
    let replaced_alpha_view = find_view(&replaced_state, &room_identity("!alpha:example"));
    assert!(view_contains(replaced_alpha_view, "alpha context"));
    assert!(!view_contains(replaced_alpha_view, typing_line));
    assert!(!view_contains(replaced_alpha_view, " is typing"));
    assert!(!view_contains(replaced_alpha_view, " are typing"));

    app.load_events(&serde_json::to_string(&vec![
        Event::Started,
        Event::SyncedRooms(vec![
            room_summary("!alpha:example", "Alpha", false),
            room_summary("!beta:example", "Beta", false),
        ]),
        text_event_with_event_id(
            "!alpha:example",
            "$alpha-context",
            "@carol:example.org",
            1_700_000_000_000,
            "alpha context",
        ),
        typing_event(
            "!alpha:example",
            &["@alice:example.org", "@bob:example.org"],
        ),
    ])?)?;
    app.wait_idle(Duration::from_secs(5))?;
    app.select_room("!alpha:example")?;
    let restored_state = app.dump_ui_state()?;
    assert!(view_contains(
        find_view(&restored_state, &room_identity("!alpha:example")),
        typing_line,
    ));

    app.load_events(&serde_json::to_string(&vec![
        Event::Started,
        Event::SyncedRooms(vec![
            room_summary("!alpha:example", "Alpha", false),
            room_summary("!beta:example", "Beta", false),
        ]),
        text_event_with_event_id(
            "!alpha:example",
            "$alpha-context",
            "@carol:example.org",
            1_700_000_000_000,
            "alpha context",
        ),
        typing_event("!alpha:example", &[]),
    ])?)?;
    app.wait_idle(Duration::from_secs(5))?;
    app.select_room("!alpha:example")?;
    let cleared_state = app.dump_ui_state()?;
    let cleared_alpha_view = find_view(&cleared_state, &room_identity("!alpha:example"));
    assert!(view_contains(cleared_alpha_view, "alpha context"));
    assert!(!view_contains(cleared_alpha_view, typing_line));
    assert!(!view_contains(cleared_alpha_view, " is typing"));
    assert!(!view_contains(cleared_alpha_view, " are typing"));

    let _ = fs::remove_file(&state_path);
    Ok(())
}

#[test]
fn automation_driver_clears_room_typing_on_logged_out_event()
-> Result<(), Box<dyn std::error::Error>> {
    let exe_path = mmatrix_ui_exe_path()
        .map_err(|error| format!("failed to locate mmatrix-ui.exe: {error}"))?;
    let state_path = write_state_file(&[
        Event::Started,
        Event::SyncedRooms(vec![room_summary("!alpha:example", "Alpha", false)]),
        text_event_with_event_id(
            "!alpha:example",
            "$alpha-context",
            "@carol:example.org",
            1_700_000_000_000,
            "alpha context",
        ),
        typing_event(
            "!alpha:example",
            &["@alice:example.org", "@bob:example.org"],
        ),
    ])?;

    let args = [
        OsString::from("--render-state"),
        state_path.as_os_str().to_os_string(),
    ];
    let app = launch_app(&exe_path, &args)?;
    app.wait_idle(Duration::from_secs(5))?;

    app.select_room("!alpha:example")?;
    let initial_state = app.dump_ui_state()?;
    assert!(view_contains(
        find_view(&initial_state, &room_identity("!alpha:example")),
        "* @alice:example.org, @bob:example.org are typing",
    ));

    app.load_events(&serde_json::to_string(&vec![
        Event::Started,
        Event::LoggedOut,
    ])?)?;
    app.wait_idle(Duration::from_secs(5))?;

    let model = app.dump_model()?;
    assert_eq!(model.status, "logged out");
    assert_eq!(model.selected_room, None);
    assert!(model.rooms.is_empty());
    let ui_state = app.dump_ui_state()?;
    assert_eq!(ui_state.mdi.active, ViewIdentity::Status);
    assert_active_switchbar_item(&ui_state, &ViewIdentity::Status);
    let status_view = find_view(&ui_state, &ViewIdentity::Status);
    assert!(!view_contains(status_view, " is typing"));
    assert!(!view_contains(status_view, " are typing"));

    let _ = fs::remove_file(&state_path);
    Ok(())
}

#[test]
fn automation_driver_loads_typing_indicator_fixture_via_file_api()
-> Result<(), Box<dyn std::error::Error>> {
    let exe_path = mmatrix_ui_exe_path()
        .map_err(|error| format!("failed to locate mmatrix-ui.exe: {error}"))?;
    let state_path = write_state_file(&base_events())?;

    let args = [
        OsString::from("--render-state"),
        state_path.as_os_str().to_os_string(),
    ];
    let app = launch_app(&exe_path, &args)?;
    app.wait_idle(Duration::from_secs(5))?;

    let attached = MmatrixApp::find_by_pid(app.pid(), Duration::from_secs(5))?;
    let fixture_path = fixture_state_path("typing-indicator.json");
    attached.load_events_from_file(&fixture_path)?;
    attached.wait_idle(Duration::from_secs(5))?;

    assert_eq!(
        attached.list_rooms()?,
        vec![listed_room("!typing:example", "Typing", false)]
    );
    assert_eq!(
        attached.dump_model()?.selected_room.as_deref(),
        Some("!typing:example")
    );
    assert_eq!(
        attached.room_activity(None)?,
        vec![text_entry_with_event_id(
            "!typing:example",
            "$typing-context",
            "@carol:example",
            1_700_000_000_000,
            "typing context",
        )]
    );

    let ui_state = attached.dump_ui_state()?;
    assert_eq!(ui_state.mdi.active, room_identity("!typing:example"));
    assert_active_switchbar_item(&ui_state, &room_identity("!typing:example"));
    let typing_view = find_view(&ui_state, &room_identity("!typing:example"));
    assert!(view_contains(typing_view, "typing context"));
    assert!(view_contains(
        typing_view,
        "* @alice:example, @bob:example are typing",
    ));

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
    let app = launch_app(&exe_path, &args)?;
    app.wait_idle(Duration::from_secs(5))?;

    app.select_status_view()?;
    let status_state = app.dump_ui_state()?;
    assert_eq!(status_state.mdi.active, ViewIdentity::Status);
    assert_active_switchbar_item(&status_state, &ViewIdentity::Status);
    assert_eq!(app.dump_model()?.selected_room, None);

    app.select_room("!verify:example")?;
    let room_state = app.dump_ui_state()?;
    assert_eq!(room_state.mdi.active, room_identity("!verify:example"));
    assert_active_switchbar_item(&room_state, &room_identity("!verify:example"));
    assert_eq!(
        app.dump_model()?.selected_room.as_deref(),
        Some("!verify:example")
    );

    Ok(())
}

#[test]
fn automation_driver_switchbar_command_activates_mdi_tree_and_focus()
-> Result<(), Box<dyn std::error::Error>> {
    let exe_path = mmatrix_ui_exe_path()
        .map_err(|error| format!("failed to locate mmatrix-ui.exe: {error}"))?;
    let fixture_path = fixture_state_path("thread-hierarchy.json");
    let args = [OsString::from("--automation")];
    let app = launch_app(&exe_path, &args)?;
    app.wait_for_lifecycle_state("core_started", Duration::from_secs(30))?;
    app.load_events(&fs::read_to_string(&fixture_path)?)?;
    app.wait_idle(Duration::from_secs(5))?;

    let initial_state = app.dump_ui_state()?;
    let room_path = navigation_path_by_substrings(
        &initial_state.navigation,
        &["mmatrix", "rooms: 1", "Work", "Ops"],
    );
    let thread_path = navigation_path_by_substrings(
        &initial_state.navigation,
        &["mmatrix", "rooms: 1", "Work", "Ops", "root msg"],
    );
    let room_view_identity = room_identity("!ops:example");
    let thread_view_identity = thread_identity("!ops:example", "$thread-root");

    app.select_navigation_path(&thread_path)?;
    app.wait_idle(Duration::from_secs(5))?;

    activate_switchbar_item_by_command(&app, &ViewIdentity::Status)?;
    let status_state = app.dump_ui_state()?;
    assert_eq!(status_state.mdi.active, ViewIdentity::Status);
    assert_active_switchbar_item(&status_state, &ViewIdentity::Status);
    assert_eq!(
        app.selected_navigation_path()?,
        nav_path(&["mmatrix", "status"])
    );
    assert_compose_has_focus(&app);

    activate_switchbar_item_by_command(&app, &room_view_identity)?;
    let room_state = app.dump_ui_state()?;
    assert_eq!(room_state.mdi.active, room_view_identity);
    assert_active_switchbar_item(&room_state, &room_view_identity);
    assert_eq!(app.selected_navigation_path()?, room_path);
    assert_compose_has_focus(&app);

    activate_switchbar_item_by_command(&app, &thread_view_identity)?;
    let thread_state = app.dump_ui_state()?;
    assert_eq!(thread_state.mdi.active, thread_view_identity);
    assert_active_switchbar_item(&thread_state, &thread_view_identity);
    assert_eq!(app.selected_navigation_path()?, thread_path);
    assert_compose_has_focus(&app);

    Ok(())
}

#[test]
fn automation_driver_activation_returns_keyboard_to_compose()
-> Result<(), Box<dyn std::error::Error>> {
    let exe_path = mmatrix_ui_exe_path()
        .map_err(|error| format!("failed to locate mmatrix-ui.exe: {error}"))?;
    let local_app_data = unique_local_app_data_path();
    let options = launch_options(Some(local_app_data.clone()));
    let args = [OsString::from("--automation")];

    let app = launch_app_with_options(&exe_path, &args, options)?;
    app.wait_for_lifecycle_state("core_started", Duration::from_secs(30))?;
    app.load_events(&serde_json::to_string(&base_events())?)?;
    app.wait_idle(Duration::from_secs(5))?;

    let initial_state = app.dump_ui_state()?;
    let alpha_path =
        navigation_path_by_substrings(&initial_state.navigation, &["mmatrix", "rooms: 2", "Alpha"]);

    app.select_navigation_path(&alpha_path)?;
    app.wait_idle(Duration::from_secs(5))?;
    assert_eq!(
        app.dump_ui_state()?.mdi.active,
        room_identity("!alpha:example")
    );
    assert_compose_has_focus(&app);

    activate_switchbar_item_by_command(&app, &ViewIdentity::Status)?;
    assert_eq!(app.dump_ui_state()?.mdi.active, ViewIdentity::Status);
    assert_compose_has_focus(&app);

    app.set_compose_text("/window !beta:example")?;
    app.send_compose_key(ComposeKey::Return)?;
    app.wait_idle(Duration::from_secs(5))?;
    assert_eq!(app.compose_text()?, "");
    assert_eq!(
        app.dump_ui_state()?.mdi.active,
        room_identity("!beta:example")
    );
    assert_compose_has_focus(&app);

    drop(app);
    let _ = fs::remove_dir_all(&local_app_data);
    Ok(())
}

#[test]
fn automation_driver_clears_room_activity_when_inactive_room_is_activated()
-> Result<(), Box<dyn std::error::Error>> {
    let exe_path = mmatrix_ui_exe_path()
        .map_err(|error| format!("failed to locate mmatrix-ui.exe: {error}"))?;
    let state_path = write_state_file(&base_events())?;

    let args = [
        OsString::from("--render-state"),
        state_path.as_os_str().to_os_string(),
    ];
    let app = launch_app(&exe_path, &args)?;
    app.wait_idle(Duration::from_secs(5))?;

    let initial_state = app.dump_ui_state()?;
    assert_switchbar_activity(&initial_state, &room_identity("!alpha:example"), true);

    app.select_room("!alpha:example")?;
    let activated_state = app.dump_ui_state()?;
    assert_active_switchbar_item(&activated_state, &room_identity("!alpha:example"));
    assert_switchbar_activity(&activated_state, &room_identity("!alpha:example"), false);

    let _ = fs::remove_file(&state_path);
    Ok(())
}

#[test]
fn automation_driver_clears_tree_and_switchbar_activity_markers_on_activation()
-> Result<(), Box<dyn std::error::Error>> {
    let exe_path = mmatrix_ui_exe_path()
        .map_err(|error| format!("failed to locate mmatrix-ui.exe: {error}"))?;
    let fixture_path = fixture_state_path("shell-activity.json");
    let update_path = fixture_update_path("shell-activity.json");
    let args = [OsString::from("--automation")];
    let app = launch_app(&exe_path, &args)?;
    app.wait_for_lifecycle_state("core_started", Duration::from_secs(30))?;
    app.load_events(&fs::read_to_string(&fixture_path)?)?;
    app.wait_idle(Duration::from_secs(5))?;
    let loaded_state = app.dump_ui_state()?;
    let alpha_room_path =
        navigation_path_by_substrings(&loaded_state.navigation, &["mmatrix", "rooms: 2", "Alpha"]);
    let beta_room_path =
        navigation_path_by_substrings(&loaded_state.navigation, &["mmatrix", "rooms: 2", "Beta"]);
    let alpha_thread_path = navigation_path_by_substrings(
        &loaded_state.navigation,
        &["mmatrix", "rooms: 2", "Alpha", "alpha root"],
    );
    app.select_navigation_path(&alpha_thread_path)?;
    app.wait_idle(Duration::from_secs(5))?;
    app.select_navigation_path(&alpha_room_path)?;
    app.wait_idle(Duration::from_secs(5))?;
    app.append_synthetic_events(&fs::read_to_string(&update_path)?)?;
    app.wait_idle(Duration::from_secs(5))?;

    let room_view_identity = room_identity("!alpha:example");
    let beta_view_identity = room_identity("!beta:example");
    let thread_view_identity = thread_identity("!alpha:example", "$alpha-thread-root");
    let initial_state = app.dump_ui_state()?;

    assert_eq!(initial_state.mdi.active, room_view_identity);
    assert_active_switchbar_item(&initial_state, &room_view_identity);
    assert_eq!(app.selected_navigation_path()?, alpha_room_path);
    assert_switchbar_activity(&initial_state, &beta_view_identity, true);
    assert_navigation_activity(&initial_state, &["mmatrix", "rooms: 2", "Beta"], true);
    assert_switchbar_activity(&initial_state, &thread_view_identity, true);
    assert_navigation_activity(
        &initial_state,
        &["mmatrix", "rooms: 2", "Alpha", "alpha root"],
        true,
    );
    assert_switchbar_activity(&initial_state, &room_view_identity, false);
    assert_navigation_activity(&initial_state, &["mmatrix", "rooms: 2", "Alpha"], false);

    app.select_navigation_path(&alpha_thread_path)?;
    app.wait_idle(Duration::from_secs(5))?;
    let thread_state = app.dump_ui_state()?;
    assert_eq!(thread_state.mdi.active, thread_view_identity);
    assert_active_switchbar_item(&thread_state, &thread_view_identity);
    assert_eq!(app.selected_navigation_path()?, alpha_thread_path);
    assert_switchbar_activity(&thread_state, &thread_view_identity, false);
    assert_navigation_activity(
        &thread_state,
        &["mmatrix", "rooms: 2", "Alpha", "alpha root"],
        false,
    );
    assert_switchbar_activity(&thread_state, &beta_view_identity, true);
    assert_navigation_activity(&thread_state, &["mmatrix", "rooms: 2", "Beta"], true);
    assert_switchbar_activity(&thread_state, &room_view_identity, false);
    assert_navigation_activity(&thread_state, &["mmatrix", "rooms: 2", "Alpha"], false);

    app.select_navigation_path(&beta_room_path)?;
    app.wait_idle(Duration::from_secs(5))?;
    let beta_state = app.dump_ui_state()?;
    assert_eq!(beta_state.mdi.active, beta_view_identity);
    assert_active_switchbar_item(&beta_state, &beta_view_identity);
    assert_eq!(app.selected_navigation_path()?, beta_room_path);
    assert_switchbar_activity(&beta_state, &beta_view_identity, false);
    assert_navigation_activity(&beta_state, &["mmatrix", "rooms: 2", "Beta"], false);
    assert_switchbar_activity(&beta_state, &thread_view_identity, false);
    assert_navigation_activity(
        &beta_state,
        &["mmatrix", "rooms: 2", "Alpha", "alpha root"],
        false,
    );
    assert_switchbar_activity(&beta_state, &room_view_identity, false);
    assert_navigation_activity(&beta_state, &["mmatrix", "rooms: 2", "Alpha"], false);

    Ok(())
}

#[test]
fn automation_driver_native_mdi_activation_reconciles_shell_state()
-> Result<(), Box<dyn std::error::Error>> {
    let exe_path = mmatrix_ui_exe_path()
        .map_err(|error| format!("failed to locate mmatrix-ui.exe: {error}"))?;
    let fixture_path = fixture_state_path("shell-activity.json");
    let update_path = fixture_update_path("shell-activity.json");
    let args = [OsString::from("--automation")];
    let app = launch_app(&exe_path, &args)?;
    app.wait_for_lifecycle_state("core_started", Duration::from_secs(30))?;
    app.load_events(&fs::read_to_string(&fixture_path)?)?;
    app.wait_idle(Duration::from_secs(5))?;

    let loaded_state = app.dump_ui_state()?;
    let alpha_room_path =
        navigation_path_by_substrings(&loaded_state.navigation, &["mmatrix", "rooms: 2", "Alpha"]);
    let beta_room_path =
        navigation_path_by_substrings(&loaded_state.navigation, &["mmatrix", "rooms: 2", "Beta"]);
    let alpha_thread_path = navigation_path_by_substrings(
        &loaded_state.navigation,
        &["mmatrix", "rooms: 2", "Alpha", "alpha root"],
    );

    app.select_navigation_path(&alpha_thread_path)?;
    app.wait_idle(Duration::from_secs(5))?;
    app.select_navigation_path(&alpha_room_path)?;
    app.wait_idle(Duration::from_secs(5))?;
    app.append_synthetic_events(&fs::read_to_string(&update_path)?)?;
    app.wait_idle(Duration::from_secs(5))?;

    let alpha_room_identity = room_identity("!alpha:example");
    let beta_room_identity = room_identity("!beta:example");
    let alpha_thread_identity = thread_identity("!alpha:example", "$alpha-thread-root");

    let initial_state = app.dump_ui_state()?;
    assert_eq!(initial_state.mdi.active, alpha_room_identity);
    assert_active_switchbar_item(&initial_state, &alpha_room_identity);
    assert_eq!(app.selected_navigation_path()?, alpha_room_path);
    assert_switchbar_activity(&initial_state, &beta_room_identity, true);
    assert_navigation_activity(&initial_state, &["mmatrix", "rooms: 2", "Beta"], true);
    assert_switchbar_activity(&initial_state, &alpha_thread_identity, true);
    assert_navigation_activity(
        &initial_state,
        &["mmatrix", "rooms: 2", "Alpha", "alpha root"],
        true,
    );
    assert_switchbar_activity(&initial_state, &alpha_room_identity, false);
    assert_navigation_activity(&initial_state, &["mmatrix", "rooms: 2", "Alpha"], false);
    assert_raw_switchbar_state(&app, &initial_state, &alpha_room_identity, true, false);
    assert_raw_switchbar_state(&app, &initial_state, &beta_room_identity, false, true);
    assert_raw_switchbar_state(&app, &initial_state, &alpha_thread_identity, false, true);

    native_activate_mdi_view(&app, &alpha_thread_identity)?;
    let thread_state = app.dump_ui_state()?;
    assert_eq!(thread_state.mdi.active, alpha_thread_identity);
    assert_active_switchbar_item(&thread_state, &alpha_thread_identity);
    assert_eq!(app.selected_navigation_path()?, alpha_thread_path);
    assert_compose_has_focus(&app);
    assert_switchbar_activity(&thread_state, &alpha_thread_identity, false);
    assert_navigation_activity(
        &thread_state,
        &["mmatrix", "rooms: 2", "Alpha", "alpha root"],
        false,
    );
    assert_switchbar_activity(&thread_state, &beta_room_identity, true);
    assert_navigation_activity(&thread_state, &["mmatrix", "rooms: 2", "Beta"], true);
    assert_switchbar_activity(&thread_state, &alpha_room_identity, false);
    assert_navigation_activity(&thread_state, &["mmatrix", "rooms: 2", "Alpha"], false);
    assert_raw_switchbar_state(&app, &thread_state, &alpha_thread_identity, true, false);
    assert_raw_switchbar_state(&app, &thread_state, &beta_room_identity, false, true);

    native_activate_mdi_view(&app, &beta_room_identity)?;
    let beta_state = app.dump_ui_state()?;
    assert_eq!(beta_state.mdi.active, beta_room_identity);
    assert_active_switchbar_item(&beta_state, &beta_room_identity);
    assert_eq!(app.selected_navigation_path()?, beta_room_path);
    assert_compose_has_focus(&app);
    assert_switchbar_activity(&beta_state, &beta_room_identity, false);
    assert_navigation_activity(&beta_state, &["mmatrix", "rooms: 2", "Beta"], false);
    assert_switchbar_activity(&beta_state, &alpha_thread_identity, false);
    assert_navigation_activity(
        &beta_state,
        &["mmatrix", "rooms: 2", "Alpha", "alpha root"],
        false,
    );
    assert_switchbar_activity(&beta_state, &alpha_room_identity, false);
    assert_navigation_activity(&beta_state, &["mmatrix", "rooms: 2", "Alpha"], false);
    assert_raw_switchbar_state(&app, &beta_state, &beta_room_identity, true, false);

    Ok(())
}

#[test]
fn automation_driver_render_state_append_records_room_read_target()
-> Result<(), Box<dyn std::error::Error>> {
    let exe_path = mmatrix_ui_exe_path()
        .map_err(|error| format!("failed to locate mmatrix-ui.exe: {error}"))?;
    let fixture_path = fixture_state_path("shell-activity.json");
    let update_path = fixture_update_path("shell-activity.json");
    let args = [
        OsString::from("--render-state"),
        fixture_path.as_os_str().to_os_string(),
    ];
    let app = launch_app(&exe_path, &args)?;
    app.wait_idle(Duration::from_secs(5))?;

    let loaded_state = app.dump_ui_state()?;
    let alpha_room_path =
        navigation_path_by_substrings(&loaded_state.navigation, &["mmatrix", "rooms: 2", "Alpha"]);
    let alpha_thread_path = navigation_path_by_substrings(
        &loaded_state.navigation,
        &["mmatrix", "rooms: 2", "Alpha", "alpha root"],
    );

    app.select_navigation_path(&alpha_thread_path)?;
    app.wait_idle(Duration::from_secs(5))?;
    app.select_navigation_path(&alpha_room_path)?;
    app.wait_idle(Duration::from_secs(5))?;
    app.append_synthetic_events(&fs::read_to_string(&update_path)?)?;
    app.wait_idle(Duration::from_secs(5))?;

    let updated_state = app.dump_ui_state()?;
    assert_eq!(
        find_ui_room(&updated_state, "!alpha:example").read_target_event_id,
        None
    );
    assert_eq!(
        find_ui_room(&updated_state, "!beta:example")
            .read_target_event_id
            .as_deref(),
        Some("$beta-activity")
    );

    Ok(())
}

#[test]
fn automation_driver_renders_shell_constrained_layout_fixture()
-> Result<(), Box<dyn std::error::Error>> {
    let exe_path = mmatrix_ui_exe_path()
        .map_err(|error| format!("failed to locate mmatrix-ui.exe: {error}"))?;
    let fixture_path = fixture_state_path("shell-constrained-layout.json");
    let frame_size = FrameSize {
        width: 900,
        height: 620,
    };
    let options = LaunchOptions {
        frame_size: Some(frame_size),
        ..launch_options(None)
    };
    assert_eq!(options.frame_size, Some(frame_size));

    let args = [
        OsString::from("--render-state"),
        fixture_path.as_os_str().to_os_string(),
    ];
    let app = launch_app_with_options(&exe_path, &args, options)?;
    app.wait_idle(Duration::from_secs(5))?;
    let frame = app.automation_frame_hwnd() as HWND;
    let rect = unsafe { window_rect(frame) };
    assert_eq!(rect.right - rect.left, 900);
    assert_eq!(rect.bottom - rect.top, 620);

    let ui_state = app.dump_ui_state()?;
    assert_eq!(ui_state.mdi.active, room_identity("!focus:example"));
    assert_active_switchbar_item(&ui_state, &room_identity("!focus:example"));
    assert_eq!(
        app.dump_model()?.selected_room.as_deref(),
        Some("!focus:example")
    );
    assert!(navigation_contains(&ui_state.navigation, "Focus"));
    assert!(navigation_contains(&ui_state.navigation, "Inbox"));
    assert!(navigation_contains(&ui_state.navigation, "Docs"));

    let switchbar_identities = ui_state
        .switchbar
        .items
        .iter()
        .map(|item| &item.identity)
        .collect::<Vec<_>>();
    assert_eq!(
        switchbar_identities,
        vec![
            &ViewIdentity::Status,
            &room_identity("!focus:example"),
            &room_identity("!inbox:example"),
            &room_identity("!docs:example"),
        ]
    );
    assert_eq!(
        find_switchbar_item(&ui_state, &ViewIdentity::Status).title,
        "Status"
    );
    assert_eq!(
        find_switchbar_item(&ui_state, &room_identity("!focus:example")).title,
        "Focus"
    );
    assert_eq!(
        find_switchbar_item(&ui_state, &room_identity("!inbox:example")).title,
        "Inbox"
    );
    assert_eq!(
        find_switchbar_item(&ui_state, &room_identity("!docs:example")).title,
        "Docs"
    );

    let focus_view = find_view(&ui_state, &room_identity("!focus:example"));
    assert!(view_contains(
        focus_view,
        "long visible timeline line for the 900x620 shell frame so the MDI content remains obvious"
    ));
    assert!(view_members_contain(focus_view, "Alice"));
    assert!(view_members_contain(focus_view, "Casey"));
    assert!(
        ui_state
            .status_bar
            .text
            .contains("timeline update from !inbox:example")
    );

    Ok(())
}

#[test]
fn automation_driver_applies_room_read_state_fixture_activity_and_clears_it_on_activation()
-> Result<(), Box<dyn std::error::Error>> {
    let exe_path = mmatrix_ui_exe_path()
        .map_err(|error| format!("failed to locate mmatrix-ui.exe: {error}"))?;
    let fixture_path = fixture_state_path("read-state-ingestion.json");

    let args = [
        OsString::from("--render-state"),
        fixture_path.as_os_str().to_os_string(),
    ];
    let app = launch_app(&exe_path, &args)?;
    app.wait_idle(Duration::from_secs(5))?;

    assert_eq!(
        app.list_rooms()?,
        vec![
            listed_room("!beta:example", "Beta", false),
            listed_room_with_metadata(
                "!alpha:example",
                "Alpha",
                false,
                true,
                vec!["@alice:example"],
                vec![room_tag("m.favourite", Some("0.25"))],
            ),
        ]
    );

    let initial_state = app.dump_ui_state()?;
    assert_eq!(initial_state.mdi.active, room_identity("!beta:example"));
    assert_active_switchbar_item(&initial_state, &room_identity("!beta:example"));
    assert_eq!(
        app.dump_model()?.selected_room.as_deref(),
        Some("!beta:example")
    );
    assert_eq!(
        find_switchbar_item(&initial_state, &room_identity("!beta:example")).title,
        "Beta"
    );
    assert_eq!(
        find_switchbar_item(&initial_state, &room_identity("!alpha:example")).title,
        "Alpha [query: @alice:example] [@1 !2 *1 M]"
    );
    assert_switchbar_activity(&initial_state, &room_identity("!beta:example"), false);
    assert_switchbar_activity(&initial_state, &room_identity("!alpha:example"), true);
    assert!(navigation_contains(
        &initial_state.navigation,
        "Alpha [query: @alice:example] [@1 !2 *1 M]"
    ));
    assert_eq!(
        find_ui_room(&initial_state, "!beta:example"),
        &UiRoomState {
            room_id: "!beta:example".to_owned(),
            display_name: "Beta".to_owned(),
            membership: RoomMembership::Join,
            encrypted: false,
            is_space: false,
            is_direct: false,
            direct_user_ids: Vec::new(),
            tags: Vec::new(),
            space_children: Vec::new(),
            has_activity: false,
            read_target_event_id: None,
            read_state: Some(UiRoomReadState {
                latest_active_event_id: Some("$beta-latest".to_owned()),
                fully_read_event_id: Some("$beta-latest".to_owned()),
                marked_unread: false,
                num_unread: 0,
                num_notifications: 0,
                num_mentions: 0,
            }),
            history_backfill: UiRoomHistoryBackfillState::Idle,
        }
    );
    assert_eq!(
        find_ui_room(&initial_state, "!alpha:example"),
        &UiRoomState {
            room_id: "!alpha:example".to_owned(),
            display_name: "Alpha".to_owned(),
            membership: RoomMembership::Join,
            encrypted: false,
            is_space: false,
            is_direct: true,
            direct_user_ids: vec!["@alice:example".to_owned()],
            tags: vec![room_tag("m.favourite", Some("0.25"))],
            space_children: Vec::new(),
            has_activity: true,
            read_target_event_id: None,
            read_state: Some(UiRoomReadState {
                latest_active_event_id: Some("$alpha-latest".to_owned()),
                fully_read_event_id: Some("$alpha-fully-read".to_owned()),
                marked_unread: true,
                num_unread: 1,
                num_notifications: 2,
                num_mentions: 1,
            }),
            history_backfill: UiRoomHistoryBackfillState::Idle,
        }
    );

    app.select_room("!alpha:example")?;
    let activated_state = app.dump_ui_state()?;
    assert_eq!(activated_state.mdi.active, room_identity("!alpha:example"));
    assert_active_switchbar_item(&activated_state, &room_identity("!alpha:example"));
    assert_eq!(
        app.dump_model()?.selected_room.as_deref(),
        Some("!alpha:example")
    );
    assert_eq!(
        find_switchbar_item(&activated_state, &room_identity("!alpha:example")).title,
        "Alpha [query: @alice:example] [@1 !2 *1 M]"
    );
    assert_switchbar_activity(&activated_state, &room_identity("!alpha:example"), false);
    assert!(navigation_contains(
        &activated_state.navigation,
        "Alpha [query: @alice:example] [@1 !2 *1 M]"
    ));
    assert_eq!(
        find_ui_room(&activated_state, "!alpha:example"),
        &UiRoomState {
            room_id: "!alpha:example".to_owned(),
            display_name: "Alpha".to_owned(),
            membership: RoomMembership::Join,
            encrypted: false,
            is_space: false,
            is_direct: true,
            direct_user_ids: vec!["@alice:example".to_owned()],
            tags: vec![room_tag("m.favourite", Some("0.25"))],
            space_children: Vec::new(),
            has_activity: false,
            read_target_event_id: None,
            read_state: Some(UiRoomReadState {
                latest_active_event_id: Some("$alpha-latest".to_owned()),
                fully_read_event_id: Some("$alpha-fully-read".to_owned()),
                marked_unread: true,
                num_unread: 1,
                num_notifications: 2,
                num_mentions: 1,
            }),
            history_backfill: UiRoomHistoryBackfillState::Idle,
        }
    );

    Ok(())
}

#[test]
fn automation_driver_caught_up_read_state_fixture_clears_navigation_suffixes()
-> Result<(), Box<dyn std::error::Error>> {
    let exe_path = mmatrix_ui_exe_path()
        .map_err(|error| format!("failed to locate mmatrix-ui.exe: {error}"))?;
    let fixture_path = fixture_state_path("read-state-ingestion.json");
    let update_path = fixture_update_path("read-state-cleared.json");
    let mut events: Vec<Event> = serde_json::from_str(&fs::read_to_string(&fixture_path)?)?;
    let update_events: Vec<Event> = serde_json::from_str(&fs::read_to_string(&update_path)?)?;
    events.extend(update_events);
    let state_path = write_state_file(&events)?;

    let args = [
        OsString::from("--render-state"),
        state_path.as_os_str().to_os_string(),
    ];
    let app = launch_app(&exe_path, &args)?;
    app.wait_idle(Duration::from_secs(5))?;
    let cleared_state = app.dump_ui_state()?;

    assert_eq!(
        find_ui_room(&cleared_state, "!alpha:example").read_state,
        Some(UiRoomReadState {
            latest_active_event_id: Some("$alpha-latest".to_owned()),
            fully_read_event_id: Some("$alpha-latest".to_owned()),
            marked_unread: false,
            num_unread: 0,
            num_notifications: 0,
            num_mentions: 0,
        })
    );
    assert_eq!(
        find_switchbar_item(&cleared_state, &room_identity("!alpha:example")).title,
        "Alpha [query: @alice:example]"
    );
    assert!(navigation_contains(
        &cleared_state.navigation,
        "Alpha [query: @alice:example]"
    ));
    assert_navigation_not_contains(
        &cleared_state.navigation,
        "Alpha [query: @alice:example] [@1 !2 *1 M]",
    );

    let _ = fs::remove_file(&state_path);
    Ok(())
}

#[test]
fn automation_driver_renders_image_message_fixture_and_exposes_typed_activity()
-> Result<(), Box<dyn std::error::Error>> {
    let exe_path = mmatrix_ui_exe_path()
        .map_err(|error| format!("failed to locate mmatrix-ui.exe: {error}"))?;
    let fixture_path = fixture_state_path("image-message.json");

    let args = [
        OsString::from("--render-state"),
        fixture_path.as_os_str().to_os_string(),
    ];
    let app = launch_app(&exe_path, &args)?;
    app.wait_idle(Duration::from_secs(5))?;

    let initial_state = app.dump_ui_state()?;
    assert_eq!(
        initial_state.mdi.active,
        room_identity("!images:example.org")
    );
    assert_active_switchbar_item(&initial_state, &room_identity("!images:example.org"));
    let initial_model = app.dump_model()?;
    assert_eq!(
        initial_model.selected_room.as_deref(),
        Some("!images:example.org")
    );
    assert_eq!(
        initial_model.last_activity,
        Some(RoomMessageActivityLine {
            room_id: "!images:example.org".to_owned(),
            activity_id: "event:$image-typed:example.org".to_owned(),
            timestamp_ms: 1_710_000_000_000,
            event_id: Some("$image-typed:example.org".to_owned()),
            transaction_id: None,
            sender: "@alice:example.org".to_owned(),
            presentation: RoomMessagePresentationKind::Text,
            body: "[image] diagram.png".to_owned(),
            send_state: RoomMessageSendState::Sent,
        })
    );
    assert_switchbar_activity(&initial_state, &room_identity("!images:example.org"), false);
    assert_eq!(
        app.room_activity(Some("!images:example.org"))?,
        vec![image_entry_with_event_id(
            "!images:example.org",
            1_710_000_000_000,
            fixture_image_activity(Some(RoomImageMediaState::Pending)),
        )]
    );

    let image_view = find_view(&initial_state, &room_identity("!images:example.org"));
    assert!(view_contains(
        image_view,
        "* [16:00] @alice:example.org sent image: diagram.png (image/png, 640x480, 12345 bytes)",
    ));
    assert!(!view_contains(image_view, "unsupported m.image"));

    Ok(())
}

#[test]
fn automation_driver_renders_file_message_fixture_and_exposes_typed_activity()
-> Result<(), Box<dyn std::error::Error>> {
    let exe_path = mmatrix_ui_exe_path()
        .map_err(|error| format!("failed to locate mmatrix-ui.exe: {error}"))?;
    let fixture_path = fixture_state_path("file-message.json");

    let args = [
        OsString::from("--render-state"),
        fixture_path.as_os_str().to_os_string(),
    ];
    let app = launch_app(&exe_path, &args)?;
    app.wait_idle(Duration::from_secs(5))?;

    let initial_state = app.dump_ui_state()?;
    assert_eq!(
        initial_state.mdi.active,
        room_identity("!media:example.org")
    );
    assert_active_switchbar_item(&initial_state, &room_identity("!media:example.org"));
    let initial_model = app.dump_model()?;
    assert_eq!(
        initial_model.selected_room.as_deref(),
        Some("!media:example.org")
    );
    assert_eq!(
        initial_model.last_activity,
        Some(RoomMessageActivityLine {
            room_id: "!media:example.org".to_owned(),
            activity_id: "event:$file1:example.org".to_owned(),
            timestamp_ms: 1_710_000_000_000,
            event_id: Some("$file1:example.org".to_owned()),
            transaction_id: None,
            sender: "@alice:example.org".to_owned(),
            presentation: RoomMessagePresentationKind::Text,
            body: "[file] report.pdf".to_owned(),
            send_state: RoomMessageSendState::Sent,
        })
    );
    assert_switchbar_activity(&initial_state, &room_identity("!media:example.org"), false);
    let media_activity = app.room_activity(Some("!media:example.org"))?;
    assert_eq!(
        media_activity,
        vec![file_entry_with_event_id(
            "!media:example.org",
            1_710_000_000_000,
            fixture_file_activity(),
        )]
    );
    let RoomActivityKind::File(activity) = &media_activity[0].kind else {
        panic!("expected typed file activity");
    };
    assert_eq!(activity.event_id, "$file1:example.org");
    assert_eq!(activity.sender, "@alice:example.org");
    assert_eq!(activity.body, "Quarterly report");
    assert_eq!(activity.filename.as_deref(), Some("report.pdf"));
    assert_eq!(
        activity.source,
        RoomMediaSource::Plain {
            url: "mxc://example.org/report".to_owned(),
        }
    );
    assert_eq!(
        activity.info,
        Some(RoomFileInfo {
            mimetype: Some("application/pdf".to_owned()),
            size: Some(45_678),
        })
    );
    assert!(
        media_activity
            .iter()
            .all(|entry| !entry.contains("unsupported"))
    );

    let media_view = find_view(&initial_state, &room_identity("!media:example.org"));
    assert!(view_contains(
        media_view,
        "* [16:00] @alice:example.org sent file: report.pdf (application/pdf, 45678 bytes)",
    ));
    assert!(!view_contains(media_view, "unsupported"));

    Ok(())
}

#[test]
fn automation_driver_renders_audio_message_fixture_and_exposes_typed_activity()
-> Result<(), Box<dyn std::error::Error>> {
    let exe_path = mmatrix_ui_exe_path()
        .map_err(|error| format!("failed to locate mmatrix-ui.exe: {error}"))?;
    let fixture_path = fixture_state_path("audio-message.json");

    let args = [
        OsString::from("--render-state"),
        fixture_path.as_os_str().to_os_string(),
    ];
    let app = launch_app(&exe_path, &args)?;
    app.wait_idle(Duration::from_secs(5))?;

    let initial_state = app.dump_ui_state()?;
    assert_eq!(
        initial_state.mdi.active,
        room_identity("!media:example.org")
    );
    assert_active_switchbar_item(&initial_state, &room_identity("!media:example.org"));
    let initial_model = app.dump_model()?;
    assert_eq!(
        initial_model.selected_room.as_deref(),
        Some("!media:example.org")
    );
    assert_eq!(
        initial_model.last_activity,
        Some(RoomMessageActivityLine {
            room_id: "!media:example.org".to_owned(),
            activity_id: "event:$audio1:example.org".to_owned(),
            timestamp_ms: 1_710_000_060_000,
            event_id: Some("$audio1:example.org".to_owned()),
            transaction_id: None,
            sender: "@alice:example.org".to_owned(),
            presentation: RoomMessagePresentationKind::Text,
            body: "[audio] voice-note.ogg".to_owned(),
            send_state: RoomMessageSendState::Sent,
        })
    );
    assert_switchbar_activity(&initial_state, &room_identity("!media:example.org"), false);
    let media_activity = app.room_activity(Some("!media:example.org"))?;
    assert_eq!(
        media_activity,
        vec![audio_entry_with_event_id(
            "!media:example.org",
            1_710_000_060_000,
            fixture_audio_activity(),
        )]
    );
    let RoomActivityKind::Audio(activity) = &media_activity[0].kind else {
        panic!("expected typed audio activity");
    };
    assert_eq!(activity.event_id, "$audio1:example.org");
    assert_eq!(activity.sender, "@alice:example.org");
    assert_eq!(activity.body, "voice-note.ogg");
    assert_eq!(activity.filename.as_deref(), Some("voice-note.ogg"));
    assert_eq!(
        activity.source,
        RoomMediaSource::Plain {
            url: "mxc://example.org/audio".to_owned(),
        }
    );
    assert_eq!(
        activity.info,
        Some(RoomAudioInfo {
            mimetype: Some("audio/ogg".to_owned()),
            size: Some(23_456),
            duration_ms: Some(12_000),
        })
    );
    assert!(
        media_activity
            .iter()
            .all(|entry| !entry.contains("unsupported"))
    );

    let media_view = find_view(&initial_state, &room_identity("!media:example.org"));
    assert!(view_contains(
        media_view,
        "* [16:01] @alice:example.org sent audio: voice-note.ogg (audio/ogg, 23456 bytes, 12000 ms)",
    ));
    assert!(!view_contains(media_view, "unsupported"));

    Ok(())
}

#[test]
fn automation_driver_renders_loaded_image_message_fixture_and_exposes_loaded_media_state()
-> Result<(), Box<dyn std::error::Error>> {
    let exe_path = mmatrix_ui_exe_path()
        .map_err(|error| format!("failed to locate mmatrix-ui.exe: {error}"))?;
    let fixture_path = fixture_state_path("image-message-loaded.json");

    let args = [
        OsString::from("--render-state"),
        fixture_path.as_os_str().to_os_string(),
    ];
    let app = launch_app(&exe_path, &args)?;
    app.wait_idle(Duration::from_secs(5))?;

    let initial_state = app.dump_ui_state()?;
    let image_view = find_view(&initial_state, &room_identity("!images:example.org"));
    assert!(view_contains(
        image_view,
        "* [16:00] @alice:example.org sent image: diagram.png (image/png, 640x480, 12345 bytes)",
    ));
    assert_eq!(
        app.room_activity(Some("!images:example.org"))?,
        vec![image_entry_with_event_id(
            "!images:example.org",
            1_710_000_000_000,
            fixture_image_activity(Some(RoomImageMediaState::Loaded {
                width: 64,
                height: 48,
            })),
        )]
    );
    assert!(!view_contains(image_view, "unsupported m.image"));

    Ok(())
}

#[test]
fn automation_driver_renders_failed_image_message_fixture_and_exposes_failed_media_state()
-> Result<(), Box<dyn std::error::Error>> {
    let exe_path = mmatrix_ui_exe_path()
        .map_err(|error| format!("failed to locate mmatrix-ui.exe: {error}"))?;
    let fixture_path = fixture_state_path("image-message-failed.json");

    let args = [
        OsString::from("--render-state"),
        fixture_path.as_os_str().to_os_string(),
    ];
    let app = launch_app(&exe_path, &args)?;
    app.wait_idle(Duration::from_secs(5))?;

    let initial_state = app.dump_ui_state()?;
    let image_view = find_view(&initial_state, &room_identity("!images:example.org"));
    assert!(view_contains(
        image_view,
        "* [16:00] @alice:example.org sent image: diagram.png (image/png, 640x480, 12345 bytes)",
    ));
    assert_eq!(
        app.room_activity(Some("!images:example.org"))?,
        vec![image_entry_with_event_id(
            "!images:example.org",
            1_710_000_000_000,
            fixture_image_activity(Some(RoomImageMediaState::Failed {
                message: "failed to fetch room image media: unavailable".to_owned(),
            })),
        )]
    );
    assert!(!view_contains(image_view, "unsupported m.image"));

    Ok(())
}

fn assert_video_fixture_media_state(
    fixture_name: &str,
    expected_media_state: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let exe_path = mmatrix_ui_exe_path()
        .map_err(|error| format!("failed to locate mmatrix-ui.exe: {error}"))?;
    let fixture_path = fixture_state_path(fixture_name);
    let app = launch_app(
        &exe_path,
        &[
            OsString::from("--render-state"),
            fixture_path.as_os_str().to_os_string(),
        ],
    )?;
    app.wait_idle(Duration::from_secs(5))?;
    let activities = app.room_activity(Some("!media:example.org"))?;
    assert_eq!(activities.len(), 1);
    let activity_json = serde_json::to_string(&activities[0])?;
    assert!(activity_json.contains("\"type\":\"video\""));
    assert!(activity_json.contains(expected_media_state));
    Ok(())
}

#[test]
fn automation_driver_video_message_exposes_pending_media_state()
-> Result<(), Box<dyn std::error::Error>> {
    assert_video_fixture_media_state(
        "video-message.json",
        "\"media_state\":{\"type\":\"pending\"}",
    )
}

#[test]
fn automation_driver_video_message_loaded_exposes_loaded_media_state()
-> Result<(), Box<dyn std::error::Error>> {
    assert_video_fixture_media_state(
        "video-message-loaded.json",
        "\"media_state\":{\"type\":\"loaded\",\"width\":64,\"height\":48}",
    )
}

#[test]
fn automation_driver_video_message_failed_exposes_failed_media_state()
-> Result<(), Box<dyn std::error::Error>> {
    assert_video_fixture_media_state(
        "video-message-failed.json",
        "\"media_state\":{\"type\":\"failed\",\"message\":\"failed to fetch room image media: unavailable\"}",
    )
}

#[test]
fn automation_driver_renders_encrypted_pending_image_and_exposes_full_source_metadata()
-> Result<(), Box<dyn std::error::Error>> {
    let exe_path = mmatrix_ui_exe_path()
        .map_err(|error| format!("failed to locate mmatrix-ui.exe: {error}"))?;
    let fixture_path = fixture_state_path("encrypted-image-message.json");

    let args = [
        OsString::from("--render-state"),
        fixture_path.as_os_str().to_os_string(),
    ];
    let app = launch_app(&exe_path, &args)?;
    app.wait_idle(Duration::from_secs(5))?;

    let initial_state = app.dump_ui_state()?;
    assert_eq!(
        initial_state.mdi.active,
        room_identity("!images:example.org")
    );
    assert_active_switchbar_item(&initial_state, &room_identity("!images:example.org"));
    let initial_model = app.dump_model()?;
    assert_eq!(
        initial_model.selected_room.as_deref(),
        Some("!images:example.org")
    );
    assert_eq!(
        initial_model.last_activity,
        Some(RoomMessageActivityLine {
            room_id: "!images:example.org".to_owned(),
            activity_id: "event:$image-typed:example.org".to_owned(),
            timestamp_ms: 1_710_000_000_000,
            event_id: Some("$image-typed:example.org".to_owned()),
            transaction_id: None,
            sender: "@alice:example.org".to_owned(),
            presentation: RoomMessagePresentationKind::Text,
            body: "[image] diagram.png".to_owned(),
            send_state: RoomMessageSendState::Sent,
        })
    );
    assert_switchbar_activity(&initial_state, &room_identity("!images:example.org"), false);
    assert_eq!(
        app.room_activity(Some("!images:example.org"))?,
        vec![image_entry_with_event_id(
            "!images:example.org",
            1_710_000_000_000,
            encrypted_fixture_image_activity(Some(RoomImageMediaState::Pending)),
        )]
    );

    let image_view = find_view(&initial_state, &room_identity("!images:example.org"));
    assert!(view_contains(
        image_view,
        "* [16:00] @alice:example.org sent image: diagram.png (image/png, 640x480, 12345 bytes)",
    ));
    assert!(!view_contains(image_view, "unsupported m.image"));

    Ok(())
}

#[test]
fn automation_driver_renders_encrypted_loaded_image_and_exposes_loaded_media_state()
-> Result<(), Box<dyn std::error::Error>> {
    let exe_path = mmatrix_ui_exe_path()
        .map_err(|error| format!("failed to locate mmatrix-ui.exe: {error}"))?;
    let fixture_path = fixture_state_path("encrypted-image-message-loaded.json");

    let args = [
        OsString::from("--render-state"),
        fixture_path.as_os_str().to_os_string(),
    ];
    let app = launch_app(&exe_path, &args)?;
    app.wait_idle(Duration::from_secs(5))?;

    let initial_state = app.dump_ui_state()?;
    let image_view = find_view(&initial_state, &room_identity("!images:example.org"));
    assert!(view_contains(
        image_view,
        "* [16:00] @alice:example.org sent image: diagram.png (image/png, 640x480, 12345 bytes)",
    ));
    assert_eq!(
        app.room_activity(Some("!images:example.org"))?,
        vec![image_entry_with_event_id(
            "!images:example.org",
            1_710_000_000_000,
            encrypted_fixture_image_activity(Some(RoomImageMediaState::Loaded {
                width: 64,
                height: 48,
            })),
        )]
    );
    assert!(!view_contains(image_view, "unsupported m.image"));

    Ok(())
}

#[test]
fn automation_driver_renders_encrypted_failed_image_and_exposes_failed_media_state()
-> Result<(), Box<dyn std::error::Error>> {
    let exe_path = mmatrix_ui_exe_path()
        .map_err(|error| format!("failed to locate mmatrix-ui.exe: {error}"))?;
    let fixture_path = fixture_state_path("encrypted-image-message-failed.json");

    let args = [
        OsString::from("--render-state"),
        fixture_path.as_os_str().to_os_string(),
    ];
    let app = launch_app(&exe_path, &args)?;
    app.wait_idle(Duration::from_secs(5))?;

    let initial_state = app.dump_ui_state()?;
    let image_view = find_view(&initial_state, &room_identity("!images:example.org"));
    assert!(view_contains(
        image_view,
        "* [16:00] @alice:example.org sent image: diagram.png (image/png, 640x480, 12345 bytes)",
    ));
    assert_eq!(
        app.room_activity(Some("!images:example.org"))?,
        vec![image_entry_with_event_id(
            "!images:example.org",
            1_710_000_000_000,
            encrypted_fixture_image_activity(Some(RoomImageMediaState::Failed {
                message: "failed to fetch room image media: unavailable".to_owned(),
            })),
        )]
    );
    assert!(!view_contains(image_view, "unsupported m.image"));

    Ok(())
}

#[test]
fn automation_driver_renders_unsupported_message_fixture_and_clears_activity_on_activation()
-> Result<(), Box<dyn std::error::Error>> {
    let exe_path = mmatrix_ui_exe_path()
        .map_err(|error| format!("failed to locate mmatrix-ui.exe: {error}"))?;
    let fixture_path = fixture_state_path("unsupported-message.json");

    let args = [
        OsString::from("--render-state"),
        fixture_path.as_os_str().to_os_string(),
    ];
    let app = launch_app(&exe_path, &args)?;
    app.wait_idle(Duration::from_secs(5))?;

    let initial_state = app.dump_ui_state()?;
    assert_eq!(
        initial_state.mdi.active,
        room_identity("!alpha:example.org")
    );
    assert_active_switchbar_item(&initial_state, &room_identity("!alpha:example.org"));
    let initial_model = app.dump_model()?;
    assert_eq!(
        initial_model.selected_room.as_deref(),
        Some("!alpha:example.org")
    );
    assert_eq!(
        initial_model.last_activity,
        Some(RoomMessageActivityLine {
            room_id: "!media:example.org".to_owned(),
            activity_id: "event:$unsupported1:example.org".to_owned(),
            timestamp_ms: 1_710_000_000_000,
            event_id: Some("$unsupported1:example.org".to_owned()),
            transaction_id: None,
            sender: "@alice:example.org".to_owned(),
            presentation: RoomMessagePresentationKind::Text,
            body: "[unsupported com.example.custom] custom payload".to_owned(),
            send_state: RoomMessageSendState::Sent,
        })
    );
    assert_switchbar_activity(&initial_state, &room_identity("!alpha:example.org"), false);
    assert_switchbar_activity(&initial_state, &room_identity("!media:example.org"), true);
    let media_activity = app.room_activity(Some("!media:example.org"))?;
    assert_eq!(
        media_activity,
        vec![unsupported_entry_with_event_id(
            "!media:example.org",
            "$unsupported1:example.org",
            "@alice:example.org",
            1_710_000_000_000,
            "com.example.custom",
            "custom payload",
        )]
    );
    assert!(
        media_activity
            .iter()
            .all(|entry| !entry.contains("\"media_state\""))
    );
    assert!(view_contains(
        find_view(&initial_state, &room_identity("!media:example.org")),
        "* [16:00] @alice:example.org sent unsupported com.example.custom: custom payload",
    ));
    assert!(!view_contains(
        find_view(&initial_state, &room_identity("!media:example.org")),
        "loading image...",
    ));
    assert!(!view_contains(
        find_view(&initial_state, &room_identity("!media:example.org")),
        "image unavailable",
    ));

    app.select_room("!media:example.org")?;
    let activated_state = app.dump_ui_state()?;
    assert_eq!(
        activated_state.mdi.active,
        room_identity("!media:example.org")
    );
    assert_active_switchbar_item(&activated_state, &room_identity("!media:example.org"));
    assert_eq!(
        app.dump_model()?.selected_room.as_deref(),
        Some("!media:example.org")
    );
    assert_switchbar_activity(
        &activated_state,
        &room_identity("!media:example.org"),
        false,
    );

    Ok(())
}

#[test]
fn automation_driver_compose_history_keys_are_noops_without_history()
-> Result<(), Box<dyn std::error::Error>> {
    let exe_path = mmatrix_ui_exe_path()
        .map_err(|error| format!("failed to locate mmatrix-ui.exe: {error}"))?;
    let state_path = write_state_file(&base_events())?;

    let args = [
        OsString::from("--render-state"),
        state_path.as_os_str().to_os_string(),
    ];
    let app = launch_app(&exe_path, &args)?;
    app.wait_idle(Duration::from_secs(5))?;
    app.wait_for_min_rooms(2, Duration::from_secs(5))?;

    app.select_room("!beta:example")?;
    app.set_compose_text("draft line")?;
    assert_eq!(app.compose_text()?, "draft line");

    app.set_compose_text("")?;
    assert_eq!(app.compose_text()?, "");

    app.set_compose_text("draft before arrows")?;
    let before = app.dump_ui_state()?;
    let beta_identity = room_identity("!beta:example");
    app.send_compose_key(ComposeKey::Up)?;
    app.send_compose_key(ComposeKey::Down)?;
    assert_eq!(app.compose_text()?, "draft before arrows");
    let after = app.dump_ui_state()?;

    assert_eq!(after.mdi.active, before.mdi.active);
    assert_eq!(
        find_view(&after, &beta_identity),
        find_view(&before, &beta_identity)
    );
    assert_eq!(after.switchbar, before.switchbar);
    assert_eq!(after.status_bar, before.status_bar);
    assert_eq!(after.mdi.active, beta_identity);
    assert_active_switchbar_item(&after, &beta_identity);

    let _ = fs::remove_file(&state_path);
    Ok(())
}

#[test]
fn automation_driver_compose_history_recalls_accepted_submissions()
-> Result<(), Box<dyn std::error::Error>> {
    let exe_path = mmatrix_ui_exe_path()
        .map_err(|error| format!("failed to locate mmatrix-ui.exe: {error}"))?;
    let local_app_data = unique_local_app_data_path();
    let options = launch_options(Some(local_app_data.clone()));
    let args = [OsString::from("--automation")];

    let app = launch_app_with_options(&exe_path, &args, options)?;
    app.wait_for_lifecycle_state("core_started", Duration::from_secs(30))?;
    app.load_events(&serde_json::to_string(&base_events())?)?;
    app.wait_idle(Duration::from_secs(5))?;
    app.select_room("!beta:example")?;

    app.set_compose_text("first accepted")?;
    app.send_compose_key(ComposeKey::Return)?;
    assert_eq!(app.compose_text()?, "");

    app.set_compose_text("second accepted")?;
    app.send_compose_key(ComposeKey::Return)?;
    assert_eq!(app.compose_text()?, "");

    app.set_compose_text("draft before arrows")?;
    app.send_compose_key(ComposeKey::Up)?;
    assert_eq!(app.compose_text()?, "second accepted");
    app.send_compose_key(ComposeKey::Up)?;
    assert_eq!(app.compose_text()?, "first accepted");
    app.send_compose_key(ComposeKey::Up)?;
    assert_eq!(app.compose_text()?, "first accepted");
    app.send_compose_key(ComposeKey::Down)?;
    assert_eq!(app.compose_text()?, "second accepted");
    app.send_compose_key(ComposeKey::Down)?;
    assert_eq!(app.compose_text()?, "draft before arrows");
    app.send_compose_key(ComposeKey::Down)?;
    assert_eq!(app.compose_text()?, "draft before arrows");

    drop(app);
    let _ = fs::remove_dir_all(&local_app_data);
    Ok(())
}

#[test]
fn automation_driver_compose_history_ignores_rejected_submissions()
-> Result<(), Box<dyn std::error::Error>> {
    let exe_path = mmatrix_ui_exe_path()
        .map_err(|error| format!("failed to locate mmatrix-ui.exe: {error}"))?;
    let local_app_data = unique_local_app_data_path();
    let options = launch_options(Some(local_app_data.clone()));
    let args = [OsString::from("--automation")];

    let app = launch_app_with_options(&exe_path, &args, options)?;
    app.wait_for_lifecycle_state("core_started", Duration::from_secs(30))?;

    app.set_compose_text("no selected room")?;
    app.send_compose_key(ComposeKey::Return)?;
    assert_eq!(app.compose_text()?, "no selected room");

    app.set_compose_text("draft after rejected")?;
    app.send_compose_key(ComposeKey::Up)?;
    assert_eq!(app.compose_text()?, "draft after rejected");
    app.send_compose_key(ComposeKey::Down)?;
    assert_eq!(app.compose_text()?, "draft after rejected");

    drop(app);
    let _ = fs::remove_dir_all(&local_app_data);
    Ok(())
}

#[test]
fn automation_driver_compose_history_ignores_empty_submission()
-> Result<(), Box<dyn std::error::Error>> {
    let exe_path = mmatrix_ui_exe_path()
        .map_err(|error| format!("failed to locate mmatrix-ui.exe: {error}"))?;
    let local_app_data = unique_local_app_data_path();
    let options = launch_options(Some(local_app_data.clone()));
    let args = [OsString::from("--automation")];

    let app = launch_app_with_options(&exe_path, &args, options)?;
    app.wait_for_lifecycle_state("core_started", Duration::from_secs(30))?;
    app.load_events(&serde_json::to_string(&base_events())?)?;
    app.wait_idle(Duration::from_secs(5))?;
    app.select_room("!beta:example")?;

    app.set_compose_text("")?;
    app.send_compose_key(ComposeKey::Return)?;
    assert_eq!(app.compose_text()?, "");

    app.set_compose_text("draft after empty submit")?;
    app.send_compose_key(ComposeKey::Up)?;
    assert_eq!(app.compose_text()?, "draft after empty submit");

    drop(app);
    let _ = fs::remove_dir_all(&local_app_data);
    Ok(())
}

#[test]
fn automation_driver_setting_text_exits_compose_history_navigation()
-> Result<(), Box<dyn std::error::Error>> {
    let exe_path = mmatrix_ui_exe_path()
        .map_err(|error| format!("failed to locate mmatrix-ui.exe: {error}"))?;
    let local_app_data = unique_local_app_data_path();
    let options = launch_options(Some(local_app_data.clone()));
    let args = [OsString::from("--automation")];

    let app = launch_app_with_options(&exe_path, &args, options)?;
    app.wait_for_lifecycle_state("core_started", Duration::from_secs(30))?;
    app.load_events(&serde_json::to_string(&base_events())?)?;
    app.wait_idle(Duration::from_secs(5))?;
    app.select_room("!beta:example")?;

    app.set_compose_text("first accepted")?;
    app.send_compose_key(ComposeKey::Return)?;
    assert_eq!(app.compose_text()?, "");

    app.set_compose_text("second accepted")?;
    app.send_compose_key(ComposeKey::Return)?;
    assert_eq!(app.compose_text()?, "");

    app.send_compose_key(ComposeKey::Up)?;
    assert_eq!(app.compose_text()?, "second accepted");

    app.set_compose_text("replacement draft")?;
    app.send_compose_key(ComposeKey::Up)?;
    assert_eq!(app.compose_text()?, "second accepted");
    app.send_compose_key(ComposeKey::Down)?;
    assert_eq!(app.compose_text()?, "replacement draft");

    drop(app);
    let _ = fs::remove_dir_all(&local_app_data);
    Ok(())
}

#[test]
fn automation_driver_typing_exits_compose_history_navigation()
-> Result<(), Box<dyn std::error::Error>> {
    let exe_path = mmatrix_ui_exe_path()
        .map_err(|error| format!("failed to locate mmatrix-ui.exe: {error}"))?;
    let local_app_data = unique_local_app_data_path();
    let options = launch_options(Some(local_app_data.clone()));
    let args = [OsString::from("--automation")];

    let app = launch_app_with_options(&exe_path, &args, options)?;
    app.wait_for_lifecycle_state("core_started", Duration::from_secs(30))?;
    app.load_events(&serde_json::to_string(&base_events())?)?;
    app.wait_idle(Duration::from_secs(5))?;
    app.select_room("!beta:example")?;

    app.set_compose_text("first accepted")?;
    app.send_compose_key(ComposeKey::Return)?;
    assert_eq!(app.compose_text()?, "");

    app.set_compose_text("second accepted")?;
    app.send_compose_key(ComposeKey::Return)?;
    assert_eq!(app.compose_text()?, "");

    app.send_compose_key(ComposeKey::Up)?;
    assert_eq!(app.compose_text()?, "second accepted");

    let frame = app.automation_frame_hwnd() as HWND;
    let edit = unsafe { GetDlgItem(frame, EDIT_ID) };
    if edit.is_null() {
        return Err("compose edit control was not found".into());
    }
    unsafe {
        SendMessageW(edit, WM_CHAR, 'x' as WPARAM, 0);
    }
    assert_eq!(app.compose_text()?, "second acceptedx");

    app.send_compose_key(ComposeKey::Up)?;
    assert_eq!(app.compose_text()?, "second accepted");
    app.send_compose_key(ComposeKey::Down)?;
    assert_eq!(app.compose_text()?, "second acceptedx");

    drop(app);
    let _ = fs::remove_dir_all(&local_app_data);
    Ok(())
}

#[test]
fn automation_driver_paste_exits_compose_history_navigation()
-> Result<(), Box<dyn std::error::Error>> {
    let exe_path = mmatrix_ui_exe_path()
        .map_err(|error| format!("failed to locate mmatrix-ui.exe: {error}"))?;
    let local_app_data = unique_local_app_data_path();
    let options = launch_options(Some(local_app_data.clone()));
    let args = [OsString::from("--automation")];

    let app = launch_app_with_options(&exe_path, &args, options)?;
    app.wait_for_lifecycle_state("core_started", Duration::from_secs(30))?;
    app.load_events(&serde_json::to_string(&base_events())?)?;
    app.wait_idle(Duration::from_secs(5))?;
    app.select_room("!beta:example")?;

    app.set_compose_text("first accepted")?;
    app.send_compose_key(ComposeKey::Return)?;
    assert_eq!(app.compose_text()?, "");

    app.set_compose_text("second accepted")?;
    app.send_compose_key(ComposeKey::Return)?;
    assert_eq!(app.compose_text()?, "");

    app.send_compose_key(ComposeKey::Up)?;
    assert_eq!(app.compose_text()?, "second accepted");

    let frame = app.automation_frame_hwnd() as HWND;
    let edit = unsafe { GetDlgItem(frame, EDIT_ID) };
    if edit.is_null() {
        return Err("compose edit control was not found".into());
    }
    set_clipboard_text(frame, "x")?;
    unsafe {
        SendMessageW(edit, WM_PASTE, 0, 0);
    }
    assert_eq!(app.compose_text()?, "second acceptedx");

    app.send_compose_key(ComposeKey::Up)?;
    assert_eq!(app.compose_text()?, "second accepted");
    app.send_compose_key(ComposeKey::Down)?;
    assert_eq!(app.compose_text()?, "second acceptedx");

    drop(app);
    let _ = fs::remove_dir_all(&local_app_data);
    Ok(())
}

#[test]
fn automation_driver_return_compose_key_submits_through_edit_control()
-> Result<(), Box<dyn std::error::Error>> {
    let exe_path = mmatrix_ui_exe_path()
        .map_err(|error| format!("failed to locate mmatrix-ui.exe: {error}"))?;
    let local_app_data = unique_local_app_data_path();
    let options = launch_options(Some(local_app_data.clone()));
    let args = [OsString::from("--automation")];

    let app = launch_app_with_options(&exe_path, &args, options)?;
    app.wait_for_lifecycle_state("core_started", Duration::from_secs(30))?;
    app.load_events(&serde_json::to_string(&base_events())?)?;
    app.wait_idle(Duration::from_secs(5))?;
    app.select_room("!beta:example")?;
    app.set_compose_text("queued by return key")?;
    assert_eq!(app.compose_text()?, "queued by return key");

    app.send_compose_key(ComposeKey::Return)?;
    assert_eq!(app.compose_text()?, "");
    app.wait_for_lifecycle_state("error", Duration::from_secs(30))?;
    assert_eq!(
        app.dump_model()?.last_error,
        Some(UiError {
            operation: "send_text".to_owned(),
            message: "send message requires an authenticated session".to_owned(),
        })
    );
    assert_eq!(
        app.dump_ui_state()?.mdi.active,
        room_identity("!beta:example")
    );

    drop(app);
    let _ = fs::remove_dir_all(&local_app_data);
    Ok(())
}

#[test]
fn automation_driver_slash_status_activates_status_view() -> Result<(), Box<dyn std::error::Error>>
{
    let exe_path = mmatrix_ui_exe_path()
        .map_err(|error| format!("failed to locate mmatrix-ui.exe: {error}"))?;
    let local_app_data = unique_local_app_data_path();
    let options = launch_options(Some(local_app_data.clone()));
    let args = [OsString::from("--automation")];

    let app = launch_app_with_options(&exe_path, &args, options)?;
    app.wait_for_lifecycle_state("core_started", Duration::from_secs(30))?;
    app.load_events(&serde_json::to_string(&base_events())?)?;
    app.wait_idle(Duration::from_secs(5))?;
    app.select_room("!beta:example")?;

    app.set_compose_text("/status")?;
    app.send_compose_key(ComposeKey::Return)?;
    app.wait_idle(Duration::from_secs(5))?;
    assert_eq!(app.compose_text()?, "");

    let model = app.dump_model()?;
    assert_eq!(model.selected_room, None);
    assert_eq!(model.last_error, None);

    let ui_state = app.dump_ui_state()?;
    assert_eq!(ui_state.mdi.active, ViewIdentity::Status);
    assert_active_switchbar_item(&ui_state, &ViewIdentity::Status);
    assert!(ui_state.status_bar.text.contains("status window active"));
    assert_eq!(
        app.selected_navigation_path()?,
        nav_path(&["mmatrix", "status"])
    );
    assert!(view_contains(
        find_view(&ui_state, &ViewIdentity::Status),
        "status window active"
    ));

    app.send_compose_key(ComposeKey::Up)?;
    assert_eq!(app.compose_text()?, "/status");

    drop(app);
    let _ = fs::remove_dir_all(&local_app_data);
    Ok(())
}

#[test]
fn automation_driver_send_text_keeps_literal_slash_body() -> Result<(), Box<dyn std::error::Error>>
{
    let exe_path = mmatrix_ui_exe_path()
        .map_err(|error| format!("failed to locate mmatrix-ui.exe: {error}"))?;
    let local_app_data = unique_local_app_data_path();
    let options = launch_options(Some(local_app_data.clone()));
    let args = [OsString::from("--automation")];

    let app = launch_app_with_options(&exe_path, &args, options)?;
    app.wait_for_lifecycle_state("core_started", Duration::from_secs(30))?;
    app.load_events(&serde_json::to_string(&base_events())?)?;
    app.wait_idle(Duration::from_secs(5))?;
    app.select_room("!beta:example")?;

    assert_eq!(
        app.send_text_target(Some("!beta:example"), "/status")?,
        "!beta:example"
    );
    assert_eq!(app.compose_text()?, "");
    app.wait_for_lifecycle_state("error", Duration::from_secs(30))?;
    assert_eq!(
        app.dump_model()?.last_error,
        Some(UiError {
            operation: "send_text".to_owned(),
            message: "send message requires an authenticated session".to_owned(),
        })
    );
    let ui_state = app.dump_ui_state()?;
    assert_eq!(ui_state.mdi.active, room_identity("!beta:example"));
    assert!(!ui_state.status_bar.text.contains("status window active"));

    drop(app);
    let _ = fs::remove_dir_all(&local_app_data);
    Ok(())
}

#[test]
fn automation_driver_slash_window_switches_by_room_id_and_display_name()
-> Result<(), Box<dyn std::error::Error>> {
    let exe_path = mmatrix_ui_exe_path()
        .map_err(|error| format!("failed to locate mmatrix-ui.exe: {error}"))?;
    let local_app_data = unique_local_app_data_path();
    let options = launch_options(Some(local_app_data.clone()));
    let args = [OsString::from("--automation")];

    let app = launch_app_with_options(&exe_path, &args, options)?;
    app.wait_for_lifecycle_state("core_started", Duration::from_secs(30))?;
    app.load_events(&serde_json::to_string(&base_events())?)?;
    app.wait_idle(Duration::from_secs(5))?;

    app.set_compose_text("/window Alpha")?;
    app.send_compose_key(ComposeKey::Return)?;
    app.wait_idle(Duration::from_secs(5))?;
    assert_eq!(app.compose_text()?, "");
    assert_eq!(app.dump_model()?.last_error, None);
    let alpha_state = app.dump_ui_state()?;
    assert_eq!(alpha_state.mdi.active, room_identity("!alpha:example"));
    assert_active_switchbar_item(&alpha_state, &room_identity("!alpha:example"));
    assert!(alpha_state.status_bar.text.contains("window active: Alpha"));
    assert_eq!(
        app.selected_navigation_path()?,
        nav_path(&["mmatrix", "rooms: 2", "Alpha"])
    );

    app.set_compose_text("/window !beta:example")?;
    app.send_compose_key(ComposeKey::Return)?;
    app.wait_idle(Duration::from_secs(5))?;
    assert_eq!(app.compose_text()?, "");
    assert_eq!(app.dump_model()?.last_error, None);
    let beta_state = app.dump_ui_state()?;
    assert_eq!(beta_state.mdi.active, room_identity("!beta:example"));
    assert_active_switchbar_item(&beta_state, &room_identity("!beta:example"));
    assert!(
        beta_state
            .status_bar
            .text
            .contains("window active: !beta:example")
    );
    assert_eq!(
        app.selected_navigation_path()?,
        nav_path(&["mmatrix", "rooms: 2", "Beta [e2ee]"])
    );

    drop(app);
    let _ = fs::remove_dir_all(&local_app_data);
    Ok(())
}

#[test]
fn automation_driver_slash_window_reports_unknown_and_ambiguous_targets()
-> Result<(), Box<dyn std::error::Error>> {
    let exe_path = mmatrix_ui_exe_path()
        .map_err(|error| format!("failed to locate mmatrix-ui.exe: {error}"))?;
    let local_app_data = unique_local_app_data_path();
    let options = launch_options(Some(local_app_data.clone()));
    let args = [OsString::from("--automation")];
    let events = vec![
        Event::Started,
        Event::SyncedRooms(vec![
            room_summary("!beta:example", "Beta", true),
            room_summary("!shared-one:example", "Shared", false),
            room_summary("!shared-two:example", "Shared", false),
        ]),
        text_event(
            "!beta:example",
            "@bob:example",
            1_700_000_060_000,
            "beta first",
        ),
    ];

    let app = launch_app_with_options(&exe_path, &args, options)?;
    app.wait_for_lifecycle_state("core_started", Duration::from_secs(30))?;
    app.load_events(&serde_json::to_string(&events)?)?;
    app.wait_idle(Duration::from_secs(5))?;
    assert_eq!(
        app.dump_ui_state()?.mdi.active,
        room_identity("!beta:example")
    );

    app.set_compose_text("/window Shared")?;
    app.send_compose_key(ComposeKey::Return)?;
    app.wait_idle(Duration::from_secs(5))?;
    assert_eq!(app.compose_text()?, "");
    let ambiguous_state = app.dump_ui_state()?;
    assert_eq!(ambiguous_state.mdi.active, room_identity("!beta:example"));
    assert!(
        ambiguous_state
            .status_bar
            .text
            .contains("ambiguous window target: Shared")
    );
    assert_eq!(app.dump_model()?.last_error, None);

    app.set_compose_text("/window Missing")?;
    app.send_compose_key(ComposeKey::Return)?;
    app.wait_idle(Duration::from_secs(5))?;
    assert_eq!(app.compose_text()?, "");
    let missing_state = app.dump_ui_state()?;
    assert_eq!(missing_state.mdi.active, room_identity("!beta:example"));
    assert!(
        missing_state
            .status_bar
            .text
            .contains("unknown window target: Missing")
    );
    assert_eq!(app.dump_model()?.last_error, None);

    drop(app);
    let _ = fs::remove_dir_all(&local_app_data);
    Ok(())
}

#[test]
fn automation_driver_slash_join_records_submitted_membership_without_creating_room()
-> Result<(), Box<dyn std::error::Error>> {
    let exe_path = mmatrix_ui_exe_path()
        .map_err(|error| format!("failed to locate mmatrix-ui.exe: {error}"))?;
    let local_app_data = unique_local_app_data_path();
    let options = launch_options(Some(local_app_data.clone()));
    let args = [OsString::from("--automation")];

    let app = launch_app_with_options(&exe_path, &args, options)?;
    app.wait_for_lifecycle_state("core_started", Duration::from_secs(30))?;
    app.load_events(&serde_json::to_string(&base_events())?)?;
    app.wait_idle(Duration::from_secs(5))?;

    let before = app.dump_ui_state()?;
    assert_eq!(before.mdi.active, room_identity("!beta:example"));
    assert!(app.dump_model()?.submitted_membership_commands.is_empty());

    app.set_compose_text("/join #ops:example.org")?;
    app.send_compose_key(ComposeKey::Return)?;
    app.wait_idle(Duration::from_secs(5))?;
    assert_eq!(app.compose_text()?, "");

    let model = app.dump_model()?;
    assert_eq!(model.status, "join submitted: #ops:example.org");
    assert_eq!(
        model.submitted_membership_commands,
        vec![SubmittedMembershipCommand {
            action: MembershipCommandAction::Join,
            target: "#ops:example.org".to_owned(),
        }]
    );
    assert!(
        model
            .rooms
            .iter()
            .all(|room| room.room_id != "#ops:example.org")
    );
    assert!(
        model
            .rooms
            .iter()
            .all(|room| room.display_name != "#ops:example.org")
    );

    let ui_state = app.dump_ui_state()?;
    assert_eq!(ui_state.mdi.active, room_identity("!beta:example"));
    assert!(
        ui_state
            .mdi
            .views
            .iter()
            .all(|view| view.identity != room_identity("#ops:example.org"))
    );
    assert!(
        ui_state
            .switchbar
            .items
            .iter()
            .all(|item| item.title != "#ops:example.org")
    );
    assert!(!serde_json::to_string(&ui_state.navigation)?.contains("#ops:example.org"));

    drop(app);
    let _ = fs::remove_dir_all(&local_app_data);
    Ok(())
}

#[test]
fn automation_driver_slash_query_records_submitted_direct_room_without_creating_room()
-> Result<(), Box<dyn std::error::Error>> {
    let exe_path = mmatrix_ui_exe_path()
        .map_err(|error| format!("failed to locate mmatrix-ui.exe: {error}"))?;
    let local_app_data = unique_local_app_data_path();
    let options = launch_options(Some(local_app_data.clone()));
    let args = [OsString::from("--automation")];

    let app = launch_app_with_options(&exe_path, &args, options)?;
    app.wait_for_lifecycle_state("core_started", Duration::from_secs(30))?;
    app.load_events(&serde_json::to_string(&base_events())?)?;
    app.wait_idle(Duration::from_secs(5))?;

    let before_rooms = app.list_rooms()?;
    let before_state = app.dump_ui_state()?;
    assert_eq!(before_state.mdi.active, room_identity("!beta:example"));
    assert!(app.dump_model()?.submitted_direct_room_commands.is_empty());

    app.set_compose_text("/query @alice:example.org")?;
    app.send_compose_key(ComposeKey::Return)?;
    app.wait_idle(Duration::from_secs(5))?;
    assert_eq!(app.compose_text()?, "");

    let model = app.dump_model()?;
    assert_eq!(model.status, "query submitted: @alice:example.org");
    assert_eq!(
        model.submitted_direct_room_commands,
        vec![SubmittedDirectRoomCommand {
            user_id: "@alice:example.org".to_owned(),
        }]
    );
    assert_eq!(app.list_rooms()?, before_rooms);

    let ui_state = app.dump_ui_state()?;
    assert_eq!(ui_state.mdi.active, room_identity("!beta:example"));
    assert_eq!(ui_state.rooms, before_state.rooms);
    assert_eq!(
        ui_state
            .mdi
            .views
            .iter()
            .map(|view| view.identity.clone())
            .collect::<Vec<_>>(),
        before_state
            .mdi
            .views
            .iter()
            .map(|view| view.identity.clone())
            .collect::<Vec<_>>()
    );

    drop(app);
    let _ = fs::remove_dir_all(&local_app_data);
    Ok(())
}

#[test]
fn automation_driver_room_menu_join_records_submitted_membership_without_creating_room()
-> Result<(), Box<dyn std::error::Error>> {
    let exe_path = mmatrix_ui_exe_path()
        .map_err(|error| format!("failed to locate mmatrix-ui.exe: {error}"))?;
    let local_app_data = unique_local_app_data_path();
    let app = launch_app_with_options(
        &exe_path,
        &[OsString::from("--automation")],
        launch_options(Some(local_app_data.clone())),
    )?;
    app.wait_for_lifecycle_state("core_started", Duration::from_secs(30))?;
    app.load_events(&serde_json::to_string(&base_events())?)?;
    app.wait_idle(Duration::from_secs(5))?;

    let before = app.dump_ui_state()?;
    app.join_room_via_room_menu("#ops:example.org")?;
    app.wait_idle(Duration::from_secs(5))?;

    let model = app.dump_model()?;
    assert_eq!(model.status, "join submitted: #ops:example.org");
    assert_eq!(app.compose_text()?, "");
    assert_eq!(
        model.submitted_membership_commands,
        vec![SubmittedMembershipCommand {
            action: MembershipCommandAction::Join,
            target: "#ops:example.org".to_owned(),
        }]
    );
    assert!(
        model
            .rooms
            .iter()
            .all(|room| room.room_id != "#ops:example.org")
    );
    let ui_state = app.dump_ui_state()?;
    assert_eq!(ui_state.mdi.active, before.mdi.active);
    assert!(
        ui_state
            .mdi
            .views
            .iter()
            .all(|view| view.identity != room_identity("#ops:example.org"))
    );
    assert!(
        ui_state
            .switchbar
            .items
            .iter()
            .all(|item| item.title != "#ops:example.org")
    );
    assert!(!serde_json::to_string(&ui_state.navigation)?.contains("#ops:example.org"));

    drop(app);
    let _ = fs::remove_dir_all(&local_app_data);
    Ok(())
}

#[test]
fn automation_driver_room_menu_query_records_submitted_direct_room_without_creating_room()
-> Result<(), Box<dyn std::error::Error>> {
    let exe_path = mmatrix_ui_exe_path()
        .map_err(|error| format!("failed to locate mmatrix-ui.exe: {error}"))?;
    let local_app_data = unique_local_app_data_path();
    let app = launch_app_with_options(
        &exe_path,
        &[OsString::from("--automation")],
        launch_options(Some(local_app_data.clone())),
    )?;
    app.wait_for_lifecycle_state("core_started", Duration::from_secs(30))?;
    app.load_events(&serde_json::to_string(&base_events())?)?;
    app.wait_idle(Duration::from_secs(5))?;

    let before_rooms = app.list_rooms()?;
    let before_state = app.dump_ui_state()?;
    app.query_user_via_room_menu("@alice:example.org")?;
    app.wait_idle(Duration::from_secs(5))?;

    let model = app.dump_model()?;
    assert_eq!(model.status, "query submitted: @alice:example.org");
    assert_eq!(app.compose_text()?, "");
    assert_eq!(
        model.submitted_direct_room_commands,
        vec![SubmittedDirectRoomCommand {
            user_id: "@alice:example.org".to_owned(),
        }]
    );
    assert_eq!(app.list_rooms()?, before_rooms);
    let ui_state = app.dump_ui_state()?;
    assert_eq!(ui_state.rooms, before_state.rooms);
    assert_eq!(
        ui_state
            .mdi
            .views
            .iter()
            .map(|view| &view.identity)
            .collect::<Vec<_>>(),
        before_state
            .mdi
            .views
            .iter()
            .map(|view| &view.identity)
            .collect::<Vec<_>>()
    );

    drop(app);
    let _ = fs::remove_dir_all(&local_app_data);
    Ok(())
}

#[test]
fn automation_driver_invited_rooms_render_in_navigation_model_and_ui_state()
-> Result<(), Box<dyn std::error::Error>> {
    let exe_path = mmatrix_ui_exe_path()
        .map_err(|error| format!("failed to locate mmatrix-ui.exe: {error}"))?;
    let local_app_data = unique_local_app_data_path();
    let options = launch_options(Some(local_app_data.clone()));
    let args = [OsString::from("--automation")];

    let app = launch_app_with_options(&exe_path, &args, options)?;
    app.wait_for_lifecycle_state("core_started", Duration::from_secs(30))?;
    app.load_events(&serde_json::to_string(&invite_events())?)?;
    app.wait_idle(Duration::from_secs(5))?;

    let listed_rooms = app.list_rooms()?;
    assert!(listed_rooms.contains(&listed_room("!beta:example", "Beta", false)));
    assert!(listed_rooms.contains(&listed_invited_room("!invite:example", "Lobby", false)));

    let initial_model = app.dump_model()?;
    assert!(initial_model.rooms.iter().any(|room| {
        room.room_id == "!invite:example" && room.membership == RoomMembership::Invite
    }));

    let initial_state = app.dump_ui_state()?;
    assert_eq!(initial_state.mdi.active, room_identity("!beta:example"));
    assert!(navigation_contains(&initial_state.navigation, "rooms: 1"));
    assert!(navigation_contains(&initial_state.navigation, "invites: 1"));
    assert!(navigation_contains(
        &initial_state.navigation,
        "Lobby [invite]"
    ));
    assert_eq!(
        find_ui_room(&initial_state, "!invite:example").membership,
        RoomMembership::Invite
    );
    assert_eq!(
        find_switchbar_item(&initial_state, &room_identity("!invite:example")).title,
        "Lobby [invite]"
    );

    app.select_navigation_path(&nav_path(&["mmatrix", "invites: 1", "Lobby [invite]"]))?;
    app.wait_idle(Duration::from_secs(5))?;

    let invite_model = app.dump_model()?;
    assert_eq!(
        invite_model.selected_room.as_deref(),
        Some("!invite:example")
    );

    let invite_state = app.dump_ui_state()?;
    assert_eq!(invite_state.mdi.active, room_identity("!invite:example"));
    assert_active_switchbar_item(&invite_state, &room_identity("!invite:example"));
    assert_eq!(
        app.selected_navigation_path()?,
        nav_path(&["mmatrix", "invites: 1", "Lobby [invite]"])
    );
    assert_eq!(
        find_switchbar_item(&invite_state, &room_identity("!invite:example")).title,
        "Lobby [invite]"
    );
    let invite_view = find_view(&invite_state, &room_identity("!invite:example"));
    assert_eq!(invite_view.title, "Lobby [invite]");
    assert!(view_contains(invite_view, "Invitation to Lobby"));
    assert!(view_contains(
        invite_view,
        "Use /accept or /reject to respond."
    ));
    assert!(invite_view.members.is_empty());
    assert!(
        invite_state
            .status_bar
            .text
            .contains("invited: Lobby (!invite:example)")
    );

    drop(app);
    let _ = fs::remove_dir_all(&local_app_data);
    Ok(())
}

#[test]
fn automation_driver_invited_room_is_navigable_but_not_sendable()
-> Result<(), Box<dyn std::error::Error>> {
    let exe_path = mmatrix_ui_exe_path()
        .map_err(|error| format!("failed to locate mmatrix-ui.exe: {error}"))?;
    let local_app_data = unique_local_app_data_path();
    let options = launch_options(Some(local_app_data.clone()));
    let args = [OsString::from("--automation")];

    let app = launch_app_with_options(&exe_path, &args, options)?;
    app.wait_for_lifecycle_state("core_started", Duration::from_secs(30))?;
    app.load_events(&serde_json::to_string(&invite_events())?)?;
    app.wait_idle(Duration::from_secs(5))?;
    app.select_room("!invite:example")?;

    let before = app.dump_model()?;
    assert_eq!(before.selected_room.as_deref(), Some("!invite:example"));
    assert!(app.room_activity(Some("!invite:example"))?.is_empty());

    app.set_compose_text("hello invite")?;
    app.send_compose_key(ComposeKey::Return)?;
    app.wait_idle(Duration::from_secs(5))?;
    assert_eq!(app.compose_text()?, "hello invite");

    let after_text = app.dump_model()?;
    assert_eq!(after_text.status, "room is not joined: !invite:example");
    assert_eq!(after_text.last_error, None);
    assert!(after_text.submitted_membership_commands.is_empty());
    assert!(after_text.submitted_room_topic_commands.is_empty());
    assert!(after_text.submitted_reaction_commands.is_empty());
    assert!(after_text.submitted_reply_commands.is_empty());
    assert!(app.room_activity(Some("!invite:example"))?.is_empty());
    assert!(
        app.dump_ui_state()?
            .status_bar
            .text
            .contains("invited: Lobby (!invite:example)")
    );

    app.set_compose_text("/topic incident review")?;
    app.send_compose_key(ComposeKey::Return)?;
    app.wait_idle(Duration::from_secs(5))?;
    assert_eq!(app.compose_text()?, "");
    let after_topic = app.dump_model()?;
    assert_eq!(after_topic.status, "room is not joined: !invite:example");
    assert!(after_topic.submitted_room_topic_commands.is_empty());

    app.set_compose_text("/reply $invite-root hello there")?;
    app.send_compose_key(ComposeKey::Return)?;
    app.wait_idle(Duration::from_secs(5))?;
    assert_eq!(app.compose_text()?, "");
    let after_reply = app.dump_model()?;
    assert_eq!(after_reply.status, "room is not joined: !invite:example");
    assert!(after_reply.submitted_reply_commands.is_empty());

    app.set_compose_text("/react $invite-root +1")?;
    app.send_compose_key(ComposeKey::Return)?;
    app.wait_idle(Duration::from_secs(5))?;
    assert_eq!(app.compose_text()?, "");
    let after_react = app.dump_model()?;
    assert_eq!(after_react.status, "room is not joined: !invite:example");
    assert!(after_react.submitted_reaction_commands.is_empty());

    app.set_compose_text("/part")?;
    app.send_compose_key(ComposeKey::Return)?;
    app.wait_idle(Duration::from_secs(5))?;
    assert_eq!(app.compose_text()?, "");
    let after_part = app.dump_model()?;
    assert_eq!(after_part.status, "room is not joined: !invite:example");
    assert!(after_part.submitted_membership_commands.is_empty());
    assert_eq!(after_part.last_error, None);
    assert!(
        app.dump_ui_state()?
            .status_bar
            .text
            .contains("invited: Lobby (!invite:example)")
    );

    drop(app);
    let _ = fs::remove_dir_all(&local_app_data);
    Ok(())
}

#[test]
fn automation_driver_invited_room_list_fixture_renders_expected_invite_state()
-> Result<(), Box<dyn std::error::Error>> {
    let exe_path = mmatrix_ui_exe_path()
        .map_err(|error| format!("failed to locate mmatrix-ui.exe: {error}"))?;
    let fixture_path = fixture_state_path("invited-room-list.json");
    let local_app_data = unique_local_app_data_path();
    let options = launch_options(Some(local_app_data.clone()));
    let args = [OsString::from("--automation")];

    let app = launch_app_with_options(&exe_path, &args, options)?;
    app.wait_for_lifecycle_state("core_started", Duration::from_secs(30))?;
    app.load_events(&fs::read_to_string(&fixture_path)?)?;
    app.wait_idle(Duration::from_secs(5))?;
    app.select_room("!invite:example")?;
    app.wait_idle(Duration::from_secs(5))?;

    let model = app.dump_model()?;
    assert!(model.rooms.iter().any(|room| {
        room.room_id == "!invite:example" && room.membership == RoomMembership::Invite
    }));

    let state = app.dump_ui_state()?;
    assert_eq!(state.mdi.active, room_identity("!invite:example"));
    assert_active_switchbar_item(&state, &room_identity("!invite:example"));
    assert!(navigation_contains(&state.navigation, "rooms: 1"));
    assert!(navigation_contains(&state.navigation, "invites: 1"));
    assert!(navigation_contains(&state.navigation, "Lobby [invite]"));
    assert_eq!(
        find_ui_room(&state, "!invite:example").membership,
        RoomMembership::Invite
    );
    assert_eq!(
        find_switchbar_item(&state, &room_identity("!invite:example")).title,
        "Lobby [invite]"
    );
    let invite_view = find_view(&state, &room_identity("!invite:example"));
    assert_eq!(invite_view.title, "Lobby [invite]");
    assert!(view_contains(invite_view, "Invitation to Lobby"));
    assert!(view_contains(
        invite_view,
        "Use /accept or /reject to respond."
    ));
    assert!(
        state
            .status_bar
            .text
            .contains("invited: Lobby (!invite:example)")
    );

    drop(app);
    let _ = fs::remove_dir_all(&local_app_data);
    Ok(())
}

#[test]
fn automation_driver_invited_direct_room_fixture_renders_expected_direct_invite_state()
-> Result<(), Box<dyn std::error::Error>> {
    let exe_path = mmatrix_ui_exe_path()
        .map_err(|error| format!("failed to locate mmatrix-ui.exe: {error}"))?;
    let fixture_path = fixture_state_path("invited-direct-room.json");
    let local_app_data = unique_local_app_data_path();
    let options = launch_options(Some(local_app_data.clone()));
    let args = [OsString::from("--automation")];

    let app = launch_app_with_options(&exe_path, &args, options)?;
    app.wait_for_lifecycle_state("core_started", Duration::from_secs(30))?;
    app.load_events(&fs::read_to_string(&fixture_path)?)?;
    app.wait_idle(Duration::from_secs(5))?;
    app.select_room("!invite-direct:example")?;
    app.wait_idle(Duration::from_secs(5))?;

    let model = app.dump_model()?;
    assert!(model.rooms.iter().any(|room| {
        room.room_id == "!invite-direct:example"
            && room.membership == RoomMembership::Invite
            && room.is_direct
            && room.direct_user_ids == ["@daria:example"]
    }));

    let state = app.dump_ui_state()?;
    assert_eq!(state.mdi.active, room_identity("!invite-direct:example"));
    assert_active_switchbar_item(&state, &room_identity("!invite-direct:example"));
    assert!(navigation_contains(&state.navigation, "rooms: 1"));
    assert!(navigation_contains(&state.navigation, "invites: 1"));
    assert!(navigation_contains(
        &state.navigation,
        "Daria [invite] [query: @daria:example]"
    ));
    let invite_room = find_ui_room(&state, "!invite-direct:example");
    assert_eq!(invite_room.membership, RoomMembership::Invite);
    assert!(invite_room.is_direct);
    assert_eq!(
        invite_room.direct_user_ids,
        vec!["@daria:example".to_owned()]
    );
    assert_eq!(
        find_switchbar_item(&state, &room_identity("!invite-direct:example")).title,
        "Daria [invite] [query: @daria:example]"
    );
    let invite_view = find_view(&state, &room_identity("!invite-direct:example"));
    assert!(invite_view.title.contains("Daria [invite]"));
    assert!(view_contains(invite_view, "Invitation to Daria"));
    assert!(view_contains(
        invite_view,
        "Use /accept or /reject to respond."
    ));
    assert!(
        state
            .status_bar
            .text
            .contains("invited: Daria (!invite-direct:example)")
    );

    drop(app);
    let _ = fs::remove_dir_all(&local_app_data);
    Ok(())
}

#[test]
fn automation_driver_invited_room_accept_fixture_update_promotes_invite_to_joined_room()
-> Result<(), Box<dyn std::error::Error>> {
    let exe_path = mmatrix_ui_exe_path()
        .map_err(|error| format!("failed to locate mmatrix-ui.exe: {error}"))?;
    let fixture_path = fixture_state_path("invited-room-accept.json");
    let update_path = fixture_update_path("invited-room-accept.json");
    let local_app_data = unique_local_app_data_path();
    let options = launch_options(Some(local_app_data.clone()));
    let args = [OsString::from("--automation")];

    let app = launch_app_with_options(&exe_path, &args, options)?;
    app.wait_for_lifecycle_state("core_started", Duration::from_secs(30))?;
    app.load_events(&fs::read_to_string(&fixture_path)?)?;
    app.wait_idle(Duration::from_secs(5))?;
    app.select_room("!invite:example")?;
    app.wait_idle(Duration::from_secs(5))?;
    app.append_synthetic_events(&fs::read_to_string(&update_path)?)?;
    app.wait_idle(Duration::from_secs(5))?;

    let model = app.dump_model()?;
    assert!(model.rooms.iter().any(|room| {
        room.room_id == "!invite:example" && room.membership == RoomMembership::Join
    }));

    let state = app.dump_ui_state()?;
    assert_eq!(state.mdi.active, room_identity("!invite:example"));
    assert_active_switchbar_item(&state, &room_identity("!invite:example"));
    assert!(navigation_contains(&state.navigation, "rooms: 2"));
    assert!(navigation_contains(&state.navigation, "invites: 0"));
    assert!(navigation_contains(&state.navigation, "Lobby"));
    assert_navigation_not_contains(&state.navigation, "Lobby [invite]");
    assert_eq!(
        find_ui_room(&state, "!invite:example").membership,
        RoomMembership::Join
    );
    assert_eq!(
        find_switchbar_item(&state, &room_identity("!invite:example")).title,
        "Lobby"
    );
    let joined_view = find_view(&state, &room_identity("!invite:example"));
    assert_eq!(joined_view.title, "Lobby");
    assert!(view_contains(joined_view, "Room: Lobby (!invite:example)"));
    assert!(!view_contains(
        joined_view,
        "Use /accept or /reject to respond."
    ));

    drop(app);
    let _ = fs::remove_dir_all(&local_app_data);
    Ok(())
}

#[test]
fn automation_driver_invited_room_reject_fixture_update_removes_invite_room()
-> Result<(), Box<dyn std::error::Error>> {
    let exe_path = mmatrix_ui_exe_path()
        .map_err(|error| format!("failed to locate mmatrix-ui.exe: {error}"))?;
    let fixture_path = fixture_state_path("invited-room-reject.json");
    let update_path = fixture_update_path("invited-room-reject.json");
    let local_app_data = unique_local_app_data_path();
    let options = launch_options(Some(local_app_data.clone()));
    let args = [OsString::from("--automation")];

    let app = launch_app_with_options(&exe_path, &args, options)?;
    app.wait_for_lifecycle_state("core_started", Duration::from_secs(30))?;
    app.load_events(&fs::read_to_string(&fixture_path)?)?;
    app.wait_idle(Duration::from_secs(5))?;
    app.select_room("!invite:example")?;
    app.wait_idle(Duration::from_secs(5))?;
    app.append_synthetic_events(&fs::read_to_string(&update_path)?)?;
    app.wait_idle(Duration::from_secs(5))?;

    let model = app.dump_model()?;
    assert_eq!(
        model
            .rooms
            .iter()
            .map(|room| room.room_id.as_str())
            .collect::<Vec<_>>(),
        vec!["!beta:example"]
    );

    let state = app.dump_ui_state()?;
    assert_eq!(state.mdi.active, room_identity("!beta:example"));
    assert_active_switchbar_item(&state, &room_identity("!beta:example"));
    assert!(navigation_contains(&state.navigation, "rooms: 1"));
    assert!(navigation_contains(&state.navigation, "invites: 0"));
    assert_navigation_not_contains(&state.navigation, "Lobby [invite]");
    assert!(
        state
            .rooms
            .iter()
            .all(|room| room.room_id != "!invite:example")
    );
    assert!(
        state
            .mdi
            .views
            .iter()
            .all(|view| view.identity != room_identity("!invite:example"))
    );
    assert!(
        state
            .switchbar
            .items
            .iter()
            .all(|item| item.identity != room_identity("!invite:example"))
    );

    drop(app);
    let _ = fs::remove_dir_all(&local_app_data);
    Ok(())
}

#[test]
fn automation_driver_hierarchical_tree_ignores_invited_spaces()
-> Result<(), Box<dyn std::error::Error>> {
    let exe_path = mmatrix_ui_exe_path()
        .map_err(|error| format!("failed to locate mmatrix-ui.exe: {error}"))?;
    let local_app_data = unique_local_app_data_path();
    let options = launch_options(Some(local_app_data.clone()));
    let args = [OsString::from("--automation")];

    let app = launch_app_with_options(&exe_path, &args, options)?;
    app.wait_for_lifecycle_state("core_started", Duration::from_secs(30))?;
    app.load_events(&serde_json::to_string(&hierarchical_invite_space_events())?)?;
    app.wait_idle(Duration::from_secs(5))?;

    let state = app.dump_ui_state()?;
    assert!(navigation_contains(&state.navigation, "rooms: 1"));
    assert!(navigation_contains(&state.navigation, "Space"));
    assert!(navigation_contains(&state.navigation, "Beta"));
    assert!(navigation_contains(&state.navigation, "invites: 0"));
    assert_navigation_not_contains(&state.navigation, "Invite Space");

    drop(app);
    let _ = fs::remove_dir_all(&local_app_data);
    Ok(())
}

#[test]
fn automation_driver_slash_accept_keeps_invite_until_synced_rooms_join()
-> Result<(), Box<dyn std::error::Error>> {
    let exe_path = mmatrix_ui_exe_path()
        .map_err(|error| format!("failed to locate mmatrix-ui.exe: {error}"))?;
    let local_app_data = unique_local_app_data_path();
    let options = launch_options(Some(local_app_data.clone()));
    let args = [OsString::from("--automation")];

    let app = launch_app_with_options(&exe_path, &args, options)?;
    app.wait_for_lifecycle_state("core_started", Duration::from_secs(30))?;
    app.load_events(&serde_json::to_string(&invite_events())?)?;
    app.wait_idle(Duration::from_secs(5))?;
    app.select_room("!invite:example")?;

    app.set_compose_text("/accept")?;
    app.send_compose_key(ComposeKey::Return)?;
    app.wait_idle(Duration::from_secs(5))?;
    assert_eq!(app.compose_text()?, "");

    let accepted = app.dump_model()?;
    assert_eq!(accepted.status, "accept submitted: !invite:example");
    assert_eq!(
        accepted.submitted_membership_commands,
        vec![SubmittedMembershipCommand {
            action: MembershipCommandAction::AcceptInvite,
            target: "!invite:example".to_owned(),
        }]
    );
    assert!(accepted.rooms.iter().any(|room| {
        room.room_id == "!invite:example" && room.membership == RoomMembership::Invite
    }));
    assert!(
        app.dump_ui_state()?
            .status_bar
            .text
            .contains("invited: Lobby (!invite:example)")
    );

    app.append_synthetic_events(&serde_json::to_string(&vec![Event::SyncedRooms(vec![
        room_summary("!beta:example", "Beta", false),
        room_summary("!invite:example", "Lobby", false),
    ])])?)?;
    app.wait_idle(Duration::from_secs(5))?;

    let joined = app.dump_model()?;
    assert!(joined.rooms.iter().any(|room| {
        room.room_id == "!invite:example" && room.membership == RoomMembership::Join
    }));
    let joined_state = app.dump_ui_state()?;
    assert_eq!(joined_state.mdi.active, room_identity("!invite:example"));
    assert_active_switchbar_item(&joined_state, &room_identity("!invite:example"));
    assert_eq!(
        app.selected_navigation_path()?,
        nav_path(&["mmatrix", "rooms: 2", "Lobby"])
    );
    assert_eq!(
        find_switchbar_item(&joined_state, &room_identity("!invite:example")).title,
        "Lobby"
    );
    let joined_view = find_view(&joined_state, &room_identity("!invite:example"));
    assert_eq!(joined_view.title, "Lobby");
    assert!(!view_contains(
        joined_view,
        "Use /accept or /reject to respond."
    ));

    drop(app);
    let _ = fs::remove_dir_all(&local_app_data);
    Ok(())
}

#[test]
fn automation_driver_slash_reject_keeps_invite_until_synced_rooms_removal()
-> Result<(), Box<dyn std::error::Error>> {
    let exe_path = mmatrix_ui_exe_path()
        .map_err(|error| format!("failed to locate mmatrix-ui.exe: {error}"))?;
    let local_app_data = unique_local_app_data_path();
    let options = launch_options(Some(local_app_data.clone()));
    let args = [OsString::from("--automation")];

    let app = launch_app_with_options(&exe_path, &args, options)?;
    app.wait_for_lifecycle_state("core_started", Duration::from_secs(30))?;
    app.load_events(&serde_json::to_string(&invite_events())?)?;
    app.wait_idle(Duration::from_secs(5))?;
    app.select_room("!invite:example")?;

    app.set_compose_text("/reject")?;
    app.send_compose_key(ComposeKey::Return)?;
    app.wait_idle(Duration::from_secs(5))?;
    assert_eq!(app.compose_text()?, "");

    let rejected = app.dump_model()?;
    assert_eq!(rejected.status, "reject submitted: !invite:example");
    assert_eq!(
        rejected.submitted_membership_commands,
        vec![SubmittedMembershipCommand {
            action: MembershipCommandAction::RejectInvite,
            target: "!invite:example".to_owned(),
        }]
    );
    assert!(rejected.rooms.iter().any(|room| {
        room.room_id == "!invite:example" && room.membership == RoomMembership::Invite
    }));
    assert_eq!(rejected.selected_room.as_deref(), Some("!invite:example"));

    app.append_synthetic_events(&serde_json::to_string(&vec![Event::SyncedRooms(vec![
        room_summary("!beta:example", "Beta", false),
    ])])?)?;
    app.wait_idle(Duration::from_secs(5))?;

    let replaced_model = app.dump_model()?;
    assert_eq!(
        replaced_model
            .rooms
            .iter()
            .map(|room| room.room_id.as_str())
            .collect::<Vec<_>>(),
        vec!["!beta:example"]
    );
    let replaced_state = app.dump_ui_state()?;
    assert_eq!(replaced_state.mdi.active, room_identity("!beta:example"));
    assert_active_switchbar_item(&replaced_state, &room_identity("!beta:example"));
    assert_eq!(
        app.selected_navigation_path()?,
        nav_path(&["mmatrix", "rooms: 1", "Beta"])
    );
    assert_navigation_not_contains(&replaced_state.navigation, "Lobby [invite]");
    assert!(
        replaced_state
            .mdi
            .views
            .iter()
            .all(|view| view.identity != room_identity("!invite:example"))
    );
    assert!(
        replaced_state
            .switchbar
            .items
            .iter()
            .all(|item| item.identity != room_identity("!invite:example"))
    );

    drop(app);
    let _ = fs::remove_dir_all(&local_app_data);
    Ok(())
}

#[test]
fn automation_driver_slash_accept_and_reject_targets_only_resolve_invites()
-> Result<(), Box<dyn std::error::Error>> {
    let exe_path = mmatrix_ui_exe_path()
        .map_err(|error| format!("failed to locate mmatrix-ui.exe: {error}"))?;
    let local_app_data = unique_local_app_data_path();
    let options = launch_options(Some(local_app_data.clone()));
    let args = [OsString::from("--automation")];

    let app = launch_app_with_options(&exe_path, &args, options)?;
    app.wait_for_lifecycle_state("core_started", Duration::from_secs(30))?;
    app.load_events(&serde_json::to_string(&invite_target_events())?)?;
    app.wait_idle(Duration::from_secs(5))?;

    app.set_compose_text("/accept !invite-target:example")?;
    app.send_compose_key(ComposeKey::Return)?;
    app.wait_idle(Duration::from_secs(5))?;
    assert_eq!(app.compose_text()?, "");
    let accepted = app.dump_model()?;
    assert_eq!(accepted.status, "accept submitted: !invite-target:example");
    assert_eq!(
        accepted.submitted_membership_commands,
        vec![SubmittedMembershipCommand {
            action: MembershipCommandAction::AcceptInvite,
            target: "!invite-target:example".to_owned(),
        }]
    );

    app.set_compose_text("/reject Lobby")?;
    app.send_compose_key(ComposeKey::Return)?;
    app.wait_idle(Duration::from_secs(5))?;
    assert_eq!(app.compose_text()?, "");
    let rejected = app.dump_model()?;
    assert_eq!(rejected.status, "reject submitted: !invite-target:example");
    assert_eq!(
        rejected.submitted_membership_commands,
        vec![
            SubmittedMembershipCommand {
                action: MembershipCommandAction::AcceptInvite,
                target: "!invite-target:example".to_owned(),
            },
            SubmittedMembershipCommand {
                action: MembershipCommandAction::RejectInvite,
                target: "!invite-target:example".to_owned(),
            },
        ]
    );

    app.set_compose_text("/accept Shared")?;
    app.send_compose_key(ComposeKey::Return)?;
    app.wait_idle(Duration::from_secs(5))?;
    assert_eq!(app.compose_text()?, "");
    let ambiguous = app.dump_model()?;
    assert_eq!(
        ambiguous.status,
        "accept failed: ambiguous invited room target: Shared"
    );
    assert_eq!(
        ambiguous.submitted_membership_commands,
        rejected.submitted_membership_commands
    );

    app.set_compose_text("/reject !beta:example")?;
    app.send_compose_key(ComposeKey::Return)?;
    app.wait_idle(Duration::from_secs(5))?;
    assert_eq!(app.compose_text()?, "");
    let joined_target = app.dump_model()?;
    assert_eq!(
        joined_target.status,
        "reject failed: room is not invited: !beta:example"
    );
    assert_eq!(
        joined_target.submitted_membership_commands,
        rejected.submitted_membership_commands
    );

    app.set_compose_text("/accept Missing")?;
    app.send_compose_key(ComposeKey::Return)?;
    app.wait_idle(Duration::from_secs(5))?;
    assert_eq!(app.compose_text()?, "");
    let missing = app.dump_model()?;
    assert_eq!(
        missing.status,
        "accept failed: invited room not found: Missing"
    );
    assert_eq!(
        missing.submitted_membership_commands,
        rejected.submitted_membership_commands
    );

    app.select_room("!beta:example")?;
    app.set_compose_text("/reject")?;
    app.send_compose_key(ComposeKey::Return)?;
    app.wait_idle(Duration::from_secs(5))?;
    assert_eq!(app.compose_text()?, "");
    let no_target = app.dump_model()?;
    assert_eq!(no_target.status, "reject failed: no invited room selected");
    assert_eq!(
        no_target.submitted_membership_commands,
        rejected.submitted_membership_commands
    );

    drop(app);
    let _ = fs::remove_dir_all(&local_app_data);
    Ok(())
}

#[test]
fn automation_driver_slash_join_part_and_verify_regressions_keep_original_command_types()
-> Result<(), Box<dyn std::error::Error>> {
    let exe_path = mmatrix_ui_exe_path()
        .map_err(|error| format!("failed to locate mmatrix-ui.exe: {error}"))?;
    let local_app_data = unique_local_app_data_path();
    let options = launch_options(Some(local_app_data.clone()));
    let args = [OsString::from("--automation")];

    let app = launch_app_with_options(&exe_path, &args, options)?;
    app.wait_for_lifecycle_state("core_started", Duration::from_secs(30))?;
    app.load_events(&serde_json::to_string(&vec![
        Event::Started,
        Event::SyncedRooms(vec![
            room_summary("!beta:example", "Beta", false),
            room_summary("!alpha:example", "Alpha", false),
        ]),
        text_event(
            "!beta:example",
            "@bob:example",
            1_700_000_120_000,
            "beta latest",
        ),
        verification_state_event(
            "flow-current",
            "@alice:example",
            CoreVerificationState::SasReady {
                emojis: vec![CoreVerificationEmoji {
                    symbol: "🐶".to_owned(),
                    description: "dog".to_owned(),
                }],
                decimals: CoreVerificationDecimals {
                    first: 1234,
                    second: 5678,
                    third: 9012,
                },
            },
        ),
    ])?)?;
    app.wait_idle(Duration::from_secs(5))?;

    app.set_compose_text("/join #ops:example.org")?;
    app.send_compose_key(ComposeKey::Return)?;
    app.wait_idle(Duration::from_secs(5))?;
    assert_eq!(app.compose_text()?, "");

    app.set_compose_text("/part")?;
    app.send_compose_key(ComposeKey::Return)?;
    app.wait_idle(Duration::from_secs(5))?;
    assert_eq!(app.compose_text()?, "");

    app.set_compose_text("/verify accept")?;
    app.send_compose_key(ComposeKey::Return)?;
    app.wait_idle(Duration::from_secs(5))?;
    assert_eq!(app.compose_text()?, "");

    app.set_compose_text("/verify reject")?;
    app.send_compose_key(ComposeKey::Return)?;
    app.wait_idle(Duration::from_secs(5))?;
    assert_eq!(app.compose_text()?, "");

    let model = app.dump_model()?;
    assert_eq!(
        model.submitted_membership_commands,
        vec![
            SubmittedMembershipCommand {
                action: MembershipCommandAction::Join,
                target: "#ops:example.org".to_owned(),
            },
            SubmittedMembershipCommand {
                action: MembershipCommandAction::Leave,
                target: "!beta:example".to_owned(),
            },
        ]
    );
    assert_eq!(
        model.submitted_verification_actions,
        vec![
            SubmittedVerificationAction {
                action: VerificationActionKind::Accept,
                flow_id: "flow-current".to_owned(),
                user_id: "@alice:example".to_owned(),
            },
            SubmittedVerificationAction {
                action: VerificationActionKind::RejectSas,
                flow_id: "flow-current".to_owned(),
                user_id: "@alice:example".to_owned(),
            },
        ]
    );

    drop(app);
    let _ = fs::remove_dir_all(&local_app_data);
    Ok(())
}

#[test]
fn automation_driver_slash_query_opens_existing_direct_room_match()
-> Result<(), Box<dyn std::error::Error>> {
    let exe_path = mmatrix_ui_exe_path()
        .map_err(|error| format!("failed to locate mmatrix-ui.exe: {error}"))?;
    let local_app_data = unique_local_app_data_path();
    let options = launch_options(Some(local_app_data.clone()));
    let args = [OsString::from("--automation")];
    let events = vec![
        Event::Started,
        Event::SyncedRooms(vec![
            room_summary("!beta:example", "Beta", true),
            RoomSummary {
                room_id: "!direct:example".to_owned(),
                display_name: "Direct".to_owned(),
                topic: None,
                membership: RoomMembership::Join,
                encrypted: false,
                is_space: false,
                is_direct: true,
                direct_user_ids: vec!["@alice:example.org".to_owned()],
                tags: Vec::new(),
                space_children: Vec::new(),
            },
        ]),
        text_event(
            "!direct:example",
            "@alice:example.org",
            1_700_000_000_000,
            "direct first",
        ),
        text_event(
            "!beta:example",
            "@bob:example",
            1_700_000_060_000,
            "beta first",
        ),
    ];

    let app = launch_app_with_options(&exe_path, &args, options)?;
    app.wait_for_lifecycle_state("core_started", Duration::from_secs(30))?;
    app.load_events(&serde_json::to_string(&events)?)?;
    app.wait_idle(Duration::from_secs(5))?;
    assert_eq!(
        app.dump_ui_state()?.mdi.active,
        room_identity("!beta:example")
    );

    app.set_compose_text("/query @alice:example.org")?;
    app.send_compose_key(ComposeKey::Return)?;
    app.wait_idle(Duration::from_secs(5))?;
    assert_eq!(app.compose_text()?, "");

    let model = app.dump_model()?;
    assert_eq!(model.status, "query opened: @alice:example.org");
    assert!(model.submitted_direct_room_commands.is_empty());

    let ui_state = app.dump_ui_state()?;
    assert_eq!(ui_state.mdi.active, room_identity("!direct:example"));
    assert_active_switchbar_item(&ui_state, &room_identity("!direct:example"));

    drop(app);
    let _ = fs::remove_dir_all(&local_app_data);
    Ok(())
}

#[test]
fn automation_driver_slash_query_reports_ambiguous_existing_direct_room_matches()
-> Result<(), Box<dyn std::error::Error>> {
    let exe_path = mmatrix_ui_exe_path()
        .map_err(|error| format!("failed to locate mmatrix-ui.exe: {error}"))?;
    let local_app_data = unique_local_app_data_path();
    let options = launch_options(Some(local_app_data.clone()));
    let args = [OsString::from("--automation")];
    let events = vec![
        Event::Started,
        Event::SyncedRooms(vec![
            room_summary("!beta:example", "Beta", true),
            RoomSummary {
                room_id: "!direct-one:example".to_owned(),
                display_name: "Direct One".to_owned(),
                topic: None,
                membership: RoomMembership::Join,
                encrypted: false,
                is_space: false,
                is_direct: true,
                direct_user_ids: vec!["@alice:example.org".to_owned()],
                tags: Vec::new(),
                space_children: Vec::new(),
            },
            RoomSummary {
                room_id: "!direct-two:example".to_owned(),
                display_name: "Direct Two".to_owned(),
                topic: None,
                membership: RoomMembership::Join,
                encrypted: false,
                is_space: false,
                is_direct: true,
                direct_user_ids: vec!["@alice:example.org".to_owned()],
                tags: Vec::new(),
                space_children: Vec::new(),
            },
        ]),
        text_event(
            "!direct-one:example",
            "@alice:example.org",
            1_700_000_000_000,
            "direct one",
        ),
        text_event(
            "!direct-two:example",
            "@alice:example.org",
            1_700_000_030_000,
            "direct two",
        ),
        text_event(
            "!beta:example",
            "@bob:example",
            1_700_000_060_000,
            "beta first",
        ),
    ];

    let app = launch_app_with_options(&exe_path, &args, options)?;
    app.wait_for_lifecycle_state("core_started", Duration::from_secs(30))?;
    app.load_events(&serde_json::to_string(&events)?)?;
    app.wait_idle(Duration::from_secs(5))?;
    assert_eq!(
        app.dump_ui_state()?.mdi.active,
        room_identity("!beta:example")
    );

    app.set_compose_text("/query @alice:example.org")?;
    app.send_compose_key(ComposeKey::Return)?;
    app.wait_idle(Duration::from_secs(5))?;
    assert_eq!(app.compose_text()?, "");

    let model = app.dump_model()?;
    assert_eq!(model.status, "ambiguous query target: @alice:example.org");
    assert!(model.submitted_direct_room_commands.is_empty());

    let ui_state = app.dump_ui_state()?;
    assert_eq!(ui_state.mdi.active, room_identity("!beta:example"));
    assert_active_switchbar_item(&ui_state, &room_identity("!beta:example"));

    drop(app);
    let _ = fs::remove_dir_all(&local_app_data);
    Ok(())
}

#[test]
fn automation_driver_literal_query_submission_is_sent_as_text()
-> Result<(), Box<dyn std::error::Error>> {
    let exe_path = mmatrix_ui_exe_path()
        .map_err(|error| format!("failed to locate mmatrix-ui.exe: {error}"))?;
    let local_app_data = unique_local_app_data_path();
    let options = launch_options(Some(local_app_data.clone()));
    let args = [OsString::from("--automation")];

    let app = launch_app_with_options(&exe_path, &args, options)?;
    app.wait_for_lifecycle_state("core_started", Duration::from_secs(30))?;
    app.load_events(&serde_json::to_string(&base_events())?)?;
    app.wait_idle(Duration::from_secs(5))?;
    app.select_room("!beta:example")?;

    app.set_compose_text("//query @alice:example.org")?;
    app.send_compose_key(ComposeKey::Return)?;
    assert_eq!(app.compose_text()?, "");
    app.wait_for_lifecycle_state("error", Duration::from_secs(30))?;
    assert_eq!(
        app.dump_model()?.last_error,
        Some(UiError {
            operation: "send_text".to_owned(),
            message: "send message requires an authenticated session".to_owned(),
        })
    );
    assert!(app.dump_model()?.submitted_direct_room_commands.is_empty());
    assert_eq!(
        app.dump_ui_state()?.mdi.active,
        room_identity("!beta:example")
    );
    assert!(
        !app.dump_ui_state()?
            .status_bar
            .text
            .contains("query submitted: @alice:example.org")
    );

    app.send_compose_key(ComposeKey::Up)?;
    assert_eq!(app.compose_text()?, "//query @alice:example.org");

    drop(app);
    let _ = fs::remove_dir_all(&local_app_data);
    Ok(())
}

#[test]
fn automation_driver_slash_query_existing_direct_room_activates_room_without_submission()
-> Result<(), Box<dyn std::error::Error>> {
    let exe_path = mmatrix_ui_exe_path()
        .map_err(|error| format!("failed to locate mmatrix-ui.exe: {error}"))?;
    let local_app_data = unique_local_app_data_path();
    let options = launch_options(Some(local_app_data.clone()));
    let args = [OsString::from("--automation")];

    let mut direct_room = room_summary("!direct:example", "Direct", true);
    direct_room.is_direct = true;
    direct_room.direct_user_ids = vec!["@alice:example.org".to_owned()];

    let app = launch_app_with_options(&exe_path, &args, options)?;
    app.wait_for_lifecycle_state("core_started", Duration::from_secs(30))?;
    app.load_events(&serde_json::to_string(&vec![
        Event::Started,
        Event::SyncedRooms(vec![
            room_summary("!beta:example", "Beta", true),
            direct_room,
        ]),
        text_event(
            "!direct:example",
            "@alice:example.org",
            1_700_000_000_000,
            "hello from direct",
        ),
        text_event(
            "!beta:example",
            "@bob:example",
            1_700_000_120_000,
            "beta latest",
        ),
    ])?)?;
    app.wait_idle(Duration::from_secs(5))?;

    assert_eq!(
        app.dump_ui_state()?.mdi.active,
        room_identity("!beta:example")
    );

    app.set_compose_text("/query @alice:example.org")?;
    app.send_compose_key(ComposeKey::Return)?;
    app.wait_idle(Duration::from_secs(5))?;
    assert_eq!(app.compose_text()?, "");

    let model = app.dump_model()?;
    assert_eq!(model.status, "query opened: @alice:example.org");
    assert!(model.submitted_direct_room_commands.is_empty());
    assert_eq!(model.selected_room.as_deref(), Some("!direct:example"));
    assert_eq!(
        app.dump_ui_state()?.mdi.active,
        room_identity("!direct:example")
    );

    drop(app);
    let _ = fs::remove_dir_all(&local_app_data);
    Ok(())
}

#[test]
fn automation_driver_slash_query_ambiguous_direct_room_reports_local_error()
-> Result<(), Box<dyn std::error::Error>> {
    let exe_path = mmatrix_ui_exe_path()
        .map_err(|error| format!("failed to locate mmatrix-ui.exe: {error}"))?;
    let local_app_data = unique_local_app_data_path();
    let options = launch_options(Some(local_app_data.clone()));
    let args = [OsString::from("--automation")];

    let mut direct_one = room_summary("!direct-one:example", "Direct One", true);
    direct_one.is_direct = true;
    direct_one.direct_user_ids = vec!["@alice:example.org".to_owned()];

    let mut direct_two = room_summary("!direct-two:example", "Direct Two", true);
    direct_two.is_direct = true;
    direct_two.direct_user_ids = vec!["@alice:example.org".to_owned()];

    let app = launch_app_with_options(&exe_path, &args, options)?;
    app.wait_for_lifecycle_state("core_started", Duration::from_secs(30))?;
    app.load_events(&serde_json::to_string(&vec![
        Event::Started,
        Event::SyncedRooms(vec![
            room_summary("!beta:example", "Beta", true),
            direct_one,
            direct_two,
        ]),
        text_event(
            "!direct-one:example",
            "@alice:example.org",
            1_700_000_000_000,
            "first direct",
        ),
        text_event(
            "!direct-two:example",
            "@alice:example.org",
            1_700_000_060_000,
            "second direct",
        ),
        text_event(
            "!beta:example",
            "@bob:example",
            1_700_000_120_000,
            "beta latest",
        ),
    ])?)?;
    app.wait_idle(Duration::from_secs(5))?;

    assert_eq!(
        app.dump_ui_state()?.mdi.active,
        room_identity("!beta:example")
    );

    app.set_compose_text("/query @alice:example.org")?;
    app.send_compose_key(ComposeKey::Return)?;
    app.wait_idle(Duration::from_secs(5))?;
    assert_eq!(app.compose_text()?, "");

    let model = app.dump_model()?;
    assert_eq!(model.status, "ambiguous query target: @alice:example.org");
    assert!(model.submitted_direct_room_commands.is_empty());
    assert_eq!(model.selected_room.as_deref(), Some("!beta:example"));
    assert_eq!(
        app.dump_ui_state()?.mdi.active,
        room_identity("!beta:example")
    );

    drop(app);
    let _ = fs::remove_dir_all(&local_app_data);
    Ok(())
}

#[test]
fn automation_driver_slash_part_active_room_waits_for_synced_rooms_removal()
-> Result<(), Box<dyn std::error::Error>> {
    let exe_path = mmatrix_ui_exe_path()
        .map_err(|error| format!("failed to locate mmatrix-ui.exe: {error}"))?;
    let local_app_data = unique_local_app_data_path();
    let options = launch_options(Some(local_app_data.clone()));
    let args = [OsString::from("--automation")];

    let app = launch_app_with_options(&exe_path, &args, options)?;
    app.wait_for_lifecycle_state("core_started", Duration::from_secs(30))?;
    app.load_events(&serde_json::to_string(&base_events())?)?;
    app.wait_idle(Duration::from_secs(5))?;

    let before = app.dump_ui_state()?;
    assert_eq!(before.mdi.active, room_identity("!beta:example"));

    app.set_compose_text("/part")?;
    app.send_compose_key(ComposeKey::Return)?;
    app.wait_idle(Duration::from_secs(5))?;
    assert_eq!(app.compose_text()?, "");

    let model = app.dump_model()?;
    assert_eq!(model.status, "part submitted: !beta:example");
    assert_eq!(
        model.submitted_membership_commands,
        vec![SubmittedMembershipCommand {
            action: MembershipCommandAction::Leave,
            target: "!beta:example".to_owned(),
        }]
    );
    assert_eq!(
        app.dump_ui_state()?.mdi.active,
        room_identity("!beta:example")
    );

    app.load_events(&serde_json::to_string(&vec![
        Event::Started,
        Event::SyncedRooms(vec![room_summary("!alpha:example", "Alpha", false)]),
        text_event(
            "!alpha:example",
            "@alice:example",
            1_700_000_180_000,
            "alpha after part",
        ),
    ])?)?;
    app.wait_idle(Duration::from_secs(5))?;

    let replaced_model = app.dump_model()?;
    assert_eq!(
        replaced_model
            .rooms
            .iter()
            .map(|room| room.room_id.as_str())
            .collect::<Vec<_>>(),
        vec!["!alpha:example"]
    );
    let replaced_state = app.dump_ui_state()?;
    assert_eq!(replaced_state.mdi.active, room_identity("!alpha:example"));
    assert_active_switchbar_item(&replaced_state, &room_identity("!alpha:example"));
    assert!(
        replaced_state
            .mdi
            .views
            .iter()
            .all(|view| view.identity != room_identity("!beta:example"))
    );
    assert!(
        replaced_state
            .switchbar
            .items
            .iter()
            .all(|item| item.identity != room_identity("!beta:example"))
    );

    drop(app);
    let _ = fs::remove_dir_all(&local_app_data);
    Ok(())
}

#[test]
fn automation_driver_slash_part_from_status_reports_no_room_selected()
-> Result<(), Box<dyn std::error::Error>> {
    let exe_path = mmatrix_ui_exe_path()
        .map_err(|error| format!("failed to locate mmatrix-ui.exe: {error}"))?;
    let local_app_data = unique_local_app_data_path();
    let options = launch_options(Some(local_app_data.clone()));
    let args = [OsString::from("--automation")];

    let app = launch_app_with_options(&exe_path, &args, options)?;
    app.wait_for_lifecycle_state("core_started", Duration::from_secs(30))?;
    app.load_events(&serde_json::to_string(&base_events())?)?;
    app.wait_idle(Duration::from_secs(5))?;
    app.select_status_view()?;

    app.set_compose_text("/part")?;
    app.send_compose_key(ComposeKey::Return)?;
    app.wait_idle(Duration::from_secs(5))?;
    assert_eq!(app.compose_text()?, "");

    let model = app.dump_model()?;
    assert_eq!(model.status, "no room selected");
    assert!(model.submitted_membership_commands.is_empty());
    assert_eq!(app.dump_ui_state()?.mdi.active, ViewIdentity::Status);

    drop(app);
    let _ = fs::remove_dir_all(&local_app_data);
    Ok(())
}

#[test]
fn automation_driver_slash_part_thread_targets_parent_and_removes_stale_thread()
-> Result<(), Box<dyn std::error::Error>> {
    let exe_path = mmatrix_ui_exe_path()
        .map_err(|error| format!("failed to locate mmatrix-ui.exe: {error}"))?;
    let local_app_data = unique_local_app_data_path();
    let options = launch_options(Some(local_app_data.clone()));
    let args = [OsString::from("--automation")];
    let fixture_path = fixture_state_path("thread-hierarchy.json");

    let app = launch_app_with_options(&exe_path, &args, options)?;
    app.wait_for_lifecycle_state("core_started", Duration::from_secs(30))?;
    app.load_events(&fs::read_to_string(&fixture_path)?)?;
    app.wait_idle(Duration::from_secs(5))?;

    let thread_view_identity = thread_identity("!ops:example", "$thread-root");
    app.select_view(thread_view_identity.clone())?;

    app.set_compose_text("/part")?;
    app.send_compose_key(ComposeKey::Return)?;
    app.wait_idle(Duration::from_secs(5))?;
    assert_eq!(app.compose_text()?, "");

    let model = app.dump_model()?;
    assert_eq!(model.status, "part submitted: !ops:example");
    assert_eq!(
        model.submitted_membership_commands,
        vec![SubmittedMembershipCommand {
            action: MembershipCommandAction::Leave,
            target: "!ops:example".to_owned(),
        }]
    );
    assert_eq!(app.dump_ui_state()?.mdi.active, thread_view_identity);

    app.load_events(&serde_json::to_string(&vec![
        Event::Started,
        Event::SyncedRooms(vec![room_summary("!alpha:example", "Alpha", false)]),
        text_event(
            "!alpha:example",
            "@alice:example",
            1_700_000_180_000,
            "alpha after thread part",
        ),
    ])?)?;
    app.wait_idle(Duration::from_secs(5))?;

    let replaced_model = app.dump_model()?;
    assert_eq!(
        replaced_model
            .rooms
            .iter()
            .map(|room| room.room_id.as_str())
            .collect::<Vec<_>>(),
        vec!["!alpha:example"]
    );
    let replaced_state = app.dump_ui_state()?;
    assert_eq!(replaced_state.mdi.active, room_identity("!alpha:example"));
    assert_active_switchbar_item(&replaced_state, &room_identity("!alpha:example"));
    assert!(
        replaced_state
            .mdi
            .views
            .iter()
            .all(|view| view.identity != room_identity("!ops:example")
                && view.identity != thread_identity("!ops:example", "$thread-root"))
    );
    assert!(
        replaced_state
            .switchbar
            .items
            .iter()
            .all(|item| item.identity != room_identity("!ops:example")
                && item.identity != thread_identity("!ops:example", "$thread-root"))
    );

    drop(app);
    let _ = fs::remove_dir_all(&local_app_data);
    Ok(())
}

#[test]
fn automation_driver_slash_topic_records_submission_and_waits_for_inbound_topic_event()
-> Result<(), Box<dyn std::error::Error>> {
    let exe_path = mmatrix_ui_exe_path()
        .map_err(|error| format!("failed to locate mmatrix-ui.exe: {error}"))?;
    let local_app_data = unique_local_app_data_path();
    let options = launch_options(Some(local_app_data.clone()));
    let args = [OsString::from("--automation")];

    let app = launch_app_with_options(&exe_path, &args, options)?;
    app.wait_for_lifecycle_state("core_started", Duration::from_secs(30))?;
    app.load_events(&serde_json::to_string(&base_events())?)?;
    app.wait_idle(Duration::from_secs(5))?;

    let before = app.room_activity(Some("!beta:example"))?;
    assert!(app.dump_model()?.submitted_room_topic_commands.is_empty());

    app.set_compose_text("/topic incident review")?;
    app.send_compose_key(ComposeKey::Return)?;
    app.wait_idle(Duration::from_secs(5))?;
    assert_eq!(app.compose_text()?, "");

    let model = app.dump_model()?;
    assert_eq!(model.status, "topic change submitted for !beta:example");
    assert_eq!(
        model.submitted_room_topic_commands,
        vec![SubmittedRoomTopicCommand {
            room_id: "!beta:example".to_owned(),
            topic: "incident review".to_owned(),
        }]
    );
    assert_eq!(app.room_activity(Some("!beta:example"))?, before);

    let before_topic_state = app.dump_ui_state()?;
    let beta_view = find_view(&before_topic_state, &room_identity("!beta:example"));
    assert!(!view_contains(beta_view, "Topic: incident review"));
    assert!(!view_contains(
        beta_view,
        "* [22:16] @alice:example set topic: incident review"
    ));

    app.append_synthetic_events(&serde_json::to_string(&vec![topic_event_with_event_id(
        "!beta:example",
        "$beta-topic-1",
        "@alice:example",
        1_700_000_180_000,
        "incident review",
    )])?)?;
    app.wait_idle(Duration::from_secs(5))?;

    assert_eq!(
        app.room_activity(Some("!beta:example"))?,
        vec![
            text_entry(
                "!beta:example",
                "@bob:example",
                1_700_000_060_000,
                "beta first",
            ),
            text_entry(
                "!beta:example",
                "@bob:example",
                1_700_000_120_000,
                "beta second",
            ),
            topic_entry_with_event_id(
                "!beta:example",
                "$beta-topic-1",
                "@alice:example",
                1_700_000_180_000,
                "incident review",
            ),
        ]
    );

    let after_topic_state = app.dump_ui_state()?;
    let beta_view = find_view(&after_topic_state, &room_identity("!beta:example"));
    assert!(view_contains(beta_view, "Topic: incident review"));
    assert!(view_contains(
        beta_view,
        "* [22:16] @alice:example set topic: incident review"
    ));

    drop(app);
    let _ = fs::remove_dir_all(&local_app_data);
    Ok(())
}

#[test]
fn automation_driver_renders_synced_topic_without_topic_notice()
-> Result<(), Box<dyn std::error::Error>> {
    let exe_path = mmatrix_ui_exe_path()
        .map_err(|error| format!("failed to locate mmatrix-ui.exe: {error}"))?;
    let local_app_data = unique_local_app_data_path();
    let options = launch_options(Some(local_app_data.clone()));
    let app = launch_app_with_options(&exe_path, &[OsString::from("--automation")], options)?;
    app.wait_for_lifecycle_state("core_started", Duration::from_secs(30))?;
    app.load_events(&serde_json::to_string(&base_events())?)?;

    let mut summary = room_summary("!beta:example", "Beta", false);
    summary.topic = Some("incident review".to_owned());
    app.append_synthetic_events(&serde_json::to_string(&vec![Event::SyncedRooms(vec![
        summary,
    ])])?)?;
    app.wait_idle(Duration::from_secs(5))?;

    let state = app.dump_ui_state()?;
    let beta_view = find_view(&state, &room_identity("!beta:example"));
    assert!(view_contains(beta_view, "Topic: incident review"));
    assert!(!view_contains(beta_view, "set topic: incident review"));

    app.append_synthetic_events(&serde_json::to_string(&vec![topic_event_with_event_id(
        "!beta:example",
        "$beta-topic-summary",
        "@alice:example",
        1_700_000_180_000,
        "deployment window",
    )])?)?;
    app.wait_idle(Duration::from_secs(5))?;
    let state = app.dump_ui_state()?;
    let beta_view = find_view(&state, &room_identity("!beta:example"));
    assert!(view_contains(beta_view, "Topic: deployment window"));
    assert!(view_contains(beta_view, "set topic: deployment window"));
    assert!(!view_contains(beta_view, "Topic: incident review"));

    drop(app);
    let _ = fs::remove_dir_all(&local_app_data);
    Ok(())
}

#[test]
fn automation_driver_slash_react_records_submitted_reaction_without_appending_text()
-> Result<(), Box<dyn std::error::Error>> {
    let exe_path = mmatrix_ui_exe_path()
        .map_err(|error| format!("failed to locate mmatrix-ui.exe: {error}"))?;
    let fixture_path = fixture_state_path("reactions.json");
    let local_app_data = unique_local_app_data_path();
    let options = launch_options(Some(local_app_data.clone()));
    let args = [OsString::from("--automation")];

    let app = launch_app_with_options(&exe_path, &args, options)?;
    app.wait_for_lifecycle_state("core_started", Duration::from_secs(30))?;
    app.load_events(&fs::read_to_string(&fixture_path)?)?;
    app.wait_idle(Duration::from_secs(5))?;

    let before = app.room_activity(Some("!reactions:example"))?;
    let before_model = app.dump_model()?;
    assert!(before_model.submitted_reaction_commands.is_empty());
    assert!(before_model.submitted_redaction_commands.is_empty());

    app.set_compose_text("/react $react-normal +1")?;
    app.send_compose_key(ComposeKey::Return)?;
    app.wait_idle(Duration::from_secs(5))?;
    assert_eq!(app.compose_text()?, "");

    let model = app.dump_model()?;
    assert_eq!(
        model.submitted_reaction_commands,
        vec![SubmittedReactionCommand {
            room_id: "!reactions:example".to_owned(),
            target_event_id: "$react-normal".to_owned(),
            key: "+1".to_owned(),
        }]
    );
    assert!(model.submitted_redaction_commands.is_empty());
    assert_eq!(app.room_activity(Some("!reactions:example"))?, before);

    drop(app);
    let _ = fs::remove_dir_all(&local_app_data);
    Ok(())
}

#[test]
fn automation_driver_slash_react_preserves_key_text() -> Result<(), Box<dyn std::error::Error>> {
    let exe_path = mmatrix_ui_exe_path()
        .map_err(|error| format!("failed to locate mmatrix-ui.exe: {error}"))?;
    let fixture_path = fixture_state_path("reactions.json");
    let local_app_data = unique_local_app_data_path();
    let options = launch_options(Some(local_app_data.clone()));
    let args = [OsString::from("--automation")];

    let app = launch_app_with_options(&exe_path, &args, options)?;
    app.wait_for_lifecycle_state("core_started", Duration::from_secs(30))?;
    app.load_events(&fs::read_to_string(&fixture_path)?)?;
    app.wait_idle(Duration::from_secs(5))?;

    assert!(app.dump_model()?.submitted_reaction_commands.is_empty());

    app.set_compose_text("/react $react-normal eyes")?;
    app.send_compose_key(ComposeKey::Return)?;
    app.wait_idle(Duration::from_secs(5))?;
    assert_eq!(app.compose_text()?, "");

    let model = app.dump_model()?;
    assert_eq!(
        model.submitted_reaction_commands,
        vec![SubmittedReactionCommand {
            room_id: "!reactions:example".to_owned(),
            target_event_id: "$react-normal".to_owned(),
            key: "eyes".to_owned(),
        }]
    );
    assert!(model.submitted_redaction_commands.is_empty());

    drop(app);
    let _ = fs::remove_dir_all(&local_app_data);
    Ok(())
}

#[test]
fn automation_driver_slash_reply_records_submitted_reply_without_appending_text()
-> Result<(), Box<dyn std::error::Error>> {
    let exe_path = mmatrix_ui_exe_path()
        .map_err(|error| format!("failed to locate mmatrix-ui.exe: {error}"))?;
    let fixture_path = fixture_state_path("reactions.json");
    let local_app_data = unique_local_app_data_path();
    let options = launch_options(Some(local_app_data.clone()));
    let args = [OsString::from("--automation")];

    let app = launch_app_with_options(&exe_path, &args, options)?;
    app.wait_for_lifecycle_state("core_started", Duration::from_secs(30))?;
    app.load_events(&fs::read_to_string(&fixture_path)?)?;
    app.wait_idle(Duration::from_secs(5))?;

    let before = app.room_activity(Some("!reactions:example"))?;

    app.set_compose_text("/reply $react-normal thanks for the detail")?;
    app.send_compose_key(ComposeKey::Return)?;
    app.wait_idle(Duration::from_secs(5))?;
    assert_eq!(app.compose_text()?, "");

    let model = app.dump_model()?;
    assert_eq!(
        model.submitted_reply_commands,
        vec![SubmittedReplyCommand {
            room_id: "!reactions:example".to_owned(),
            target_event_id: "$react-normal".to_owned(),
            body: "thanks for the detail".to_owned(),
            thread: None,
        }]
    );
    assert!(model.submitted_reaction_commands.is_empty());
    assert!(model.submitted_edit_commands.is_empty());
    assert!(model.submitted_redaction_commands.is_empty());
    assert_eq!(app.room_activity(Some("!reactions:example"))?, before);

    drop(app);
    let _ = fs::remove_dir_all(&local_app_data);
    Ok(())
}

#[test]
fn automation_driver_slash_edit_records_submitted_edit_without_appending_text()
-> Result<(), Box<dyn std::error::Error>> {
    let exe_path = mmatrix_ui_exe_path()
        .map_err(|error| format!("failed to locate mmatrix-ui.exe: {error}"))?;
    let fixture_path = fixture_state_path("reactions.json");
    let local_app_data = unique_local_app_data_path();
    let options = launch_options(Some(local_app_data.clone()));
    let args = [OsString::from("--automation")];

    let app = launch_app_with_options(&exe_path, &args, options)?;
    app.wait_for_lifecycle_state("core_started", Duration::from_secs(30))?;
    app.load_events(&fs::read_to_string(&fixture_path)?)?;
    app.wait_idle(Duration::from_secs(5))?;

    let before = app.room_activity(Some("!reactions:example"))?;
    let before_model = app.dump_model()?;
    assert!(before_model.submitted_edit_commands.is_empty());
    assert!(before_model.submitted_reaction_commands.is_empty());
    assert!(before_model.submitted_redaction_commands.is_empty());

    app.set_compose_text("/edit $event-to-edit replacement text")?;
    app.send_compose_key(ComposeKey::Return)?;
    app.wait_idle(Duration::from_secs(5))?;
    assert_eq!(app.compose_text()?, "");

    let model = app.dump_model()?;
    assert_eq!(
        model.submitted_edit_commands,
        vec![SubmittedEditCommand {
            room_id: "!reactions:example".to_owned(),
            target_event_id: "$event-to-edit".to_owned(),
            body: "replacement text".to_owned(),
        }]
    );
    assert!(model.submitted_reaction_commands.is_empty());
    assert!(model.submitted_redaction_commands.is_empty());
    assert_eq!(app.room_activity(Some("!reactions:example"))?, before);

    drop(app);
    let _ = fs::remove_dir_all(&local_app_data);
    Ok(())
}

#[test]
fn automation_driver_invalid_reply_reports_usage_without_recording_commands()
-> Result<(), Box<dyn std::error::Error>> {
    let exe_path = mmatrix_ui_exe_path()
        .map_err(|error| format!("failed to locate mmatrix-ui.exe: {error}"))?;
    let fixture_path = fixture_state_path("reactions.json");
    let local_app_data = unique_local_app_data_path();
    let options = launch_options(Some(local_app_data.clone()));
    let args = [OsString::from("--automation")];

    let app = launch_app_with_options(&exe_path, &args, options)?;
    app.wait_for_lifecycle_state("core_started", Duration::from_secs(30))?;
    app.load_events(&fs::read_to_string(&fixture_path)?)?;
    app.wait_idle(Duration::from_secs(5))?;

    let before = app.room_activity(Some("!reactions:example"))?;

    app.set_compose_text("/reply $react-normal")?;
    app.send_compose_key(ComposeKey::Return)?;
    app.wait_idle(Duration::from_secs(5))?;
    assert_eq!(app.compose_text()?, "");

    let model = app.dump_model()?;
    assert_eq!(model.status, "usage: /reply <event-id> <message>");
    assert!(model.submitted_reply_commands.is_empty());
    assert!(model.submitted_reaction_commands.is_empty());
    assert!(model.submitted_edit_commands.is_empty());
    assert!(model.submitted_redaction_commands.is_empty());
    assert_eq!(model.last_error, None);
    assert_eq!(app.room_activity(Some("!reactions:example"))?, before);

    let ui_state = app.dump_ui_state()?;
    assert_eq!(ui_state.mdi.active, room_identity("!reactions:example"));
    assert!(
        ui_state
            .status_bar
            .text
            .contains("usage: /reply <event-id> <message>")
    );

    drop(app);
    let _ = fs::remove_dir_all(&local_app_data);
    Ok(())
}

#[test]
fn automation_driver_slash_redact_records_submitted_redaction_without_appending_text()
-> Result<(), Box<dyn std::error::Error>> {
    let exe_path = mmatrix_ui_exe_path()
        .map_err(|error| format!("failed to locate mmatrix-ui.exe: {error}"))?;
    let fixture_path = fixture_state_path("reactions.json");
    let local_app_data = unique_local_app_data_path();
    let options = launch_options(Some(local_app_data.clone()));
    let args = [OsString::from("--automation")];

    let app = launch_app_with_options(&exe_path, &args, options)?;
    app.wait_for_lifecycle_state("core_started", Duration::from_secs(30))?;
    app.load_events(&fs::read_to_string(&fixture_path)?)?;
    app.wait_idle(Duration::from_secs(5))?;

    let before = app.room_activity(Some("!reactions:example"))?;
    let before_model = app.dump_model()?;
    assert!(before_model.submitted_reaction_commands.is_empty());
    assert!(before_model.submitted_redaction_commands.is_empty());

    app.set_compose_text("/redact $event-to-redact")?;
    app.send_compose_key(ComposeKey::Return)?;
    app.wait_idle(Duration::from_secs(5))?;
    assert_eq!(app.compose_text()?, "");

    let model = app.dump_model()?;
    assert!(model.submitted_reaction_commands.is_empty());
    assert_eq!(
        model.submitted_redaction_commands,
        vec![SubmittedRedactionCommand {
            room_id: "!reactions:example".to_owned(),
            event_id: "$event-to-redact".to_owned(),
        }]
    );
    assert_eq!(app.room_activity(Some("!reactions:example"))?, before);

    drop(app);
    let _ = fs::remove_dir_all(&local_app_data);
    Ok(())
}

#[test]
fn automation_driver_slash_unreact_records_submitted_reaction_redaction()
-> Result<(), Box<dyn std::error::Error>> {
    let exe_path = mmatrix_ui_exe_path()
        .map_err(|error| format!("failed to locate mmatrix-ui.exe: {error}"))?;
    let fixture_path = fixture_state_path("reactions.json");
    let local_app_data = unique_local_app_data_path();
    let options = launch_options(Some(local_app_data.clone()));
    let args = [OsString::from("--automation")];

    let app = launch_app_with_options(&exe_path, &args, options)?;
    app.wait_for_lifecycle_state("core_started", Duration::from_secs(30))?;
    app.load_events(&fs::read_to_string(&fixture_path)?)?;
    app.wait_idle(Duration::from_secs(5))?;

    let before_model = app.dump_model()?;
    assert!(before_model.submitted_reaction_commands.is_empty());
    assert!(before_model.submitted_redaction_commands.is_empty());

    app.set_compose_text("/unreact $reaction-alice-plus-one")?;
    app.send_compose_key(ComposeKey::Return)?;
    app.wait_idle(Duration::from_secs(5))?;
    assert_eq!(app.compose_text()?, "");

    let model = app.dump_model()?;
    assert!(model.submitted_reaction_commands.is_empty());
    assert_eq!(
        model.submitted_redaction_commands,
        vec![SubmittedRedactionCommand {
            room_id: "!reactions:example".to_owned(),
            event_id: "$reaction-alice-plus-one".to_owned(),
        }]
    );

    drop(app);
    let _ = fs::remove_dir_all(&local_app_data);
    Ok(())
}

#[test]
fn automation_driver_invalid_redact_reports_usage_without_recording_commands()
-> Result<(), Box<dyn std::error::Error>> {
    let exe_path = mmatrix_ui_exe_path()
        .map_err(|error| format!("failed to locate mmatrix-ui.exe: {error}"))?;
    let fixture_path = fixture_state_path("reactions.json");
    let local_app_data = unique_local_app_data_path();
    let options = launch_options(Some(local_app_data.clone()));
    let args = [OsString::from("--automation")];

    let app = launch_app_with_options(&exe_path, &args, options)?;
    app.wait_for_lifecycle_state("core_started", Duration::from_secs(30))?;
    app.load_events(&fs::read_to_string(&fixture_path)?)?;
    app.wait_idle(Duration::from_secs(5))?;

    let before = app.room_activity(Some("!reactions:example"))?;

    app.set_compose_text("/redact")?;
    app.send_compose_key(ComposeKey::Return)?;
    app.wait_idle(Duration::from_secs(5))?;
    assert_eq!(app.compose_text()?, "");

    let model = app.dump_model()?;
    assert_eq!(model.status, "usage: /redact <event-id>");
    assert!(model.submitted_reaction_commands.is_empty());
    assert!(model.submitted_redaction_commands.is_empty());
    assert_eq!(model.last_error, None);
    assert_eq!(app.room_activity(Some("!reactions:example"))?, before);

    let ui_state = app.dump_ui_state()?;
    assert_eq!(ui_state.mdi.active, room_identity("!reactions:example"));
    assert!(
        ui_state
            .status_bar
            .text
            .contains("usage: /redact <event-id>")
    );

    drop(app);
    let _ = fs::remove_dir_all(&local_app_data);
    Ok(())
}

#[test]
fn automation_driver_invalid_react_reports_usage_without_recording_commands()
-> Result<(), Box<dyn std::error::Error>> {
    let exe_path = mmatrix_ui_exe_path()
        .map_err(|error| format!("failed to locate mmatrix-ui.exe: {error}"))?;
    let fixture_path = fixture_state_path("reactions.json");
    let local_app_data = unique_local_app_data_path();
    let options = launch_options(Some(local_app_data.clone()));
    let args = [OsString::from("--automation")];

    let app = launch_app_with_options(&exe_path, &args, options)?;
    app.wait_for_lifecycle_state("core_started", Duration::from_secs(30))?;
    app.load_events(&fs::read_to_string(&fixture_path)?)?;
    app.wait_idle(Duration::from_secs(5))?;

    let before = app.room_activity(Some("!reactions:example"))?;

    app.set_compose_text("/react $react-normal")?;
    app.send_compose_key(ComposeKey::Return)?;
    app.wait_idle(Duration::from_secs(5))?;
    assert_eq!(app.compose_text()?, "");

    let model = app.dump_model()?;
    assert_eq!(model.status, "usage: /react <event-id> <key>");
    assert!(model.submitted_reaction_commands.is_empty());
    assert!(model.submitted_redaction_commands.is_empty());
    assert_eq!(model.last_error, None);
    assert_eq!(app.room_activity(Some("!reactions:example"))?, before);

    let ui_state = app.dump_ui_state()?;
    assert_eq!(ui_state.mdi.active, room_identity("!reactions:example"));
    assert!(
        ui_state
            .status_bar
            .text
            .contains("usage: /react <event-id> <key>")
    );

    drop(app);
    let _ = fs::remove_dir_all(&local_app_data);
    Ok(())
}

#[test]
fn automation_driver_invalid_edit_reports_usage_without_recording_commands()
-> Result<(), Box<dyn std::error::Error>> {
    let exe_path = mmatrix_ui_exe_path()
        .map_err(|error| format!("failed to locate mmatrix-ui.exe: {error}"))?;
    let fixture_path = fixture_state_path("reactions.json");
    let local_app_data = unique_local_app_data_path();
    let options = launch_options(Some(local_app_data.clone()));
    let args = [OsString::from("--automation")];

    let app = launch_app_with_options(&exe_path, &args, options)?;
    app.wait_for_lifecycle_state("core_started", Duration::from_secs(30))?;
    app.load_events(&fs::read_to_string(&fixture_path)?)?;
    app.wait_idle(Duration::from_secs(5))?;

    let before = app.room_activity(Some("!reactions:example"))?;

    app.set_compose_text("/edit $event-to-edit")?;
    app.send_compose_key(ComposeKey::Return)?;
    app.wait_idle(Duration::from_secs(5))?;
    assert_eq!(app.compose_text()?, "");

    let model = app.dump_model()?;
    assert_eq!(model.status, "usage: /edit <event-id> <replacement text>");
    assert!(model.submitted_edit_commands.is_empty());
    assert!(model.submitted_reaction_commands.is_empty());
    assert!(model.submitted_redaction_commands.is_empty());
    assert_eq!(model.last_error, None);
    assert_eq!(app.room_activity(Some("!reactions:example"))?, before);

    let ui_state = app.dump_ui_state()?;
    assert_eq!(ui_state.mdi.active, room_identity("!reactions:example"));
    assert!(
        ui_state
            .status_bar
            .text
            .contains("usage: /edit <event-id> <replacement text>")
    );

    drop(app);
    let _ = fs::remove_dir_all(&local_app_data);
    Ok(())
}

#[test]
fn automation_driver_literal_react_submission_is_sent_as_text()
-> Result<(), Box<dyn std::error::Error>> {
    let exe_path = mmatrix_ui_exe_path()
        .map_err(|error| format!("failed to locate mmatrix-ui.exe: {error}"))?;
    let fixture_path = fixture_state_path("reactions.json");
    let local_app_data = unique_local_app_data_path();
    let options = launch_options(Some(local_app_data.clone()));
    let args = [OsString::from("--automation")];

    let app = launch_app_with_options(&exe_path, &args, options)?;
    app.wait_for_lifecycle_state("core_started", Duration::from_secs(30))?;
    app.load_events(&fs::read_to_string(&fixture_path)?)?;
    app.wait_idle(Duration::from_secs(5))?;

    app.set_compose_text("//react $react-normal +1")?;
    app.send_compose_key(ComposeKey::Return)?;
    assert_eq!(app.compose_text()?, "");
    app.wait_for_lifecycle_state("error", Duration::from_secs(30))?;

    let model = app.dump_model()?;
    assert_eq!(
        model.last_error,
        Some(UiError {
            operation: "send_text".to_owned(),
            message: "send message requires an authenticated session".to_owned(),
        })
    );
    assert!(model.submitted_reaction_commands.is_empty());
    assert!(model.submitted_redaction_commands.is_empty());
    assert_eq!(
        app.dump_ui_state()?.mdi.active,
        room_identity("!reactions:example")
    );
    assert!(
        !app.dump_ui_state()?
            .status_bar
            .text
            .contains("Reaction queued for $react-normal")
    );

    app.send_compose_key(ComposeKey::Up)?;
    assert_eq!(app.compose_text()?, "//react $react-normal +1");

    drop(app);
    let _ = fs::remove_dir_all(&local_app_data);
    Ok(())
}

#[test]
fn automation_driver_literal_edit_submission_is_sent_as_text()
-> Result<(), Box<dyn std::error::Error>> {
    let exe_path = mmatrix_ui_exe_path()
        .map_err(|error| format!("failed to locate mmatrix-ui.exe: {error}"))?;
    let fixture_path = fixture_state_path("reactions.json");
    let local_app_data = unique_local_app_data_path();
    let options = launch_options(Some(local_app_data.clone()));
    let args = [OsString::from("--automation")];

    let app = launch_app_with_options(&exe_path, &args, options)?;
    app.wait_for_lifecycle_state("core_started", Duration::from_secs(30))?;
    app.load_events(&fs::read_to_string(&fixture_path)?)?;
    app.wait_idle(Duration::from_secs(5))?;

    app.set_compose_text("//edit $event-to-edit replacement text")?;
    app.send_compose_key(ComposeKey::Return)?;
    assert_eq!(app.compose_text()?, "");
    app.wait_for_lifecycle_state("error", Duration::from_secs(30))?;

    let model = app.dump_model()?;
    assert_eq!(
        model.last_error,
        Some(UiError {
            operation: "send_text".to_owned(),
            message: "send message requires an authenticated session".to_owned(),
        })
    );
    assert!(model.submitted_edit_commands.is_empty());
    assert!(model.submitted_reaction_commands.is_empty());
    assert!(model.submitted_redaction_commands.is_empty());
    assert_eq!(
        app.dump_ui_state()?.mdi.active,
        room_identity("!reactions:example")
    );
    assert!(
        !app.dump_ui_state()?
            .status_bar
            .text
            .contains("Edit queued for $event-to-edit")
    );

    app.send_compose_key(ComposeKey::Up)?;
    assert_eq!(
        app.compose_text()?,
        "//edit $event-to-edit replacement text"
    );

    drop(app);
    let _ = fs::remove_dir_all(&local_app_data);
    Ok(())
}

#[test]
fn automation_driver_literal_reply_submission_is_sent_as_text()
-> Result<(), Box<dyn std::error::Error>> {
    let exe_path = mmatrix_ui_exe_path()
        .map_err(|error| format!("failed to locate mmatrix-ui.exe: {error}"))?;
    let fixture_path = fixture_state_path("reactions.json");
    let local_app_data = unique_local_app_data_path();
    let options = launch_options(Some(local_app_data.clone()));
    let args = [OsString::from("--automation")];

    let app = launch_app_with_options(&exe_path, &args, options)?;
    app.wait_for_lifecycle_state("core_started", Duration::from_secs(30))?;
    app.load_events(&fs::read_to_string(&fixture_path)?)?;
    app.wait_idle(Duration::from_secs(5))?;

    app.set_compose_text("//reply $react-normal thanks")?;
    app.send_compose_key(ComposeKey::Return)?;
    assert_eq!(app.compose_text()?, "");
    app.wait_for_lifecycle_state("error", Duration::from_secs(30))?;

    let model = app.dump_model()?;
    assert_eq!(
        model.last_error,
        Some(UiError {
            operation: "send_text".to_owned(),
            message: "send message requires an authenticated session".to_owned(),
        })
    );
    assert!(model.submitted_reply_commands.is_empty());
    assert!(model.submitted_reaction_commands.is_empty());
    assert!(model.submitted_edit_commands.is_empty());
    assert!(model.submitted_redaction_commands.is_empty());
    assert_eq!(
        app.dump_ui_state()?.mdi.active,
        room_identity("!reactions:example")
    );
    assert!(
        !app.dump_ui_state()?
            .status_bar
            .text
            .contains("Reply queued for $react-normal")
    );

    app.send_compose_key(ComposeKey::Up)?;
    assert_eq!(app.compose_text()?, "//reply $react-normal thanks");

    drop(app);
    let _ = fs::remove_dir_all(&local_app_data);
    Ok(())
}

#[test]
fn automation_driver_slash_react_from_thread_targets_parent_room()
-> Result<(), Box<dyn std::error::Error>> {
    let exe_path = mmatrix_ui_exe_path()
        .map_err(|error| format!("failed to locate mmatrix-ui.exe: {error}"))?;
    let fixture_path = fixture_state_path("reactions.json");
    let local_app_data = unique_local_app_data_path();
    let options = launch_options(Some(local_app_data.clone()));
    let args = [OsString::from("--automation")];

    let app = launch_app_with_options(&exe_path, &args, options)?;
    app.wait_for_lifecycle_state("core_started", Duration::from_secs(30))?;
    app.load_events(&fs::read_to_string(&fixture_path)?)?;
    app.wait_idle(Duration::from_secs(5))?;

    let thread_view_identity = thread_identity("!reactions:example", "$react-thread-root");
    let before = app.room_activity(Some("!reactions:example"))?;
    app.select_view(thread_view_identity.clone())?;

    app.set_compose_text("/react $react-normal +1")?;
    app.send_compose_key(ComposeKey::Return)?;
    app.wait_idle(Duration::from_secs(5))?;
    assert_eq!(app.compose_text()?, "");

    let model = app.dump_model()?;
    assert_eq!(
        model.submitted_reaction_commands,
        vec![SubmittedReactionCommand {
            room_id: "!reactions:example".to_owned(),
            target_event_id: "$react-normal".to_owned(),
            key: "+1".to_owned(),
        }]
    );
    assert!(model.submitted_redaction_commands.is_empty());
    assert_eq!(app.room_activity(Some("!reactions:example"))?, before);

    let ui_state = app.dump_ui_state()?;
    assert_eq!(ui_state.mdi.active, thread_view_identity);

    drop(app);
    let _ = fs::remove_dir_all(&local_app_data);
    Ok(())
}

#[test]
fn automation_driver_slash_reply_from_thread_uses_compose_thread_destination()
-> Result<(), Box<dyn std::error::Error>> {
    let exe_path = mmatrix_ui_exe_path()
        .map_err(|error| format!("failed to locate mmatrix-ui.exe: {error}"))?;
    let fixture_path = fixture_state_path("reactions.json");
    let local_app_data = unique_local_app_data_path();
    let options = launch_options(Some(local_app_data.clone()));
    let args = [OsString::from("--automation")];

    let app = launch_app_with_options(&exe_path, &args, options)?;
    app.wait_for_lifecycle_state("core_started", Duration::from_secs(30))?;
    app.load_events(&fs::read_to_string(&fixture_path)?)?;
    app.wait_idle(Duration::from_secs(5))?;

    let thread_view_identity = thread_identity("!reactions:example", "$react-thread-root");
    let before = app.room_activity(Some("!reactions:example"))?;
    app.select_view(thread_view_identity.clone())?;

    app.set_compose_text("/reply $react-thread-reply threaded acknowledgement")?;
    app.send_compose_key(ComposeKey::Return)?;
    app.wait_idle(Duration::from_secs(5))?;
    assert_eq!(app.compose_text()?, "");

    let model = app.dump_model()?;
    assert_eq!(
        model.submitted_reply_commands,
        vec![SubmittedReplyCommand {
            room_id: "!reactions:example".to_owned(),
            target_event_id: "$react-thread-reply".to_owned(),
            body: "threaded acknowledgement".to_owned(),
            thread: Some(SubmittedReplyThreadTarget {
                thread_root_event_id: "$react-thread-root".to_owned(),
                latest_event_id: "$react-thread-reply".to_owned(),
            }),
        }]
    );
    assert_eq!(app.room_activity(Some("!reactions:example"))?, before);

    let ui_state = app.dump_ui_state()?;
    assert_eq!(ui_state.mdi.active, thread_view_identity);

    drop(app);
    let _ = fs::remove_dir_all(&local_app_data);
    Ok(())
}

#[test]
fn automation_driver_slash_edit_from_thread_targets_parent_room()
-> Result<(), Box<dyn std::error::Error>> {
    let exe_path = mmatrix_ui_exe_path()
        .map_err(|error| format!("failed to locate mmatrix-ui.exe: {error}"))?;
    let fixture_path = fixture_state_path("reactions.json");
    let local_app_data = unique_local_app_data_path();
    let options = launch_options(Some(local_app_data.clone()));
    let args = [OsString::from("--automation")];

    let app = launch_app_with_options(&exe_path, &args, options)?;
    app.wait_for_lifecycle_state("core_started", Duration::from_secs(30))?;
    app.load_events(&fs::read_to_string(&fixture_path)?)?;
    app.wait_idle(Duration::from_secs(5))?;

    let thread_view_identity = thread_identity("!reactions:example", "$react-thread-root");
    let before = app.room_activity(Some("!reactions:example"))?;
    app.select_view(thread_view_identity.clone())?;

    app.set_compose_text("/edit $event-to-edit replacement text")?;
    app.send_compose_key(ComposeKey::Return)?;
    app.wait_idle(Duration::from_secs(5))?;
    assert_eq!(app.compose_text()?, "");

    let model = app.dump_model()?;
    assert_eq!(
        model.submitted_edit_commands,
        vec![SubmittedEditCommand {
            room_id: "!reactions:example".to_owned(),
            target_event_id: "$event-to-edit".to_owned(),
            body: "replacement text".to_owned(),
        }]
    );
    assert!(model.submitted_reaction_commands.is_empty());
    assert!(model.submitted_redaction_commands.is_empty());
    assert_eq!(app.room_activity(Some("!reactions:example"))?, before);

    let ui_state = app.dump_ui_state()?;
    assert_eq!(ui_state.mdi.active, thread_view_identity);

    drop(app);
    let _ = fs::remove_dir_all(&local_app_data);
    Ok(())
}

#[test]
fn automation_driver_slash_redact_from_thread_targets_parent_room()
-> Result<(), Box<dyn std::error::Error>> {
    let exe_path = mmatrix_ui_exe_path()
        .map_err(|error| format!("failed to locate mmatrix-ui.exe: {error}"))?;
    let fixture_path = fixture_state_path("reactions.json");
    let local_app_data = unique_local_app_data_path();
    let options = launch_options(Some(local_app_data.clone()));
    let args = [OsString::from("--automation")];

    let app = launch_app_with_options(&exe_path, &args, options)?;
    app.wait_for_lifecycle_state("core_started", Duration::from_secs(30))?;
    app.load_events(&fs::read_to_string(&fixture_path)?)?;
    app.wait_idle(Duration::from_secs(5))?;

    let thread_view_identity = thread_identity("!reactions:example", "$react-thread-root");
    let before = app.room_activity(Some("!reactions:example"))?;
    app.select_view(thread_view_identity.clone())?;

    app.set_compose_text("/redact $event-to-redact")?;
    app.send_compose_key(ComposeKey::Return)?;
    app.wait_idle(Duration::from_secs(5))?;
    assert_eq!(app.compose_text()?, "");

    let model = app.dump_model()?;
    assert!(model.submitted_reaction_commands.is_empty());
    assert_eq!(
        model.submitted_redaction_commands,
        vec![SubmittedRedactionCommand {
            room_id: "!reactions:example".to_owned(),
            event_id: "$event-to-redact".to_owned(),
        }]
    );
    assert_eq!(app.room_activity(Some("!reactions:example"))?, before);

    let ui_state = app.dump_ui_state()?;
    assert_eq!(ui_state.mdi.active, thread_view_identity);

    drop(app);
    let _ = fs::remove_dir_all(&local_app_data);
    Ok(())
}

#[test]
fn automation_driver_room_menu_accept_keeps_invite_until_synced_rooms_join()
-> Result<(), Box<dyn std::error::Error>> {
    let exe_path = mmatrix_ui_exe_path()
        .map_err(|error| format!("failed to locate mmatrix-ui.exe: {error}"))?;
    let local_app_data = unique_local_app_data_path();
    let options = launch_options(Some(local_app_data.clone()));
    let args = [OsString::from("--automation")];

    let app = launch_app_with_options(&exe_path, &args, options)?;
    app.wait_for_lifecycle_state("core_started", Duration::from_secs(30))?;
    app.load_events(&serde_json::to_string(&invite_events())?)?;
    app.wait_idle(Duration::from_secs(5))?;
    app.select_room("!invite:example")?;

    let menu = app.dump_room_menu_state()?;
    assert!(menu.accept_invite_enabled);
    assert!(menu.reject_invite_enabled);
    assert!(!menu.part_room_enabled);

    app.open_selected_navigation_context_menu(ContextMenuCommand::AcceptInvite)?;
    app.wait_idle(Duration::from_secs(5))?;

    let accepted = app.dump_model()?;
    assert_eq!(accepted.status, "accept submitted: !invite:example");
    assert_eq!(
        accepted.submitted_membership_commands,
        vec![SubmittedMembershipCommand {
            action: MembershipCommandAction::AcceptInvite,
            target: "!invite:example".to_owned(),
        }]
    );
    assert!(accepted.rooms.iter().any(|room| {
        room.room_id == "!invite:example" && room.membership == RoomMembership::Invite
    }));

    app.append_synthetic_events(&serde_json::to_string(&vec![Event::SyncedRooms(vec![
        room_summary("!beta:example", "Beta", false),
        room_summary("!invite:example", "Lobby", false),
    ])])?)?;
    app.wait_idle(Duration::from_secs(5))?;

    let joined = app.dump_model()?;
    assert!(joined.rooms.iter().any(|room| {
        room.room_id == "!invite:example" && room.membership == RoomMembership::Join
    }));

    drop(app);
    let _ = fs::remove_dir_all(&local_app_data);
    Ok(())
}

#[test]
fn automation_driver_room_menu_part_waits_for_synced_rooms_removal()
-> Result<(), Box<dyn std::error::Error>> {
    let exe_path = mmatrix_ui_exe_path()
        .map_err(|error| format!("failed to locate mmatrix-ui.exe: {error}"))?;
    let local_app_data = unique_local_app_data_path();
    let options = launch_options(Some(local_app_data.clone()));
    let args = [OsString::from("--automation")];

    let app = launch_app_with_options(&exe_path, &args, options)?;
    app.wait_for_lifecycle_state("core_started", Duration::from_secs(30))?;
    app.load_events(&serde_json::to_string(&base_events())?)?;
    app.wait_idle(Duration::from_secs(5))?;

    let menu = app.dump_room_menu_state()?;
    assert!(!menu.accept_invite_enabled);
    assert!(!menu.reject_invite_enabled);
    assert!(menu.part_room_enabled);

    let path = app.selected_navigation_path()?;
    app.open_navigation_context_menu(path, ContextMenuCommand::PartRoom)?;
    app.wait_idle(Duration::from_secs(5))?;

    let model = app.dump_model()?;
    assert_eq!(model.status, "part submitted: !beta:example");
    assert_eq!(
        model.submitted_membership_commands,
        vec![SubmittedMembershipCommand {
            action: MembershipCommandAction::Leave,
            target: "!beta:example".to_owned(),
        }]
    );
    assert_eq!(
        app.dump_ui_state()?.mdi.active,
        room_identity("!beta:example")
    );

    app.load_events(&serde_json::to_string(&vec![
        Event::Started,
        Event::SyncedRooms(vec![room_summary("!alpha:example", "Alpha", false)]),
        text_event(
            "!alpha:example",
            "@alice:example",
            1_700_000_180_000,
            "alpha after part",
        ),
    ])?)?;
    app.wait_idle(Duration::from_secs(5))?;

    let replaced_state = app.dump_ui_state()?;
    assert_eq!(replaced_state.mdi.active, room_identity("!alpha:example"));
    assert!(
        replaced_state
            .mdi
            .views
            .iter()
            .all(|view| view.identity != room_identity("!beta:example"))
    );
    assert!(
        replaced_state
            .switchbar
            .items
            .iter()
            .all(|item| item.identity != room_identity("!beta:example"))
    );

    drop(app);
    let _ = fs::remove_dir_all(&local_app_data);
    Ok(())
}

#[test]
fn automation_driver_selected_navigation_context_menu_rejects_invite()
-> Result<(), Box<dyn std::error::Error>> {
    let exe_path = mmatrix_ui_exe_path()
        .map_err(|error| format!("failed to locate mmatrix-ui.exe: {error}"))?;
    let local_app_data = unique_local_app_data_path();
    let app = launch_app_with_options(
        &exe_path,
        &[OsString::from("--automation")],
        launch_options(Some(local_app_data.clone())),
    )?;
    app.wait_for_lifecycle_state("core_started", Duration::from_secs(30))?;
    app.load_events(&serde_json::to_string(&invite_events())?)?;
    app.wait_idle(Duration::from_secs(5))?;
    app.select_room("!invite:example")?;

    app.open_selected_navigation_context_menu(ContextMenuCommand::RejectInvite)?;
    app.wait_idle(Duration::from_secs(5))?;

    let model = app.dump_model()?;
    assert_eq!(model.status, "reject submitted: !invite:example");
    assert_eq!(
        model.submitted_membership_commands,
        vec![SubmittedMembershipCommand {
            action: MembershipCommandAction::RejectInvite,
            target: "!invite:example".to_owned(),
        }]
    );

    drop(app);
    let _ = fs::remove_dir_all(&local_app_data);
    Ok(())
}

#[test]
fn automation_driver_message_menu_actions_prefill_selected_event()
-> Result<(), Box<dyn std::error::Error>> {
    let exe_path = mmatrix_ui_exe_path()
        .map_err(|error| format!("failed to locate mmatrix-ui.exe: {error}"))?;
    let fixture_path = fixture_state_path("reactions.json");
    let local_app_data = unique_local_app_data_path();
    let options = launch_options(Some(local_app_data.clone()));
    let args = [OsString::from("--automation")];

    let app = launch_app_with_options(&exe_path, &args, options)?;
    app.wait_for_lifecycle_state("core_started", Duration::from_secs(30))?;
    app.load_events(&fs::read_to_string(&fixture_path)?)?;
    app.wait_idle(Duration::from_secs(5))?;
    app.select_room("!reactions:example")?;

    let disabled = app.dump_message_menu_state()?;
    assert!(!disabled.reply_enabled);
    assert!(!disabled.edit_enabled);
    assert!(!disabled.react_enabled);
    assert!(!disabled.redact_enabled);
    assert!(!disabled.save_attachment_enabled);
    assert_eq!(disabled.selected_event_id, None);

    let error = app
        .edit_selected_message_via_menu()
        .expect_err("edit must reject without a selected message");
    assert!(matches!(
        error,
        TestkitError::Protocol { code, .. } if code == "selected_message_unavailable"
    ));

    let error = app
        .react_to_selected_message_via_menu()
        .expect_err("react must reject without a selected message");
    assert!(matches!(
        error,
        TestkitError::Protocol { code, .. } if code == "selected_message_unavailable"
    ));

    app.select_timeline_event("$react-normal")?;
    let enabled = app.dump_message_menu_state()?;
    assert!(enabled.reply_enabled);
    assert!(enabled.edit_enabled);
    assert!(enabled.react_enabled);
    assert!(enabled.redact_enabled);
    assert!(!enabled.save_attachment_enabled);
    assert_eq!(enabled.selected_event_id.as_deref(), Some("$react-normal"));

    let error = app
        .save_attachment_via_message_menu()
        .expect_err("save attachment must reject a selected text message");
    assert!(matches!(
        error,
        TestkitError::Protocol { code, .. } if code == "selected_message_unavailable"
    ));

    app.reply_to_selected_message_via_menu()?;
    assert_eq!(app.compose_text()?, "/reply $react-normal ");
    let model = app.dump_model()?;
    assert!(model.submitted_reply_commands.is_empty());
    assert!(model.submitted_edit_commands.is_empty());
    assert!(model.submitted_reaction_commands.is_empty());
    assert!(model.submitted_redaction_commands.is_empty());

    app.edit_selected_message_via_menu()?;
    assert_eq!(app.compose_text()?, "/edit $react-normal ");
    let model = app.dump_model()?;
    assert!(model.submitted_reply_commands.is_empty());
    assert!(model.submitted_edit_commands.is_empty());
    assert!(model.submitted_reaction_commands.is_empty());
    assert!(model.submitted_redaction_commands.is_empty());

    app.react_to_selected_message_via_menu()?;
    assert_eq!(app.compose_text()?, "/react $react-normal ");
    let model = app.dump_model()?;
    assert!(model.submitted_reply_commands.is_empty());
    assert!(model.submitted_edit_commands.is_empty());
    assert!(model.submitted_reaction_commands.is_empty());
    assert!(model.submitted_redaction_commands.is_empty());

    drop(app);
    let _ = fs::remove_dir_all(&local_app_data);
    Ok(())
}

#[test]
fn automation_driver_timeline_context_menu_prefills_clicked_message_without_prior_selection()
-> Result<(), Box<dyn std::error::Error>> {
    let exe_path = mmatrix_ui_exe_path()
        .map_err(|error| format!("failed to locate mmatrix-ui.exe: {error}"))?;
    let local_app_data = unique_local_app_data_path();
    let app = launch_app_with_options(
        &exe_path,
        &[OsString::from("--automation")],
        launch_options(Some(local_app_data.clone())),
    )?;
    app.wait_for_lifecycle_state("core_started", Duration::from_secs(30))?;
    app.load_events(&fs::read_to_string(fixture_state_path("reactions.json"))?)?;
    app.wait_idle(Duration::from_secs(5))?;
    app.select_room("!reactions:example")?;
    assert_eq!(app.dump_message_menu_state()?.selected_event_id, None);
    app.open_timeline_context_menu("$react-normal", ContextMenuCommand::ReplyMessage)?;
    assert_eq!(app.compose_text()?, "/reply $react-normal ");
    assert_eq!(
        app.dump_message_menu_state()?.selected_event_id.as_deref(),
        Some("$react-normal")
    );
    let model = app.dump_model()?;
    assert!(model.submitted_reply_commands.is_empty());
    drop(app);
    let _ = fs::remove_dir_all(&local_app_data);
    Ok(())
}

#[test]
fn automation_driver_selected_timeline_context_menu_uses_keyboard_selection()
-> Result<(), Box<dyn std::error::Error>> {
    let exe_path = mmatrix_ui_exe_path()
        .map_err(|error| format!("failed to locate mmatrix-ui.exe: {error}"))?;
    let local_app_data = unique_local_app_data_path();
    let app = launch_app_with_options(
        &exe_path,
        &[OsString::from("--automation")],
        launch_options(Some(local_app_data.clone())),
    )?;
    app.wait_for_lifecycle_state("core_started", Duration::from_secs(30))?;
    app.load_events(&fs::read_to_string(fixture_state_path("reactions.json"))?)?;
    app.wait_idle(Duration::from_secs(5))?;
    app.select_room("!reactions:example")?;
    app.select_timeline_event("$react-normal")?;
    app.open_selected_timeline_context_menu(ContextMenuCommand::EditMessage)?;
    assert_eq!(app.compose_text()?, "/edit $react-normal ");
    drop(app);
    let _ = fs::remove_dir_all(&local_app_data);
    Ok(())
}

fn assert_message_menu_attachment_save(
    fixture_name: &str,
    event_id: &str,
    filename: &str,
    bytes: Vec<u8>,
) -> Result<(), Box<dyn std::error::Error>> {
    let exe_path = mmatrix_ui_exe_path()
        .map_err(|error| format!("failed to locate mmatrix-ui.exe: {error}"))?;
    let fixture_path = fixture_state_path(fixture_name);
    let local_app_data = unique_local_app_data_path();
    let options = launch_options(Some(local_app_data.clone()));
    let args = [OsString::from("--automation")];

    let app = launch_app_with_options(&exe_path, &args, options)?;
    app.wait_for_lifecycle_state("core_started", Duration::from_secs(30))?;
    app.load_events(&fs::read_to_string(&fixture_path)?)?;
    app.wait_idle(Duration::from_secs(5))?;
    app.select_room("!media:example.org")?;
    app.select_timeline_event(event_id)?;

    let menu = app.dump_message_menu_state()?;
    assert!(menu.save_attachment_enabled);
    assert_eq!(menu.selected_event_id.as_deref(), Some(event_id));

    app.open_timeline_context_menu(event_id, ContextMenuCommand::SaveAttachment)?;
    app.append_synthetic_events(&serde_json::to_string(&vec![Event::RoomMedia(
        CoreRoomMedia {
            room_id: "!media:example.org".to_owned(),
            activity_id: format!("event:{event_id}"),
            event_id: event_id.to_owned(),
            kind: CoreRoomMediaKind::Loaded {
                format: CoreRoomMediaFormat::File,
                bytes: bytes.clone(),
            },
        },
    )])?)?;
    app.wait_idle(Duration::from_secs(5))?;

    let saved_path = local_app_data
        .join("mmatrix")
        .join("ui-shell")
        .join("attachments")
        .join("_media_example.org")
        .join(event_id.replace(['$', ':'], "_"))
        .join(filename);
    assert_eq!(fs::read(&saved_path)?, bytes);

    drop(app);
    let _ = fs::remove_dir_all(&local_app_data);
    Ok(())
}

#[test]
fn automation_driver_message_menu_save_file_attachment_writes_file()
-> Result<(), Box<dyn std::error::Error>> {
    assert_message_menu_attachment_save(
        "file-message.json",
        "$file1:example.org",
        "report.pdf",
        b"deterministic file attachment bytes\n".to_vec(),
    )
}

#[test]
fn automation_driver_message_menu_save_audio_attachment_writes_file()
-> Result<(), Box<dyn std::error::Error>> {
    assert_message_menu_attachment_save(
        "audio-message.json",
        "$audio1:example.org",
        "voice-note.ogg",
        vec![0x4f, 0x67, 0x67, 0x53, 0, 2, 4, 6, 8],
    )
}

#[test]
fn automation_driver_message_menu_save_video_attachment_writes_file()
-> Result<(), Box<dyn std::error::Error>> {
    assert_message_menu_attachment_save(
        "video-message.json",
        "$video1:example.org",
        "clip.mp4",
        vec![0, 0, 0, 0x18, b'f', b't', b'y', b'p', b'm', b'p', b'4'],
    )
}

#[test]
fn automation_driver_message_menu_redact_records_selected_event()
-> Result<(), Box<dyn std::error::Error>> {
    let exe_path = mmatrix_ui_exe_path()
        .map_err(|error| format!("failed to locate mmatrix-ui.exe: {error}"))?;
    let fixture_path = fixture_state_path("redactions.json");
    let local_app_data = unique_local_app_data_path();
    let options = launch_options(Some(local_app_data.clone()));
    let args = [OsString::from("--automation")];

    let app = launch_app_with_options(&exe_path, &args, options)?;
    app.wait_for_lifecycle_state("core_started", Duration::from_secs(30))?;
    app.load_events(&fs::read_to_string(&fixture_path)?)?;
    app.wait_idle(Duration::from_secs(5))?;
    app.select_room("!redactions:example")?;
    let before = app.room_activity(Some("!redactions:example"))?;

    app.select_timeline_event("$redact-text")?;
    app.redact_selected_message_via_menu()?;
    app.wait_idle(Duration::from_secs(5))?;

    let model = app.dump_model()?;
    assert_eq!(
        model.submitted_redaction_commands,
        vec![SubmittedRedactionCommand {
            room_id: "!redactions:example".to_owned(),
            event_id: "$redact-text".to_owned(),
        }]
    );
    assert_eq!(app.room_activity(Some("!redactions:example"))?, before);

    drop(app);
    let _ = fs::remove_dir_all(&local_app_data);
    Ok(())
}

#[test]
fn automation_driver_message_menu_redact_from_thread_targets_parent_room()
-> Result<(), Box<dyn std::error::Error>> {
    let exe_path = mmatrix_ui_exe_path()
        .map_err(|error| format!("failed to locate mmatrix-ui.exe: {error}"))?;
    let fixture_path = fixture_state_path("reactions.json");
    let local_app_data = unique_local_app_data_path();
    let options = launch_options(Some(local_app_data.clone()));
    let args = [OsString::from("--automation")];

    let app = launch_app_with_options(&exe_path, &args, options)?;
    app.wait_for_lifecycle_state("core_started", Duration::from_secs(30))?;
    app.load_events(&fs::read_to_string(&fixture_path)?)?;
    app.wait_idle(Duration::from_secs(5))?;

    let thread_view_identity = thread_identity("!reactions:example", "$react-thread-root");
    app.select_view(thread_view_identity.clone())?;
    app.open_timeline_context_menu("$react-thread-reply", ContextMenuCommand::RedactMessage)?;
    app.wait_idle(Duration::from_secs(5))?;

    assert_eq!(
        app.dump_model()?.submitted_redaction_commands,
        vec![SubmittedRedactionCommand {
            room_id: "!reactions:example".to_owned(),
            event_id: "$react-thread-reply".to_owned(),
        }]
    );
    assert_eq!(app.dump_ui_state()?.mdi.active, thread_view_identity);

    drop(app);
    let _ = fs::remove_dir_all(&local_app_data);
    Ok(())
}

#[test]
fn automation_driver_slash_clear_clears_only_active_room_timeline()
-> Result<(), Box<dyn std::error::Error>> {
    let exe_path = mmatrix_ui_exe_path()
        .map_err(|error| format!("failed to locate mmatrix-ui.exe: {error}"))?;
    let local_app_data = unique_local_app_data_path();
    let options = launch_options(Some(local_app_data.clone()));
    let args = [OsString::from("--automation")];

    let app = launch_app_with_options(&exe_path, &args, options)?;
    app.wait_for_lifecycle_state("core_started", Duration::from_secs(30))?;
    app.load_events(&serde_json::to_string(&base_events())?)?;
    app.wait_idle(Duration::from_secs(5))?;
    app.select_room("!beta:example")?;

    let before_clear = app.room_activity(Some("!beta:example"))?;
    assert!(before_clear.iter().any(|entry| entry
        == &text_entry(
            "!beta:example",
            "@bob:example",
            1_700_000_060_000,
            "beta first",
        )));
    assert!(before_clear.iter().any(|entry| entry
        == &text_entry(
            "!beta:example",
            "@bob:example",
            1_700_000_120_000,
            "beta second",
        )));

    app.set_compose_text("/clear")?;
    app.send_compose_key(ComposeKey::Return)?;
    app.wait_idle(Duration::from_secs(5))?;
    assert_eq!(app.compose_text()?, "");
    assert!(app.room_activity(Some("!beta:example"))?.is_empty());
    assert_eq!(
        app.room_activity(Some("!alpha:example"))?,
        vec![text_entry(
            "!alpha:example",
            "@alice:example",
            1_700_000_000_000,
            "alpha first",
        )]
    );
    let cleared_state = app.dump_ui_state()?;
    assert_eq!(cleared_state.mdi.active, room_identity("!beta:example"));
    assert!(
        cleared_state
            .status_bar
            .text
            .contains("cleared window: !beta:example")
    );
    assert_eq!(app.dump_model()?.last_error, None);

    drop(app);
    let _ = fs::remove_dir_all(&local_app_data);
    Ok(())
}

#[test]
fn automation_driver_slash_clear_preserves_pending_row_for_later_sent_update()
-> Result<(), Box<dyn std::error::Error>> {
    let exe_path = mmatrix_ui_exe_path()
        .map_err(|error| format!("failed to locate mmatrix-ui.exe: {error}"))?;
    let local_app_data = unique_local_app_data_path();
    let options = launch_options(Some(local_app_data.clone()));
    let args = [OsString::from("--automation")];

    let app = launch_app_with_options(&exe_path, &args, options)?;
    app.wait_for_lifecycle_state("core_started", Duration::from_secs(30))?;
    app.load_events(&serde_json::to_string(&vec![
        Event::Started,
        Event::SyncedRooms(vec![room_summary("!ops:example", "Ops", false)]),
        text_event_with_event_id(
            "!ops:example",
            "$ops-sent-1",
            "@alice:example",
            1_700_000_000_000,
            "already sent",
        ),
        pending_message_event(
            "!ops:example",
            "txn-1",
            "@alice:example",
            1_700_000_060_000,
            "hello",
        ),
    ])?)?;
    app.wait_idle(Duration::from_secs(5))?;
    app.select_room("!ops:example")?;

    app.set_compose_text("/clear")?;
    app.send_compose_key(ComposeKey::Return)?;
    app.wait_idle(Duration::from_secs(5))?;

    assert_eq!(
        app.room_activity(Some("!ops:example"))?,
        vec![pending_entry(
            "!ops:example",
            "txn-1",
            "@alice:example",
            1_700_000_060_000,
            "hello",
        )]
    );

    app.append_synthetic_events(&serde_json::to_string(&vec![message_sent_update_event(
        "!ops:example",
        "txn-1",
        "$event:example.org",
    )])?)?;
    app.wait_idle(Duration::from_secs(5))?;

    assert_eq!(
        app.room_activity(Some("!ops:example"))?,
        vec![RoomActivity {
            room_id: "!ops:example".to_owned(),
            activity_id: "event:$event:example.org".to_owned(),
            timestamp_ms: 1_700_000_060_000,
            relation: None,
            reactions: Vec::new(),
            kind: RoomActivityKind::Message(RoomMessageActivity {
                event_id: Some("$event:example.org".to_owned()),
                transaction_id: Some("txn-1".to_owned()),
                sender: "@alice:example".to_owned(),
                presentation: RoomMessagePresentationKind::Text,
                body: "hello".to_owned(),
                send_state: RoomMessageSendState::Sent,
            }),
        }]
    );

    drop(app);
    let _ = fs::remove_dir_all(&local_app_data);
    Ok(())
}

#[test]
fn automation_driver_slash_clear_in_status_view_is_a_no_op()
-> Result<(), Box<dyn std::error::Error>> {
    let exe_path = mmatrix_ui_exe_path()
        .map_err(|error| format!("failed to locate mmatrix-ui.exe: {error}"))?;
    let local_app_data = unique_local_app_data_path();
    let options = launch_options(Some(local_app_data.clone()));
    let args = [OsString::from("--automation")];

    let app = launch_app_with_options(&exe_path, &args, options)?;
    app.wait_for_lifecycle_state("core_started", Duration::from_secs(30))?;
    app.load_events(&serde_json::to_string(&base_events())?)?;
    app.wait_idle(Duration::from_secs(5))?;

    app.set_compose_text("/status")?;
    app.send_compose_key(ComposeKey::Return)?;
    app.wait_idle(Duration::from_secs(5))?;
    assert_eq!(app.compose_text()?, "");
    assert_eq!(app.dump_ui_state()?.mdi.active, ViewIdentity::Status);

    app.set_compose_text("/clear")?;
    app.send_compose_key(ComposeKey::Return)?;
    app.wait_idle(Duration::from_secs(5))?;
    assert_eq!(app.compose_text()?, "");

    let ui_state = app.dump_ui_state()?;
    assert_eq!(ui_state.mdi.active, ViewIdentity::Status);
    assert!(
        ui_state
            .status_bar
            .text
            .contains("nothing to clear in status view")
    );
    assert!(view_contains(
        find_view(&ui_state, &ViewIdentity::Status),
        "nothing to clear in status view"
    ));
    assert_eq!(app.dump_model()?.last_error, None);

    app.send_compose_key(ComposeKey::Up)?;
    assert_eq!(app.compose_text()?, "/clear");

    drop(app);
    let _ = fs::remove_dir_all(&local_app_data);
    Ok(())
}

#[test]
fn automation_driver_slash_find_updates_active_room_without_sending_text()
-> Result<(), Box<dyn std::error::Error>> {
    let exe_path = mmatrix_ui_exe_path()
        .map_err(|error| format!("failed to locate mmatrix-ui.exe: {error}"))?;
    let local_app_data = unique_local_app_data_path();
    let options = launch_options(Some(local_app_data.clone()));
    let args = [OsString::from("--automation")];

    let app = launch_app_with_options(&exe_path, &args, options)?;
    app.wait_for_lifecycle_state("core_started", Duration::from_secs(30))?;
    app.load_events(&serde_json::to_string(&base_events())?)?;
    app.wait_idle(Duration::from_secs(5))?;
    app.select_room("!beta:example")?;

    let beta_view_identity = room_identity("!beta:example");
    let before_activity = app.room_activity(Some("!beta:example"))?;
    let before_model = app.dump_model()?;
    assert_no_submitted_commands(&before_model);

    app.set_compose_text("/find beta second")?;
    app.send_compose_key(ComposeKey::Return)?;
    app.wait_idle(Duration::from_secs(5))?;
    assert_eq!(app.compose_text()?, "");

    let ui_state = app.dump_ui_state()?;
    let selected_text =
        only_view_line_containing(find_view(&ui_state, &beta_view_identity), "beta second");
    assert_eq!(ui_state.mdi.active, beta_view_identity);
    assert_active_switchbar_item(&ui_state, &beta_view_identity);
    assert_ui_active_find(
        &ui_state,
        &room_identity("!beta:example"),
        "beta second",
        true,
        1,
        Some(0),
        Some(selected_text),
        UiFindHistorySearchState::LocalOnly,
    );
    assert!(
        ui_state
            .status_bar
            .text
            .contains("find: 1/1 \"beta second\"")
    );

    let model = app.dump_model()?;
    assert_eq!(model.selected_room.as_deref(), Some("!beta:example"));
    assert_eq!(model.last_error, None);
    assert_no_submitted_commands(&model);
    assert_eq!(app.room_activity(Some("!beta:example"))?, before_activity);

    app.send_compose_key(ComposeKey::Up)?;
    assert_eq!(app.compose_text()?, "/find beta second");

    drop(app);
    let _ = fs::remove_dir_all(&local_app_data);
    Ok(())
}

#[test]
fn automation_driver_find_active_room_timeline_matches_advances_and_wraps()
-> Result<(), Box<dyn std::error::Error>> {
    let exe_path = mmatrix_ui_exe_path()
        .map_err(|error| format!("failed to locate mmatrix-ui.exe: {error}"))?;
    let state_path = write_state_file(&[
        Event::Started,
        Event::SyncedRooms(vec![room_summary("!search:example", "Search", false)]),
        text_event_with_event_id(
            "!search:example",
            "$needle-1",
            "@alice:example",
            1_700_000_000_000,
            "first needle",
        ),
        text_event_with_event_id(
            "!search:example",
            "$other",
            "@bob:example",
            1_700_000_060_000,
            "other text",
        ),
        text_event_with_event_id(
            "!search:example",
            "$needle-2",
            "@carol:example",
            1_700_000_120_000,
            "second needle",
        ),
    ])?;
    let args = [
        OsString::from("--render-state"),
        state_path.as_os_str().to_os_string(),
    ];
    let app = launch_app(&exe_path, &args)?;
    app.wait_idle(Duration::from_secs(5))?;

    let search_view_identity = room_identity("!search:example");
    let initial_state = app.dump_ui_state()?;
    let matching_lines =
        view_lines_containing(find_view(&initial_state, &search_view_identity), "needle");
    assert_eq!(matching_lines.len(), 2);

    let first = app.find_active_view("needle")?;
    assert_find_result(
        &first,
        &search_view_identity,
        "needle",
        true,
        2,
        Some(0),
        Some(matching_lines[0]),
        UiFindHistorySearchState::LocalOnly,
    );
    let first_state = app.dump_ui_state()?;
    assert_eq!(first_state.mdi.active, search_view_identity);
    assert_ui_active_find(
        &first_state,
        &room_identity("!search:example"),
        "needle",
        true,
        2,
        Some(0),
        Some(matching_lines[0]),
        UiFindHistorySearchState::LocalOnly,
    );
    assert!(first_state.status_bar.text.contains("find: 1/2 \"needle\""));

    let second = app.find_active_view("needle")?;
    assert_find_result(
        &second,
        &search_view_identity,
        "needle",
        true,
        2,
        Some(1),
        Some(matching_lines[1]),
        UiFindHistorySearchState::LocalOnly,
    );
    let second_state = app.dump_ui_state()?;
    assert_eq!(second_state.mdi.active, search_view_identity);
    assert_ui_active_find(
        &second_state,
        &room_identity("!search:example"),
        "needle",
        true,
        2,
        Some(1),
        Some(matching_lines[1]),
        UiFindHistorySearchState::LocalOnly,
    );
    assert!(
        second_state
            .status_bar
            .text
            .contains("find: 2/2 \"needle\"")
    );

    let wrapped = app.find_active_view("needle")?;
    assert_find_result(
        &wrapped,
        &search_view_identity,
        "needle",
        true,
        2,
        Some(0),
        Some(matching_lines[0]),
        UiFindHistorySearchState::LocalOnly,
    );
    let wrapped_state = app.dump_ui_state()?;
    assert_eq!(wrapped_state.mdi.active, search_view_identity);
    assert_ui_active_find(
        &wrapped_state,
        &room_identity("!search:example"),
        "needle",
        true,
        2,
        Some(0),
        Some(matching_lines[0]),
        UiFindHistorySearchState::LocalOnly,
    );
    assert!(
        wrapped_state
            .status_bar
            .text
            .contains("find: 1/2 \"needle\"")
    );

    let model = app.dump_model()?;
    assert_eq!(model.selected_room.as_deref(), Some("!search:example"));
    assert_eq!(model.last_error, None);
    assert_no_submitted_commands(&model);

    let _ = fs::remove_file(&state_path);
    Ok(())
}

#[test]
fn automation_driver_find_active_thread_timeline_scopes_to_thread()
-> Result<(), Box<dyn std::error::Error>> {
    let exe_path = mmatrix_ui_exe_path()
        .map_err(|error| format!("failed to locate mmatrix-ui.exe: {error}"))?;
    let fixture_path = fixture_state_path("thread-hierarchy.json");
    let args = [
        OsString::from("--render-state"),
        fixture_path.as_os_str().to_os_string(),
    ];
    let app = launch_app(&exe_path, &args)?;
    app.wait_idle(Duration::from_secs(5))?;

    let initial_state = app.dump_ui_state()?;
    let room_view_identity = room_identity("!ops:example");
    assert!(view_contains(
        find_view(&initial_state, &room_view_identity),
        "lobby"
    ));
    let thread_path = navigation_path_by_substrings(
        &initial_state.navigation,
        &["mmatrix", "rooms: 1", "Work", "Ops", "root msg"],
    );
    let thread_view_identity = thread_identity("!ops:example", "$thread-root");
    app.select_navigation_path(&thread_path)?;

    let thread_state = app.dump_ui_state()?;
    let thread_view = find_view(&thread_state, &thread_view_identity);
    let selected_text = only_view_line_containing(thread_view, "thread rep");

    let positive = app.find_active_view("thread rep")?;
    assert_find_result(
        &positive,
        &thread_view_identity,
        "thread rep",
        true,
        1,
        Some(0),
        Some(selected_text),
        UiFindHistorySearchState::LocalOnly,
    );
    let positive_state = app.dump_ui_state()?;
    assert_eq!(positive_state.mdi.active, thread_view_identity);
    assert_ui_active_find(
        &positive_state,
        &thread_identity("!ops:example", "$thread-root"),
        "thread rep",
        true,
        1,
        Some(0),
        Some(selected_text),
        UiFindHistorySearchState::LocalOnly,
    );
    assert!(
        positive_state
            .status_bar
            .text
            .contains("find: 1/1 \"thread rep\"")
    );

    let scoped_miss = app.find_active_view("lobby")?;
    assert_find_result(
        &scoped_miss,
        &thread_view_identity,
        "lobby",
        false,
        0,
        None,
        None,
        UiFindHistorySearchState::Loading,
    );
    let miss_state = app.dump_ui_state()?;
    assert_eq!(miss_state.mdi.active, thread_view_identity);
    assert_ui_active_find(
        &miss_state,
        &thread_identity("!ops:example", "$thread-root"),
        "lobby",
        false,
        0,
        None,
        None,
        UiFindHistorySearchState::Loading,
    );
    assert!(
        miss_state
            .status_bar
            .text
            .contains("find: loading older history for \"lobby\"")
    );

    let model = app.dump_model()?;
    assert_eq!(model.selected_room.as_deref(), Some("!ops:example"));
    assert_eq!(model.last_error, None);
    assert_no_non_history_submitted_commands(&model);
    assert_eq!(
        model.submitted_history_commands,
        vec![SubmittedHistoryCommand {
            room_id: "!ops:example".to_owned(),
            from: None,
            limit: 50,
        }]
    );

    Ok(())
}

#[test]
fn automation_driver_find_active_status_view_matches_status_lines()
-> Result<(), Box<dyn std::error::Error>> {
    let exe_path = mmatrix_ui_exe_path()
        .map_err(|error| format!("failed to locate mmatrix-ui.exe: {error}"))?;
    let state_path = write_state_file(&[Event::Started, Event::E2eeStatus(sample_e2ee_status())])?;
    let args = [
        OsString::from("--render-state"),
        state_path.as_os_str().to_os_string(),
    ];
    let app = launch_app(&exe_path, &args)?;
    app.wait_idle(Duration::from_secs(5))?;
    app.select_status_view()?;
    let before_status = app.dump_model()?.status;

    let initial_state = app.dump_ui_state()?;
    let selected_text = only_view_line_containing(
        find_view(&initial_state, &ViewIdentity::Status),
        "cross-signing",
    );

    let result = app.find_active_view("cross-signing")?;
    assert_find_result(
        &result,
        &ViewIdentity::Status,
        "cross-signing",
        true,
        1,
        Some(0),
        Some(selected_text),
        UiFindHistorySearchState::LocalOnly,
    );

    let ui_state = app.dump_ui_state()?;
    assert_eq!(ui_state.mdi.active, ViewIdentity::Status);
    assert_active_switchbar_item(&ui_state, &ViewIdentity::Status);
    assert_ui_active_find(
        &ui_state,
        &ViewIdentity::Status,
        "cross-signing",
        true,
        1,
        Some(0),
        Some(selected_text),
        UiFindHistorySearchState::LocalOnly,
    );
    assert!(
        ui_state
            .status_bar
            .text
            .contains("find: 1/1 \"cross-signing\"")
    );

    let model = app.dump_model()?;
    assert_eq!(model.selected_room, None);
    assert_eq!(model.last_error, None);
    assert_eq!(model.status, before_status);
    assert_no_submitted_commands(&model);

    let _ = fs::remove_file(&state_path);
    Ok(())
}

#[test]
fn automation_driver_find_active_view_reports_no_match_without_changing_view()
-> Result<(), Box<dyn std::error::Error>> {
    let exe_path = mmatrix_ui_exe_path()
        .map_err(|error| format!("failed to locate mmatrix-ui.exe: {error}"))?;
    let state_path = write_state_file(&base_events())?;
    let args = [
        OsString::from("--render-state"),
        state_path.as_os_str().to_os_string(),
    ];
    let app = launch_app(&exe_path, &args)?;
    app.wait_idle(Duration::from_secs(5))?;
    app.select_room("!beta:example")?;

    let beta_view_identity = room_identity("!beta:example");
    let before_state = app.dump_ui_state()?;
    assert_eq!(before_state.mdi.active, beta_view_identity);

    let result = app.find_active_view("missing search term")?;
    assert_find_result(
        &result,
        &beta_view_identity,
        "missing search term",
        false,
        0,
        None,
        None,
        UiFindHistorySearchState::Loading,
    );

    let ui_state = app.dump_ui_state()?;
    assert_eq!(ui_state.mdi.active, beta_view_identity);
    assert_active_switchbar_item(&ui_state, &beta_view_identity);
    assert!(view_contains(
        find_view(&ui_state, &beta_view_identity),
        "* Loading older history..."
    ));
    assert_ui_active_find(
        &ui_state,
        &room_identity("!beta:example"),
        "missing search term",
        false,
        0,
        None,
        None,
        UiFindHistorySearchState::Loading,
    );
    assert!(
        ui_state
            .status_bar
            .text
            .contains("find: loading older history for \"missing search term\"")
    );

    let model = app.dump_model()?;
    assert_eq!(model.selected_room.as_deref(), Some("!beta:example"));
    assert_eq!(model.last_error, None);
    assert_no_non_history_submitted_commands(&model);
    assert_eq!(
        model.submitted_history_commands,
        vec![SubmittedHistoryCommand {
            room_id: "!beta:example".to_owned(),
            from: None,
            limit: 50,
        }]
    );

    let _ = fs::remove_file(&state_path);
    Ok(())
}

#[test]
fn automation_driver_find_active_view_empty_query_matches_slash_find_validation()
-> Result<(), Box<dyn std::error::Error>> {
    let exe_path = mmatrix_ui_exe_path()
        .map_err(|error| format!("failed to locate mmatrix-ui.exe: {error}"))?;
    let state_path = write_state_file(&base_events())?;
    let args = [
        OsString::from("--render-state"),
        state_path.as_os_str().to_os_string(),
    ];
    let app = launch_app(&exe_path, &args)?;
    app.wait_idle(Duration::from_secs(5))?;
    app.select_room("!beta:example")?;
    app.set_compose_text("/find   ")?;
    app.send_compose_key(ComposeKey::Return)?;
    assert_eq!(app.compose_text()?, "");

    let slash_ui_state = app.dump_ui_state()?;
    let beta_view_identity = room_identity("!beta:example");
    assert_eq!(slash_ui_state.mdi.active, beta_view_identity);
    assert_active_switchbar_item(&slash_ui_state, &beta_view_identity);
    assert_eq!(slash_ui_state.active_find, None);
    assert!(
        slash_ui_state
            .status_bar
            .text
            .contains("usage: /find <text>")
    );

    let slash_model = app.dump_model()?;
    assert_eq!(slash_model.selected_room.as_deref(), Some("!beta:example"));
    assert_eq!(slash_model.status, "usage: /find <text>");
    assert_eq!(slash_model.last_error, None);
    assert_no_submitted_commands(&slash_model);

    let result = app.find_active_view("")?;
    assert_find_result(
        &result,
        &beta_view_identity,
        "",
        false,
        0,
        None,
        None,
        UiFindHistorySearchState::LocalOnly,
    );

    let automation_ui_state = app.dump_ui_state()?;
    assert_eq!(automation_ui_state.mdi.active, beta_view_identity);
    assert_active_switchbar_item(&automation_ui_state, &beta_view_identity);
    assert_eq!(automation_ui_state.active_find, None);
    assert!(
        automation_ui_state
            .status_bar
            .text
            .contains("usage: /find <text>")
    );

    let automation_model = app.dump_model()?;
    assert_eq!(
        automation_model.selected_room.as_deref(),
        Some("!beta:example")
    );
    assert_eq!(automation_model.status, "usage: /find <text>");
    assert_eq!(automation_model.last_error, None);
    assert_no_submitted_commands(&automation_model);

    let _ = fs::remove_file(&state_path);
    Ok(())
}

#[test]
fn automation_driver_find_active_view_preserves_per_view_state()
-> Result<(), Box<dyn std::error::Error>> {
    let exe_path = mmatrix_ui_exe_path()
        .map_err(|error| format!("failed to locate mmatrix-ui.exe: {error}"))?;
    let state_path = write_state_file(&[
        Event::Started,
        Event::SyncedRooms(vec![
            room_summary("!alpha:example", "Alpha", false),
            room_summary("!beta:example", "Beta", false),
        ]),
        text_event_with_event_id(
            "!alpha:example",
            "$alpha-needle",
            "@alice:example",
            1_700_000_000_000,
            "alpha needle",
        ),
        text_event_with_event_id(
            "!beta:example",
            "$beta-needle-1",
            "@bob:example",
            1_700_000_060_000,
            "beta needle one",
        ),
        text_event_with_event_id(
            "!beta:example",
            "$beta-needle-2",
            "@carol:example",
            1_700_000_120_000,
            "beta needle two",
        ),
    ])?;
    let args = [
        OsString::from("--render-state"),
        state_path.as_os_str().to_os_string(),
    ];
    let app = launch_app(&exe_path, &args)?;
    app.wait_idle(Duration::from_secs(5))?;

    let alpha_view_identity = room_identity("!alpha:example");
    let beta_view_identity = room_identity("!beta:example");

    app.select_room("!alpha:example")?;
    let alpha_state = app.dump_ui_state()?;
    let alpha_line = only_view_line_containing(
        find_view(&alpha_state, &alpha_view_identity),
        "alpha needle",
    );
    let alpha_result = app.find_active_view("alpha needle")?;
    assert_find_result(
        &alpha_result,
        &alpha_view_identity,
        "alpha needle",
        true,
        1,
        Some(0),
        Some(alpha_line),
        UiFindHistorySearchState::LocalOnly,
    );

    app.select_room("!beta:example")?;
    let beta_initial_state = app.dump_ui_state()?;
    assert_eq!(beta_initial_state.active_find, None);
    let beta_lines = view_lines_containing(
        find_view(&beta_initial_state, &beta_view_identity),
        "beta needle",
    );
    assert_eq!(beta_lines.len(), 2);
    let _ = app.find_active_view("beta needle")?;
    let beta_result = app.find_active_view("beta needle")?;
    assert_find_result(
        &beta_result,
        &beta_view_identity,
        "beta needle",
        true,
        2,
        Some(1),
        Some(beta_lines[1]),
        UiFindHistorySearchState::LocalOnly,
    );

    app.select_room("!alpha:example")?;
    let restored_alpha_state = app.dump_ui_state()?;
    assert_eq!(restored_alpha_state.mdi.active, alpha_view_identity);
    assert_ui_active_find(
        &restored_alpha_state,
        &room_identity("!alpha:example"),
        "alpha needle",
        true,
        1,
        Some(0),
        Some(alpha_line),
        UiFindHistorySearchState::LocalOnly,
    );
    assert!(
        restored_alpha_state
            .status_bar
            .text
            .contains("find: 1/1 \"alpha needle\"")
    );

    app.select_room("!beta:example")?;
    let restored_beta_state = app.dump_ui_state()?;
    assert_eq!(restored_beta_state.mdi.active, beta_view_identity);
    assert_ui_active_find(
        &restored_beta_state,
        &room_identity("!beta:example"),
        "beta needle",
        true,
        2,
        Some(1),
        Some(beta_lines[1]),
        UiFindHistorySearchState::LocalOnly,
    );
    assert!(
        restored_beta_state
            .status_bar
            .text
            .contains("find: 2/2 \"beta needle\"")
    );

    let model = app.dump_model()?;
    assert_eq!(model.selected_room.as_deref(), Some("!beta:example"));
    assert_eq!(model.last_error, None);
    assert_no_submitted_commands(&model);

    let _ = fs::remove_file(&state_path);
    Ok(())
}

#[test]
fn automation_driver_find_active_view_recomputes_after_timeline_rebuild()
-> Result<(), Box<dyn std::error::Error>> {
    let exe_path = mmatrix_ui_exe_path()
        .map_err(|error| format!("failed to locate mmatrix-ui.exe: {error}"))?;
    let state_path = write_state_file(&[
        Event::Started,
        Event::SyncedRooms(vec![room_summary("!rebuild:example", "Rebuild", false)]),
        text_event_with_event_id(
            "!rebuild:example",
            "$needle-1",
            "@alice:example",
            1_700_000_060_000,
            "first needle",
        ),
        text_event_with_event_id(
            "!rebuild:example",
            "$needle-2",
            "@bob:example",
            1_700_000_120_000,
            "second needle",
        ),
    ])?;
    let args = [
        OsString::from("--render-state"),
        state_path.as_os_str().to_os_string(),
    ];
    let app = launch_app(&exe_path, &args)?;
    app.wait_idle(Duration::from_secs(5))?;

    let rebuild_view_identity = room_identity("!rebuild:example");
    let initial_state = app.dump_ui_state()?;
    let selected_text = only_view_line_containing(
        find_view(&initial_state, &rebuild_view_identity),
        "second needle",
    );
    let _ = app.find_active_view("needle")?;
    let before_rebuild = app.find_active_view("needle")?;
    assert_find_result(
        &before_rebuild,
        &rebuild_view_identity,
        "needle",
        true,
        2,
        Some(1),
        Some(selected_text),
        UiFindHistorySearchState::LocalOnly,
    );

    app.append_synthetic_events(&serde_json::to_string(&vec![room_history_backfill_event(
        "!rebuild:example",
        "backfill-start",
        Some("backfill-end"),
        vec![history_backfill_event_from_event(text_event_with_event_id(
            "!rebuild:example",
            "$needle-0",
            "@carol:example",
            1_700_000_000_000,
            "older needle",
        ))],
    )])?)?;
    app.wait_idle(Duration::from_secs(5))?;

    let ui_state = app.dump_ui_state()?;
    assert_eq!(ui_state.mdi.active, rebuild_view_identity);
    assert_ui_active_find(
        &ui_state,
        &room_identity("!rebuild:example"),
        "needle",
        true,
        3,
        Some(2),
        Some(selected_text),
        UiFindHistorySearchState::LocalOnly,
    );
    assert!(ui_state.status_bar.text.contains("find: 3/3 \"needle\""));
    assert_eq!(
        app.room_activity(Some("!rebuild:example"))?
            .iter()
            .filter(|entry| entry.contains("needle"))
            .count(),
        3
    );

    let model = app.dump_model()?;
    assert_eq!(model.selected_room.as_deref(), Some("!rebuild:example"));
    assert_eq!(model.last_error, None);
    assert_no_submitted_commands(&model);

    let _ = fs::remove_file(&state_path);
    Ok(())
}

#[test]
fn automation_driver_slash_unknown_feedback_is_local() -> Result<(), Box<dyn std::error::Error>> {
    let exe_path = mmatrix_ui_exe_path()
        .map_err(|error| format!("failed to locate mmatrix-ui.exe: {error}"))?;
    let local_app_data = unique_local_app_data_path();
    let options = launch_options(Some(local_app_data.clone()));
    let args = [OsString::from("--automation")];

    let app = launch_app_with_options(&exe_path, &args, options)?;
    app.wait_for_lifecycle_state("core_started", Duration::from_secs(30))?;
    app.load_events(&serde_json::to_string(&base_events())?)?;
    app.wait_idle(Duration::from_secs(5))?;
    app.select_room("!beta:example")?;

    app.set_compose_text("/doesnotexist")?;
    app.send_compose_key(ComposeKey::Return)?;
    app.wait_idle(Duration::from_secs(5))?;
    assert_eq!(app.compose_text()?, "");

    let ui_state = app.dump_ui_state()?;
    assert_eq!(ui_state.mdi.active, room_identity("!beta:example"));
    assert!(
        ui_state
            .status_bar
            .text
            .contains("unknown command: /doesnotexist")
    );
    assert!(
        !ui_state
            .status_bar
            .text
            .contains("send message requires an authenticated session")
    );
    assert_eq!(app.dump_model()?.last_error, None);

    app.send_compose_key(ComposeKey::Up)?;
    assert_eq!(app.compose_text()?, "/doesnotexist");

    drop(app);
    let _ = fs::remove_dir_all(&local_app_data);
    Ok(())
}

#[test]
fn automation_driver_literal_slash_submission_is_sent_as_text()
-> Result<(), Box<dyn std::error::Error>> {
    let exe_path = mmatrix_ui_exe_path()
        .map_err(|error| format!("failed to locate mmatrix-ui.exe: {error}"))?;
    let local_app_data = unique_local_app_data_path();
    let options = launch_options(Some(local_app_data.clone()));
    let args = [OsString::from("--automation")];

    let app = launch_app_with_options(&exe_path, &args, options)?;
    app.wait_for_lifecycle_state("core_started", Duration::from_secs(30))?;
    app.load_events(&serde_json::to_string(&base_events())?)?;
    app.wait_idle(Duration::from_secs(5))?;
    app.select_room("!beta:example")?;

    app.set_compose_text("//me waves")?;
    app.send_compose_key(ComposeKey::Return)?;
    assert_eq!(app.compose_text()?, "");
    app.wait_for_lifecycle_state("error", Duration::from_secs(30))?;
    assert_eq!(
        app.dump_model()?.last_error,
        Some(UiError {
            operation: "send_text".to_owned(),
            message: "send message requires an authenticated session".to_owned(),
        })
    );
    assert_eq!(
        app.dump_ui_state()?.mdi.active,
        room_identity("!beta:example")
    );
    assert!(
        !app.dump_ui_state()?
            .status_bar
            .text
            .contains("usage: /me <action>")
    );

    app.send_compose_key(ComposeKey::Up)?;
    assert_eq!(app.compose_text()?, "//me waves");

    drop(app);
    let _ = fs::remove_dir_all(&local_app_data);
    Ok(())
}

#[test]
fn automation_driver_me_submission_uses_edit_control_path_and_preserves_history()
-> Result<(), Box<dyn std::error::Error>> {
    let exe_path = mmatrix_ui_exe_path()
        .map_err(|error| format!("failed to locate mmatrix-ui.exe: {error}"))?;
    let local_app_data = unique_local_app_data_path();
    let options = launch_options(Some(local_app_data.clone()));
    let args = [OsString::from("--automation")];

    let app = launch_app_with_options(&exe_path, &args, options)?;
    app.wait_for_lifecycle_state("core_started", Duration::from_secs(30))?;
    app.load_events(&serde_json::to_string(&base_events())?)?;
    app.wait_idle(Duration::from_secs(5))?;
    app.select_room("!beta:example")?;

    app.set_compose_text("/me waves")?;
    app.send_compose_key(ComposeKey::Return)?;
    assert_eq!(app.compose_text()?, "");
    app.wait_for_lifecycle_state("error", Duration::from_secs(30))?;
    assert_eq!(
        app.dump_model()?.last_error,
        Some(UiError {
            operation: "send_text".to_owned(),
            message: "send message requires an authenticated session".to_owned(),
        })
    );
    assert_eq!(
        app.dump_ui_state()?.mdi.active,
        room_identity("!beta:example")
    );

    app.send_compose_key(ComposeKey::Up)?;
    assert_eq!(app.compose_text()?, "/me waves");

    drop(app);
    let _ = fs::remove_dir_all(&local_app_data);
    Ok(())
}

#[test]
fn automation_driver_empty_me_reports_usage_without_sending()
-> Result<(), Box<dyn std::error::Error>> {
    let exe_path = mmatrix_ui_exe_path()
        .map_err(|error| format!("failed to locate mmatrix-ui.exe: {error}"))?;
    let local_app_data = unique_local_app_data_path();
    let options = launch_options(Some(local_app_data.clone()));
    let args = [OsString::from("--automation")];

    let app = launch_app_with_options(&exe_path, &args, options)?;
    app.wait_for_lifecycle_state("core_started", Duration::from_secs(30))?;
    app.load_events(&serde_json::to_string(&base_events())?)?;
    app.wait_idle(Duration::from_secs(5))?;
    app.select_room("!beta:example")?;

    app.set_compose_text("/me   ")?;
    app.send_compose_key(ComposeKey::Return)?;
    app.wait_idle(Duration::from_secs(5))?;
    assert_eq!(app.compose_text()?, "");
    assert_eq!(app.dump_model()?.last_error, None);
    assert!(
        app.dump_ui_state()?
            .status_bar
            .text
            .contains("usage: /me <action>")
    );

    app.send_compose_key(ComposeKey::Up)?;
    assert_eq!(app.compose_text()?, "/me   ");

    drop(app);
    let _ = fs::remove_dir_all(&local_app_data);
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

    let app = launch_app_with_options(&exe_path, &args, options.clone())?;
    app.wait_for_lifecycle_state("core_started", Duration::from_secs(30))?;
    app.login(&homeserver_url, &username, &password, None)?;
    app.wait_for_lifecycle_state("authenticated", Duration::from_secs(120))?;
    app.wait_for_lifecycle_state("ready", Duration::from_secs(120))?;
    app.wait_for_min_rooms(1, Duration::from_secs(120))?;
    assert!(session_path.is_file());
    let raw_session_bytes = fs::read(&session_path)?;
    assert_bytes_do_not_contain(&raw_session_bytes, "\"access_token\"");
    assert_bytes_do_not_contain(&raw_session_bytes, "\"refresh_token\"");
    assert_bytes_do_not_contain(&raw_session_bytes, &password);

    app.start_sync()?;
    app.wait_for_lifecycle_state("syncing", Duration::from_secs(60))?;

    if let Some(room_id) = room_id.as_deref() {
        let unique_body = format!(
            "mmatrix ui live {}",
            SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos()
        );
        app.select_room(room_id)?;
        app.send_text(Some(room_id), &unique_body)?;
        app.wait_for_room_activity_contains(Some(room_id), &unique_body, Duration::from_secs(120))?;
    }

    drop(app);

    let restored = launch_app_with_options(&exe_path, &args, options)?;
    restored.wait_for_lifecycle_state("authenticated", Duration::from_secs(120))?;
    restored.wait_for_lifecycle_state("ready", Duration::from_secs(120))?;
    restored.wait_for_min_rooms(1, Duration::from_secs(120))?;
    restored.logout()?;
    restored.wait_for_lifecycle_state("logged_out", Duration::from_secs(120))?;
    assert!(!session_path.exists());

    let _ = fs::remove_dir_all(&local_app_data);
    Ok(())
}
