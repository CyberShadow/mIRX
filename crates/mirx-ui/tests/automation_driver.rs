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

use mirx_core::{
    E2eeBackupState, E2eeBackupStatus, E2eeCrossSigningStatus, E2eeRecoveryState, E2eeStatus,
    Event, HighlightWord, RoomActivity as CoreRoomActivity,
    RoomActivityKind as CoreRoomActivityKind, RoomActivityRelation as CoreRoomActivityRelation,
    RoomActivityUpdate, RoomActivityUpdateKind, RoomActivityUpdateTarget, RoomHistoryBackfill,
    RoomHistoryBackfillEvent, RoomMedia as CoreRoomMedia, RoomMediaFormat as CoreRoomMediaFormat,
    RoomMediaKind as CoreRoomMediaKind, RoomMemberRank, RoomMemberSummary, RoomMembers,
    RoomMembership, RoomMessageActivity as CoreRoomMessageActivity,
    RoomMessagePresentationKind as CoreRoomMessagePresentationKind,
    RoomMessageSendState as CoreRoomMessageSendState, RoomNotificationMode, RoomSummary,
    RoomTyping, SessionAuth as CoreSessionAuth, SpaceChildSummary,
    StoredSession as CoreStoredSession, UserProfile,
    VerificationDecimals as CoreVerificationDecimals, VerificationEmoji as CoreVerificationEmoji,
    VerificationState as CoreVerificationState,
    VerificationStateEvent as CoreVerificationStateEvent,
};
use mirx_testkit::{
    AutomationDialog, ComposeKey, ContextMenuCommand, E2eeRecoverySetupResult,
    FindActiveViewResult, FrameSize, LaunchOptions, MdiArrangement, MdiView, MdiWindowPlacement,
    MdiWindowState, MemberModerationAction, MemberPowerRole, MembershipCommandAction,
    MircFormatKey, MirxApp, Model, NavigationNode, NavigationPathSegment, Room, RoomActivity,
    RoomActivityKind, RoomActivityRelation, RoomAudioActivity, RoomAudioInfo,
    RoomEncryptedFileHashes, RoomEncryptedFileKey, RoomFileActivity, RoomFileInfo,
    RoomImageActivity, RoomImageInfo, RoomImageMediaState, RoomMediaSource, RoomMemberChange,
    RoomMessageActivity, RoomMessageActivityLine, RoomMessagePresentationKind,
    RoomMessageSendState, RoomReactionSummary, RoomRedactedActivity, RoomStateNoticeActivity,
    RoomStateNoticeContent, RoomTag, SessionAuth, SpaceChild, StoredSession,
    SubmittedDirectRoomCommand, SubmittedEditCommand, SubmittedFileCommand,
    SubmittedHistoryCommand, SubmittedMemberModerationCommand, SubmittedMembershipCommand,
    SubmittedMessageCommand, SubmittedNotificationCommand, SubmittedPowerLevelCommand,
    SubmittedProfileCommand, SubmittedReactionCommand, SubmittedRecoveryCommand,
    SubmittedRedactionCommand, SubmittedReplyCommand, SubmittedReplyThreadTarget,
    SubmittedRoomListRequest, SubmittedRoomTopicCommand, SubmittedSetupRecoveryCommand,
    SubmittedUserCommand, SubmittedVerificationAction, SwitchbarItem, TestkitError,
    UiActiveTimeline, UiComposeTarget, UiComposeTargetKind, UiError, UiFindHistorySearchState,
    UiFindState, UiMenu, UiMenuItem, UiPoint, UiRoomHistoryBackfillState, UiRoomReadState,
    UiRoomState, UiState, UiTimelineLineKind, UiTimelineLink, UiTimestamp,
    UnsupportedRoomMessageActivity, UserCommandAction, Verification, VerificationActionKind,
    VerificationState, ViewActivityLevel, ViewIdentity,
};
use windows_sys::Win32::Foundation::{GetLastError, HWND, POINT, RECT, WPARAM};
use windows_sys::Win32::Graphics::Gdi::MapWindowPoints;
use windows_sys::Win32::System::DataExchange::{
    CloseClipboard, EmptyClipboard, GetClipboardData, OpenClipboard, SetClipboardData,
};
use windows_sys::Win32::System::Memory::{GMEM_MOVEABLE, GlobalAlloc, GlobalLock, GlobalUnlock};
use windows_sys::Win32::System::SystemServices::{MK_CONTROL, MK_LBUTTON};
use windows_sys::Win32::UI::Controls::{
    EM_GETSEL, EM_SETSEL, TB_GETSTATE, TBSTATE_CHECKED, TBSTATE_MARKED,
};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_KEYUP, SendInput, VK_CONTROL, VK_DOWN,
    VK_END, VK_ESCAPE, VK_HOME, VK_MENU, VK_NEXT, VK_PRIOR, VK_RETURN, VK_SHIFT, VK_TAB,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    BM_SETCHECK, CS_DBLCLKS, EnumThreadWindows, FindWindowExW, FindWindowW, GCL_STYLE,
    GUITHREADINFO, GW_OWNER, GWL_STYLE, GetClassLongPtrW, GetClassNameW, GetClientRect,
    GetDlgCtrlID, GetDlgItem, GetGUIThreadInfo, GetLastActivePopup, GetScrollInfo, GetWindow,
    GetWindowLongPtrW, GetWindowRect, GetWindowThreadProcessId, IDCANCEL, IDNO, IDOK, IDYES,
    IsWindow, IsWindowVisible, LB_GETCOUNT, LB_GETITEMHEIGHT, LB_GETTEXT, LB_GETTEXTLEN,
    LBS_OWNERDRAWFIXED, PostMessageW, SB_BOTTOM, SB_PAGEUP, SB_TOP, SB_VERT, SC_CLOSE, SCROLLINFO,
    SIF_ALL, SMTO_NORMAL, SPI_GETWHEELSCROLLLINES, SWP_NOMOVE, SWP_NOZORDER, SendMessageTimeoutW,
    SendMessageW, SetForegroundWindow, SetWindowPos, SetWindowTextW, SystemParametersInfoW,
    WM_CHAR, WM_COMMAND, WM_GETTEXT, WM_GETTEXTLENGTH, WM_KEYDOWN, WM_LBUTTONDBLCLK,
    WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MDIACTIVATE, WM_MDIGETACTIVE, WM_MOUSEMOVE, WM_MOUSEWHEEL,
    WM_PASTE, WM_SETTEXT, WM_SYSCOMMAND, WM_VSCROLL, WS_VSCROLL,
};

fn user_profile(user_id: &str) -> mirx_core::RoomUserProfile {
    mirx_core::RoomUserProfile {
        user_id: user_id.to_owned(),
        display_name: None,
        display_name_ambiguous: false,
    }
}

#[allow(dead_code)]
#[path = "../src/dpapi.rs"]
mod dpapi;

const PROTECTED_SESSION_MAGIC: &[u8] = b"mirx-ui-session:v1\0";
const SESSION_DPAPI_ENTROPY: &[u8] = b"mirx-ui/session/v1";
const CF_UNICODETEXT: u32 = 13;
const MDI_ID: i32 = 1002;
const SWITCHBAR_ID: i32 = 1005;
const VIEW_CONTENT_ID: i32 = 2001;
const VIEW_EDIT_ID: i32 = 2002;
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

fn mirx_ui_exe_path() -> Result<PathBuf, String> {
    if let Some(path) = option_env!("CARGO_BIN_EXE_mirx-ui") {
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
    unique_temp_path("mirx-ui-automation-driver", Some("json"))
}

fn unique_screenshot_path() -> PathBuf {
    unique_temp_path("mirx-ui-automation-driver", Some("png"))
}

fn unique_local_app_data_path() -> PathBuf {
    unique_temp_path("mirx-ui-local-app-data", None)
}

fn session_file_path(local_app_data: &Path) -> PathBuf {
    local_app_data
        .join("mirx")
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
    app: MirxApp,
    _gui_test_guard: MutexGuard<'static, ()>,
}

impl Deref for GuiTestApp {
    type Target = MirxApp;

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
    let app = mirx_testkit::MirxApp::launch(exe_path, args)?;
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
    let app = mirx_testkit::MirxApp::launch_with_options(exe_path, args, options)?;
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
        auth: SessionAuth::Matrix,
    }
}

fn core_stored_session() -> CoreStoredSession {
    CoreStoredSession {
        user_id: "@alice:example".to_owned(),
        device_id: "DEVICE".to_owned(),
        access_token: "secret-token".to_owned(),
        refresh_token: Some("refresh-token".to_owned()),
        auth: CoreSessionAuth::Matrix,
    }
}

/// Saves a password login's session, as the app does.
fn write_saved_session(
    local_app_data: &Path,
    homeserver_url: &str,
) -> Result<PathBuf, Box<dyn std::error::Error>> {
    write_saved_session_payload(
        local_app_data,
        &serde_json::json!({
            "version": 2,
            "homeserver_url": homeserver_url,
            "session": core_stored_session(),
        }),
    )
}

/// Saves a session the way mirx did before sessions recorded how they
/// signed in.
fn write_version_1_saved_session(
    local_app_data: &Path,
    homeserver_url: &str,
) -> Result<PathBuf, Box<dyn std::error::Error>> {
    write_saved_session_payload(
        local_app_data,
        &serde_json::json!({
            "version": 1,
            "homeserver_url": homeserver_url,
            "session": {
                "user_id": "@alice:example",
                "device_id": "DEVICE",
                "access_token": "secret-token",
                "refresh_token": "refresh-token"
            }
        }),
    )
}

fn write_saved_session_payload(
    local_app_data: &Path,
    payload: &serde_json::Value,
) -> Result<PathBuf, Box<dyn std::error::Error>> {
    let path = session_file_path(local_app_data);
    let parent = path
        .parent()
        .ok_or_else(|| format!("missing parent for {}", path.display()))?;
    fs::create_dir_all(parent)?;
    let payload = serde_json::to_vec(payload)?;
    let protected = dpapi::protect(&payload, "mirx saved session test", SESSION_DPAPI_ENTROPY)?;
    let mut bytes = Vec::with_capacity(PROTECTED_SESSION_MAGIC.len() + protected.len());
    bytes.extend_from_slice(PROTECTED_SESSION_MAGIC);
    bytes.extend_from_slice(&protected);
    fs::write(&path, bytes)?;
    Ok(path)
}

/// The session file the app saved, unprotected.
fn read_saved_session(
    local_app_data: &Path,
) -> Result<serde_json::Value, Box<dyn std::error::Error>> {
    let bytes = fs::read(session_file_path(local_app_data))?;
    let protected = bytes
        .strip_prefix(PROTECTED_SESSION_MAGIC)
        .ok_or("saved session is not protected")?;
    Ok(serde_json::from_slice(&dpapi::unprotect(
        protected,
        SESSION_DPAPI_ENTROPY,
    )?)?)
}

/// The homeserver URL the app saved with its session.
fn saved_session_homeserver_url(
    local_app_data: &Path,
) -> Result<String, Box<dyn std::error::Error>> {
    Ok(read_saved_session(local_app_data)?["homeserver_url"]
        .as_str()
        .ok_or("saved session has no homeserver_url")?
        .to_owned())
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

/// The `reactions.json` fixture, signed in as `user_id`, whose messages
/// become the user's own.
fn reactions_fixture_signed_in_as(user_id: &str) -> Result<String, Box<dyn std::error::Error>> {
    let mut events: Vec<serde_json::Value> =
        serde_json::from_str(&fs::read_to_string(fixture_state_path("reactions.json"))?)?;
    assert_eq!(events[0], serde_json::json!("Started"));
    events.insert(
        1,
        serde_json::to_value(Event::LoggedIn {
            session: CoreStoredSession {
                user_id: user_id.to_owned(),
                ..core_stored_session()
            },
            homeserver_url: "https://matrix.example/".to_owned(),
        })?,
    );
    Ok(serde_json::to_string(&events)?)
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

/// How the mock homeserver answers `/messages` history requests.
enum MockMessages {
    /// Never answer, so the request stays in flight for the whole test.
    Hang,
    /// Answer every request with this response body.
    Respond(serde_json::Value),
}

fn spawn_mock_backfill_homeserver() -> Result<MockBackfillHomeserver, Box<dyn std::error::Error>> {
    spawn_mock_homeserver(MockMessages::Hang, vec![])
}

/// How the mock homeserver signs users in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MockAuth {
    /// `/login` answers every request with Alice's session.
    Password,
    /// An OAuth 2.0 authorization server on the same port, as MAS behind a
    /// homeserver: it registers `mirx-client` and grants `oauth-token`.
    /// The test plays the browser, so `/oauth2/authorize` is not served.
    OAuth,
}

/// Serves `!beta:example` with Alice and Bob in the sync `state` section and
/// `timeline_events` as its sync timeline. Every `/sync` gets the same body.
/// The `/members` list also has the members that join in the timeline.
fn spawn_mock_homeserver(
    messages: MockMessages,
    timeline_events: Vec<serde_json::Value>,
) -> Result<MockBackfillHomeserver, Box<dyn std::error::Error>> {
    spawn_mock_homeserver_with_auth(MockAuth::Password, messages, timeline_events)
}

fn spawn_mock_homeserver_with_auth(
    auth: MockAuth,
    messages: MockMessages,
    timeline_events: Vec<serde_json::Value>,
) -> Result<MockBackfillHomeserver, Box<dyn std::error::Error>> {
    let listener = TcpListener::bind("127.0.0.1:0")?;
    let address = listener.local_addr()?;
    let issuer = format!("http://{address}/");
    std::thread::spawn(move || {
        let alice_member = serde_json::json!({
            "content": {
                "membership": "join",
                "displayname": "Alice"
            },
            "event_id": "$member",
            "origin_server_ts": 1700000000001u64,
            "sender": "@alice:example",
            "state_key": "@alice:example",
            "type": "m.room.member"
        });
        let bob_member = serde_json::json!({
            "content": {
                "membership": "join",
                "displayname": "Bob"
            },
            "event_id": "$bob-member",
            "origin_server_ts": 1700000000004u64,
            "sender": "@bob:example",
            "state_key": "@bob:example",
            "type": "m.room.member"
        });
        let members = [alice_member.clone(), bob_member.clone()]
            .into_iter()
            .chain(
                timeline_events
                    .iter()
                    .filter(|event| event["type"] == "m.room.member")
                    .cloned(),
            )
            .collect::<Vec<_>>();
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

            // Read the body too: closing with unread bytes resets the
            // connection before the client reads the response.
            let header_end = request
                .windows(4)
                .position(|window| window == b"\r\n\r\n")
                .map_or(request.len(), |position| position + 4);
            let content_length = String::from_utf8_lossy(&request[..header_end])
                .lines()
                .find_map(|line| {
                    let (name, value) = line.split_once(':')?;
                    name.eq_ignore_ascii_case("content-length")
                        .then(|| value.trim().parse::<usize>().ok())?
                })
                .unwrap_or(0);
            while request.len() < header_end + content_length {
                match stream.read(&mut buffer) {
                    Ok(0) | Err(_) => break,
                    Ok(read) => request.extend_from_slice(&buffer[..read]),
                }
            }

            let request = String::from_utf8_lossy(&request);
            let request_line = request.lines().next().unwrap_or_default();
            let path = request_line.split_whitespace().nth(1).unwrap_or("/");

            if auth == MockAuth::OAuth {
                let body = match path {
                    // The versions below 1.15 pick the unstable path.
                    "/_matrix/client/unstable/org.matrix.msc2965/auth_metadata" => {
                        Some(serde_json::json!({
                            "issuer": issuer,
                            "authorization_endpoint": format!("{issuer}oauth2/authorize"),
                            "token_endpoint": format!("{issuer}oauth2/token"),
                            "registration_endpoint": format!("{issuer}oauth2/registration"),
                            "revocation_endpoint": format!("{issuer}oauth2/revoke"),
                            "response_types_supported": ["code"],
                            "response_modes_supported": ["query", "fragment"],
                            "grant_types_supported": ["authorization_code", "refresh_token"],
                            "code_challenge_methods_supported": ["S256"],
                            "prompt_values_supported": ["create"],
                        }))
                    }
                    "/oauth2/registration" => Some(serde_json::json!({
                        "client_id": "mirx-client",
                    })),
                    "/oauth2/token" => Some(serde_json::json!({
                        "access_token": "oauth-token",
                        "token_type": "Bearer",
                        "expires_in": 3600,
                        "refresh_token": "oauth-refresh",
                    })),
                    "/_matrix/client/v3/account/whoami" => Some(serde_json::json!({
                        "user_id": "@alice:example",
                    })),
                    _ => None,
                };
                if let Some(body) = body {
                    respond_json(&mut stream, 200, &body);
                    continue;
                }
            }

            if path == "/_matrix/client/v3/login" {
                respond_json(
                    &mut stream,
                    200,
                    &serde_json::json!({
                        "user_id": "@alice:example",
                        "access_token": "secret-token",
                        "device_id": "DEVICE"
                    }),
                );
                continue;
            }

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
                // A continuous sync loop polls with `since`; idle like a
                // long-poll instead of spinning.
                if path.contains("since=") {
                    std::thread::sleep(Duration::from_millis(500));
                }
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
                                        "m.joined_member_count": 2,
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
                                            alice_member,
                                            bob_member,
                                            {
                                                "content": {
                                                    "users": {
                                                        "@alice:example": 100,
                                                        "@bob:example": 50
                                                    }
                                                },
                                                "event_id": "$power",
                                                "origin_server_ts": 1700000000005u64,
                                                "sender": "@alice:example",
                                                "state_key": "",
                                                "type": "m.room.power_levels"
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
                                        "events": timeline_events,
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

            // `/join #name:example` (the SDK percent-encodes `#`) joins
            // `!name:example`. The room has no state yet; the snapshot after
            // the join reads its encryption state.
            if let Some(name) = path
                .strip_prefix("/_matrix/client/v3/join/%23")
                .and_then(|alias| alias.strip_suffix(":example"))
            {
                respond_json(
                    &mut stream,
                    200,
                    &serde_json::json!({ "room_id": format!("!{name}:example") }),
                );
                continue;
            }
            if path.starts_with("/_matrix/client/v3/rooms/!")
                && path.ends_with(":example/state/m.room.encryption/")
            {
                respond_json(
                    &mut stream,
                    404,
                    &serde_json::json!({ "errcode": "M_NOT_FOUND", "error": "not found" }),
                );
                continue;
            }

            // `/create` makes `!created:example`.
            if path == "/_matrix/client/v3/createRoom" {
                respond_json(
                    &mut stream,
                    200,
                    &serde_json::json!({ "room_id": "!created:example" }),
                );
                continue;
            }

            // The room directory, in two pages.
            if path == "/_matrix/client/v3/publicRooms" {
                let page = if request.contains("\"since\":\"page2\"") {
                    serde_json::json!({
                        "chunk": [
                            public_room_chunk("!zed:example", Some("#zed:example"), "Zed", 40),
                            public_room_chunk("!alpha:example", Some("#alpha:example"), "Alpha", 1),
                        ],
                        "total_room_count_estimate": 5
                    })
                } else {
                    serde_json::json!({
                        "chunk": [
                            public_room_chunk("!rust:example", Some("#rust:example"), "Rust", 120),
                            public_room_chunk("!beta:example", Some("#beta:example"), "Beta", 2),
                            public_room_chunk("!opaque:example", None, "Opaque", 7),
                        ],
                        "next_batch": "page2",
                        "total_room_count_estimate": 5
                    })
                };
                respond_json(&mut stream, 200, &page);
                continue;
            }

            if path.contains("/rooms/%21beta%3Aexample/members")
                || path.contains("/rooms/!beta:example/members")
            {
                respond_json(&mut stream, 200, &serde_json::json!({ "chunk": members }));
                continue;
            }

            // A message replied to in the timeline but not part of it. The
            // SDK sends `!`, `:` and `$` in the path unencoded.
            if path == "/_matrix/client/v3/rooms/!beta:example/event/$old" {
                respond_json(
                    &mut stream,
                    200,
                    &serde_json::json!({
                        "content": { "msgtype": "m.text", "body": "from the archive" },
                        "event_id": "$old",
                        "origin_server_ts": 1690000000000u64,
                        "room_id": "!beta:example",
                        "sender": "@alice:example",
                        "type": "m.room.message"
                    }),
                );
                continue;
            }

            if path.contains("/rooms/%21beta%3Aexample/messages")
                || path.contains("/rooms/!beta:example/messages")
            {
                match &messages {
                    MockMessages::Hang => std::thread::sleep(Duration::from_secs(60)),
                    MockMessages::Respond(body) => respond_json(&mut stream, 200, body),
                }
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

/// A room directory entry whose topic is "About {name}".
fn public_room_chunk(
    room_id: &str,
    alias: Option<&str>,
    name: &str,
    joined_members: u64,
) -> serde_json::Value {
    let mut chunk = serde_json::json!({
        "room_id": room_id,
        "name": name,
        "topic": format!("About {name}"),
        "num_joined_members": joined_members,
        "world_readable": false,
        "guest_can_join": false
    });
    if let Some(alias) = alias {
        chunk["canonical_alias"] = serde_json::json!(alias);
    }
    chunk
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
        notification_mode: Some(RoomNotificationMode::AllMessages),
        canonical_alias: None,
        alt_aliases: Vec::new(),
    }
}

fn invited_room_summary(room_id: &str, display_name: &str, encrypted: bool) -> RoomSummary {
    RoomSummary {
        membership: RoomMembership::Invite,
        notification_mode: None,
        canonical_alias: None,
        alt_aliases: Vec::new(),
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
        notification_mode: Some(RoomNotificationMode::AllMessages),
        canonical_alias: None,
        alt_aliases: Vec::new(),
    }
}

fn invited_space_summary(
    room_id: &str,
    display_name: &str,
    children: Vec<SpaceChildSummary>,
) -> RoomSummary {
    RoomSummary {
        membership: RoomMembership::Invite,
        notification_mode: None,
        canonical_alias: None,
        alt_aliases: Vec::new(),
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
        highlight: false,
        relation: None,
        kind: CoreRoomActivityKind::Message(CoreRoomMessageActivity {
            event_id: Some(generated_event_id(room_id, body)),
            transaction_id: None,
            sender: user_profile(sender),
            presentation,
            body: body.to_owned(),
            styles: Vec::new(),
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
        highlight: false,
        relation,
        kind: CoreRoomActivityKind::Message(CoreRoomMessageActivity {
            event_id: Some(event_id.to_owned()),
            transaction_id: None,
            sender: user_profile(sender),
            presentation,
            body: body.to_owned(),
            styles: Vec::new(),
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
        highlight: false,
        relation: None,
        kind: CoreRoomActivityKind::Message(CoreRoomMessageActivity {
            event_id: None,
            transaction_id: Some(transaction_id.to_owned()),
            sender: user_profile(sender),
            presentation,
            body: body.to_owned(),
            styles: Vec::new(),
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
            styles: Vec::new(),
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
                display_name_ambiguous: false,
                is_own_user: false,
                rank: RoomMemberRank::Member,
                can_kick: false,
                can_ban: false,
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
        highlight: false,
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
        highlight: false,
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
        highlight: false,
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
        highlight: false,
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
        highlight: false,
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
        highlight: false,
        relation: None,
        kind: CoreRoomActivityKind::StateNotice(mirx_core::RoomStateNoticeActivity {
            event_id: event_id.to_owned(),
            sender: user_profile(sender),
            content: mirx_core::RoomStateNoticeContent::Topic {
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
    change: RoomMemberChange,
    target_display_name: Option<&str>,
) -> RoomActivity {
    RoomActivity {
        room_id: room_id.to_owned(),
        activity_id: format!("state:{event_id}"),
        timestamp_ms,
        highlight: false,
        relation: None,
        reactions: Vec::new(),
        kind: RoomActivityKind::StateNotice(RoomStateNoticeActivity {
            event_id: event_id.to_owned(),
            sender: sender.to_owned(),
            state_key: state_key.to_owned(),
            content: RoomStateNoticeContent::Member {
                change,
                target_display_name: target_display_name.map(str::to_owned),
                reason: None,
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
        highlight: false,
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
        highlight: false,
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
        highlight: false,
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
        highlight: false,
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
        highlight: false,
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

fn assert_switchbar_activity(
    ui_state: &UiState,
    identity: &ViewIdentity,
    expected: Option<ViewActivityLevel>,
) {
    assert_eq!(
        find_switchbar_item(ui_state, identity).activity,
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

/// A tree item standing for `view`, for full-tree equality tests.
fn nav_view(text: &str, view: ViewIdentity, children: Vec<NavigationNode>) -> NavigationNode {
    nav_view_with_activity(text, view, None, children)
}

fn nav_view_with_activity(
    text: &str,
    view: ViewIdentity,
    activity: Option<ViewActivityLevel>,
    children: Vec<NavigationNode>,
) -> NavigationNode {
    NavigationNode {
        text: text.to_owned(),
        view: Some(view),
        activity,
        children,
    }
}

/// A tree item standing for no view, for full-tree equality tests.
fn nav_folder(text: &str, children: Vec<NavigationNode>) -> NavigationNode {
    NavigationNode {
        text: text.to_owned(),
        view: None,
        activity: None,
        children,
    }
}

fn nav_path(labels: &[&str]) -> Vec<NavigationPathSegment> {
    labels
        .iter()
        .map(|label| NavigationPathSegment::first(*label))
        .collect()
}

/// Every tree item of the view carries `expected`: a room listed under two
/// spaces has two.
fn assert_navigation_activity(
    ui_state: &UiState,
    view: &ViewIdentity,
    expected: Option<ViewActivityLevel>,
) {
    let nodes = ui_state.navigation.find_view(view);
    assert!(!nodes.is_empty(), "missing navigation node for {view:?}");
    for node in nodes {
        assert_eq!(
            node.activity, expected,
            "unexpected navigation activity for {view:?} ({:?})",
            node.text
        );
    }
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

struct ExpectedFind<'a> {
    active: &'a ViewIdentity,
    query: &'a str,
    matched: bool,
    match_count: usize,
    current_match_index: Option<usize>,
    selected_text: Option<&'a str>,
    history_search: UiFindHistorySearchState,
}

impl ExpectedFind<'_> {
    fn into_ui_find_state(self) -> UiFindState {
        UiFindState {
            active: self.active.clone(),
            query: self.query.to_owned(),
            matched: self.matched,
            match_count: self.match_count,
            current_match_index: self.current_match_index,
            selected_text: self.selected_text.map(str::to_owned),
            history_search: self.history_search,
        }
    }
}

fn assert_find_result(result: &FindActiveViewResult, expected: ExpectedFind<'_>) {
    assert_eq!(result.active, expected.active.clone());
    assert_eq!(result.query, expected.query);
    assert_eq!(result.matched, expected.matched);
    assert_eq!(result.match_count, expected.match_count);
    assert_eq!(result.current_match_index, expected.current_match_index);
    assert_eq!(result.selected_text.as_deref(), expected.selected_text);
    assert_eq!(result.history_search, expected.history_search);
}

fn assert_ui_active_find(ui_state: &UiState, expected: ExpectedFind<'_>) {
    assert_eq!(ui_state.active_find, Some(expected.into_ui_find_state()));
}

fn activate_switchbar_item_by_command(
    app: &MirxApp,
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
    app: &MirxApp,
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

fn wide_null(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}

unsafe fn recover_e2ee_dialog_hwnd(frame: HWND, class_name_wide: *const u16) -> HWND {
    let dialog = unsafe { FindWindowW(class_name_wide, std::ptr::null()) };
    if !dialog.is_null() {
        return dialog;
    }

    let dialog = unsafe { GetLastActivePopup(frame) };
    if dialog != frame && unsafe { class_name(dialog) } == "mirxWin32RecoverE2eeDialog" {
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
    if unsafe { class_name(hwnd) } == "mirxWin32RecoverE2eeDialog" {
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
    if dialog != frame && unsafe { class_name(dialog) } == "mirxWin32RecoveryKeyHandoffDialog" {
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
    if unsafe { class_name(hwnd) } == "mirxWin32RecoveryKeyHandoffDialog" {
        unsafe { *(lparam as *mut HWND) = hwnd };
        0
    } else {
        1
    }
}

fn acknowledge_recovery_key_handoff(app: &MirxApp) -> Result<(), Box<dyn std::error::Error>> {
    let frame = app.automation_frame_hwnd() as HWND;
    let class_name_wide = wide_null("mirxWin32RecoveryKeyHandoffDialog");
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
        "mirxWin32RecoveryKeyHandoffDialog"
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
    // The key has the focus, so Ctrl+C copies it; its multi-line edit
    // passes Tab on to the button, and Enter presses it.
    unsafe { SetForegroundWindow(dialog) };
    wait_for_dialog_focus(app, dialog, RECOVERY_KEY_HANDOFF_DIALOG_KEY_ID)?;
    press_keys(&[VK_TAB]);
    wait_for_dialog_focus(app, dialog, RECOVERY_KEY_HANDOFF_DIALOG_ACK_ID as i32)?;
    press_keys(&[VK_RETURN]);

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
    app: &MirxApp,
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

    let class_name = wide_null("mirxWin32RecoverE2eeDialog");
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
    app: &MirxApp,
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

    let class_name = wide_null("mirxWin32SetupE2eeDialog");
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

unsafe fn mdi_client_hwnd(app: &MirxApp) -> HWND {
    let frame = app.automation_frame_hwnd() as HWND;
    let mdi_client = unsafe { GetDlgItem(frame, MDI_ID) };
    assert!(!mdi_client.is_null(), "MDI client control must exist");
    mdi_client
}

unsafe fn mdi_child_hwnd_by_title(app: &MirxApp, title: &str) -> HWND {
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
    app: &MirxApp,
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
    app: &MirxApp,
    ui_state: &UiState,
    identity: &ViewIdentity,
    checked: bool,
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
    // Activity is drawn as the button's text colour, never the marked fill.
    assert_eq!(
        state & TBSTATE_MARKED as usize,
        0,
        "unexpected raw marked state for {identity:?}"
    );
}

unsafe fn active_mdi_child(app: &MirxApp) -> HWND {
    let active_child = unsafe { SendMessageW(mdi_client_hwnd(app), WM_MDIGETACTIVE, 0, 0) } as HWND;
    assert!(!active_child.is_null(), "an MDI child must be active");
    active_child
}

/// The active MDI child's editbox.
unsafe fn compose_edit_hwnd(app: &MirxApp) -> HWND {
    let active_child = unsafe { SendMessageW(mdi_client_hwnd(app), WM_MDIGETACTIVE, 0, 0) } as HWND;
    assert!(!active_child.is_null(), "an MDI child must be active");
    let edit = unsafe { GetDlgItem(active_child, VIEW_EDIT_ID) };
    assert!(!edit.is_null(), "compose edit control must exist");
    edit
}

/// The window with the focus in the app's UI thread. Read with
/// `GetGUIThreadInfo`, not by attaching to the thread's input: Wine's
/// `AttachThreadInput` clears the shared key state, which drops a Shift
/// still held for keys the app has not read yet.
unsafe fn focused_hwnd(app: &MirxApp) -> HWND {
    let frame = app.automation_frame_hwnd() as HWND;
    let ui_thread = unsafe { GetWindowThreadProcessId(frame, null_mut()) };
    assert_ne!(ui_thread, 0, "UI thread ID must exist");
    let mut info = GUITHREADINFO {
        cbSize: std::mem::size_of::<GUITHREADINFO>() as u32,
        ..Default::default()
    };
    assert_ne!(
        unsafe { GetGUIThreadInfo(ui_thread, &mut info) },
        0,
        "GetGUIThreadInfo must succeed"
    );
    info.hwndFocus
}

fn assert_compose_has_focus(app: &MirxApp) {
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
    let exe_path =
        mirx_ui_exe_path().map_err(|error| format!("failed to locate mirx-ui.exe: {error}"))?;
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
    assert!(ui_state.status_bar.text.contains("synced 2 room(s)"));
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
        vec!["Status", "Beta", "Alpha"]
    );
    assert_active_switchbar_item(&ui_state, &ui_state.mdi.active);
    assert_switchbar_activity(&ui_state, &ViewIdentity::Status, None);
    assert_switchbar_activity(&ui_state, &room_identity("!beta:example"), None);
    assert_switchbar_activity(
        &ui_state,
        &room_identity("!alpha:example"),
        Some(ViewActivityLevel::Message),
    );

    let beta_view = find_view(&ui_state, &room_identity("!beta:example"));
    assert!(beta_view.active);
    assert!(beta_view.title.contains("Beta"));
    assert!(view_contains(beta_view, "beta first"));
    assert!(view_contains(beta_view, "beta second"));
    assert_eq!(ui_state.navigation.text, "Status");
    assert_eq!(ui_state.navigation.view, Some(ViewIdentity::Status));
    assert!(navigation_contains(&ui_state.navigation, "Beta"));
    assert!(navigation_contains(&ui_state.navigation, "Alpha"));
    assert_navigation_not_contains(&ui_state.navigation, "[e2ee]");
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
    let exe_path =
        mirx_ui_exe_path().map_err(|error| format!("failed to locate mirx-ui.exe: {error}"))?;
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
        Event::Error(mirx_core::CoreErrorEvent {
            operation: mirx_core::Operation::SendMessage,
            room_id: Some("!ops:example".to_owned()),
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
    let exe_path =
        mirx_ui_exe_path().map_err(|error| format!("failed to locate mirx-ui.exe: {error}"))?;
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
            highlight: false,
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
    let exe_path =
        mirx_ui_exe_path().map_err(|error| format!("failed to locate mirx-ui.exe: {error}"))?;
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
    let exe_path =
        mirx_ui_exe_path().map_err(|error| format!("failed to locate mirx-ui.exe: {error}"))?;
    let state_path = write_state_file(&base_events())?;

    let args = [
        OsString::from("--render-state"),
        state_path.as_os_str().to_os_string(),
    ];
    let app = launch_app(&exe_path, &args)?;
    app.wait_idle(Duration::from_secs(5))?;

    let attached = MirxApp::find_by_pid(app.pid(), Duration::from_secs(5))?;
    attached.ping()?;

    let version = attached.get_version()?;
    assert_eq!(version.protocol_version, 47);
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
    assert_switchbar_activity(
        &replacement_state,
        &room_identity("!delta:example"),
        Some(ViewActivityLevel::Message),
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
fn automation_driver_exposes_space_metadata_but_rejects_space_actions()
-> Result<(), Box<dyn std::error::Error>> {
    let exe_path =
        mirx_ui_exe_path().map_err(|error| format!("failed to locate mirx-ui.exe: {error}"))?;
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
                notification_mode: Some(RoomNotificationMode::AllMessages),
                canonical_alias: None,
                alt_aliases: Vec::new(),
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
fn automation_driver_lifecycle_commands_are_submitted_in_render_state_mode()
-> Result<(), Box<dyn std::error::Error>> {
    let exe_path =
        mirx_ui_exe_path().map_err(|error| format!("failed to locate mirx-ui.exe: {error}"))?;
    let state_path = write_state_file(&base_events())?;
    let local_app_data = unique_local_app_data_path();
    let session_path = write_saved_session(&local_app_data, "https://matrix.example")?;
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
        app.start_sync(),
        app.logout(),
    ] {
        result?;
    }
    // The inert core never answers, so the login stays in flight and a
    // second sign-in is refused before it reaches the core.
    assert_eq!(app.dump_model()?.connection, "signing_in");
    assert!(
        app.restore_session("https://matrix.example", stored_session())
            .is_err()
    );
    assert!(app.restore_saved_session().is_err());

    let _ = fs::remove_file(&state_path);
    let _ = fs::remove_dir_all(&local_app_data);
    Ok(())
}

const MENU_ID_FILE_CONNECT: usize = 40030;
const MENU_ID_FILE_DISCONNECT: usize = 40031;
const MENU_ID_FILE_SELECT_SERVER: usize = 40032;
const LOGIN_DIALOG_SERVER_ID: i32 = 5001;
const LOGIN_DIALOG_USER_ID: i32 = 5002;
const LOGIN_DIALOG_PASSWORD_ID: i32 = 5003;
const LOGIN_DIALOG_OK_ID: usize = 5005;
const LOGIN_DIALOG_CANCEL_ID: usize = 5006;
const CONNECT_HINT: &str = "* Not connected. Use File > Select Server... (Alt+E) or /server to connect to a Matrix homeserver";

fn launch_automation_app(local_app_data: &Path) -> Result<GuiTestApp, Box<dyn std::error::Error>> {
    let exe_path =
        mirx_ui_exe_path().map_err(|error| format!("failed to locate mirx-ui.exe: {error}"))?;
    let app = launch_app_with_options(
        &exe_path,
        &[OsString::from("--automation")],
        launch_options(Some(local_app_data.to_path_buf())),
    )?;
    app.wait_for_lifecycle_state("core_started", Duration::from_secs(30))?;
    Ok(app)
}

fn status_view_contains(app: &MirxApp, line: &str) -> Result<bool, Box<dyn std::error::Error>> {
    Ok(view_contains(
        find_view(&app.dump_ui_state()?, &ViewIdentity::Status),
        line,
    ))
}

#[test]
fn startup_without_session_logs_connect_hint() -> Result<(), Box<dyn std::error::Error>> {
    let local_app_data = unique_local_app_data_path();
    let app = launch_automation_app(&local_app_data)?;

    assert_eq!(app.dump_model()?.connection, "signed_out");
    assert!(status_view_contains(&app, CONNECT_HINT)?);
    // Automation runs never open Select Server by themselves.
    let class_name = wide_null("mirxWin32LoginDialog");
    assert!(unsafe { FindWindowW(class_name.as_ptr(), std::ptr::null()) }.is_null());

    drop(app);
    let _ = fs::remove_dir_all(&local_app_data);
    Ok(())
}

#[test]
fn startup_with_saved_session_restores_and_syncs() -> Result<(), Box<dyn std::error::Error>> {
    let local_app_data = unique_local_app_data_path();
    let server = spawn_mock_backfill_homeserver()?;
    // A session saved by an older mirx still restores.
    write_version_1_saved_session(&local_app_data, &server.base_url)?;
    // Not launch_automation_app: the restore starts before the core reports
    // Started, so the lifecycle never shows core_started.
    let exe_path =
        mirx_ui_exe_path().map_err(|error| format!("failed to locate mirx-ui.exe: {error}"))?;
    let app = launch_app_with_options(
        &exe_path,
        &[OsString::from("--automation")],
        launch_options(Some(local_app_data.clone())),
    )?;

    app.wait_for_connection("syncing", Duration::from_secs(30))?;
    assert!(status_view_contains(&app, "* Syncing with the homeserver")?);
    assert!(!status_view_contains(&app, CONNECT_HINT)?);

    drop(app);
    let _ = fs::remove_dir_all(&local_app_data);
    Ok(())
}

#[test]
fn startup_with_connect_on_startup_off_restores_without_syncing()
-> Result<(), Box<dyn std::error::Error>> {
    let local_app_data = unique_local_app_data_path();
    let server = spawn_mock_backfill_homeserver()?;
    write_saved_session(&local_app_data, &server.base_url)?;
    fs::write(
        settings_file_path(&local_app_data),
        "[connect]\r\nstartup=off\r\n",
    )?;
    let exe_path =
        mirx_ui_exe_path().map_err(|error| format!("failed to locate mirx-ui.exe: {error}"))?;
    let app = launch_app_with_options(
        &exe_path,
        &[OsString::from("--automation")],
        launch_options(Some(local_app_data.clone())),
    )?;

    app.wait_for_connection("signed_in", Duration::from_secs(30))?;
    assert!(status_view_contains(
        &app,
        "* Connect on startup is off; File > Connect or /server connects"
    )?);
    app.wait_idle(Duration::from_secs(5))?;
    assert_eq!(app.dump_model()?.connection, "signed_in");
    assert!(!status_view_contains(
        &app,
        "* Syncing with the homeserver"
    )?);

    invoke_account_menu_command(&app, MENU_ID_FILE_CONNECT)?;
    app.wait_for_connection("syncing", Duration::from_secs(30))?;

    drop(app);
    let _ = fs::remove_dir_all(&local_app_data);
    Ok(())
}

#[test]
fn file_disconnect_and_connect_toggle_sync() -> Result<(), Box<dyn std::error::Error>> {
    let local_app_data = unique_local_app_data_path();
    let server = spawn_mock_backfill_homeserver()?;
    let app = launch_automation_app(&local_app_data)?;
    app.restore_session(&server.base_url, stored_session())?;
    app.wait_for_lifecycle_state("ready", Duration::from_secs(30))?;
    assert_eq!(app.dump_model()?.connection, "signed_in");

    invoke_account_menu_command(&app, MENU_ID_FILE_CONNECT)?;
    app.wait_for_connection("syncing", Duration::from_secs(30))?;
    invoke_account_menu_command(&app, MENU_ID_FILE_DISCONNECT)?;
    app.wait_for_connection("signed_in", Duration::from_secs(30))?;
    assert!(status_view_contains(&app, "] * Disconnected")?);

    app.select_status_view()?;
    type_commands(&app, &["/server"])?;
    app.wait_for_connection("syncing", Duration::from_secs(30))?;
    type_commands(&app, &["/server"])?;
    assert!(status_view_contains(&app, "* /server: already connected")?);
    type_commands(&app, &["/quit"])?;
    app.wait_for_connection("signed_in", Duration::from_secs(30))?;
    type_commands(&app, &["/quit", "/quit see you"])?;
    assert!(status_view_contains(&app, "* /quit: not connected")?);
    assert!(status_view_contains(
        &app,
        "* /quit: Matrix has no quit message"
    )?);

    drop(app);
    let _ = fs::remove_dir_all(&local_app_data);
    Ok(())
}

/// Opens File > Select Server..., which runs modally on the thread this
/// returns, and fills in its fields; the dialog is returned open.
fn open_select_server(
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
fn submit_select_server(
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

#[test]
fn select_server_dialog_logs_in_and_syncs() -> Result<(), Box<dyn std::error::Error>> {
    let local_app_data = unique_local_app_data_path();
    let server = spawn_mock_backfill_homeserver()?;
    let app = launch_automation_app(&local_app_data)?;
    assert_eq!(app.dump_model()?.connection, "signed_out");

    // A user ID's server part cannot name the http://127.0.0.1 mock, so the
    // server is typed; the empty-server path is a login_server unit test.
    let (dialog, menu_thread) = open_select_server(&app, [&server.base_url, "alice", "secret"])?;
    submit_select_server(dialog, menu_thread)?;

    app.wait_for_connection("syncing", Duration::from_secs(30))?;
    assert_eq!(
        saved_session_homeserver_url(&local_app_data)?,
        format!("{}/", server.base_url)
    );

    drop(app);
    let _ = fs::remove_dir_all(&local_app_data);
    Ok(())
}

/// Opens Select Server with User and Password empty, presses OK and
/// waits for the browser sign-in; returns the page the app handed to the
/// shell.
fn start_browser_sign_in(
    app: &MirxApp,
    server: &str,
) -> Result<String, Box<dyn std::error::Error>> {
    let (dialog, menu_thread) = open_select_server(app, [server, "", ""])?;
    submit_select_server(dialog, menu_thread)?;
    app.wait_for_connection("waiting_for_browser", Duration::from_secs(30))?;
    app.wait_idle(Duration::from_secs(5))?;
    let opened_links = app.dump_model()?.opened_links;
    let [sign_in] = &opened_links[..] else {
        return Err(format!("expected the sign-in page alone in {opened_links:?}").into());
    };
    Ok(sign_in.clone())
}

/// The value of `name` in the query of `url`, decoded.
fn query_param(url: &str, name: &str) -> Option<String> {
    let (_, query) = url.split_once('?')?;
    query.split('&').find_map(|pair| {
        let (key, value) = pair.split_once('=')?;
        (key == name).then(|| percent_decode(value))
    })
}

fn percent_decode(text: &str) -> String {
    let mut bytes = Vec::new();
    let mut rest = text.as_bytes();
    while let [first, tail @ ..] = rest {
        match (first, tail) {
            (b'%', [high, low, tail @ ..]) => {
                let hex = std::str::from_utf8(&[*high, *low])
                    .expect("percent escapes are ASCII")
                    .to_owned();
                bytes.push(u8::from_str_radix(&hex, 16).expect("a percent escape is hex"));
                rest = tail;
            }
            (b'+', _) => {
                bytes.push(b' ');
                rest = tail;
            }
            (byte, _) => {
                bytes.push(*byte);
                rest = tail;
            }
        }
    }
    String::from_utf8(bytes).expect("a decoded query value is UTF-8")
}

/// `http://127.0.0.1:<port>/<path>` as (port, path).
fn loopback_redirect(redirect_uri: &str) -> (u16, String) {
    let rest = redirect_uri
        .strip_prefix("http://127.0.0.1:")
        .unwrap_or_else(|| panic!("{redirect_uri} is not a loopback address"));
    let (port, path) = rest.split_once('/').expect("the redirect has a path");
    (
        port.parse().expect("the redirect names its port"),
        format!("/{path}"),
    )
}

/// Plays the browser coming back from the sign-in page with `query`, and
/// returns the page mirx answered.
fn browse_to_redirect(sign_in: &str, query: &str) -> Result<String, Box<dyn std::error::Error>> {
    let redirect_uri = query_param(sign_in, "redirect_uri").ok_or("no redirect_uri")?;
    let (port, path) = loopback_redirect(&redirect_uri);
    let mut stream = std::net::TcpStream::connect(("127.0.0.1", port))?;
    write!(
        stream,
        "GET {path}?{query} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\n\r\n"
    )?;
    let mut response = String::new();
    stream.read_to_string(&mut response)?;
    Ok(response)
}

#[test]
fn select_server_browser_sign_in_with_oauth_restores_after_restart()
-> Result<(), Box<dyn std::error::Error>> {
    let local_app_data = unique_local_app_data_path();
    let server = spawn_mock_homeserver_with_auth(MockAuth::OAuth, MockMessages::Hang, vec![])?;
    let app = launch_automation_app(&local_app_data)?;

    let sign_in = start_browser_sign_in(&app, &server.base_url)?;
    let authorize = format!("{}/oauth2/authorize?", server.base_url);
    assert!(sign_in.starts_with(&authorize), "{sign_in}");
    assert!(status_view_contains(
        &app,
        &format!(
            "* Connecting to {} to sign in with your browser",
            server.base_url
        )
    )?);
    assert!(status_view_contains(
        &app,
        "* Sign in on the page your browser opened; File > Disconnect cancels"
    )?);
    assert!(status_view_contains(
        &app,
        &format!("* If no browser opened, go to {sign_in}")
    )?);
    // The devshell's Wine has no browser to start; the printed address
    // still works.
    assert!(status_view_contains(
        &app,
        &format!("* Unable to open {sign_in}: ShellExecuteExW failed")
    )?);

    let state = query_param(&sign_in, "state").ok_or("no state")?;
    let page = browse_to_redirect(&sign_in, &format!("code=c&state={state}"))?;
    assert!(page.starts_with("HTTP/1.1 200 OK\r\n"), "{page}");
    assert!(
        page.contains("Signed in to mirx as @alice:example. You can close this page."),
        "{page}"
    );
    app.wait_for_connection("syncing", Duration::from_secs(30))?;
    let saved = read_saved_session(&local_app_data)?;
    assert_eq!(saved["version"], 2);
    assert_eq!(
        saved["session"]["auth"],
        serde_json::json!({ "OAuth": { "client_id": "mirx-client" } })
    );
    assert_eq!(saved["session"]["access_token"], "oauth-token");
    drop(app);

    // The next start restores the OAuth session.
    let exe_path =
        mirx_ui_exe_path().map_err(|error| format!("failed to locate mirx-ui.exe: {error}"))?;
    let app = launch_app_with_options(
        &exe_path,
        &[OsString::from("--automation")],
        launch_options(Some(local_app_data.clone())),
    )?;
    app.wait_for_connection("syncing", Duration::from_secs(30))?;
    assert!(status_view_contains(&app, "* Session restored")?);

    drop(app);
    let _ = fs::remove_dir_all(&local_app_data);
    Ok(())
}

#[test]
fn file_disconnect_cancels_a_browser_sign_in() -> Result<(), Box<dyn std::error::Error>> {
    let local_app_data = unique_local_app_data_path();
    let server = spawn_mock_homeserver_with_auth(MockAuth::OAuth, MockMessages::Hang, vec![])?;
    let app = launch_automation_app(&local_app_data)?;

    let sign_in = start_browser_sign_in(&app, &server.base_url)?;
    let (port, _) = loopback_redirect(&query_param(&sign_in, "redirect_uri").ok_or("no redirect")?);
    invoke_account_menu_command(&app, MENU_ID_FILE_DISCONNECT)?;
    app.wait_for_connection("signed_out", Duration::from_secs(30))?;
    app.wait_idle(Duration::from_secs(5))?;
    assert!(status_view_contains(&app, "* Sign-in cancelled")?);
    assert_eq!(app.dump_model()?.status, "sign-in cancelled");
    assert!(std::net::TcpStream::connect(("127.0.0.1", port)).is_err());

    drop(app);
    let _ = fs::remove_dir_all(&local_app_data);
    Ok(())
}

#[test]
fn select_server_refuses_a_bare_user_without_a_password() -> Result<(), Box<dyn std::error::Error>>
{
    let local_app_data = unique_local_app_data_path();
    let app = launch_automation_app(&local_app_data)?;

    let (dialog, menu_thread) = open_select_server(&app, ["matrix.example", "alice", ""])?;
    assert_ne!(
        unsafe { PostMessageW(dialog, WM_COMMAND, LOGIN_DIALOG_OK_ID as WPARAM, 0) },
        0
    );
    let (error_box, text) = wait_for_owned_message_box(dialog);
    assert_eq!(
        text,
        "to sign in with your browser, leave User empty or enter a full user ID like @alice:matrix.org"
    );
    answer_message_box(&app, error_box, IDOK)?;
    // The form stays open to be corrected.
    assert_ne!(unsafe { IsWindow(dialog) }, 0);
    assert_ne!(
        unsafe { PostMessageW(dialog, WM_COMMAND, LOGIN_DIALOG_CANCEL_ID as WPARAM, 0) },
        0
    );
    wait_for_window_gone(dialog, "Select Server dialog")?;
    menu_thread
        .join()
        .expect("Select Server menu command thread must not panic");
    assert_eq!(app.dump_model()?.connection, "signed_out");

    drop(app);
    let _ = fs::remove_dir_all(&local_app_data);
    Ok(())
}

fn wait_for_window(class_name: &[u16], what: &str) -> Result<HWND, Box<dyn std::error::Error>> {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let window = unsafe { FindWindowW(class_name.as_ptr(), std::ptr::null()) };
        if !window.is_null() {
            return Ok(window);
        }
        if Instant::now() >= deadline {
            return Err(format!("{what} did not appear within five seconds").into());
        }
        std::thread::sleep(POLL_INTERVAL);
    }
}

fn wait_for_window_gone(window: HWND, what: &str) -> Result<(), Box<dyn std::error::Error>> {
    let deadline = Instant::now() + Duration::from_secs(5);
    while unsafe { IsWindow(window) } != 0 {
        if Instant::now() >= deadline {
            return Err(format!("{what} did not close within five seconds").into());
        }
        std::thread::sleep(POLL_INTERVAL);
    }
    Ok(())
}

#[test]
fn automation_driver_updates_lifecycle_from_synthetic_events_without_leaking_tokens()
-> Result<(), Box<dyn std::error::Error>> {
    let exe_path =
        mirx_ui_exe_path().map_err(|error| format!("failed to locate mirx-ui.exe: {error}"))?;
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
            homeserver_url: "https://matrix.example/".to_owned(),
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
    let ui_state = app.dump_ui_state()?;
    assert_eq!(
        find_view(&ui_state, &ViewIdentity::Status).title,
        "Status: @alice:example"
    );
    assert_eq!(
        find_switchbar_item(&ui_state, &ViewIdentity::Status).title,
        "@alice:example"
    );
    assert_eq!(ui_state.navigation.text, "@alice:example");

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
    let exe_path =
        mirx_ui_exe_path().map_err(|error| format!("failed to locate mirx-ui.exe: {error}"))?;
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
    assert!(view_contains(
        status_view,
        "* E2EE: recovery incomplete, local backup enabled, server backup exists, cross-signing incomplete",
    ));
    assert!(view_contains(
        status_view,
        "* Use /e2ee refresh to check server backup, /e2ee setup to enable recovery, /e2ee recover to restore keys",
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
    let exe_path =
        mirx_ui_exe_path().map_err(|error| format!("failed to locate mirx-ui.exe: {error}"))?;
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
                emojis: vec![mirx_testkit::VerificationEmoji {
                    symbol: "🐶".to_owned(),
                    description: "dog".to_owned(),
                }],
                decimals: mirx_testkit::VerificationDecimals {
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
    let exe_path =
        mirx_ui_exe_path().map_err(|error| format!("failed to locate mirx-ui.exe: {error}"))?;
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
    let exe_path =
        mirx_ui_exe_path().map_err(|error| format!("failed to locate mirx-ui.exe: {error}"))?;
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
    let exe_path =
        mirx_ui_exe_path().map_err(|error| format!("failed to locate mirx-ui.exe: {error}"))?;
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
    let exe_path =
        mirx_ui_exe_path().map_err(|error| format!("failed to locate mirx-ui.exe: {error}"))?;

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
    let exe_path =
        mirx_ui_exe_path().map_err(|error| format!("failed to locate mirx-ui.exe: {error}"))?;
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
    let exe_path =
        mirx_ui_exe_path().map_err(|error| format!("failed to locate mirx-ui.exe: {error}"))?;
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
    let exe_path =
        mirx_ui_exe_path().map_err(|error| format!("failed to locate mirx-ui.exe: {error}"))?;
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
    let exe_path =
        mirx_ui_exe_path().map_err(|error| format!("failed to locate mirx-ui.exe: {error}"))?;
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
    let exe_path =
        mirx_ui_exe_path().map_err(|error| format!("failed to locate mirx-ui.exe: {error}"))?;
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
    let exe_path =
        mirx_ui_exe_path().map_err(|error| format!("failed to locate mirx-ui.exe: {error}"))?;
    let recovery_key = "automation recovery key must not persist";
    let state_path = write_state_file(&[
        Event::Started,
        Event::E2eeRecoverySetupSucceeded(mirx_core::E2eeRecoverySetupSucceeded {
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
    let exe_path =
        mirx_ui_exe_path().map_err(|error| format!("failed to locate mirx-ui.exe: {error}"))?;
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
    let exe_path =
        mirx_ui_exe_path().map_err(|error| format!("failed to locate mirx-ui.exe: {error}"))?;
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
                RoomMemberChange::Joined,
                Some("Carol"),
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
    assert!(view_contains(
        ops_view,
        "* Now talking in Ops (!ops:example)"
    ));
    assert!(view_contains(ops_view, "* Topic is 'incident review'"));
    assert!(view_contains(
        ops_view,
        "[22:15] <Alice> ops topic: incident review"
    ));
    assert!(view_contains(
        ops_view,
        "[22:13] * Alice changes topic to 'incident review'"
    ));
    assert!(view_contains(
        ops_view,
        "[22:14] * Joins: Carol (@carol:example)"
    ));

    Ok(())
}

fn launch_multi_room_fixture() -> Result<GuiTestApp, Box<dyn std::error::Error>> {
    let exe_path =
        mirx_ui_exe_path().map_err(|error| format!("failed to locate mirx-ui.exe: {error}"))?;
    let fixture_path = fixture_state_path("multi-room.json");
    let args = [
        OsString::from("--render-state"),
        fixture_path.as_os_str().to_os_string(),
    ];
    let app = launch_app(&exe_path, &args)?;
    app.wait_idle(Duration::from_secs(5))?;
    Ok(app)
}

unsafe fn mdi_children(app: &MirxApp) -> Vec<HWND> {
    let mdi_client = unsafe { mdi_client_hwnd(app) };
    let mut children = Vec::new();
    let mut child =
        unsafe { FindWindowExW(mdi_client, 0 as HWND, std::ptr::null(), std::ptr::null()) };
    while !child.is_null() {
        children.push(child);
        child = unsafe { FindWindowExW(mdi_client, child, std::ptr::null(), std::ptr::null()) };
    }
    children
}

#[test]
fn per_window_editbox_keeps_drafts_across_activation() -> Result<(), Box<dyn std::error::Error>> {
    let app = launch_multi_room_fixture()?;
    let lobby = room_identity("!lobby:example");
    let ops = room_identity("!ops:example");

    app.select_view(lobby.clone())?;
    app.set_compose_text("lobby draft")?;
    app.select_view(ops.clone())?;
    assert_eq!(app.compose_text()?, "");
    app.set_compose_text("ops draft")?;
    app.select_view(lobby)?;
    assert_eq!(app.compose_text()?, "lobby draft");
    app.select_view(ops)?;
    assert_eq!(app.compose_text()?, "ops draft");
    Ok(())
}

#[test]
fn editbox_lives_inside_each_mdi_child() -> Result<(), Box<dyn std::error::Error>> {
    let app = launch_multi_room_fixture()?;
    let frame = app.automation_frame_hwnd() as HWND;
    let edit_class = wide_null("Edit");
    let frame_edit =
        unsafe { FindWindowExW(frame, 0 as HWND, edit_class.as_ptr(), std::ptr::null()) };
    assert!(
        frame_edit.is_null(),
        "the frame must not have its own editbox"
    );

    let children = unsafe { mdi_children(&app) };
    // Status, Lobby, Ops and Random.
    assert_eq!(children.len(), 4);
    for child in children {
        let edit = unsafe { GetDlgItem(child, VIEW_EDIT_ID) };
        assert!(!edit.is_null(), "every MDI child must have an editbox");
        assert!(unsafe { class_name(edit) }.eq_ignore_ascii_case("Edit"));

        let child_client = unsafe { client_rect(child) };
        let mut edit_rect = RECT::default();
        unsafe {
            GetWindowRect(edit, &mut edit_rect);
            MapWindowPoints(
                null_mut(),
                child,
                (&mut edit_rect as *mut RECT).cast::<POINT>(),
                2,
            );
        }
        assert_eq!(edit_rect.bottom, child_client.bottom);
        assert_eq!(edit_rect.left, child_client.left);
        assert_eq!(edit_rect.right, child_client.right);
    }
    Ok(())
}

#[test]
fn slash_command_switching_windows_clears_the_submitting_editbox()
-> Result<(), Box<dyn std::error::Error>> {
    let app = launch_multi_room_fixture()?;
    app.select_view(ViewIdentity::Status)?;
    app.set_compose_text("status draft")?;
    app.select_view(room_identity("!ops:example"))?;
    app.set_compose_text("/status")?;
    app.send_compose_key(ComposeKey::Return)?;

    assert_eq!(app.dump_ui_state()?.mdi.active, ViewIdentity::Status);
    assert_eq!(app.compose_text()?, "status draft");
    app.select_view(room_identity("!ops:example"))?;
    assert_eq!(app.compose_text()?, "");
    Ok(())
}

fn view_placement(ui_state: &UiState, identity: &ViewIdentity) -> MdiWindowPlacement {
    find_view(ui_state, identity).placement.clone()
}

fn rects_overlap(a: &MdiWindowPlacement, b: &MdiWindowPlacement) -> bool {
    a.x < b.x + b.width && b.x < a.x + a.width && a.y < b.y + b.height && b.y < a.y + a.height
}

#[test]
fn window_menu_tile_vertical_restores_and_tiles_children() -> Result<(), Box<dyn std::error::Error>>
{
    let app = launch_multi_room_fixture()?;
    let active_before = app.dump_ui_state()?.mdi.active;
    app.arrange_windows(MdiArrangement::TileVertical)?;

    let ui_state = app.dump_ui_state()?;
    assert_eq!(ui_state.mdi.active, active_before);
    let client = unsafe { client_rect(mdi_client_hwnd(&app)) };
    let placements = ui_state
        .mdi
        .views
        .iter()
        .map(|view| view.placement.clone())
        .collect::<Vec<_>>();
    assert_eq!(placements.len(), 4);
    for (index, placement) in placements.iter().enumerate() {
        assert_eq!(placement.state, MdiWindowState::Normal);
        assert!(placement.x >= 0 && placement.y >= 0);
        assert!(placement.x + placement.width <= client.right);
        assert!(placement.y + placement.height <= client.bottom);
        for other in &placements[index + 1..] {
            assert!(
                !rects_overlap(placement, other),
                "tiled windows overlap: {placement:?} {other:?}"
            );
        }
    }
    let covered = placements
        .iter()
        .map(|placement| placement.width * placement.height)
        .sum::<i32>();
    // MDI divides the client area by integer division.
    assert!(covered * 100 >= client.right * client.bottom * 98);
    Ok(())
}

#[test]
fn window_menu_cascade_offsets_children() -> Result<(), Box<dyn std::error::Error>> {
    let app = launch_multi_room_fixture()?;
    app.arrange_windows(MdiArrangement::Cascade)?;

    let ui_state = app.dump_ui_state()?;
    let mut placements = ui_state
        .mdi
        .views
        .iter()
        .map(|view| view.placement.clone())
        .collect::<Vec<_>>();
    assert!(
        placements
            .iter()
            .all(|placement| placement.state == MdiWindowState::Normal)
    );
    placements.sort_by_key(|placement| placement.x);
    for pair in placements.windows(2) {
        assert!(
            pair[1].x > pair[0].x && pair[1].y > pair[0].y,
            "cascaded windows must step down and right: {pair:?}"
        );
    }
    Ok(())
}

#[test]
fn activating_minimized_view_restores_it() -> Result<(), Box<dyn std::error::Error>> {
    let app = launch_multi_room_fixture()?;
    let lobby = room_identity("!lobby:example");
    app.arrange_windows(MdiArrangement::TileVertical)?;
    app.set_view_window_state(lobby.clone(), MdiWindowState::Minimized)?;
    app.wait_idle(Duration::from_secs(5))?;
    assert_eq!(
        view_placement(&app.dump_ui_state()?, &lobby).state,
        MdiWindowState::Minimized
    );

    app.select_view(lobby.clone())?;
    let ui_state = app.dump_ui_state()?;
    assert_eq!(ui_state.mdi.active, lobby);
    assert_eq!(
        view_placement(&ui_state, &lobby).state,
        MdiWindowState::Normal
    );
    Ok(())
}

#[test]
fn maximized_state_carries_across_activation() -> Result<(), Box<dyn std::error::Error>> {
    let app = launch_multi_room_fixture()?;
    let ops = room_identity("!ops:example");
    let lobby = room_identity("!lobby:example");
    let random = room_identity("!random:example");
    assert_eq!(
        view_placement(&app.dump_ui_state()?, &ops).state,
        MdiWindowState::Maximized
    );

    app.select_view(lobby.clone())?;
    let ui_state = app.dump_ui_state()?;
    assert_eq!(
        view_placement(&ui_state, &lobby).state,
        MdiWindowState::Maximized
    );
    assert_eq!(
        view_placement(&ui_state, &ops).state,
        MdiWindowState::Normal
    );

    app.arrange_windows(MdiArrangement::TileVertical)?;
    app.select_view(random.clone())?;
    let ui_state = app.dump_ui_state()?;
    assert_eq!(ui_state.mdi.active, random);
    assert!(
        ui_state
            .mdi
            .views
            .iter()
            .all(|view| view.placement.state == MdiWindowState::Normal)
    );
    Ok(())
}

/// An `--automation` app with a channel (Beta), a query with @alice
/// (Alpha) and an invite (Lobby), for the window close lifecycle.
fn launch_close_lifecycle_app() -> Result<(GuiTestApp, PathBuf), Box<dyn std::error::Error>> {
    let exe_path =
        mirx_ui_exe_path().map_err(|error| format!("failed to locate mirx-ui.exe: {error}"))?;
    let local_app_data = unique_local_app_data_path();
    let app = launch_app_with_options(
        &exe_path,
        &[OsString::from("--automation")],
        launch_options(Some(local_app_data.clone())),
    )?;
    app.wait_for_lifecycle_state("core_started", Duration::from_secs(30))?;
    app.load_events(&serde_json::to_string(&close_lifecycle_events())?)?;
    app.wait_idle(Duration::from_secs(5))?;
    Ok((app, local_app_data))
}

fn close_lifecycle_rooms() -> Vec<RoomSummary> {
    let mut alpha = room_summary("!alpha:example", "Alpha", false);
    alpha.is_direct = true;
    alpha.direct_user_ids = vec!["@alice:example".to_owned()];
    vec![
        room_summary("!beta:example", "Beta", false),
        alpha,
        invited_room_summary("!invite:example", "Lobby", false),
    ]
}

fn close_lifecycle_events() -> Vec<Event> {
    vec![
        Event::Started,
        Event::SyncedRooms(close_lifecycle_rooms()),
        text_event(
            "!beta:example",
            "@bob:example",
            1_700_000_000_000,
            "beta first",
        ),
        text_event(
            "!alpha:example",
            "@alice:example",
            1_700_000_060_000,
            "alpha first",
        ),
    ]
}

fn view_child_hwnd(
    app: &MirxApp,
    identity: &ViewIdentity,
) -> Result<HWND, Box<dyn std::error::Error>> {
    let title = find_view(&app.dump_ui_state()?, identity).title.clone();
    Ok(unsafe { mdi_child_hwnd_by_title(app, &title) })
}

/// Asks the view's MDI child to close the way its caption X does, for a
/// close that asks first. Posted, not sent: the confirmation box runs a
/// modal loop inside the handler.
fn request_view_close(
    app: &MirxApp,
    identity: &ViewIdentity,
) -> Result<HWND, Box<dyn std::error::Error>> {
    let child = view_child_hwnd(app, identity)?;
    assert_ne!(
        unsafe { PostMessageW(child, WM_SYSCOMMAND, SC_CLOSE as WPARAM, 0) },
        0,
        "posting SC_CLOSE must succeed"
    );
    Ok(child)
}

/// Closes the view's MDI child the way its caption X does, for a close
/// that must not ask: the send returns only once the close is handled,
/// and times out if a modal box blocks it.
fn close_view_without_prompt(
    app: &MirxApp,
    identity: &ViewIdentity,
) -> Result<HWND, Box<dyn std::error::Error>> {
    let child = view_child_hwnd(app, identity)?;
    let mut result = 0;
    assert_ne!(
        unsafe {
            SendMessageTimeoutW(
                child,
                WM_SYSCOMMAND,
                SC_CLOSE as WPARAM,
                0,
                SMTO_NORMAL,
                5_000,
                &mut result,
            )
        },
        0,
        "closing the view must not block on a prompt"
    );
    app.wait_idle(Duration::from_secs(5))?;
    Ok(child)
}

/// The dialog box (MessageBox or common dialog) `owner` shows, if any.
unsafe fn owned_message_box(owner: HWND) -> HWND {
    let thread_id = unsafe { GetWindowThreadProcessId(owner, std::ptr::null_mut()) };
    let mut search = (owner, 0 as HWND);
    unsafe {
        EnumThreadWindows(
            thread_id,
            Some(find_owned_message_box),
            (&raw mut search) as isize,
        );
    }
    search.1
}

unsafe extern "system" fn find_owned_message_box(hwnd: HWND, lparam: isize) -> i32 {
    let search = unsafe { &mut *(lparam as *mut (HWND, HWND)) };
    if unsafe { class_name(hwnd) } == "#32770" && unsafe { GetWindow(hwnd, GW_OWNER) } == search.0 {
        search.1 = hwnd;
        0
    } else {
        1
    }
}

/// Waits for the frame's MessageBox and returns it with its text.
fn wait_for_message_box(app: &MirxApp) -> (HWND, String) {
    wait_for_owned_message_box(app.automation_frame_hwnd() as HWND)
}

/// Waits for the MessageBox `owner` shows and returns it with its text.
fn wait_for_owned_message_box(owner: HWND) -> (HWND, String) {
    let (dialog, _) = wait_for_owned_dialog(owner);
    // MessageBox's text is the static control with ID 0xFFFF.
    let text = unsafe { read_window_text(GetDlgItem(dialog, 0xFFFF)) };
    (dialog, text)
}

/// Waits for the dialog box (`#32770`: a MessageBox or a common dialog)
/// `owner` shows and returns it with its caption.
fn wait_for_owned_dialog(owner: HWND) -> (HWND, String) {
    // The file dialog loads the shell first, so it can take a while.
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let dialog = unsafe { owned_message_box(owner) };
        // The box exists before WM_INITDIALOG fills it; a dialog box shows
        // only once that is done.
        if !dialog.is_null() && unsafe { IsWindowVisible(dialog) } != 0 {
            return (dialog, unsafe { read_window_text(dialog) });
        }
        assert!(
            Instant::now() < deadline,
            "dialog box did not appear within ten seconds"
        );
        std::thread::sleep(POLL_INTERVAL);
    }
}

fn answer_message_box(
    app: &MirxApp,
    dialog: HWND,
    button: i32,
) -> Result<(), Box<dyn std::error::Error>> {
    assert_ne!(
        unsafe { PostMessageW(dialog, WM_COMMAND, button as WPARAM, 0) },
        0,
        "posting the message box answer must succeed"
    );
    let deadline = Instant::now() + Duration::from_secs(5);
    while unsafe { IsWindow(dialog) } != 0 {
        assert!(
            Instant::now() < deadline,
            "message box did not close within five seconds"
        );
        std::thread::sleep(POLL_INTERVAL);
    }
    app.wait_idle(Duration::from_secs(5))?;
    Ok(())
}

#[test]
fn closing_a_room_window_confirms_then_parts() -> Result<(), Box<dyn std::error::Error>> {
    let (app, local_app_data) = launch_close_lifecycle_app()?;
    let beta = room_identity("!beta:example");
    app.select_view(beta.clone())?;

    let child = request_view_close(&app, &beta)?;
    let (dialog, text) = wait_for_message_box(&app);
    assert_eq!(text, "Leave Beta?\n\nYou may need a new invite to rejoin.");
    answer_message_box(&app, dialog, IDNO)?;
    assert_eq!(unsafe { IsWindow(child) }, 1);
    assert!(app.dump_model()?.submitted_membership_commands.is_empty());

    request_view_close(&app, &beta)?;
    let (dialog, _) = wait_for_message_box(&app);
    answer_message_box(&app, dialog, IDYES)?;
    let model = app.dump_model()?;
    assert_eq!(model.status, "part submitted: !beta:example");
    assert_eq!(
        model.submitted_membership_commands,
        vec![SubmittedMembershipCommand {
            action: MembershipCommandAction::Leave,
            target: "!beta:example".to_owned(),
        }]
    );
    // As in mIRC, the window stays until the part lands.
    assert_eq!(unsafe { IsWindow(child) }, 1);
    assert_eq!(app.dump_ui_state()?.mdi.active, beta);

    let mut rooms = close_lifecycle_rooms();
    rooms.retain(|room| room.room_id != "!beta:example");
    app.append_synthetic_events(&serde_json::to_string(&vec![Event::SyncedRooms(rooms)])?)?;
    app.wait_idle(Duration::from_secs(5))?;
    assert_eq!(unsafe { IsWindow(child) }, 0);
    let ui_state = app.dump_ui_state()?;
    assert_ne!(ui_state.mdi.active, beta);
    assert!(ui_state.navigation.find_view(&beta).is_empty());
    assert_eq!(ui_state.switchbar.items.len(), ui_state.mdi.views.len());
    assert_compose_has_focus(&app);

    drop(app);
    let _ = fs::remove_dir_all(&local_app_data);
    Ok(())
}

#[test]
fn closing_an_invite_window_confirms_then_rejects() -> Result<(), Box<dyn std::error::Error>> {
    let (app, local_app_data) = launch_close_lifecycle_app()?;
    let invite = room_identity("!invite:example");
    app.select_view(invite.clone())?;

    let child = request_view_close(&app, &invite)?;
    let (dialog, text) = wait_for_message_box(&app);
    assert_eq!(text, "Reject the invite to Lobby?");
    answer_message_box(&app, dialog, IDYES)?;
    let model = app.dump_model()?;
    assert_eq!(model.status, "reject submitted: !invite:example");
    assert_eq!(
        model.submitted_membership_commands,
        vec![SubmittedMembershipCommand {
            action: MembershipCommandAction::RejectInvite,
            target: "!invite:example".to_owned(),
        }]
    );
    assert_eq!(unsafe { IsWindow(child) }, 1);

    drop(app);
    let _ = fs::remove_dir_all(&local_app_data);
    Ok(())
}

#[test]
fn closing_a_query_window_hides_it_until_its_next_message() -> Result<(), Box<dyn std::error::Error>>
{
    let (app, local_app_data) = launch_close_lifecycle_app()?;
    let alpha = room_identity("!alpha:example");
    app.select_view(alpha.clone())?;

    let child = close_view_without_prompt(&app, &alpha)?;
    assert_eq!(unsafe { IsWindow(child) }, 0);
    let ui_state = app.dump_ui_state()?;
    assert!(ui_state.mdi.views.iter().all(|view| view.identity != alpha));
    assert!(
        ui_state
            .switchbar
            .items
            .iter()
            .all(|item| item.identity != alpha)
    );
    assert_eq!(ui_state.navigation.find_view(&alpha).len(), 1);
    assert_compose_has_focus(&app);

    // A sync leaves it closed; the other side talking reopens it.
    app.append_synthetic_events(&serde_json::to_string(&vec![Event::SyncedRooms(
        close_lifecycle_rooms(),
    )])?)?;
    app.wait_idle(Duration::from_secs(5))?;
    assert!(
        app.dump_ui_state()?
            .mdi
            .views
            .iter()
            .all(|view| view.identity != alpha)
    );
    app.append_synthetic_events(&serde_json::to_string(&vec![text_event(
        "!alpha:example",
        "@alice:example",
        1_700_000_120_000,
        "alpha second",
    )])?)?;
    app.wait_idle(Duration::from_secs(5))?;
    let ui_state = app.dump_ui_state()?;
    assert!(ui_state.mdi.views.iter().any(|view| view.identity == alpha));
    assert_ne!(ui_state.mdi.active, alpha);
    assert_switchbar_activity(&ui_state, &alpha, Some(ViewActivityLevel::Message));

    drop(app);
    let _ = fs::remove_dir_all(&local_app_data);
    Ok(())
}

#[test]
fn automation_driver_irc_info_commands_echo_in_active_window()
-> Result<(), Box<dyn std::error::Error>> {
    let (app, local_app_data) = launch_close_lifecycle_app()?;
    app.append_synthetic_events(&serde_json::to_string(&vec![
        room_members_event(
            "!beta:example",
            &[
                ("@alice:example", Some("Alice")),
                ("@bob:example", Some("Bob")),
            ],
        ),
        room_members_event("!alpha:example", &[("@alice:example", Some("Alice"))]),
    ])?)?;
    app.wait_idle(Duration::from_secs(5))?;
    let beta = room_identity("!beta:example");
    let alpha = room_identity("!alpha:example");
    app.select_view(beta.clone())?;

    for input in [
        "/names",
        "/whois alice",
        "/help names",
        "/msg alice hi there",
    ] {
        app.set_compose_text(input)?;
        app.send_compose_key(ComposeKey::Return)?;
        app.wait_idle(Duration::from_secs(5))?;
        assert_eq!(app.compose_text()?, "", "{input}");
    }
    // The server's answer to /whois.
    app.append_synthetic_events(&serde_json::to_string(&vec![Event::UserProfile(
        UserProfile {
            user_id: "@alice:example".to_owned(),
            display_name: Some("Alice Liddell".to_owned()),
        },
    )])?)?;
    app.wait_idle(Duration::from_secs(5))?;
    let ui_state = app.dump_ui_state()?;
    assert_eq!(ui_state.mdi.active, beta);
    let view = find_view(&ui_state, &beta);
    for line in [
        "] * Names Beta: Alice Bob",
        "] * End of /NAMES list",
        "] * Alice is @alice:example * Alice Liddell",
        "] * Alice on Beta Alpha",
        "] * Alice using example",
        "] * Alice End of /WHOIS list",
        "] * /names [#room] - list this room's members, or the named room's",
        "] -> *alice* hi there",
    ] {
        assert!(
            only_view_line_containing(view, line).ends_with(line),
            "{line:?} in {:?}",
            view.lines
        );
    }
    // /msg does not open or activate the query.
    assert!(!view_contains(find_view(&ui_state, &alpha), "hi there"));

    app.select_status_view()?;
    app.set_compose_text("/close")?;
    app.send_compose_key(ComposeKey::Return)?;
    app.wait_idle(Duration::from_secs(5))?;
    let ui_state = app.dump_ui_state()?;
    assert_eq!(ui_state.mdi.active, ViewIdentity::Status);
    assert!(view_contains(
        find_view(&ui_state, &ViewIdentity::Status),
        "* /close: the Status window cannot be closed"
    ));

    // /close closes the active window as its close button does: a query
    // window hides.
    app.select_view(alpha.clone())?;
    app.set_compose_text("/close")?;
    app.send_compose_key(ComposeKey::Return)?;
    app.wait_idle(Duration::from_secs(5))?;
    let ui_state = app.dump_ui_state()?;
    assert!(ui_state.mdi.views.iter().all(|view| view.identity != alpha));
    assert!(
        ui_state
            .switchbar
            .items
            .iter()
            .all(|item| item.identity != alpha)
    );

    drop(app);
    let _ = fs::remove_dir_all(&local_app_data);
    Ok(())
}

fn settings_file_path(local_app_data: &Path) -> PathBuf {
    local_app_data
        .join("mirx")
        .join("ui-shell")
        .join("mirx.ini")
}

/// Types each command in the active window and waits for it to run.
fn type_commands(app: &MirxApp, inputs: &[&str]) -> Result<(), Box<dyn std::error::Error>> {
    for input in inputs {
        app.set_compose_text(input)?;
        app.send_compose_key(ComposeKey::Return)?;
        app.wait_idle(Duration::from_secs(5))?;
        assert_eq!(app.compose_text()?, "", "{input}");
    }
    Ok(())
}

/// The window menu's Font... id.
const SYSMENU_ID_FONT: WPARAM = 0x0110;

#[test]
fn default_text_font_is_fixedsys_in_every_window() -> Result<(), Box<dyn std::error::Error>> {
    let (app, local_app_data) = launch_close_lifecycle_app()?;
    for identity in [ViewIdentity::Status, room_identity("!beta:example")] {
        if identity == ViewIdentity::Status {
            app.select_status_view()?;
        } else {
            app.select_view(identity.clone())?;
        }
        let ui_state = app.dump_ui_state()?;
        assert_eq!(ui_state.mdi.active, identity);
        let font = &ui_state.text_font;
        assert_eq!(font.face, "Fixedsys", "{identity:?}");
        assert_eq!((font.points, font.bold), (9, false));
        assert!((12..=18).contains(&font.line_height), "{font:?}");
        assert!(font.edit_height < 28, "{font:?}");
        // The harness prefix has Wine's Tahoma and the installed Noto Emoji.
        assert_eq!(font.fallback_faces, ["Tahoma", "Noto Emoji"]);
        assert!(
            ui_state
                .active_system_menu_items
                .contains(&"&Font...".to_owned()),
            "{:?}",
            ui_state.active_system_menu_items
        );
    }
    assert_eq!(
        app.dump_ui_state()?.timestamp,
        UiTimestamp {
            show: true,
            format: "[HH:nn]".to_owned(),
        }
    );
    // Nothing changed, so nothing was written.
    assert!(!settings_file_path(&local_app_data).exists());
    drop(app);
    let _ = fs::remove_dir_all(&local_app_data);
    Ok(())
}

#[test]
fn font_command_changes_and_persists_the_text_font() -> Result<(), Box<dyn std::error::Error>> {
    let exe_path =
        mirx_ui_exe_path().map_err(|error| format!("failed to locate mirx-ui.exe: {error}"))?;
    let local_app_data = unique_local_app_data_path();
    let launch = || {
        launch_app_with_options(
            &exe_path,
            &[OsString::from("--automation")],
            launch_options(Some(local_app_data.clone())),
        )
    };

    let app = launch()?;
    let before = app.dump_ui_state()?.text_font;
    type_commands(&app, &["/font -b 12 Courier"])?;
    let ui_state = app.dump_ui_state()?;
    let font = &ui_state.text_font;
    assert_eq!(font.face, "Courier");
    assert_eq!((font.points, font.bold), (12, true));
    assert_ne!(font.line_height, before.line_height);
    // The fallback chain is rebuilt for the new font.
    assert_eq!(font.fallback_faces, before.fallback_faces);
    assert_eq!(ui_state.status_bar.text, "font: Courier 12 bold");
    assert!(
        fs::read_to_string(settings_file_path(&local_app_data))?
            .contains("\r\ntext=Courier,12,bold\r\n")
    );

    type_commands(&app, &["/font -i 12 Courier"])?;
    let ui_state = app.dump_ui_state()?;
    assert_eq!(ui_state.status_bar.text, "* /font: unknown option -i");
    assert_eq!(ui_state.text_font.face, "Courier");
    drop(app);

    let app = launch()?;
    let font = app.dump_ui_state()?.text_font;
    assert_eq!(font.face, "Courier");
    assert_eq!((font.points, font.bold), (12, true));
    drop(app);
    let _ = fs::remove_dir_all(&local_app_data);
    Ok(())
}

#[test]
fn nick_list_is_owner_drawn_in_the_text_font() -> Result<(), Box<dyn std::error::Error>> {
    let (app, local_app_data) = launch_close_lifecycle_app()?;
    app.select_view(room_identity("!beta:example"))?;
    let item_height = || unsafe {
        SendMessageW(active_room_member_list_hwnd(&app), LB_GETITEMHEIGHT, 0, 0) as i32
    };
    let members = unsafe { active_room_member_list_hwnd(&app) };
    assert_ne!(
        unsafe { window_style(members) } & LBS_OWNERDRAWFIXED as u32,
        0
    );
    // Items are a text font line tall, as the stock listbox makes them.
    assert_eq!(item_height(), app.dump_ui_state()?.text_font.line_height);

    type_commands(&app, &["/font 12 Courier"])?;
    let font = app.dump_ui_state()?.text_font;
    assert_eq!(font.face, "Courier");
    assert_eq!(item_height(), font.line_height);
    drop(app);
    let _ = fs::remove_dir_all(&local_app_data);
    Ok(())
}

#[test]
fn font_dialog_from_the_window_menu_cancels_and_applies() -> Result<(), Box<dyn std::error::Error>>
{
    let (app, local_app_data) = launch_close_lifecycle_app()?;
    let beta = room_identity("!beta:example");
    app.select_view(beta.clone())?;
    let child = view_child_hwnd(&app, &beta)?;
    let open_dialog = || {
        // Posted: the dialog runs a modal loop inside the handler.
        assert_ne!(
            unsafe { PostMessageW(child, WM_SYSCOMMAND, SYSMENU_ID_FONT, 0) },
            0,
            "posting Font... must succeed"
        );
        wait_for_message_box(&app).0
    };

    let dialog = open_dialog();
    answer_message_box(&app, dialog, IDCANCEL)?;
    assert!(!settings_file_path(&local_app_data).exists());
    assert_eq!(app.dump_ui_state()?.text_font.face, "Fixedsys");

    // The dialog starts at the current font, so OK keeps it, and saves it.
    let dialog = open_dialog();
    answer_message_box(&app, dialog, IDOK)?;
    let ui_state = app.dump_ui_state()?;
    assert_eq!(ui_state.status_bar.text, "font: Fixedsys 9");
    assert_eq!(ui_state.text_font.face, "Fixedsys");
    assert!(
        fs::read_to_string(settings_file_path(&local_app_data))?
            .contains("\r\ntext=Fixedsys,9\r\n")
    );
    drop(app);
    let _ = fs::remove_dir_all(&local_app_data);
    Ok(())
}

#[test]
fn timestamp_command_toggles_formats_and_persists() -> Result<(), Box<dyn std::error::Error>> {
    let exe_path =
        mirx_ui_exe_path().map_err(|error| format!("failed to locate mirx-ui.exe: {error}"))?;
    let local_app_data = unique_local_app_data_path();
    let launch = || -> Result<GuiTestApp, Box<dyn std::error::Error>> {
        let app = launch_app_with_options(
            &exe_path,
            &[OsString::from("--automation")],
            launch_options(Some(local_app_data.clone())),
        )?;
        app.wait_for_lifecycle_state("core_started", Duration::from_secs(30))?;
        let mut events = close_lifecycle_events();
        // 2023-11-14 22:13:20 UTC; the harness runs in UTC.
        events.push(text_event(
            "!beta:example",
            "@bob:example",
            1_700_000_000_000,
            "timestamped hello",
        ));
        app.load_events(&serde_json::to_string(&events)?)?;
        app.wait_idle(Duration::from_secs(5))?;
        app.select_view(room_identity("!beta:example"))?;
        Ok(app)
    };
    let beta = room_identity("!beta:example");
    let hello_line = |app: &GuiTestApp| -> Result<String, Box<dyn std::error::Error>> {
        let ui_state = app.dump_ui_state()?;
        Ok(only_view_line_containing(find_view(&ui_state, &beta), "timestamped hello").to_owned())
    };

    let app = launch()?;
    assert!(hello_line(&app)?.starts_with("[22:13] <"));
    type_commands(&app, &["/timestamp off"])?;
    assert!(hello_line(&app)?.starts_with("<"), "{}", hello_line(&app)?);
    assert_eq!(app.dump_ui_state()?.status_bar.text, "timestamp: off");
    type_commands(&app, &["/timestamp -f [HH:nn:ss]", "/timestamp on"])?;
    assert!(hello_line(&app)?.starts_with("[22:13:20] <"));
    type_commands(&app, &["/timestamp"])?;
    let ui_state = app.dump_ui_state()?;
    assert!(
        only_view_line_containing(find_view(&ui_state, &beta), "* Timestamp is")
            .ends_with("] * Timestamp is on ([HH:nn:ss])")
    );
    // Timestamps change in every window, not just the active one.
    let status_lines = &find_view(&ui_state, &ViewIdentity::Status).lines;
    assert!(!status_lines.is_empty());
    assert!(
        status_lines
            .iter()
            .all(|line| line.starts_with('[') && line.as_bytes()[9] == b']'),
        "{status_lines:?}"
    );
    assert_eq!(
        fs::read_to_string(settings_file_path(&local_app_data))?,
        "[fonts]\r\ntext=Fixedsys,9\r\n\
         [timestamp]\r\nshow=on\r\nformat=[HH:nn:ss]\r\n\
         [connect]\r\nstartup=on\r\n\
         [switchbar]\r\nshow=on\r\nbuttons=120\r\n\
         [treebar]\r\nshow=on\r\n"
    );
    drop(app);

    let app = launch()?;
    assert!(hello_line(&app)?.starts_with("[22:13:20] <"));
    drop(app);
    let _ = fs::remove_dir_all(&local_app_data);
    Ok(())
}

fn ranked_member(
    user_id: &str,
    display_name: &str,
    rank: RoomMemberRank,
    moderatable: bool,
) -> RoomMemberSummary {
    RoomMemberSummary {
        user_id: user_id.to_owned(),
        display_name: Some(display_name.to_owned()),
        display_name_ambiguous: false,
        is_own_user: false,
        rank,
        can_kick: moderatable,
        can_ban: moderatable,
    }
}

#[test]
fn automation_driver_irc_moderation_commands_submit_core_commands()
-> Result<(), Box<dyn std::error::Error>> {
    let (app, local_app_data) = launch_close_lifecycle_app()?;
    app.append_synthetic_events(&serde_json::to_string(&vec![Event::RoomMembers(
        RoomMembers {
            room_id: "!beta:example".to_owned(),
            members: vec![
                ranked_member("@alice:example", "Alice", RoomMemberRank::Member, true),
                ranked_member("@bob:example", "Bob", RoomMemberRank::Moderator, false),
                ranked_member("@vic:example", "Vic", RoomMemberRank::Voiced, false),
            ],
        },
    )])?)?;
    app.wait_idle(Duration::from_secs(5))?;
    app.select_view(room_identity("!beta:example"))?;

    for input in [
        "/kick alice spamming links",
        "/ban Alice",
        "/unban @gone:example",
        "/invite @carol:example",
        "/op alice",
        "/voice alice",
        "/deop Bob",
        "/devoice Vic",
        "/nick New Name",
    ] {
        app.set_compose_text(input)?;
        app.send_compose_key(ComposeKey::Return)?;
        app.wait_idle(Duration::from_secs(5))?;
        assert_eq!(app.compose_text()?, "", "{input}");
    }

    let model = app.dump_model()?;
    let moderation =
        |action, user_id: &str, reason: Option<&str>| SubmittedMemberModerationCommand {
            action,
            room_id: "!beta:example".to_owned(),
            user_id: user_id.to_owned(),
            reason: reason.map(str::to_owned),
        };
    assert_eq!(
        model.submitted_member_moderation_commands,
        vec![
            moderation(
                MemberModerationAction::Kick,
                "@alice:example",
                Some("spamming links")
            ),
            moderation(MemberModerationAction::Ban, "@alice:example", None),
            moderation(MemberModerationAction::Unban, "@gone:example", None),
            moderation(MemberModerationAction::Invite, "@carol:example", None),
        ]
    );
    let power = |user_id: &str, role| SubmittedPowerLevelCommand {
        room_id: "!beta:example".to_owned(),
        user_id: user_id.to_owned(),
        role,
    };
    assert_eq!(
        model.submitted_power_level_commands,
        vec![
            power("@alice:example", MemberPowerRole::Moderator),
            power("@alice:example", MemberPowerRole::Voiced),
            power("@bob:example", MemberPowerRole::Default),
            power("@vic:example", MemberPowerRole::Default),
        ]
    );
    assert_eq!(
        model.submitted_profile_commands,
        vec![SubmittedProfileCommand {
            display_name: "New Name".to_owned(),
        }]
    );

    // The unauthenticated core answers each one, which proves they reached
    // it: the room's errors in its window, the account's in Status.
    let ui_state = app.dump_ui_state()?;
    let beta = find_view(&ui_state, &room_identity("!beta:example"));
    let status = find_view(&ui_state, &ViewIdentity::Status);
    for (view, operation) in [
        (beta, "kick room member"),
        (beta, "ban room member"),
        (beta, "unban room member"),
        (beta, "invite room member"),
        (beta, "set room member power level"),
        (status, "set display name"),
    ] {
        assert!(
            view_contains(
                view,
                &format!("{operation} requires an authenticated session")
            ),
            "{operation}: {:?}",
            view.lines
        );
    }
    assert!(!view_contains(status, "room member"), "{:?}", status.lines);

    drop(app);
    let _ = fs::remove_dir_all(&local_app_data);
    Ok(())
}

#[test]
fn closed_query_reopens_from_the_treebar() -> Result<(), Box<dyn std::error::Error>> {
    let (app, local_app_data) = launch_close_lifecycle_app()?;
    let alpha = room_identity("!alpha:example");
    app.select_view(alpha.clone())?;
    let child = close_view_without_prompt(&app, &alpha)?;
    assert_eq!(unsafe { IsWindow(child) }, 0);

    app.select_navigation_view(&alpha)?;
    app.wait_idle(Duration::from_secs(5))?;
    let ui_state = app.dump_ui_state()?;
    assert_eq!(ui_state.mdi.active, alpha);
    assert_active_switchbar_item(&ui_state, &alpha);
    assert_eq!(app.selected_navigation_view()?, Some(alpha));

    drop(app);
    let _ = fs::remove_dir_all(&local_app_data);
    Ok(())
}

#[test]
fn status_window_cannot_be_closed() -> Result<(), Box<dyn std::error::Error>> {
    let app = launch_multi_room_fixture()?;
    app.select_view(ViewIdentity::Status)?;
    let child = close_view_without_prompt(&app, &ViewIdentity::Status)?;
    let ui_state = app.dump_ui_state()?;
    assert_eq!(ui_state.mdi.active, ViewIdentity::Status);
    assert_eq!(unsafe { IsWindow(child) }, 1);
    Ok(())
}

#[test]
fn room_caption_shows_topic_and_frame_title_follows() -> Result<(), Box<dyn std::error::Error>> {
    let exe_path =
        mirx_ui_exe_path().map_err(|error| format!("failed to locate mirx-ui.exe: {error}"))?;
    let fixture_path = fixture_state_path("room-topic.json");
    let args = [
        OsString::from("--render-state"),
        fixture_path.as_os_str().to_os_string(),
    ];
    let app = launch_app(&exe_path, &args)?;
    app.wait_idle(Duration::from_secs(5))?;
    let topic_room = room_identity("!topic:example");
    let frame = app.automation_frame_hwnd() as HWND;

    let ui_state = app.dump_ui_state()?;
    assert_eq!(
        find_view(&ui_state, &topic_room).title,
        "Topic Room: incident review"
    );
    assert_eq!(
        view_placement(&ui_state, &topic_room).state,
        MdiWindowState::Maximized
    );
    // MDI shows the maximized window's caption in the frame title.
    assert_eq!(
        unsafe { read_window_text(frame) },
        "mirx - [Topic Room: incident review]"
    );

    app.append_synthetic_events(&serde_json::to_string(&vec![topic_event_with_event_id(
        "!topic:example",
        "$topic-2",
        "@alice:example",
        1_700_000_000_000,
        "deploy window",
    )])?)?;
    app.wait_idle(Duration::from_secs(5))?;
    assert_eq!(
        find_view(&app.dump_ui_state()?, &topic_room).title,
        "Topic Room: deploy window"
    );
    assert_eq!(
        unsafe { read_window_text(frame) },
        "mirx - [Topic Room: deploy window]"
    );

    app.arrange_windows(MdiArrangement::Cascade)?;
    assert_eq!(unsafe { read_window_text(frame) }, "mirx");
    Ok(())
}

#[test]
fn automation_driver_loads_member_list_fixture() -> Result<(), Box<dyn std::error::Error>> {
    let exe_path =
        mirx_ui_exe_path().map_err(|error| format!("failed to locate mirx-ui.exe: {error}"))?;
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
    assert!(view_contains(
        members_view,
        "[22:13] <Alice> member list context line"
    ));
    assert!(view_contains(
        members_view,
        "[22:15] <Sam (@sam-one:example)> shared names show the user ID"
    ));

    Ok(())
}

fn named_profile(user_id: &str, display_name: &str) -> mirx_core::RoomUserProfile {
    mirx_core::RoomUserProfile {
        user_id: user_id.to_owned(),
        display_name: Some(display_name.to_owned()),
        display_name_ambiguous: false,
    }
}

fn power_levels_event(
    room_id: &str,
    event_id: &str,
    timestamp_ms: u64,
    changes: Vec<mirx_core::RoomPowerLevelUserChange>,
    users_default: Option<mirx_core::RoomPowerLevelValueChange>,
    other_permissions_changed: bool,
) -> Event {
    Event::RoomActivity(Box::new(CoreRoomActivity {
        room_id: room_id.to_owned(),
        activity_id: format!("state:{event_id}"),
        timestamp_ms,
        highlight: false,
        relation: None,
        kind: CoreRoomActivityKind::StateNotice(mirx_core::RoomStateNoticeActivity {
            event_id: event_id.to_owned(),
            sender: named_profile("@admin:example", "Admin"),
            content: mirx_core::RoomStateNoticeContent::PowerLevels {
                changes,
                users_default,
                other_permissions_changed,
            },
        }),
    }))
}

#[test]
fn automation_driver_power_level_change_renders_mode_line() -> Result<(), Box<dyn std::error::Error>>
{
    let exe_path =
        mirx_ui_exe_path().map_err(|error| format!("failed to locate mirx-ui.exe: {error}"))?;
    let room_id = "!ops:example";
    let state_path = write_state_file(&[
        Event::Started,
        Event::SyncedRooms(vec![room_summary(room_id, "Ops", false)]),
        power_levels_event(
            room_id,
            "$power-1",
            1_700_000_000_000,
            vec![
                mirx_core::RoomPowerLevelUserChange {
                    target: named_profile("@bob:example", "Bob"),
                    old: 0,
                    new: 50,
                },
                mirx_core::RoomPowerLevelUserChange {
                    target: user_profile("@carol:example"),
                    old: 50,
                    new: 0,
                },
            ],
            Some(mirx_core::RoomPowerLevelValueChange { old: 0, new: 10 }),
            false,
        ),
        power_levels_event(
            room_id,
            "$power-2",
            1_700_000_060_000,
            Vec::new(),
            None,
            true,
        ),
    ])?;
    let app = launch_app(
        &exe_path,
        &[
            OsString::from("--render-state"),
            state_path.as_os_str().to_os_string(),
        ],
    )?;
    app.wait_idle(Duration::from_secs(5))?;

    app.select_room(room_id)?;
    let ui_state = app.dump_ui_state()?;
    let view = find_view(&ui_state, &room_identity(room_id));
    assert!(
        view_contains(
            view,
            "[22:13] * Admin sets power level: Bob 0 -> 50, @carol:example 50 -> 0; default 0 -> 10"
        ),
        "{:?}",
        view.lines
    );
    assert!(
        view_contains(view, "[22:14] * Admin changes room permissions"),
        "{:?}",
        view.lines
    );
    let activity = app.room_activity(Some(room_id))?;
    assert_eq!(
        activity[0].kind,
        RoomActivityKind::StateNotice(RoomStateNoticeActivity {
            event_id: "$power-1".to_owned(),
            sender: "@admin:example".to_owned(),
            state_key: String::new(),
            content: RoomStateNoticeContent::PowerLevels {
                changes: vec![
                    mirx_testkit::RoomPowerLevelUserChange {
                        user_id: "@bob:example".to_owned(),
                        old: 0,
                        new: 50,
                    },
                    mirx_testkit::RoomPowerLevelUserChange {
                        user_id: "@carol:example".to_owned(),
                        old: 50,
                        new: 0,
                    },
                ],
                users_default: Some(mirx_testkit::RoomPowerLevelValueChange { old: 0, new: 10 }),
                other_permissions_changed: false,
            },
        })
    );

    let _ = fs::remove_file(&state_path);
    Ok(())
}

#[test]
fn automation_driver_member_list_shows_rank_prefixes_in_rank_order()
-> Result<(), Box<dyn std::error::Error>> {
    let exe_path =
        mirx_ui_exe_path().map_err(|error| format!("failed to locate mirx-ui.exe: {error}"))?;
    let fixture_path = fixture_state_path("member-ranks.json");
    let app = launch_app(
        &exe_path,
        &[
            OsString::from("--render-state"),
            fixture_path.as_os_str().to_os_string(),
        ],
    )?;
    app.wait_idle(Duration::from_secs(5))?;

    app.select_room("!ranks:example")?;
    let expected = [
        "~Creator",
        "&Admin",
        "@Mod",
        "+Voice",
        "@ghost:example",
        "Alan",
        "Alice",
        "Casey",
        "Sam (@sam-one:example)",
        "Sam (@sam-two:example)",
    ];
    assert_eq!(
        unsafe { listbox_texts(active_room_member_list_hwnd(&app)) },
        expected
    );
    let ui_state = app.dump_ui_state()?;
    let view = find_view(&ui_state, &room_identity("!ranks:example"));
    assert_eq!(
        view.members
            .iter()
            .map(|member| (member.label.as_str(), member.rank))
            .collect::<Vec<_>>(),
        vec![
            ("Creator", RoomMemberRank::Creator),
            ("Admin", RoomMemberRank::Administrator),
            ("Mod", RoomMemberRank::Moderator),
            ("Voice", RoomMemberRank::Voiced),
            ("@ghost:example", RoomMemberRank::Member),
            ("Alan", RoomMemberRank::Member),
            ("Alice", RoomMemberRank::Member),
            ("Casey", RoomMemberRank::Member),
            ("Sam (@sam-one:example)", RoomMemberRank::Member),
            ("Sam (@sam-two:example)", RoomMemberRank::Member),
        ]
    );
    assert!(view_contains(
        view,
        "[22:14] * Admin sets power level: Mod 0 -> 50, Voice 0 -> 10; default 10 -> 0"
    ));
    assert!(view_contains(
        view,
        "[22:15] * Admin changes room permissions"
    ));

    Ok(())
}

#[test]
fn automation_driver_member_list_ranks_members_from_synced_power_levels()
-> Result<(), Box<dyn std::error::Error>> {
    let exe_path =
        mirx_ui_exe_path().map_err(|error| format!("failed to locate mirx-ui.exe: {error}"))?;
    let local_app_data = unique_local_app_data_path();
    let server = spawn_mock_backfill_homeserver()?;
    let app = launch_app_with_options(
        &exe_path,
        &[OsString::from("--automation")],
        launch_options(Some(local_app_data.clone())),
    )?;
    app.wait_for_lifecycle_state("core_started", Duration::from_secs(30))?;
    app.restore_session(&server.base_url, stored_session())?;
    app.wait_for_lifecycle_state("ready", Duration::from_secs(30))?;
    app.wait_idle(Duration::from_secs(5))?;
    app.select_room("!beta:example")?;

    assert_eq!(
        unsafe { listbox_texts(active_room_member_list_hwnd(&app)) },
        ["&Alice", "@Bob"]
    );

    drop(app);
    let _ = fs::remove_dir_all(&local_app_data);
    Ok(())
}

#[test]
fn automation_driver_initial_sync_state_is_not_logged_as_joins()
-> Result<(), Box<dyn std::error::Error>> {
    let exe_path =
        mirx_ui_exe_path().map_err(|error| format!("failed to locate mirx-ui.exe: {error}"))?;
    let local_app_data = unique_local_app_data_path();
    // Alice and Bob are in the sync `state` section: existing state, no
    // log line. Carol joins in the timeline: something that happened.
    let server = spawn_mock_homeserver(
        MockMessages::Hang,
        vec![
            serde_json::json!({
                "content": { "membership": "join", "displayname": "Carol" },
                "event_id": "$carol-member",
                "origin_server_ts": 1700000000010u64,
                "sender": "@carol:example",
                "state_key": "@carol:example",
                "type": "m.room.member"
            }),
            serde_json::json!({
                "content": { "msgtype": "m.text", "body": "welcome Carol" },
                "event_id": "$bob-hello",
                "origin_server_ts": 1700000000011u64,
                "sender": "@bob:example",
                "type": "m.room.message"
            }),
        ],
    )?;
    let app = launch_app_with_options(
        &exe_path,
        &[OsString::from("--automation")],
        launch_options(Some(local_app_data.clone())),
    )?;
    app.wait_for_lifecycle_state("core_started", Duration::from_secs(30))?;
    app.restore_session(&server.base_url, stored_session())?;
    app.wait_for_lifecycle_state("ready", Duration::from_secs(30))?;
    app.wait_idle(Duration::from_secs(5))?;
    app.select_room("!beta:example")?;
    app.wait_for_room_activity_contains(
        Some("!beta:example"),
        "welcome Carol",
        Duration::from_secs(5),
    )?;

    let ui_state = app.dump_ui_state()?;
    let view = find_view(&ui_state, &room_identity("!beta:example"));
    let lines_containing = |needle: &str| {
        view.lines
            .iter()
            .filter(|line| line.contains(needle))
            .count()
    };
    // The mock serves this timeline to every continuous sync too.
    assert_eq!(
        lines_containing("* Joins: Carol (@carol:example)"),
        1,
        "{:?}",
        view.lines
    );
    assert_eq!(
        lines_containing("<Bob> welcome Carol"),
        1,
        "{:?}",
        view.lines
    );
    assert_eq!(lines_containing("Joins: Alice"), 0, "{:?}", view.lines);
    assert_eq!(lines_containing("Joins: Bob"), 0, "{:?}", view.lines);
    assert_eq!(
        unsafe { listbox_texts(active_room_member_list_hwnd(&app)) },
        ["&Alice", "@Bob", "Carol"]
    );

    drop(app);
    let _ = fs::remove_dir_all(&local_app_data);
    Ok(())
}

#[test]
fn automation_driver_fetches_unloaded_reply_target() -> Result<(), Box<dyn std::error::Error>> {
    let exe_path =
        mirx_ui_exe_path().map_err(|error| format!("failed to locate mirx-ui.exe: {error}"))?;
    let local_app_data = unique_local_app_data_path();
    let server = spawn_mock_homeserver(
        MockMessages::Hang,
        vec![serde_json::json!({
            "content": {
                "msgtype": "m.text",
                "body": "still true",
                "m.relates_to": { "m.in_reply_to": { "event_id": "$old" } }
            },
            "event_id": "$bob-reply",
            "origin_server_ts": 1700000000011u64,
            "sender": "@bob:example",
            "type": "m.room.message"
        })],
    )?;
    let app = launch_app_with_options(
        &exe_path,
        &[OsString::from("--automation")],
        launch_options(Some(local_app_data.clone())),
    )?;
    app.wait_for_lifecycle_state("core_started", Duration::from_secs(30))?;
    app.restore_session(&server.base_url, stored_session())?;
    app.wait_for_lifecycle_state("ready", Duration::from_secs(30))?;
    app.select_room("!beta:example")?;

    let quote = "    > <Alice> from the archive";
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    let lines = loop {
        let ui_state = app.dump_ui_state()?;
        let lines = find_view(&ui_state, &room_identity("!beta:example"))
            .lines
            .clone();
        if lines.iter().any(|line| line == quote) || std::time::Instant::now() > deadline {
            break lines;
        }
        std::thread::sleep(Duration::from_millis(100));
    };
    let reply = lines
        .iter()
        .position(|line| line.ends_with("<Bob> still true"))
        .ok_or_else(|| format!("no reply line in {lines:?}"))?;
    assert_eq!(lines[reply - 1], quote, "{lines:?}");
    assert_eq!(app.dump_model()?.last_error, None);

    drop(app);
    let _ = fs::remove_dir_all(&local_app_data);
    Ok(())
}

#[test]
fn automation_driver_renders_every_membership_change_in_mirc_format()
-> Result<(), Box<dyn std::error::Error>> {
    let exe_path =
        mirx_ui_exe_path().map_err(|error| format!("failed to locate mirx-ui.exe: {error}"))?;
    let fixture_path = fixture_state_path("membership-changes.json");
    let app = launch_app(
        &exe_path,
        &[
            OsString::from("--render-state"),
            fixture_path.as_os_str().to_os_string(),
        ],
    )?;
    app.wait_idle(Duration::from_secs(5))?;

    app.select_room("!membership:example")?;
    let ui_state = app.dump_ui_state()?;
    let view = find_view(&ui_state, &room_identity("!membership:example"));
    let expected = [
        "[22:13] * Joins: Carol (@carol:example)",
        "[22:14] * Parts: @dave:example",
        "[22:15] * Eve was kicked by Op (flooding)",
        "[22:16] * Frank was banned by Op (spam)",
        "[22:17] * Grace was banned by Op",
        "[22:18] * Op removed ban on Frank",
        "[22:19] * Op invites Heidi",
        "[22:20] * Joins: Heidi (@heidi:example)",
        "[22:21] * Ivan declined the invitation",
        "[22:22] * Op revoked the invitation for Judy",
        "[22:23] * Mallory knocks (let me in)",
        "[22:24] * Op accepted knock from Mallory",
        "[22:25] * Niaj withdrew knock",
        "[22:26] * Op denied knock from Olivia",
        "[22:27] * Carol is now known as Caz",
        "[22:28] * Sam (@sam:example) changed avatar",
    ];
    assert_eq!(
        view.lines
            .iter()
            .filter(|line| line.starts_with("[22:"))
            .map(String::as_str)
            .collect::<Vec<_>>(),
        expected
    );

    Ok(())
}

fn launch_member_list_app() -> Result<GuiTestApp, Box<dyn std::error::Error>> {
    let exe_path =
        mirx_ui_exe_path().map_err(|error| format!("failed to locate mirx-ui.exe: {error}"))?;
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
    assert!(state.whois_enabled && state.query_enabled && state.mention_enabled);
    assert!(state.kick_enabled && state.ban_enabled);
    // A plain member can be given op or voice.
    assert!(state.op_enabled && state.voice_enabled);
    assert!(!state.deop_enabled && !state.devoice_enabled);
    // The fixture has no ignore list.
    assert!(!state.ignore_enabled && !state.unignore_enabled);
    assert_eq!(state.selected_user_id.as_deref(), Some("@casey:example"));
    assert_eq!(state.selected_label.as_deref(), Some("Casey"));
    app.select_member("@sam-two:example")?;
    let state = app.dump_member_menu_state()?;
    assert_eq!(state.selected_user_id.as_deref(), Some("@sam-two:example"));
    assert!(!state.kick_enabled && !state.ban_enabled);
    assert_eq!(
        state.selected_label.as_deref(),
        Some("Sam (@sam-two:example)")
    );
    Ok(())
}

#[test]
fn member_menu_control_items() -> Result<(), Box<dyn std::error::Error>> {
    let exe_path =
        mirx_ui_exe_path().map_err(|error| format!("failed to locate mirx-ui.exe: {error}"))?;
    let local_app_data = unique_local_app_data_path();
    let options = launch_options(Some(local_app_data.clone()));
    let app = launch_app_with_options(&exe_path, &[OsString::from("--automation")], options)?;
    app.wait_for_lifecycle_state("core_started", Duration::from_secs(30))?;
    let member = |user_id: &str, display_name: &str, rank, is_own_user| RoomMemberSummary {
        user_id: user_id.to_owned(),
        display_name: Some(display_name.to_owned()),
        display_name_ambiguous: false,
        is_own_user,
        rank,
        can_kick: false,
        can_ban: false,
    };
    let mut events = base_events();
    events.insert(
        1,
        Event::LoggedIn {
            session: core_stored_session(),
            homeserver_url: "https://matrix.example/".to_owned(),
        },
    );
    events.extend([
        Event::RoomMembers(RoomMembers {
            room_id: "!beta:example".to_owned(),
            members: vec![
                member("@alice:example", "Alice", RoomMemberRank::Moderator, true),
                member("@vic:example", "Vic", RoomMemberRank::Voiced, false),
                member("@bob:example", "Bob", RoomMemberRank::Member, false),
            ],
        }),
        Event::IgnoredUsers(Vec::new()),
    ]);
    app.load_events(&serde_json::to_string(&events)?)?;
    app.wait_idle(Duration::from_secs(5))?;
    app.select_view(room_identity("!beta:example"))?;

    app.select_member("@bob:example")?;
    let state = app.dump_member_menu_state()?;
    assert!(state.whois_enabled && state.ignore_enabled && !state.unignore_enabled);
    assert!(state.op_enabled && state.voice_enabled);
    assert!(!state.deop_enabled && !state.devoice_enabled);
    assert!(!state.kick_enabled && !state.ban_enabled);

    app.select_member("@vic:example")?;
    let state = app.dump_member_menu_state()?;
    assert!(state.devoice_enabled && !state.voice_enabled);

    // You cannot ignore or deop yourself.
    app.select_member("@alice:example")?;
    let state = app.dump_member_menu_state()?;
    assert!(!state.ignore_enabled && !state.deop_enabled);
    let error = app
        .open_selected_member_context_menu(ContextMenuCommand::IgnoreMember)
        .expect_err("ignoring yourself is greyed");
    assert!(matches!(
        error,
        TestkitError::Protocol { message, .. }
            if message == "member context menu action is disabled"
    ));

    // Each item runs the typed command on the member.
    app.open_member_context_menu("@bob:example", ContextMenuCommand::OpMember)?;
    app.open_member_context_menu("@bob:example", ContextMenuCommand::IgnoreMember)?;
    app.open_member_context_menu("@bob:example", ContextMenuCommand::WhoisMember)?;
    let model = app.dump_model()?;
    assert_eq!(
        model.submitted_power_level_commands,
        vec![SubmittedPowerLevelCommand {
            room_id: "!beta:example".to_owned(),
            user_id: "@bob:example".to_owned(),
            role: MemberPowerRole::Moderator,
        }]
    );
    assert_eq!(
        model.submitted_user_commands,
        vec![
            SubmittedUserCommand {
                action: UserCommandAction::Ignore,
                user_id: "@bob:example".to_owned(),
            },
            SubmittedUserCommand {
                action: UserCommandAction::FetchProfile,
                user_id: "@bob:example".to_owned(),
            },
        ]
    );

    drop(app);
    let _ = fs::remove_dir_all(&local_app_data);
    Ok(())
}

#[test]
fn automation_driver_member_menu_moderation_uses_clicked_member()
-> Result<(), Box<dyn std::error::Error>> {
    let app = launch_member_list_app()?;
    app.select_member("@sam-one:example")?;
    app.open_member_context_menu("@casey:example", ContextMenuCommand::KickMember)?;
    assert_eq!(
        app.dump_model()?.submitted_member_moderation_commands,
        vec![SubmittedMemberModerationCommand {
            action: MemberModerationAction::Kick,
            room_id: "!members:example".to_owned(),
            user_id: "@casey:example".to_owned(),
            reason: None,
        }]
    );
    app.open_member_context_menu("@casey:example", ContextMenuCommand::BanMember)?;
    assert_eq!(
        app.dump_model()?.submitted_member_moderation_commands,
        vec![
            SubmittedMemberModerationCommand {
                action: MemberModerationAction::Kick,
                room_id: "!members:example".to_owned(),
                user_id: "@casey:example".to_owned(),
                reason: None,
            },
            SubmittedMemberModerationCommand {
                action: MemberModerationAction::Ban,
                room_id: "!members:example".to_owned(),
                user_id: "@casey:example".to_owned(),
                reason: None,
            },
        ]
    );
    assert_eq!(
        app.dump_member_menu_state()?.selected_user_id.as_deref(),
        Some("@casey:example")
    );
    Ok(())
}

#[test]
fn automation_driver_member_menu_moderation_respects_independent_permissions()
-> Result<(), Box<dyn std::error::Error>> {
    let app = launch_member_list_app()?;
    app.select_member("@sam-one:example")?;
    let state = app.dump_member_menu_state()?;
    assert!(state.kick_enabled);
    assert!(!state.ban_enabled);
    app.open_selected_member_context_menu(ContextMenuCommand::KickMember)?;
    let error = app
        .open_selected_member_context_menu(ContextMenuCommand::BanMember)
        .expect_err("disabled ban must fail");
    assert!(matches!(
        error,
        TestkitError::Protocol { message, .. }
            if message == "member context menu action is disabled"
    ));
    assert_eq!(
        app.dump_model()?.submitted_member_moderation_commands,
        vec![SubmittedMemberModerationCommand {
            action: MemberModerationAction::Kick,
            room_id: "!members:example".to_owned(),
            user_id: "@sam-one:example".to_owned(),
            reason: None,
        }]
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

    // The name goes in at the caret; the rest of the line is kept.
    app.set_compose_text("hi  there")?;
    unsafe { SendMessageW(compose_edit_hwnd(&app), EM_SETSEL, 3, 3) };
    app.select_member("@casey:example")?;
    app.mention_selected_member_via_menu()?;
    assert_eq!(app.compose_text()?, "hi Casey  there");
    Ok(())
}

fn launch_member_ranks_app() -> Result<GuiTestApp, Box<dyn std::error::Error>> {
    let exe_path =
        mirx_ui_exe_path().map_err(|error| format!("failed to locate mirx-ui.exe: {error}"))?;
    let fixture_path = fixture_state_path("member-ranks.json");
    let app = launch_app(
        &exe_path,
        &[
            OsString::from("--render-state"),
            fixture_path.as_os_str().to_os_string(),
        ],
    )?;
    app.wait_idle(Duration::from_secs(5))?;
    app.select_room("!ranks:example")?;
    Ok(app)
}

#[test]
fn automation_driver_tab_completes_member_nick() -> Result<(), Box<dyn std::error::Error>> {
    let app = launch_member_ranks_app()?;

    app.set_compose_text("hi al")?;
    app.send_compose_key(ComposeKey::Tab)?;
    assert_eq!(app.compose_text()?, "hi Alan");
    app.send_compose_key(ComposeKey::Tab)?;
    assert_eq!(app.compose_text()?, "hi Alice");
    app.send_compose_key(ComposeKey::Tab)?;
    assert_eq!(app.compose_text()?, "hi Alan", "cycling wraps");
    app.send_compose_key(ComposeKey::ShiftTab)?;
    assert_eq!(app.compose_text()?, "hi Alice");
    app.send_compose_key(ComposeKey::ShiftTab)?;
    assert_eq!(app.compose_text()?, "hi Alan");

    // Typing ends the completion: the next Tab completes the new word
    // instead of cycling on to Alice.
    let edit = unsafe { compose_edit_hwnd(&app) };
    for character in [' ', 'c', 'a'] {
        unsafe { SendMessageW(edit, WM_CHAR, character as WPARAM, 0) };
    }
    app.send_compose_key(ComposeKey::Tab)?;
    assert_eq!(app.compose_text()?, "hi Alan Casey");

    // The character TranslateMessage makes of the Tab key changes nothing.
    unsafe { SendMessageW(edit, WM_CHAR, '\t' as WPARAM, 0) };
    assert_eq!(app.compose_text()?, "hi Alan Casey");

    // Our own nick (Admin) is never offered; unmatched words stay.
    app.set_compose_text("adm")?;
    app.send_compose_key(ComposeKey::Tab)?;
    assert_eq!(app.compose_text()?, "adm");
    Ok(())
}

#[test]
fn automation_driver_tab_completion_at_sign_inserts_user_id()
-> Result<(), Box<dyn std::error::Error>> {
    let app = launch_member_ranks_app()?;

    app.set_compose_text("/query @ca")?;
    app.send_compose_key(ComposeKey::Tab)?;
    assert_eq!(app.compose_text()?, "/query @casey:example");

    // Members sharing a name complete once by name, apart by user ID.
    app.set_compose_text("sa")?;
    app.send_compose_key(ComposeKey::Tab)?;
    assert_eq!(app.compose_text()?, "Sam");
    app.send_compose_key(ComposeKey::Tab)?;
    assert_eq!(app.compose_text()?, "Sam");
    app.set_compose_text("@sa")?;
    app.send_compose_key(ComposeKey::Tab)?;
    assert_eq!(app.compose_text()?, "@sam-one:example");
    app.send_compose_key(ComposeKey::Tab)?;
    assert_eq!(app.compose_text()?, "@sam-two:example");
    Ok(())
}

#[test]
fn automation_driver_tab_completion_in_thread_uses_parent_room_members()
-> Result<(), Box<dyn std::error::Error>> {
    let app = launch_member_list_app()?;
    app.append_synthetic_events(&serde_json::to_string(&vec![
        message_event_with_event_id(
            "!members:example",
            "$tab-thread-root",
            "@alice:example",
            1_700_000_090_000,
            CoreRoomMessagePresentationKind::Text,
            "tab thread root",
        ),
        message_event_with_event_id_and_relation(
            "!members:example",
            "$tab-thread-reply",
            "@casey:example",
            1_700_000_120_000,
            Some(CoreRoomActivityRelation::Thread {
                thread_root_event_id: "$tab-thread-root".to_owned(),
                in_reply_to_event_id: Some("$tab-thread-root".to_owned()),
                is_falling_back: false,
            }),
            CoreRoomMessagePresentationKind::Text,
            "tab thread reply",
        ),
    ])?)?;
    app.select_view(thread_identity("!members:example", "$tab-thread-root"))?;
    app.set_compose_text("thanks ca")?;
    app.send_compose_key(ComposeKey::Tab)?;
    assert_eq!(app.compose_text()?, "thanks Casey");

    // The status view has no members, so Tab leaves the text alone.
    app.select_view(ViewIdentity::Status)?;
    app.set_compose_text("ca")?;
    app.send_compose_key(ComposeKey::Tab)?;
    assert_eq!(app.compose_text()?, "ca");
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
fn automation_driver_member_moderation_uses_current_keyboard_selection_after_click()
-> Result<(), Box<dyn std::error::Error>> {
    let app = launch_member_list_app()?;
    app.open_member_context_menu("@alice:example", ContextMenuCommand::MentionMember)?;
    app.select_member("@casey:example")?;
    assert_eq!(
        app.dump_member_menu_state()?.selected_user_id.as_deref(),
        Some("@casey:example")
    );
    app.open_selected_member_context_menu(ContextMenuCommand::KickMember)?;
    assert_eq!(
        app.dump_model()?.submitted_member_moderation_commands,
        vec![SubmittedMemberModerationCommand {
            action: MemberModerationAction::Kick,
            room_id: "!members:example".to_owned(),
            user_id: "@casey:example".to_owned(),
            reason: None,
        }]
    );
    Ok(())
}

#[test]
fn automation_driver_member_moderation_resolves_room_and_thread_parent_room()
-> Result<(), Box<dyn std::error::Error>> {
    let app = launch_member_list_app()?;
    app.select_member("@casey:example")?;
    app.open_selected_member_context_menu(ContextMenuCommand::KickMember)?;
    app.open_selected_member_context_menu(ContextMenuCommand::BanMember)?;
    app.append_synthetic_events(&serde_json::to_string(&vec![
        message_event_with_event_id(
            "!members:example",
            "$members-thread-root",
            "@alice:example",
            1_700_000_090_000,
            CoreRoomMessagePresentationKind::Text,
            "member list thread root",
        ),
        message_event_with_event_id_and_relation(
            "!members:example",
            "$members-thread-reply",
            "@casey:example",
            1_700_000_120_000,
            Some(CoreRoomActivityRelation::Thread {
                thread_root_event_id: "$members-thread-root".to_owned(),
                in_reply_to_event_id: Some("$members-thread-root".to_owned()),
                is_falling_back: false,
            }),
            CoreRoomMessagePresentationKind::Text,
            "member list thread reply",
        ),
    ])?)?;
    app.select_view(thread_identity("!members:example", "$members-thread-root"))?;
    app.select_member("@casey:example")?;
    app.open_selected_member_context_menu(ContextMenuCommand::KickMember)?;
    app.open_selected_member_context_menu(ContextMenuCommand::BanMember)?;
    assert_eq!(
        app.dump_model()?.submitted_member_moderation_commands,
        vec![
            SubmittedMemberModerationCommand {
                action: MemberModerationAction::Kick,
                room_id: "!members:example".to_owned(),
                user_id: "@casey:example".to_owned(),
                reason: None,
            },
            SubmittedMemberModerationCommand {
                action: MemberModerationAction::Ban,
                room_id: "!members:example".to_owned(),
                user_id: "@casey:example".to_owned(),
                reason: None,
            },
            SubmittedMemberModerationCommand {
                action: MemberModerationAction::Kick,
                room_id: "!members:example".to_owned(),
                user_id: "@casey:example".to_owned(),
                reason: None,
            },
            SubmittedMemberModerationCommand {
                action: MemberModerationAction::Ban,
                room_id: "!members:example".to_owned(),
                user_id: "@casey:example".to_owned(),
                reason: None,
            },
        ]
    );
    Ok(())
}

#[test]
fn automation_driver_member_moderation_status_view_has_no_target()
-> Result<(), Box<dyn std::error::Error>> {
    let app = launch_member_list_app()?;
    app.select_view(ViewIdentity::Status)?;
    assert_eq!(app.dump_member_menu_state()?.selected_user_id, None);
    let error = app
        .open_selected_member_context_menu(ContextMenuCommand::KickMember)
        .expect_err("status view must not submit member moderation");
    assert!(matches!(
        error,
        TestkitError::Protocol { message, .. } if message == "member list is unavailable"
    ));
    assert!(
        app.dump_model()?
            .submitted_member_moderation_commands
            .is_empty()
    );
    Ok(())
}

/// Every switchbar button is the fixed width and inside the switchbar, and
/// the treebar and MDI client start right below its last row.
fn assert_switchbar_rows_fit(ui_state: &UiState, button_width: i32) {
    let bar = ui_state
        .frame_layout
        .switchbar
        .expect("the switchbar is shown");
    for item in &ui_state.switchbar.items {
        assert_eq!(item.rect.width, button_width, "{item:?}");
        assert!(item.rect.right() <= bar.width, "{item:?} is clipped");
        assert!(item.rect.y < bar.height, "{item:?} is below the switchbar");
    }
    let rows = ui_state
        .switchbar
        .items
        .iter()
        .map(|item| item.rect.y)
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(rows.len(), ui_state.switchbar.rows as usize);
    assert_eq!(ui_state.frame_layout.mdi_client.y, bar.bottom());
    assert_eq!(
        ui_state
            .frame_layout
            .treebar
            .expect("the treebar is shown")
            .y,
        bar.bottom()
    );
}

#[test]
fn switchbar_wraps_fixed_width_buttons_and_relayouts() -> Result<(), Box<dyn std::error::Error>> {
    let exe_path =
        mirx_ui_exe_path().map_err(|error| format!("failed to locate mirx-ui.exe: {error}"))?;
    let fixture_path = fixture_state_path("shell-many-open-views.json");
    let options = LaunchOptions {
        frame_size: Some(FrameSize {
            width: 640,
            height: 480,
        }),
        ..launch_options(None)
    };
    let args = [
        OsString::from("--render-state"),
        fixture_path.as_os_str().to_os_string(),
    ];
    let app = launch_app_with_options(&exe_path, &args, options)?;
    app.wait_idle(Duration::from_secs(5))?;

    let narrow = app.dump_ui_state()?;
    assert_eq!(narrow.switchbar.items.len(), 11);
    assert!(narrow.switchbar.rows >= 3, "{:?}", narrow.switchbar);
    assert_switchbar_rows_fit(&narrow, 120);

    let frame = app.automation_frame_hwnd() as HWND;
    assert_ne!(
        unsafe {
            SetWindowPos(
                frame,
                null_mut(),
                0,
                0,
                1280,
                480,
                SWP_NOMOVE | SWP_NOZORDER,
            )
        },
        0
    );
    app.wait_idle(Duration::from_secs(5))?;
    let wide = app.dump_ui_state()?;
    assert!(
        wide.switchbar.rows < narrow.switchbar.rows,
        "{:?}",
        wide.switchbar
    );
    assert_switchbar_rows_fit(&wide, 120);
    assert!(wide.frame_layout.mdi_client.y < narrow.frame_layout.mdi_client.y);
    Ok(())
}

#[test]
fn automation_driver_loads_member_list_fixture_in_narrow_frame()
-> Result<(), Box<dyn std::error::Error>> {
    let exe_path =
        mirx_ui_exe_path().map_err(|error| format!("failed to locate mirx-ui.exe: {error}"))?;
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
    assert!(view_contains(
        members_view,
        "[22:13] <Alice> member list context line"
    ));
    assert!(view_contains(
        members_view,
        "[22:15] <Sam (@sam-one:example)> shared names show the user ID"
    ));

    // mIRC keeps the nick list in a narrow channel window and shrinks the
    // text area beside it.
    let timeline_rect = unsafe { client_rect(app.active_timeline_hwnd()? as HWND) };
    let member_list = unsafe { active_room_member_list_hwnd(&app) };
    let member_list_rect = unsafe { client_rect(member_list) };
    assert!(unsafe { IsWindowVisible(member_list) } != 0);
    assert!(timeline_rect.right > 0);
    assert!(member_list_rect.right > 0);

    Ok(())
}

#[test]
fn automation_driver_replaces_room_members_on_append_and_state_reload()
-> Result<(), Box<dyn std::error::Error>> {
    let exe_path =
        mirx_ui_exe_path().map_err(|error| format!("failed to locate mirx-ui.exe: {error}"))?;
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
    let exe_path =
        mirx_ui_exe_path().map_err(|error| format!("failed to locate mirx-ui.exe: {error}"))?;
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
    assert_eq!(unsafe { class_name(content_hwnd) }, "mirxWin32RoomContent");
    let timeline_hwnd = app.active_timeline_hwnd()? as HWND;
    let member_list_hwnd = unsafe { active_room_member_list_hwnd(&app) };
    assert_eq!(unsafe { class_name(timeline_hwnd) }, "mirxWin32Timeline");
    assert_eq!(unsafe { class_name(member_list_hwnd) }, "ListBox");
    let timeline_style = unsafe { window_style(timeline_hwnd) };
    let timeline_rect = unsafe { client_rect(timeline_hwnd) };
    assert!(
        timeline_style & WS_VSCROLL != 0,
        "timeline child must advertise WS_VSCROLL"
    );
    assert!(
        unsafe { read_window_text(timeline_hwnd) }
            .contains("* Now talking in Beta (!beta:example)"),
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

/// An `--automation` app showing `!beta:example` with `rows` lines of
/// history, signed in to a homeserver that never answers `/messages`, so
/// a request for older history stays loading.
fn launch_long_beta_room(
    rows: u64,
) -> Result<(GuiTestApp, MockBackfillHomeserver, PathBuf), Box<dyn std::error::Error>> {
    let exe_path =
        mirx_ui_exe_path().map_err(|error| format!("failed to locate mirx-ui.exe: {error}"))?;
    let local_app_data = unique_local_app_data_path();
    let server = spawn_mock_backfill_homeserver()?;
    let options = launch_options(Some(local_app_data.clone()));
    let app = launch_app_with_options(&exe_path, &[OsString::from("--automation")], options)?;
    app.wait_for_lifecycle_state("core_started", Duration::from_secs(30))?;
    app.restore_session(&server.base_url, stored_session())?;
    app.wait_for_lifecycle_state("ready", Duration::from_secs(30))?;
    let mut events = base_events();
    for index in 0..rows {
        events.push(text_event(
            "!beta:example",
            "@bob:example",
            1_700_001_000_000 + index * 60_000,
            &format!("history row {index:03}"),
        ));
    }
    app.load_events(&serde_json::to_string(&events)?)?;
    app.wait_idle(Duration::from_secs(5))?;
    app.select_room("!beta:example")?;
    app.wait_for_room_activity_contains(
        Some("!beta:example"),
        &format!("history row {:03}", rows - 1),
        Duration::from_secs(5),
    )?;
    Ok((app, server, local_app_data))
}

/// The lines one wheel notch scrolls, as the system is set.
fn wheel_scroll_lines() -> i32 {
    let mut lines = 0u32;
    assert_ne!(
        unsafe {
            SystemParametersInfoW(
                SPI_GETWHEELSCROLLLINES,
                0,
                (&mut lines as *mut u32).cast(),
                0,
            )
        },
        0
    );
    assert!(
        (1..=10).contains(&lines),
        "the test needs a wheel that scrolls lines, not pages: {lines}"
    );
    lines as i32
}

/// Turns the wheel by `delta` over `hwnd`, sent so that it is handled
/// before this returns.
fn send_wheel(hwnd: HWND, delta: i16) {
    let mut result = 0;
    assert_ne!(
        unsafe {
            SendMessageTimeoutW(
                hwnd,
                WM_MOUSEWHEEL,
                (delta as u16 as WPARAM) << 16,
                0,
                SMTO_NORMAL,
                5_000,
                &mut result,
            )
        },
        0,
        "the window must handle the wheel without blocking"
    );
}

/// Waits until `timeline`'s top line is `expected`: typed keys are queued
/// input, which the app reads after any sent automation request.
fn wait_for_top_line(timeline: HWND, expected: i32) -> Result<(), Box<dyn std::error::Error>> {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let top_line = unsafe { timeline_top_line(timeline) };
        if top_line == expected {
            return Ok(());
        }
        if Instant::now() >= deadline {
            return Err(format!("top line is {top_line}, expected {expected}").into());
        }
        std::thread::sleep(POLL_INTERVAL);
    }
}

#[test]
fn wheel_scrolls_the_timeline_under_the_cursor_and_from_the_editbox()
-> Result<(), Box<dyn std::error::Error>> {
    let (app, _server, local_app_data) = launch_long_beta_room(80)?;
    let timeline = app.active_timeline_hwnd()? as HWND;
    let lines = wheel_scroll_lines();
    let bottom = unsafe { timeline_top_line(timeline) };
    assert!(bottom > 3 * lines, "the room must be long: {bottom}");

    send_wheel(timeline, 120);
    assert_eq!(unsafe { timeline_top_line(timeline) }, bottom - lines);
    // Wine turns the wheel at the focused editbox: it scrolls its window.
    send_wheel(unsafe { compose_edit_hwnd(&app) }, 120);
    assert_eq!(unsafe { timeline_top_line(timeline) }, bottom - 2 * lines);
    // Half notches add up.
    send_wheel(timeline, 60);
    assert_eq!(unsafe { timeline_top_line(timeline) }, bottom - 2 * lines);
    send_wheel(timeline, 60);
    assert_eq!(unsafe { timeline_top_line(timeline) }, bottom - 3 * lines);
    send_wheel(timeline, -360);
    assert_eq!(unsafe { timeline_top_line(timeline) }, bottom);
    assert!(app.dump_model()?.submitted_history_commands.is_empty());

    drop(app);
    let _ = fs::remove_dir_all(&local_app_data);
    Ok(())
}

#[test]
fn wheel_to_the_top_asks_for_older_history() -> Result<(), Box<dyn std::error::Error>> {
    let (app, _server, local_app_data) = launch_long_beta_room(80)?;
    let timeline = app.active_timeline_hwnd()? as HWND;

    send_wheel(timeline, i16::MAX);
    assert_eq!(unsafe { timeline_top_line(timeline) }, 0);
    assert_eq!(
        app.dump_model()?.submitted_history_commands,
        vec![SubmittedHistoryCommand {
            room_id: "!beta:example".to_owned(),
            from: None,
            limit: 50,
        }]
    );
    // The request is loading: more turns at the top ask for nothing more.
    send_wheel(timeline, 120);
    send_wheel(unsafe { compose_edit_hwnd(&app) }, 120);
    assert_eq!(app.dump_model()?.submitted_history_commands.len(), 1);

    drop(app);
    let _ = fs::remove_dir_all(&local_app_data);
    Ok(())
}

/// The editbox's caret position.
unsafe fn edit_caret(edit: HWND) -> usize {
    (unsafe { SendMessageW(edit, EM_GETSEL, 0, 0) } as usize) >> 16
}

#[test]
fn page_keys_in_the_editbox_scroll_the_timeline() -> Result<(), Box<dyn std::error::Error>> {
    let (app, _server, local_app_data) = launch_long_beta_room(160)?;
    let timeline = app.active_timeline_hwnd()? as HWND;
    let frame = app.automation_frame_hwnd() as HWND;
    unsafe { SetForegroundWindow(frame) };
    assert_compose_has_focus(&app);
    let bottom = unsafe { timeline_top_line(timeline) };
    // A page as the scroll bar pages it.
    unsafe { SendMessageW(timeline, WM_VSCROLL, SB_PAGEUP as WPARAM, 0) };
    let page = bottom - unsafe { timeline_top_line(timeline) };
    unsafe { SendMessageW(timeline, WM_VSCROLL, SB_BOTTOM as WPARAM, 0) };
    assert_eq!(unsafe { timeline_top_line(timeline) }, bottom);
    assert!(bottom > 2 * page, "the room must be long: {bottom}");

    press_keys(&[VK_PRIOR]);
    wait_for_top_line(timeline, bottom - page)?;
    press_keys(&[VK_PRIOR]);
    wait_for_top_line(timeline, bottom - 2 * page)?;
    press_keys(&[VK_NEXT]);
    wait_for_top_line(timeline, bottom - page)?;
    press_keys(&[VK_CONTROL, VK_PRIOR]);
    wait_for_top_line(timeline, bottom - page - 1)?;
    press_keys(&[VK_CONTROL, VK_NEXT]);
    wait_for_top_line(timeline, bottom - page)?;

    // With text in the editbox, Ctrl+Home moves its caret.
    app.set_compose_text("draft")?;
    let edit = unsafe { compose_edit_hwnd(&app) };
    assert_eq!(unsafe { edit_caret(edit) }, "draft".len());
    press_keys(&[VK_CONTROL, VK_HOME]);
    let deadline = Instant::now() + Duration::from_secs(5);
    while unsafe { edit_caret(edit) } != 0 {
        assert!(Instant::now() < deadline, "Ctrl+Home must move the caret");
        std::thread::sleep(POLL_INTERVAL);
    }
    assert_eq!(unsafe { timeline_top_line(timeline) }, bottom - page);
    assert_eq!(app.compose_text()?, "draft");

    // Empty, Ctrl+Home and Ctrl+End go to the top and the bottom.
    app.set_compose_text("")?;
    press_keys(&[VK_CONTROL, VK_HOME]);
    wait_for_top_line(timeline, 0)?;
    assert_eq!(app.dump_model()?.submitted_history_commands.len(), 1);
    press_keys(&[VK_CONTROL, VK_END]);
    wait_for_top_line(timeline, bottom)?;

    // The Status window's editbox pages its own timeline.
    activate_switchbar_item_by_command(&app, &ViewIdentity::Status)?;
    assert_compose_has_focus(&app);
    for _ in 0..12 {
        app.set_compose_text("/help")?;
        app.send_compose_key(ComposeKey::Return)?;
    }
    app.wait_idle(Duration::from_secs(5))?;
    let status = app.active_timeline_hwnd()? as HWND;
    let status_bottom = unsafe { timeline_top_line(status) };
    assert!(status_bottom > 0, "/help must fill the Status window");
    // Filling it showed the scroll bar, which narrowed the window: it still
    // shows its newest line.
    let status_timeline = app
        .dump_ui_state()?
        .active_timeline
        .ok_or("Status has a timeline")?;
    assert_eq!(status_timeline.top_line, status_bottom as usize);
    assert!(
        status_timeline
            .visible_lines
            .last()
            .is_some_and(|line| line.text.ends_with("* /help <command> for usage")),
        "{:?}",
        status_timeline.visible_lines.last()
    );
    press_keys(&[VK_CONTROL, VK_PRIOR]);
    wait_for_top_line(status, status_bottom - 1)?;
    press_keys(&[VK_CONTROL, VK_HOME]);
    wait_for_top_line(status, 0)?;
    press_keys(&[VK_CONTROL, VK_END]);
    wait_for_top_line(status, status_bottom)?;
    assert_eq!(app.dump_model()?.submitted_history_commands.len(), 1);

    drop(app);
    let _ = fs::remove_dir_all(&local_app_data);
    Ok(())
}

#[test]
fn custom_room_timeline_history_backfill_submits_once_while_loading()
-> Result<(), Box<dyn std::error::Error>> {
    let exe_path =
        mirx_ui_exe_path().map_err(|error| format!("failed to locate mirx-ui.exe: {error}"))?;
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
    let exe_path =
        mirx_ui_exe_path().map_err(|error| format!("failed to locate mirx-ui.exe: {error}"))?;
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
        ExpectedFind {
            active: &room_identity("!history:example"),
            query: "older needle",
            matched: true,
            match_count: 1,
            current_match_index: Some(0),
            selected_text: Some(selected_text),
            history_search: UiFindHistorySearchState::LocalOnly,
        },
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
    let exe_path =
        mirx_ui_exe_path().map_err(|error| format!("failed to locate mirx-ui.exe: {error}"))?;
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
        mirx_core::CoreErrorEvent {
            operation: mirx_core::Operation::BackfillRoomHistory,
            room_id: Some("!beta:example".to_owned()),
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
    let exe_path =
        mirx_ui_exe_path().map_err(|error| format!("failed to locate mirx-ui.exe: {error}"))?;
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
    let exe_path =
        mirx_ui_exe_path().map_err(|error| format!("failed to locate mirx-ui.exe: {error}"))?;
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
    let exe_path =
        mirx_ui_exe_path().map_err(|error| format!("failed to locate mirx-ui.exe: {error}"))?;
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
        ExpectedFind {
            active: &view_id,
            query: "older thread needle",
            matched: true,
            match_count: 1,
            current_match_index: Some(0),
            selected_text: Some(selected_text),
            history_search: UiFindHistorySearchState::LocalOnly,
        },
    );
    assert!(view_contains(thread_view, "older thread needle"));
    assert!(!view_contains(thread_view, "older room-only needle"));
    Ok(())
}

#[test]
fn automation_driver_backfilled_messages_show_the_senders_display_name()
-> Result<(), Box<dyn std::error::Error>> {
    let exe_path =
        mirx_ui_exe_path().map_err(|error| format!("failed to locate mirx-ui.exe: {error}"))?;
    let local_app_data = unique_local_app_data_path();
    // The sync response makes @alice:example a member named "Alice"; the
    // backfilled message only carries her user ID.
    let server = spawn_mock_homeserver(
        MockMessages::Respond(serde_json::json!({
            "start": "s-current",
            "chunk": [{
                "content": {
                    "msgtype": "m.text",
                    "body": "hello from history"
                },
                "event_id": "$history-alice",
                "origin_server_ts": 1_699_999_000_000u64,
                "sender": "@alice:example",
                "type": "m.room.message"
            }]
        })),
        vec![],
    )?;
    let options = launch_options(Some(local_app_data.clone()));
    let args = [OsString::from("--automation")];

    let app = launch_app_with_options(&exe_path, &args, options)?;
    app.wait_for_lifecycle_state("core_started", Duration::from_secs(30))?;
    app.restore_session(&server.base_url, stored_session())?;
    app.wait_for_lifecycle_state("ready", Duration::from_secs(30))?;
    app.wait_for_min_rooms(1, Duration::from_secs(30))?;
    app.select_room("!beta:example")?;
    app.trigger_active_timeline_history_backfill()?;
    app.wait_for_room_activity_contains(
        Some("!beta:example"),
        "hello from history",
        Duration::from_secs(30),
    )?;

    let ui_state = app.dump_ui_state()?;
    let view = find_view(&ui_state, &room_identity("!beta:example"));
    assert_eq!(
        view_lines_containing(view, "hello from history"),
        vec!["[21:56] <Alice> hello from history"]
    );

    drop(app);
    let _ = fs::remove_dir_all(&local_app_data);
    Ok(())
}

#[test]
fn automation_driver_history_backfill_prepends_older_messages_without_disturbing_active_room()
-> Result<(), Box<dyn std::error::Error>> {
    let exe_path =
        mirx_ui_exe_path().map_err(|error| format!("failed to locate mirx-ui.exe: {error}"))?;
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

    let expected_view = Some(room_identity("!beta:example"));
    assert_eq!(
        app.dump_model()?.selected_room.as_deref(),
        Some("!beta:example")
    );
    assert_eq!(app.selected_navigation_view()?, expected_view);
    assert_compose_has_focus(&app);
    let timeline_hwnd = app.active_timeline_hwnd()? as HWND;
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
    assert_eq!(app.selected_navigation_view()?, expected_view);
    assert_compose_has_focus(&app);
    assert_switchbar_activity(&ui_state, &room_identity("!beta:example"), None);
    assert_navigation_activity(&ui_state, &room_identity("!beta:example"), None);
    assert_eq!(find_ui_room(&ui_state, "!beta:example").activity, None);
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
    let exe_path =
        mirx_ui_exe_path().map_err(|error| format!("failed to locate mirx-ui.exe: {error}"))?;
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

    let timeline_hwnd = app.active_timeline_hwnd()? as HWND;
    let text = unsafe { read_window_text(timeline_hwnd) };
    assert!(
        text.contains("multiline row 00 line a\r\nmultiline row 00 line b"),
        "timeline WM_GETTEXT must normalize embedded multiline bodies to CRLF: {text:?}"
    );

    app.select_status_view()?;
    app.wait_idle(Duration::from_secs(2))?;
    app.select_room("!beta:example")?;
    app.wait_idle(Duration::from_secs(2))?;

    let text_after_switch = unsafe { read_window_text(app.active_timeline_hwnd()? as HWND) };
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
    let exe_path =
        mirx_ui_exe_path().map_err(|error| format!("failed to locate mirx-ui.exe: {error}"))?;
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

    let timeline_hwnd = app.active_timeline_hwnd()? as HWND;
    let text = unsafe { read_window_text(timeline_hwnd) };
    assert!(
        text.contains("trailing blank line a\r\n"),
        "timeline WM_GETTEXT must preserve a trailing blank line as CRLF: {text:?}"
    );

    app.select_status_view()?;
    app.wait_idle(Duration::from_secs(2))?;
    app.select_room("!beta:example")?;
    app.wait_idle(Duration::from_secs(2))?;

    let text_after_switch = unsafe { read_window_text(app.active_timeline_hwnd()? as HWND) };
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
    let exe_path =
        mirx_ui_exe_path().map_err(|error| format!("failed to locate mirx-ui.exe: {error}"))?;
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

    let timeline_hwnd = app.active_timeline_hwnd()? as HWND;
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

unsafe fn active_mdi_content_hwnd(app: &MirxApp) -> HWND {
    let frame = app.automation_frame_hwnd() as HWND;
    let mdi_client = unsafe { GetDlgItem(frame, MDI_ID) };
    assert!(!mdi_client.is_null(), "MDI client control must exist");
    let active_child = unsafe { SendMessageW(mdi_client, WM_MDIGETACTIVE, 0, 0) as HWND };
    assert!(!active_child.is_null(), "active MDI child must exist");
    let content = unsafe { GetDlgItem(active_child, VIEW_CONTENT_ID) };
    assert!(!content.is_null(), "active MDI child content must exist");
    content
}

unsafe fn active_room_member_list_hwnd(app: &MirxApp) -> HWND {
    unsafe { child_window_with_class(active_mdi_content_hwnd(app), "ListBox") }
}

/// The entries the nick list control itself shows, read back with
/// `LB_GETTEXT` rather than from the automation model.
unsafe fn listbox_texts(hwnd: HWND) -> Vec<String> {
    let count = unsafe { SendMessageW(hwnd, LB_GETCOUNT, 0, 0) };
    assert!(count >= 0, "LB_GETCOUNT must succeed");
    (0..count as usize)
        .map(|index| {
            let len = unsafe { SendMessageW(hwnd, LB_GETTEXTLEN, index, 0) };
            assert!(len >= 0, "LB_GETTEXTLEN must succeed for item {index}");
            let mut buffer = vec![0u16; len as usize + 1];
            let copied =
                unsafe { SendMessageW(hwnd, LB_GETTEXT, index, buffer.as_mut_ptr() as isize) };
            assert_eq!(copied, len, "LB_GETTEXT must copy item {index}");
            String::from_utf16_lossy(&buffer[..copied as usize])
        })
        .collect()
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
    let exe_path =
        mirx_ui_exe_path().map_err(|error| format!("failed to locate mirx-ui.exe: {error}"))?;
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
        nav_view(
            "Status",
            ViewIdentity::Status,
            vec![
                nav_folder(
                    "Work",
                    vec![
                        nav_view("Ops", room_identity("!ops:example"), vec![]),
                        nav_view_with_activity(
                            "Shared",
                            room_identity("!shared:example"),
                            Some(ViewActivityLevel::Message),
                            vec![]
                        ),
                    ],
                ),
                nav_folder(
                    "Social",
                    vec![
                        nav_view_with_activity(
                            "Shared",
                            room_identity("!shared:example"),
                            Some(ViewActivityLevel::Message),
                            vec![]
                        ),
                        nav_view_with_activity(
                            "Random",
                            room_identity("!random:example"),
                            Some(ViewActivityLevel::Message),
                            vec![]
                        ),
                    ],
                ),
                nav_view("Direct", room_identity("!direct:example"), vec![]),
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
            activity: None,
            read_target_event_id: None,
            read_state: None,
            history_backfill: UiRoomHistoryBackfillState::Idle,
        }
    );
    assert_eq!(
        find_view(&initial_state, &room_identity("!direct:example")).title,
        "Direct (@alice:example)"
    );
    assert!(
        find_view(&initial_state, &room_identity("!direct:example"))
            .lines
            .contains(&"* Now talking in Direct (!direct:example)".to_owned())
    );
    assert_eq!(
        find_switchbar_item(&initial_state, &room_identity("!direct:example")).title,
        "Direct"
    );
    assert_switchbar_activity(&initial_state, &room_identity("!ops:example"), None);
    assert_switchbar_activity(
        &initial_state,
        &room_identity("!shared:example"),
        Some(ViewActivityLevel::Message),
    );
    assert_switchbar_activity(
        &initial_state,
        &room_identity("!random:example"),
        Some(ViewActivityLevel::Message),
    );
    assert_switchbar_activity(&initial_state, &room_identity("!direct:example"), None);
    assert_eq!(
        app.selected_navigation_view()?,
        Some(room_identity("!ops:example"))
    );

    app.select_navigation_path(&nav_path(&["Status", "Work"]))?;
    assert_eq!(
        app.selected_navigation_path()?,
        nav_path(&["Status", "Work"])
    );
    assert_eq!(app.selected_navigation_view()?, None);
    let work_state = app.dump_ui_state()?;
    assert_eq!(work_state.mdi.active, room_identity("!ops:example"));
    assert_eq!(
        app.dump_model()?.selected_room.as_deref(),
        Some("!ops:example")
    );
    assert_eq!(
        find_view(&work_state, &room_identity("!direct:example")).title,
        "Direct (@alice:example)"
    );
    assert!(
        find_view(&work_state, &room_identity("!direct:example"))
            .lines
            .contains(&"* Now talking in Direct (!direct:example)".to_owned())
    );
    assert_eq!(
        find_switchbar_item(&work_state, &room_identity("!direct:example")).title,
        "Direct"
    );

    let shared_paths = initial_state
        .navigation
        .view_paths(&room_identity("!shared:example"));
    assert_eq!(
        shared_paths,
        vec![
            nav_path(&["Status", "Work", "Shared"]),
            nav_path(&["Status", "Social", "Shared"]),
        ]
    );
    app.select_navigation_path(&shared_paths[1])?;
    assert_eq!(app.selected_navigation_path()?, shared_paths[1]);
    let shared_state = app.dump_ui_state()?;
    assert_eq!(shared_state.mdi.active, room_identity("!shared:example"));
    assert_eq!(
        app.dump_model()?.selected_room.as_deref(),
        Some("!shared:example")
    );
    assert_switchbar_activity(&shared_state, &room_identity("!shared:example"), None);

    app.select_room("!shared:example")?;
    assert_eq!(app.selected_navigation_path()?, shared_paths[0]);
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

    app.select_navigation_view(&room_identity("!ops:example"))?;
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

/// mIRC's treebar: the Status window is the root, labelled with the account;
/// spaces are folders; then channels, queries and invites. The switchbar
/// follows the same window order.
#[test]
fn automation_driver_lays_out_the_treebar_like_mirc() -> Result<(), Box<dyn std::error::Error>> {
    let exe_path =
        mirx_ui_exe_path().map_err(|error| format!("failed to locate mirx-ui.exe: {error}"))?;
    let fixture_path = fixture_state_path("treebar-status-root.json");
    let args = [
        OsString::from("--render-state"),
        fixture_path.as_os_str().to_os_string(),
    ];
    let app = launch_app(&exe_path, &args)?;
    app.wait_idle(Duration::from_secs(5))?;

    let ui_state = app.dump_ui_state()?;
    assert_eq!(
        ui_state.navigation,
        nav_view(
            "@me:example",
            ViewIdentity::Status,
            vec![
                nav_folder(
                    "Work",
                    vec![
                        nav_view("Ops", room_identity("!ops:example"), vec![]),
                        nav_view("Shared", room_identity("!shared:example"), vec![]),
                    ],
                ),
                nav_view("Lobby", room_identity("!lobby:example"), vec![]),
                nav_view("Alice", room_identity("!alice:example"), vec![]),
                nav_view("Invited [invite]", room_identity("!invite:example"), vec![]),
            ],
        )
    );
    assert_eq!(
        ui_state
            .switchbar
            .items
            .iter()
            .map(|item| (item.identity.clone(), item.title.as_str()))
            .collect::<Vec<_>>(),
        vec![
            (ViewIdentity::Status, "@me:example"),
            (room_identity("!ops:example"), "Ops"),
            (room_identity("!shared:example"), "Shared"),
            (room_identity("!lobby:example"), "Lobby"),
            (room_identity("!alice:example"), "Alice"),
            (room_identity("!invite:example"), "Invited [invite]"),
        ]
    );
    assert_eq!(
        find_view(&ui_state, &ViewIdentity::Status).title,
        "Status: @me:example"
    );
    assert_eq!(
        find_view(&ui_state, &room_identity("!alice:example")).title,
        "Alice (@alice:example)"
    );

    app.select_navigation_view(&ViewIdentity::Status)?;
    assert_eq!(app.dump_ui_state()?.mdi.active, ViewIdentity::Status);
    assert_eq!(app.selected_navigation_path()?, nav_path(&["@me:example"]));

    Ok(())
}

#[test]
fn automation_driver_navigates_thread_hierarchy_fixture() -> Result<(), Box<dyn std::error::Error>>
{
    let exe_path =
        mirx_ui_exe_path().map_err(|error| format!("failed to locate mirx-ui.exe: {error}"))?;
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
                highlight: false,
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
    let thread_view_identity = thread_identity("!ops:example", "$thread-root");
    let room_paths = initial_state
        .navigation
        .view_paths(&room_identity("!ops:example"));
    let thread_paths = initial_state.navigation.view_paths(&thread_view_identity);
    assert_eq!(room_paths.len(), 1);
    assert_eq!(thread_paths.len(), 1);
    let (thread_label, thread_parent) = thread_paths[0].split_last().unwrap();
    assert_eq!(thread_parent, room_paths[0], "a thread sits under its room");
    assert!(thread_label.text.contains("root msg"));

    app.select_navigation_view(&room_identity("!ops:example"))?;
    assert_eq!(
        app.selected_navigation_view()?,
        Some(room_identity("!ops:example"))
    );
    let room_state = app.dump_ui_state()?;
    assert_eq!(room_state.mdi.active, room_identity("!ops:example"));
    let parent_view = find_view(&room_state, &room_identity("!ops:example"));
    let parent_lines = parent_view.lines.clone();
    assert!(view_contains(parent_view, "lobby"));
    assert!(view_contains(parent_view, "root msg"));
    assert!(view_contains(parent_view, "plain"));
    assert!(view_contains(parent_view, "thread rep"));
    assert!(view_contains(parent_view, "pending threaded draft"));

    app.select_navigation_view(&thread_view_identity)?;
    assert_eq!(
        app.selected_navigation_view()?,
        Some(thread_view_identity.clone())
    );
    let thread_state = app.dump_ui_state()?;
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
    let exe_path =
        mirx_ui_exe_path().map_err(|error| format!("failed to locate mirx-ui.exe: {error}"))?;
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
    assert_switchbar_activity(&thread_state, &room_identity("!ops:example"), None);
    assert_switchbar_activity(&thread_state, &thread_view_identity, None);

    assert_eq!(
        app.send_text_target(None, "queued threaded automation send")?,
        "!ops:example"
    );
    app.wait_for_lifecycle_state("error", Duration::from_secs(30))?;

    let post_send_state = app.dump_ui_state()?;
    assert_eq!(post_send_state.mdi.active, thread_view_identity);
    assert_active_switchbar_item(&post_send_state, &thread_view_identity);
    assert_switchbar_activity(&post_send_state, &room_identity("!ops:example"), None);
    assert_switchbar_activity(&post_send_state, &thread_view_identity, None);
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
    let exe_path =
        mirx_ui_exe_path().map_err(|error| format!("failed to locate mirx-ui.exe: {error}"))?;
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

/// The window menu's Send File... (`SYSMENU_ID_SEND_FILE` in win32.rs).
const SYSMENU_ID_SEND_FILE: WPARAM = 0x0130;
/// The Explorer-style Open dialog's file name box: Wine's comdlg32 keeps the
/// ComboBoxEx `cmb13` and destroys the edit `edt1` unless a hook or a
/// template is given (`filename_is_edit`).
const OPEN_DIALOG_FILE_NAME_ID: i32 = 0x047C;

/// Launches thread-hierarchy with a real file to send in the test's own
/// `LOCALAPPDATA`, a Windows path the app can open.
fn launch_send_file_app() -> Result<(GuiTestApp, PathBuf, PathBuf), Box<dyn std::error::Error>> {
    let local_app_data = unique_local_app_data_path();
    let app = launch_automation_app(&local_app_data)?;
    app.load_events(&fs::read_to_string(fixture_state_path(
        "thread-hierarchy.json",
    ))?)?;
    app.wait_idle(Duration::from_secs(5))?;
    fs::create_dir_all(&local_app_data)?;
    let file = local_app_data.join("shot.png");
    fs::write(&file, b"not really a png")?;
    Ok((app, local_app_data, file))
}

/// Waits for the frame's Send File dialog and checks its caption.
fn wait_for_send_file_dialog(app: &MirxApp, room_name: &str) -> HWND {
    let (dialog, caption) = wait_for_owned_dialog(app.automation_frame_hwnd() as HWND);
    assert_eq!(caption, format!("Send File to {room_name}"));
    dialog
}

/// Types `path` into the open Send File dialog and presses Open.
fn choose_file_in_send_file_dialog(
    app: &MirxApp,
    dialog: HWND,
    path: &Path,
) -> Result<(), Box<dyn std::error::Error>> {
    let name = unsafe { GetDlgItem(dialog, OPEN_DIALOG_FILE_NAME_ID) };
    assert!(!name.is_null(), "the Open dialog has its file name box");
    let text = wide_null(path.to_str().expect("UTF-8 test path"));
    unsafe { SendMessageW(name, WM_SETTEXT, 0, text.as_ptr() as isize) };
    assert_eq!(
        unsafe { read_window_text(name) },
        path.to_str().expect("UTF-8 test path")
    );
    answer_message_box(app, dialog, IDOK)
}

#[test]
fn send_file_dialog_from_the_window_menu_sends_the_chosen_file()
-> Result<(), Box<dyn std::error::Error>> {
    let (app, local_app_data, file) = launch_send_file_app()?;
    let ops = room_identity("!ops:example");
    app.select_view(ops.clone())?;
    let items = app.dump_ui_state()?.active_system_menu_items;
    let find = items
        .iter()
        .position(|item| item == "Find &Text...\tCtrl+F")
        .expect("room window menu has Find Text...");
    assert_eq!(
        items[find..],
        ["Find &Text...\tCtrl+F", "Send &File...", "&Font..."]
    );
    let child = view_child_hwnd(&app, &ops)?;
    let status_before = app.dump_model()?.status;

    // Cancel sends nothing and leaves the status bar alone.
    assert_ne!(
        unsafe { PostMessageW(child, WM_SYSCOMMAND, SYSMENU_ID_SEND_FILE, 0) },
        0
    );
    let dialog = wait_for_send_file_dialog(&app, "Ops");
    answer_message_box(&app, dialog, IDCANCEL)?;
    let model = app.dump_model()?;
    assert!(model.submitted_file_commands.is_empty());
    assert_eq!(model.status, status_before);

    // The file goes to the window's own room, even when another window is
    // activated while the dialog is open.
    assert_ne!(
        unsafe { PostMessageW(child, WM_SYSCOMMAND, SYSMENU_ID_SEND_FILE, 0) },
        0
    );
    let dialog = wait_for_send_file_dialog(&app, "Ops");
    app.select_status_view()?;
    choose_file_in_send_file_dialog(&app, dialog, &file)?;
    assert_eq!(
        app.dump_model()?.submitted_file_commands,
        [SubmittedFileCommand {
            room_id: "!ops:example".to_owned(),
            path: file.to_str().expect("UTF-8 test path").to_owned(),
        }]
    );

    drop(app);
    let _ = fs::remove_dir_all(&local_app_data);
    Ok(())
}

#[test]
fn sendfile_without_a_path_opens_the_send_file_dialog() -> Result<(), Box<dyn std::error::Error>> {
    let (app, local_app_data, file) = launch_send_file_app()?;
    app.select_view(room_identity("!ops:example"))?;
    app.set_compose_text("/sendfile")?;
    // Posted: Return runs the command, whose dialog is a modal loop.
    let edit = unsafe { compose_edit_hwnd(&app) };
    assert_ne!(
        unsafe { PostMessageW(edit, WM_KEYDOWN, VK_RETURN as WPARAM, 0) },
        0
    );
    let dialog = wait_for_send_file_dialog(&app, "Ops");
    choose_file_in_send_file_dialog(&app, dialog, &file)?;
    assert_eq!(
        app.dump_model()?.submitted_file_commands,
        [SubmittedFileCommand {
            room_id: "!ops:example".to_owned(),
            path: file.to_str().expect("UTF-8 test path").to_owned(),
        }]
    );

    // A thread window has no Send File..., and /sendfile there refuses
    // before any dialog.
    app.select_view(thread_identity("!ops:example", "$thread-root"))?;
    assert!(
        !app.dump_ui_state()?
            .active_system_menu_items
            .contains(&"Send &File...".to_owned())
    );
    type_commands(&app, &["/sendfile"])?;
    assert_eq!(
        app.dump_model()?.status,
        "* /sendfile: file attachments cannot target threads yet"
    );

    drop(app);
    let _ = fs::remove_dir_all(&local_app_data);
    Ok(())
}

#[test]
fn automation_driver_sendfile_slash_rejects_active_thread() -> Result<(), Box<dyn std::error::Error>>
{
    let exe_path =
        mirx_ui_exe_path().map_err(|error| format!("failed to locate mirx-ui.exe: {error}"))?;
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
        "* /sendfile: file attachments cannot target threads yet"
    );
    Ok(())
}

#[test]
fn automation_driver_inactive_thread_activity_marks_only_thread_until_reactivated()
-> Result<(), Box<dyn std::error::Error>> {
    let exe_path =
        mirx_ui_exe_path().map_err(|error| format!("failed to locate mirx-ui.exe: {error}"))?;
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
    assert_switchbar_activity(&inactive_thread_state, &room_view_identity, None);
    assert_switchbar_activity(
        &inactive_thread_state,
        &thread_view_identity,
        Some(ViewActivityLevel::Message),
    );
    assert_eq!(
        find_switchbar_item(&inactive_thread_state, &thread_view_identity).title,
        "Thread: root msg"
    );

    app.select_view(thread_view_identity.clone())?;
    let reactivated_thread_state = app.dump_ui_state()?;
    assert_eq!(reactivated_thread_state.mdi.active, thread_view_identity);
    assert_active_switchbar_item(&reactivated_thread_state, &thread_view_identity);
    assert_switchbar_activity(&reactivated_thread_state, &room_view_identity, None);
    assert_switchbar_activity(&reactivated_thread_state, &thread_view_identity, None);

    Ok(())
}

#[test]
fn automation_driver_applies_receive_side_message_edits_in_place()
-> Result<(), Box<dyn std::error::Error>> {
    let exe_path =
        mirx_ui_exe_path().map_err(|error| format!("failed to locate mirx-ui.exe: {error}"))?;
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
    let exe_path =
        mirx_ui_exe_path().map_err(|error| format!("failed to locate mirx-ui.exe: {error}"))?;
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
                        5,
                        &[
                            "@alice:example",
                            "@bob:example",
                            "@carol:example",
                            "@dave:example",
                            "@erin:example",
                        ],
                        &[
                            "$reaction-plus-one-a",
                            "$reaction-plus-one-b",
                            "$reaction-plus-one-c",
                            "$reaction-plus-one-d",
                            "$reaction-plus-one-e",
                        ],
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
    // Reactors by member nick, three per key and a count for the rest;
    // @dave:example is not a member.
    assert!(view_contains(
        room_view,
        "    reactions: +1 Alice, Bob, Carol +2  heart Carol"
    ));

    let thread_view_identity = thread_identity("!reactions:example", "$react-thread-root");
    app.select_view(thread_view_identity.clone())?;
    app.wait_idle(Duration::from_secs(5))?;
    let thread_state = app.dump_ui_state()?;
    let thread_view = find_view(&thread_state, &thread_view_identity);
    assert!(view_contains(
        thread_view,
        "    reactions: eyes @dave:example"
    ));

    drop(app);
    let _ = fs::remove_dir_all(&local_app_data);
    Ok(())
}

#[test]
fn automation_driver_applies_receive_side_reactions_redactions()
-> Result<(), Box<dyn std::error::Error>> {
    let exe_path =
        mirx_ui_exe_path().map_err(|error| format!("failed to locate mirx-ui.exe: {error}"))?;
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
                    4,
                    &[
                        "@bob:example",
                        "@carol:example",
                        "@dave:example",
                        "@erin:example",
                    ],
                    &[
                        "$reaction-plus-one-b",
                        "$reaction-plus-one-c",
                        "$reaction-plus-one-d",
                        "$reaction-plus-one-e",
                    ],
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
    assert!(view_contains(
        room_view,
        "    reactions: +1 Bob, Carol, @dave:example +1"
    ));
    assert!(!view_contains(room_view, "heart"));

    let thread_view_identity = thread_identity("!reactions:example", "$react-thread-root");
    app.select_view(thread_view_identity.clone())?;
    app.wait_idle(Duration::from_secs(5))?;
    let thread_state = app.dump_ui_state()?;
    let thread_view = find_view(&thread_state, &thread_view_identity);
    assert!(view_contains(
        thread_view,
        "    reactions: eyes @dave:example"
    ));

    drop(app);
    let _ = fs::remove_dir_all(&local_app_data);
    Ok(())
}

#[test]
fn automation_driver_applies_receive_side_redactions_without_leaking_payloads()
-> Result<(), Box<dyn std::error::Error>> {
    let exe_path =
        mirx_ui_exe_path().map_err(|error| format!("failed to locate mirx-ui.exe: {error}"))?;
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
    let redacted_topic_row = "@grace:example changes topic to 'redaction topic should disappear'";
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
    let thread_nodes = ui_state.navigation.find_view(&thread_view_identity);
    assert_eq!(thread_nodes.len(), 1);
    assert!(thread_nodes[0].text.contains("Message redacted"));
    app.select_navigation_view(&thread_view_identity)?;
    assert_eq!(
        app.selected_navigation_view()?,
        Some(thread_view_identity.clone())
    );
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
    let exe_path =
        mirx_ui_exe_path().map_err(|error| format!("failed to locate mirx-ui.exe: {error}"))?;
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

    let thread_nodes = updated_state.navigation.find_view(&thread_view_identity);
    assert_eq!(thread_nodes.len(), 1);
    assert!(thread_nodes[0].text.contains("root msg edited"));
    assert_eq!(
        app.selected_navigation_view()?,
        Some(thread_view_identity.clone())
    );

    drop(app);
    let _ = fs::remove_dir_all(&local_app_data);
    Ok(())
}

#[test]
fn automation_driver_rejects_invalid_append_message_edit_update_without_exiting()
-> Result<(), Box<dyn std::error::Error>> {
    let exe_path =
        mirx_ui_exe_path().map_err(|error| format!("failed to locate mirx-ui.exe: {error}"))?;
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
    let exe_path =
        mirx_ui_exe_path().map_err(|error| format!("failed to locate mirx-ui.exe: {error}"))?;
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
    assert_switchbar_activity(&post_send_state, &room_identity("!ops:example"), None);
    assert_switchbar_activity(&post_send_state, &thread_view_identity, None);
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
    let exe_path =
        mirx_ui_exe_path().map_err(|error| format!("failed to locate mirx-ui.exe: {error}"))?;
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
    assert_switchbar_activity(&post_send_state, &room_identity("!ops:example"), None);
    assert_switchbar_activity(&post_send_state, &thread_view_identity, None);

    Ok(())
}

#[test]
fn automation_driver_loads_checked_in_event_fixture_via_file_api()
-> Result<(), Box<dyn std::error::Error>> {
    let exe_path =
        mirx_ui_exe_path().map_err(|error| format!("failed to locate mirx-ui.exe: {error}"))?;
    let state_path = write_state_file(&base_events())?;

    let args = [
        OsString::from("--render-state"),
        state_path.as_os_str().to_os_string(),
    ];
    let app = launch_app(&exe_path, &args)?;
    app.wait_idle(Duration::from_secs(5))?;

    let attached = MirxApp::find_by_pid(app.pid(), Duration::from_secs(5))?;
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
    assert_switchbar_activity(
        &ui_state,
        &room_identity("!support:example"),
        Some(ViewActivityLevel::Message),
    );
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
    let exe_path =
        mirx_ui_exe_path().map_err(|error| format!("failed to locate mirx-ui.exe: {error}"))?;
    let state_path = write_state_file(&base_events())?;

    let args = [
        OsString::from("--render-state"),
        state_path.as_os_str().to_os_string(),
    ];
    let app = launch_app(&exe_path, &args)?;
    app.wait_idle(Duration::from_secs(5))?;

    let attached = MirxApp::find_by_pid(app.pid(), Duration::from_secs(5))?;
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
                RoomMemberChange::Joined,
                Some("Bob"),
            ),
            member_entry_with_event_id(
                "!mixed:example",
                "$mixed-leave-1",
                "@carol:example",
                1_700_000_180_000,
                "@carol:example",
                RoomMemberChange::Left,
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
                highlight: false,
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
    assert!(view_contains(mixed_view, "[22:13] <Alice> synced hello"));
    assert!(view_contains(
        mixed_view,
        "[22:14] * Alice changes topic to 'mixed room topic'"
    ));
    assert!(view_contains(
        mixed_view,
        "[22:17] -Server- server maintenance"
    ));
    assert!(view_contains(mixed_view, "[22:18] * Alice waves hello"));
    assert!(view_contains(
        mixed_view,
        "[22:17] * Alice pending local echo (sending)"
    ));
    assert!(view_contains(
        mixed_view,
        "[22:18] * Alice failed local echo (failed)"
    ));

    let _ = fs::remove_file(&state_path);
    Ok(())
}

#[test]
fn automation_driver_renders_room_typing_only_for_selected_room_view()
-> Result<(), Box<dyn std::error::Error>> {
    let exe_path =
        mirx_ui_exe_path().map_err(|error| format!("failed to locate mirx-ui.exe: {error}"))?;
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
    let exe_path =
        mirx_ui_exe_path().map_err(|error| format!("failed to locate mirx-ui.exe: {error}"))?;
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
    let exe_path =
        mirx_ui_exe_path().map_err(|error| format!("failed to locate mirx-ui.exe: {error}"))?;
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
    let exe_path =
        mirx_ui_exe_path().map_err(|error| format!("failed to locate mirx-ui.exe: {error}"))?;
    let state_path = write_state_file(&base_events())?;

    let args = [
        OsString::from("--render-state"),
        state_path.as_os_str().to_os_string(),
    ];
    let app = launch_app(&exe_path, &args)?;
    app.wait_idle(Duration::from_secs(5))?;

    let attached = MirxApp::find_by_pid(app.pid(), Duration::from_secs(5))?;
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
    let exe_path =
        mirx_ui_exe_path().map_err(|error| format!("failed to locate mirx-ui.exe: {error}"))?;
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
    let exe_path =
        mirx_ui_exe_path().map_err(|error| format!("failed to locate mirx-ui.exe: {error}"))?;
    let fixture_path = fixture_state_path("thread-hierarchy.json");
    let args = [OsString::from("--automation")];
    let app = launch_app(&exe_path, &args)?;
    app.wait_for_lifecycle_state("core_started", Duration::from_secs(30))?;
    app.load_events(&fs::read_to_string(&fixture_path)?)?;
    app.wait_idle(Duration::from_secs(5))?;

    let room_view_identity = room_identity("!ops:example");
    let thread_view_identity = thread_identity("!ops:example", "$thread-root");

    app.select_navigation_view(&thread_view_identity)?;
    app.wait_idle(Duration::from_secs(5))?;

    activate_switchbar_item_by_command(&app, &ViewIdentity::Status)?;
    let status_state = app.dump_ui_state()?;
    assert_eq!(status_state.mdi.active, ViewIdentity::Status);
    assert_active_switchbar_item(&status_state, &ViewIdentity::Status);
    assert_eq!(app.selected_navigation_view()?, Some(ViewIdentity::Status));
    assert_compose_has_focus(&app);

    activate_switchbar_item_by_command(&app, &room_view_identity)?;
    let room_state = app.dump_ui_state()?;
    assert_eq!(room_state.mdi.active, room_view_identity);
    assert_active_switchbar_item(&room_state, &room_view_identity);
    assert_eq!(
        app.selected_navigation_view()?,
        Some(room_view_identity.clone())
    );
    assert_compose_has_focus(&app);

    activate_switchbar_item_by_command(&app, &thread_view_identity)?;
    let thread_state = app.dump_ui_state()?;
    assert_eq!(thread_state.mdi.active, thread_view_identity);
    assert_active_switchbar_item(&thread_state, &thread_view_identity);
    assert_eq!(
        app.selected_navigation_view()?,
        Some(thread_view_identity.clone())
    );
    assert_compose_has_focus(&app);

    Ok(())
}

#[test]
fn automation_driver_activation_returns_keyboard_to_compose()
-> Result<(), Box<dyn std::error::Error>> {
    let exe_path =
        mirx_ui_exe_path().map_err(|error| format!("failed to locate mirx-ui.exe: {error}"))?;
    let local_app_data = unique_local_app_data_path();
    let options = launch_options(Some(local_app_data.clone()));
    let args = [OsString::from("--automation")];

    let app = launch_app_with_options(&exe_path, &args, options)?;
    app.wait_for_lifecycle_state("core_started", Duration::from_secs(30))?;
    app.load_events(&serde_json::to_string(&base_events())?)?;
    app.wait_idle(Duration::from_secs(5))?;

    app.select_navigation_view(&room_identity("!alpha:example"))?;
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
    let exe_path =
        mirx_ui_exe_path().map_err(|error| format!("failed to locate mirx-ui.exe: {error}"))?;
    let state_path = write_state_file(&base_events())?;

    let args = [
        OsString::from("--render-state"),
        state_path.as_os_str().to_os_string(),
    ];
    let app = launch_app(&exe_path, &args)?;
    app.wait_idle(Duration::from_secs(5))?;

    let initial_state = app.dump_ui_state()?;
    assert_switchbar_activity(
        &initial_state,
        &room_identity("!alpha:example"),
        Some(ViewActivityLevel::Message),
    );

    app.select_room("!alpha:example")?;
    let activated_state = app.dump_ui_state()?;
    assert_active_switchbar_item(&activated_state, &room_identity("!alpha:example"));
    assert_switchbar_activity(&activated_state, &room_identity("!alpha:example"), None);

    let _ = fs::remove_file(&state_path);
    Ok(())
}

#[test]
fn automation_driver_reports_activity_levels_and_highlight_lines()
-> Result<(), Box<dyn std::error::Error>> {
    let exe_path =
        mirx_ui_exe_path().map_err(|error| format!("failed to locate mirx-ui.exe: {error}"))?;
    let fixture_path = fixture_state_path("activity-levels.json");
    let update_path = fixture_update_path("activity-levels.json");
    let args = [
        OsString::from("--render-state"),
        fixture_path.as_os_str().to_os_string(),
    ];
    let app = launch_app(&exe_path, &args)?;
    app.wait_idle(Duration::from_secs(5))?;
    app.append_synthetic_events(&fs::read_to_string(&update_path)?)?;
    app.wait_idle(Duration::from_secs(5))?;

    let ui_state = app.dump_ui_state()?;
    assert_eq!(ui_state.mdi.active, room_identity("!lobby:example"));
    for (room_id, level) in [
        ("!lobby:example", None),
        ("!events:example", Some(ViewActivityLevel::Event)),
        ("!chat:example", Some(ViewActivityLevel::Message)),
        ("!pings:example", Some(ViewActivityLevel::Highlight)),
        ("!casey:example", Some(ViewActivityLevel::Message)),
        ("!counts:example", Some(ViewActivityLevel::Highlight)),
    ] {
        assert_switchbar_activity(&ui_state, &room_identity(room_id), level);
        assert_navigation_activity(&ui_state, &room_identity(room_id), level);
        assert_eq!(
            find_ui_room(&ui_state, room_id).activity,
            level,
            "{room_id}"
        );
    }

    let highlights = app
        .room_activity(Some("!lobby:example"))?
        .into_iter()
        .map(|activity| (activity.activity_id, activity.highlight))
        .collect::<Vec<_>>();
    assert_eq!(
        highlights,
        vec![
            ("event:$lobby-1".to_owned(), false),
            ("event:$lobby-2".to_owned(), true),
            ("event:$lobby-3".to_owned(), true),
            ("event:$lobby-4".to_owned(), false),
        ]
    );

    app.select_room("!pings:example")?;
    app.wait_idle(Duration::from_secs(5))?;
    let activated_state = app.dump_ui_state()?;
    assert_switchbar_activity(&activated_state, &room_identity("!pings:example"), None);
    assert_switchbar_activity(
        &activated_state,
        &room_identity("!chat:example"),
        Some(ViewActivityLevel::Message),
    );
    Ok(())
}

#[test]
fn notification_mode_caps_muted_room_activity() -> Result<(), Box<dyn std::error::Error>> {
    let exe_path =
        mirx_ui_exe_path().map_err(|error| format!("failed to locate mirx-ui.exe: {error}"))?;
    let fixture_path = fixture_state_path("notification-modes.json");
    let update_path = fixture_update_path("notification-modes.json");
    let args = [
        OsString::from("--render-state"),
        fixture_path.as_os_str().to_os_string(),
    ];
    let app = launch_app(&exe_path, &args)?;
    app.wait_idle(Duration::from_secs(5))?;
    app.append_synthetic_events(&fs::read_to_string(&update_path)?)?;
    app.wait_idle(Duration::from_secs(5))?;

    let ui_state = app.dump_ui_state()?;
    assert_eq!(ui_state.mdi.active, room_identity("!lobby:example"));
    // Noisy is muted: a message, a highlight and three unread stay maroon.
    for (room_id, level) in [
        ("!lobby:example", None),
        ("!busy:example", Some(ViewActivityLevel::Message)),
        ("!quiet:example", Some(ViewActivityLevel::Message)),
        ("!noisy:example", Some(ViewActivityLevel::Event)),
    ] {
        assert_switchbar_activity(&ui_state, &room_identity(room_id), level);
        assert_navigation_activity(&ui_state, &room_identity(room_id), level);
        assert_eq!(
            find_ui_room(&ui_state, room_id).activity,
            level,
            "{room_id}"
        );
    }
    Ok(())
}

#[test]
fn automation_driver_own_messages_do_not_colour_the_switchbar()
-> Result<(), Box<dyn std::error::Error>> {
    let exe_path =
        mirx_ui_exe_path().map_err(|error| format!("failed to locate mirx-ui.exe: {error}"))?;
    let mut direct_room = room_summary("!dm:example", "Bob", false);
    direct_room.is_direct = true;
    direct_room.direct_user_ids = vec!["@bob:example".to_owned()];
    // `--render-state` activates the first room, so both rooms under test
    // stay in the background.
    let state_path = write_state_file(&[
        Event::Started,
        Event::LoggedIn {
            session: core_stored_session(),
            homeserver_url: "https://matrix.example/".to_owned(),
        },
        Event::SyncedRooms(vec![
            room_summary("!lobby:example", "Lobby", false),
            room_summary("!a:example", "A", false),
            direct_room,
        ]),
        // Ours, say sent from the phone: no colour, as in mIRC.
        text_event(
            "!dm:example",
            "@alice:example",
            1_700_000_000_000,
            "sent elsewhere",
        ),
        text_event("!a:example", "@bob:example", 1_700_000_060_000, "hi all"),
    ])?;
    let app = launch_app(
        &exe_path,
        &[
            OsString::from("--render-state"),
            state_path.as_os_str().to_os_string(),
        ],
    )?;
    app.wait_idle(Duration::from_secs(5))?;

    let ui_state = app.dump_ui_state()?;
    assert_eq!(ui_state.mdi.active, room_identity("!lobby:example"));
    assert_switchbar_activity(&ui_state, &room_identity("!dm:example"), None);
    assert_switchbar_activity(
        &ui_state,
        &room_identity("!a:example"),
        Some(ViewActivityLevel::Message),
    );
    assert!(view_contains(
        find_view(&ui_state, &room_identity("!dm:example")),
        "sent elsewhere"
    ));

    let _ = fs::remove_file(&state_path);
    Ok(())
}

#[test]
fn automation_driver_clears_tree_and_switchbar_activity_markers_on_activation()
-> Result<(), Box<dyn std::error::Error>> {
    let exe_path =
        mirx_ui_exe_path().map_err(|error| format!("failed to locate mirx-ui.exe: {error}"))?;
    let fixture_path = fixture_state_path("shell-activity.json");
    let update_path = fixture_update_path("shell-activity.json");
    let args = [OsString::from("--automation")];
    let app = launch_app(&exe_path, &args)?;
    app.wait_for_lifecycle_state("core_started", Duration::from_secs(30))?;
    app.load_events(&fs::read_to_string(&fixture_path)?)?;
    app.wait_idle(Duration::from_secs(5))?;
    app.select_navigation_view(&thread_identity("!alpha:example", "$alpha-thread-root"))?;
    app.wait_idle(Duration::from_secs(5))?;
    app.select_navigation_view(&room_identity("!alpha:example"))?;
    app.wait_idle(Duration::from_secs(5))?;
    app.append_synthetic_events(&fs::read_to_string(&update_path)?)?;
    app.wait_idle(Duration::from_secs(5))?;

    let room_view_identity = room_identity("!alpha:example");
    let beta_view_identity = room_identity("!beta:example");
    let thread_view_identity = thread_identity("!alpha:example", "$alpha-thread-root");
    let initial_state = app.dump_ui_state()?;

    assert_eq!(initial_state.mdi.active, room_view_identity);
    assert_active_switchbar_item(&initial_state, &room_view_identity);
    assert_eq!(
        app.selected_navigation_view()?,
        Some(room_identity("!alpha:example"))
    );
    assert_switchbar_activity(
        &initial_state,
        &beta_view_identity,
        Some(ViewActivityLevel::Message),
    );
    assert_navigation_activity(
        &initial_state,
        &room_identity("!beta:example"),
        Some(ViewActivityLevel::Message),
    );
    assert_switchbar_activity(
        &initial_state,
        &thread_view_identity,
        Some(ViewActivityLevel::Message),
    );
    assert_navigation_activity(
        &initial_state,
        &thread_identity("!alpha:example", "$alpha-thread-root"),
        Some(ViewActivityLevel::Message),
    );
    assert_switchbar_activity(&initial_state, &room_view_identity, None);
    assert_navigation_activity(&initial_state, &room_identity("!alpha:example"), None);

    app.select_navigation_view(&thread_identity("!alpha:example", "$alpha-thread-root"))?;
    app.wait_idle(Duration::from_secs(5))?;
    let thread_state = app.dump_ui_state()?;
    assert_eq!(thread_state.mdi.active, thread_view_identity);
    assert_active_switchbar_item(&thread_state, &thread_view_identity);
    assert_eq!(
        app.selected_navigation_view()?,
        Some(thread_identity("!alpha:example", "$alpha-thread-root"))
    );
    assert_switchbar_activity(&thread_state, &thread_view_identity, None);
    assert_navigation_activity(
        &thread_state,
        &thread_identity("!alpha:example", "$alpha-thread-root"),
        None,
    );
    assert_switchbar_activity(
        &thread_state,
        &beta_view_identity,
        Some(ViewActivityLevel::Message),
    );
    assert_navigation_activity(
        &thread_state,
        &room_identity("!beta:example"),
        Some(ViewActivityLevel::Message),
    );
    assert_switchbar_activity(&thread_state, &room_view_identity, None);
    assert_navigation_activity(&thread_state, &room_identity("!alpha:example"), None);

    app.select_navigation_view(&room_identity("!beta:example"))?;
    app.wait_idle(Duration::from_secs(5))?;
    let beta_state = app.dump_ui_state()?;
    assert_eq!(beta_state.mdi.active, beta_view_identity);
    assert_active_switchbar_item(&beta_state, &beta_view_identity);
    assert_eq!(
        app.selected_navigation_view()?,
        Some(room_identity("!beta:example"))
    );
    assert_switchbar_activity(&beta_state, &beta_view_identity, None);
    assert_navigation_activity(&beta_state, &room_identity("!beta:example"), None);
    assert_switchbar_activity(&beta_state, &thread_view_identity, None);
    assert_navigation_activity(
        &beta_state,
        &thread_identity("!alpha:example", "$alpha-thread-root"),
        None,
    );
    assert_switchbar_activity(&beta_state, &room_view_identity, None);
    assert_navigation_activity(&beta_state, &room_identity("!alpha:example"), None);

    Ok(())
}

#[test]
fn automation_driver_native_mdi_activation_reconciles_shell_state()
-> Result<(), Box<dyn std::error::Error>> {
    let exe_path =
        mirx_ui_exe_path().map_err(|error| format!("failed to locate mirx-ui.exe: {error}"))?;
    let fixture_path = fixture_state_path("shell-activity.json");
    let update_path = fixture_update_path("shell-activity.json");
    let args = [OsString::from("--automation")];
    let app = launch_app(&exe_path, &args)?;
    app.wait_for_lifecycle_state("core_started", Duration::from_secs(30))?;
    app.load_events(&fs::read_to_string(&fixture_path)?)?;
    app.wait_idle(Duration::from_secs(5))?;

    app.select_navigation_view(&thread_identity("!alpha:example", "$alpha-thread-root"))?;
    app.wait_idle(Duration::from_secs(5))?;
    app.select_navigation_view(&room_identity("!alpha:example"))?;
    app.wait_idle(Duration::from_secs(5))?;
    app.append_synthetic_events(&fs::read_to_string(&update_path)?)?;
    app.wait_idle(Duration::from_secs(5))?;

    let alpha_room_identity = room_identity("!alpha:example");
    let beta_room_identity = room_identity("!beta:example");
    let alpha_thread_identity = thread_identity("!alpha:example", "$alpha-thread-root");

    let initial_state = app.dump_ui_state()?;
    assert_eq!(initial_state.mdi.active, alpha_room_identity);
    assert_active_switchbar_item(&initial_state, &alpha_room_identity);
    assert_eq!(
        app.selected_navigation_view()?,
        Some(room_identity("!alpha:example"))
    );
    assert_switchbar_activity(
        &initial_state,
        &beta_room_identity,
        Some(ViewActivityLevel::Message),
    );
    assert_navigation_activity(
        &initial_state,
        &room_identity("!beta:example"),
        Some(ViewActivityLevel::Message),
    );
    assert_switchbar_activity(
        &initial_state,
        &alpha_thread_identity,
        Some(ViewActivityLevel::Message),
    );
    assert_navigation_activity(
        &initial_state,
        &thread_identity("!alpha:example", "$alpha-thread-root"),
        Some(ViewActivityLevel::Message),
    );
    assert_switchbar_activity(&initial_state, &alpha_room_identity, None);
    assert_navigation_activity(&initial_state, &room_identity("!alpha:example"), None);
    assert_raw_switchbar_state(&app, &initial_state, &alpha_room_identity, true);
    assert_raw_switchbar_state(&app, &initial_state, &beta_room_identity, false);
    assert_raw_switchbar_state(&app, &initial_state, &alpha_thread_identity, false);

    native_activate_mdi_view(&app, &alpha_thread_identity)?;
    let thread_state = app.dump_ui_state()?;
    assert_eq!(thread_state.mdi.active, alpha_thread_identity);
    assert_active_switchbar_item(&thread_state, &alpha_thread_identity);
    assert_eq!(
        app.selected_navigation_view()?,
        Some(thread_identity("!alpha:example", "$alpha-thread-root"))
    );
    assert_compose_has_focus(&app);
    assert_switchbar_activity(&thread_state, &alpha_thread_identity, None);
    assert_navigation_activity(
        &thread_state,
        &thread_identity("!alpha:example", "$alpha-thread-root"),
        None,
    );
    assert_switchbar_activity(
        &thread_state,
        &beta_room_identity,
        Some(ViewActivityLevel::Message),
    );
    assert_navigation_activity(
        &thread_state,
        &room_identity("!beta:example"),
        Some(ViewActivityLevel::Message),
    );
    assert_switchbar_activity(&thread_state, &alpha_room_identity, None);
    assert_navigation_activity(&thread_state, &room_identity("!alpha:example"), None);
    assert_raw_switchbar_state(&app, &thread_state, &alpha_thread_identity, true);
    assert_raw_switchbar_state(&app, &thread_state, &beta_room_identity, false);

    native_activate_mdi_view(&app, &beta_room_identity)?;
    let beta_state = app.dump_ui_state()?;
    assert_eq!(beta_state.mdi.active, beta_room_identity);
    assert_active_switchbar_item(&beta_state, &beta_room_identity);
    assert_eq!(
        app.selected_navigation_view()?,
        Some(room_identity("!beta:example"))
    );
    assert_compose_has_focus(&app);
    assert_switchbar_activity(&beta_state, &beta_room_identity, None);
    assert_navigation_activity(&beta_state, &room_identity("!beta:example"), None);
    assert_switchbar_activity(&beta_state, &alpha_thread_identity, None);
    assert_navigation_activity(
        &beta_state,
        &thread_identity("!alpha:example", "$alpha-thread-root"),
        None,
    );
    assert_switchbar_activity(&beta_state, &alpha_room_identity, None);
    assert_navigation_activity(&beta_state, &room_identity("!alpha:example"), None);
    assert_raw_switchbar_state(&app, &beta_state, &beta_room_identity, true);

    Ok(())
}

#[test]
fn automation_driver_render_state_append_records_room_read_target()
-> Result<(), Box<dyn std::error::Error>> {
    let exe_path =
        mirx_ui_exe_path().map_err(|error| format!("failed to locate mirx-ui.exe: {error}"))?;
    let fixture_path = fixture_state_path("shell-activity.json");
    let update_path = fixture_update_path("shell-activity.json");
    let args = [
        OsString::from("--render-state"),
        fixture_path.as_os_str().to_os_string(),
    ];
    let app = launch_app(&exe_path, &args)?;
    app.wait_idle(Duration::from_secs(5))?;

    app.select_navigation_view(&thread_identity("!alpha:example", "$alpha-thread-root"))?;
    app.wait_idle(Duration::from_secs(5))?;
    app.select_navigation_view(&room_identity("!alpha:example"))?;
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
    let exe_path =
        mirx_ui_exe_path().map_err(|error| format!("failed to locate mirx-ui.exe: {error}"))?;
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
    assert!(ui_state.status_bar.text.contains("synced 3 room(s)"));

    Ok(())
}

#[test]
fn automation_driver_applies_room_read_state_fixture_activity_and_clears_it_on_activation()
-> Result<(), Box<dyn std::error::Error>> {
    let exe_path =
        mirx_ui_exe_path().map_err(|error| format!("failed to locate mirx-ui.exe: {error}"))?;
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
        "Alpha [@1 !2 *1 M]"
    );
    assert_switchbar_activity(&initial_state, &room_identity("!beta:example"), None);
    assert_switchbar_activity(
        &initial_state,
        &room_identity("!alpha:example"),
        Some(ViewActivityLevel::Highlight),
    );
    assert!(navigation_contains(
        &initial_state.navigation,
        "Alpha [@1 !2 *1 M]"
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
            activity: None,
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
            activity: Some(ViewActivityLevel::Highlight),
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
        "Alpha [@1 !2 *1 M]"
    );
    assert_switchbar_activity(&activated_state, &room_identity("!alpha:example"), None);
    assert!(navigation_contains(
        &activated_state.navigation,
        "Alpha [@1 !2 *1 M]"
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
            activity: None,
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
    let exe_path =
        mirx_ui_exe_path().map_err(|error| format!("failed to locate mirx-ui.exe: {error}"))?;
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
        "Alpha"
    );
    let alpha_nodes = cleared_state
        .navigation
        .find_view(&room_identity("!alpha:example"));
    assert_eq!(
        alpha_nodes
            .iter()
            .map(|node| node.text.as_str())
            .collect::<Vec<_>>(),
        vec!["Alpha"]
    );

    let _ = fs::remove_file(&state_path);
    Ok(())
}

#[test]
fn automation_driver_renders_image_message_fixture_and_exposes_typed_activity()
-> Result<(), Box<dyn std::error::Error>> {
    let exe_path =
        mirx_ui_exe_path().map_err(|error| format!("failed to locate mirx-ui.exe: {error}"))?;
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
    assert_switchbar_activity(&initial_state, &room_identity("!images:example.org"), None);
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
        "[16:00] * @alice:example.org sent image: diagram.png (image/png, 640x480, 12345 bytes)",
    ));
    assert!(!view_contains(image_view, "unsupported m.image"));

    Ok(())
}

#[test]
fn automation_driver_renders_file_message_fixture_and_exposes_typed_activity()
-> Result<(), Box<dyn std::error::Error>> {
    let exe_path =
        mirx_ui_exe_path().map_err(|error| format!("failed to locate mirx-ui.exe: {error}"))?;
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
    assert_switchbar_activity(&initial_state, &room_identity("!media:example.org"), None);
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
        "[16:00] * @alice:example.org sent file: report.pdf (application/pdf, 45678 bytes)",
    ));
    assert!(!view_contains(media_view, "unsupported"));

    Ok(())
}

#[test]
fn automation_driver_renders_audio_message_fixture_and_exposes_typed_activity()
-> Result<(), Box<dyn std::error::Error>> {
    let exe_path =
        mirx_ui_exe_path().map_err(|error| format!("failed to locate mirx-ui.exe: {error}"))?;
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
    assert_switchbar_activity(&initial_state, &room_identity("!media:example.org"), None);
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
        "[16:01] * @alice:example.org sent audio: voice-note.ogg (audio/ogg, 23456 bytes, 12000 ms)",
    ));
    assert!(!view_contains(media_view, "unsupported"));

    Ok(())
}

#[test]
fn automation_driver_renders_loaded_image_message_fixture_and_exposes_loaded_media_state()
-> Result<(), Box<dyn std::error::Error>> {
    let exe_path =
        mirx_ui_exe_path().map_err(|error| format!("failed to locate mirx-ui.exe: {error}"))?;
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
        "[16:00] * @alice:example.org sent image: diagram.png (image/png, 640x480, 12345 bytes)",
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
    let exe_path =
        mirx_ui_exe_path().map_err(|error| format!("failed to locate mirx-ui.exe: {error}"))?;
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
        "[16:00] * @alice:example.org sent image: diagram.png (image/png, 640x480, 12345 bytes)",
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
    let exe_path =
        mirx_ui_exe_path().map_err(|error| format!("failed to locate mirx-ui.exe: {error}"))?;
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
    let exe_path =
        mirx_ui_exe_path().map_err(|error| format!("failed to locate mirx-ui.exe: {error}"))?;
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
    assert_switchbar_activity(&initial_state, &room_identity("!images:example.org"), None);
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
        "[16:00] * @alice:example.org sent image: diagram.png (image/png, 640x480, 12345 bytes)",
    ));
    assert!(!view_contains(image_view, "unsupported m.image"));

    Ok(())
}

#[test]
fn automation_driver_renders_encrypted_loaded_image_and_exposes_loaded_media_state()
-> Result<(), Box<dyn std::error::Error>> {
    let exe_path =
        mirx_ui_exe_path().map_err(|error| format!("failed to locate mirx-ui.exe: {error}"))?;
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
        "[16:00] * @alice:example.org sent image: diagram.png (image/png, 640x480, 12345 bytes)",
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
    let exe_path =
        mirx_ui_exe_path().map_err(|error| format!("failed to locate mirx-ui.exe: {error}"))?;
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
        "[16:00] * @alice:example.org sent image: diagram.png (image/png, 640x480, 12345 bytes)",
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
    let exe_path =
        mirx_ui_exe_path().map_err(|error| format!("failed to locate mirx-ui.exe: {error}"))?;
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
    assert_switchbar_activity(&initial_state, &room_identity("!alpha:example.org"), None);
    assert_switchbar_activity(
        &initial_state,
        &room_identity("!media:example.org"),
        Some(ViewActivityLevel::Message),
    );
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
        "[16:00] * @alice:example.org sent unsupported com.example.custom: custom payload",
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
    assert_switchbar_activity(&activated_state, &room_identity("!media:example.org"), None);

    Ok(())
}

#[test]
fn automation_driver_compose_history_keys_are_noops_without_history()
-> Result<(), Box<dyn std::error::Error>> {
    let exe_path =
        mirx_ui_exe_path().map_err(|error| format!("failed to locate mirx-ui.exe: {error}"))?;
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
    let exe_path =
        mirx_ui_exe_path().map_err(|error| format!("failed to locate mirx-ui.exe: {error}"))?;
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
fn automation_driver_compose_history_is_per_window() -> Result<(), Box<dyn std::error::Error>> {
    let exe_path =
        mirx_ui_exe_path().map_err(|error| format!("failed to locate mirx-ui.exe: {error}"))?;
    let local_app_data = unique_local_app_data_path();
    let options = launch_options(Some(local_app_data.clone()));
    let args = [OsString::from("--automation")];

    let app = launch_app_with_options(&exe_path, &args, options)?;
    app.wait_for_lifecycle_state("core_started", Duration::from_secs(30))?;
    app.load_events(&serde_json::to_string(&base_events())?)?;
    app.wait_idle(Duration::from_secs(5))?;

    app.select_room("!beta:example")?;
    app.set_compose_text("typed in beta")?;
    app.send_compose_key(ComposeKey::Return)?;
    assert_eq!(app.compose_text()?, "");

    // Like mIRC, each window's editbox recalls only what was typed in it.
    app.select_room("!alpha:example")?;
    app.send_compose_key(ComposeKey::Up)?;
    assert_eq!(app.compose_text()?, "");
    app.set_compose_text("/help")?;
    app.send_compose_key(ComposeKey::Return)?;
    assert_eq!(app.compose_text()?, "");

    app.select_status_view()?;
    app.send_compose_key(ComposeKey::Up)?;
    assert_eq!(app.compose_text()?, "");

    app.select_room("!beta:example")?;
    app.send_compose_key(ComposeKey::Up)?;
    assert_eq!(app.compose_text()?, "typed in beta");
    app.send_compose_key(ComposeKey::Up)?;
    assert_eq!(app.compose_text()?, "typed in beta");

    app.select_room("!alpha:example")?;
    app.send_compose_key(ComposeKey::Up)?;
    assert_eq!(app.compose_text()?, "/help");

    drop(app);
    let _ = fs::remove_dir_all(&local_app_data);
    Ok(())
}

#[test]
fn automation_driver_compose_history_ignores_rejected_submissions()
-> Result<(), Box<dyn std::error::Error>> {
    let exe_path =
        mirx_ui_exe_path().map_err(|error| format!("failed to locate mirx-ui.exe: {error}"))?;
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
    let exe_path =
        mirx_ui_exe_path().map_err(|error| format!("failed to locate mirx-ui.exe: {error}"))?;
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
    let exe_path =
        mirx_ui_exe_path().map_err(|error| format!("failed to locate mirx-ui.exe: {error}"))?;
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
    let exe_path =
        mirx_ui_exe_path().map_err(|error| format!("failed to locate mirx-ui.exe: {error}"))?;
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

    let edit = unsafe { compose_edit_hwnd(&app) };
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
    let exe_path =
        mirx_ui_exe_path().map_err(|error| format!("failed to locate mirx-ui.exe: {error}"))?;
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
    let edit = unsafe { compose_edit_hwnd(&app) };
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
    let exe_path =
        mirx_ui_exe_path().map_err(|error| format!("failed to locate mirx-ui.exe: {error}"))?;
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
    let exe_path =
        mirx_ui_exe_path().map_err(|error| format!("failed to locate mirx-ui.exe: {error}"))?;
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
    assert_eq!(app.selected_navigation_view()?, Some(ViewIdentity::Status));

    // The line belongs to the history of the window it was typed in.
    app.send_compose_key(ComposeKey::Up)?;
    assert_eq!(app.compose_text()?, "");
    app.select_room("!beta:example")?;
    app.send_compose_key(ComposeKey::Up)?;
    assert_eq!(app.compose_text()?, "/status");

    drop(app);
    let _ = fs::remove_dir_all(&local_app_data);
    Ok(())
}

#[test]
fn automation_driver_send_text_keeps_literal_slash_body() -> Result<(), Box<dyn std::error::Error>>
{
    let exe_path =
        mirx_ui_exe_path().map_err(|error| format!("failed to locate mirx-ui.exe: {error}"))?;
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
    let exe_path =
        mirx_ui_exe_path().map_err(|error| format!("failed to locate mirx-ui.exe: {error}"))?;
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
        app.selected_navigation_view()?,
        Some(room_identity("!alpha:example"))
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
        app.selected_navigation_view()?,
        Some(room_identity("!beta:example"))
    );

    drop(app);
    let _ = fs::remove_dir_all(&local_app_data);
    Ok(())
}

#[test]
fn automation_driver_slash_window_reports_unknown_and_ambiguous_targets()
-> Result<(), Box<dyn std::error::Error>> {
    let exe_path =
        mirx_ui_exe_path().map_err(|error| format!("failed to locate mirx-ui.exe: {error}"))?;
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
    let exe_path =
        mirx_ui_exe_path().map_err(|error| format!("failed to locate mirx-ui.exe: {error}"))?;
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
fn automation_driver_slash_join_activates_the_joined_room() -> Result<(), Box<dyn std::error::Error>>
{
    let exe_path =
        mirx_ui_exe_path().map_err(|error| format!("failed to locate mirx-ui.exe: {error}"))?;
    let local_app_data = unique_local_app_data_path();
    let server = spawn_mock_homeserver(MockMessages::Hang, vec![])?;
    let app = launch_app_with_options(
        &exe_path,
        &[OsString::from("--automation")],
        launch_options(Some(local_app_data.clone())),
    )?;
    app.wait_for_lifecycle_state("core_started", Duration::from_secs(30))?;
    app.restore_session(&server.base_url, stored_session())?;
    app.wait_for_lifecycle_state("ready", Duration::from_secs(30))?;
    app.wait_idle(Duration::from_secs(5))?;
    app.select_room("!beta:example")?;
    app.wait_idle(Duration::from_secs(5))?;

    app.set_compose_text("/join #new:example")?;
    app.send_compose_key(ComposeKey::Return)?;
    let ui_state = app.wait_for_ui_state(
        "the joined room active",
        Duration::from_secs(10),
        |ui_state| ui_state.mdi.active == room_identity("!new:example"),
    )?;

    find_view(&ui_state, &room_identity("!new:example"));
    find_view(&ui_state, &room_identity("!beta:example"));
    assert_eq!(app.dump_model()?.last_error, None);

    drop(app);
    let _ = fs::remove_dir_all(&local_app_data);
    Ok(())
}

#[test]
fn slash_create_activates_the_created_room() -> Result<(), Box<dyn std::error::Error>> {
    let local_app_data = unique_local_app_data_path();
    let server = spawn_mock_homeserver(MockMessages::Hang, vec![])?;
    let app = launch_automation_app(&local_app_data)?;
    app.restore_session(&server.base_url, stored_session())?;
    app.wait_for_lifecycle_state("ready", Duration::from_secs(30))?;
    app.wait_idle(Duration::from_secs(5))?;
    app.select_room("!beta:example")?;
    app.wait_idle(Duration::from_secs(5))?;

    app.set_compose_text("/create -public #team:elsewhere Team room")?;
    app.send_compose_key(ComposeKey::Return)?;
    app.wait_idle(Duration::from_secs(5))?;
    assert!(view_contains(
        find_view(&app.dump_ui_state()?, &room_identity("!beta:example")),
        "* /create: the alias must be on your server (example)"
    ));

    app.set_compose_text("/create Team room")?;
    app.send_compose_key(ComposeKey::Return)?;
    let ui_state = app.wait_for_ui_state(
        "the created room active",
        Duration::from_secs(10),
        |ui_state| ui_state.mdi.active == room_identity("!created:example"),
    )?;
    find_view(&ui_state, &room_identity("!beta:example"));
    assert_eq!(app.dump_model()?.last_error, None);

    drop(app);
    let _ = fs::remove_dir_all(&local_app_data);
    Ok(())
}

#[test]
fn automation_driver_slash_query_records_submitted_direct_room_without_creating_room()
-> Result<(), Box<dyn std::error::Error>> {
    let exe_path =
        mirx_ui_exe_path().map_err(|error| format!("failed to locate mirx-ui.exe: {error}"))?;
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

const MENU_ID_COMMANDS_FIRST: usize = 40100;
const COMMANDS_MENU_JOIN: usize = 0;
const COMMANDS_MENU_LIST: usize = 1;
const COMMANDS_MENU_PART: usize = 2;
const COMMANDS_MENU_QUERY: usize = 3;
const MENU_ID_HELP_TOPICS: usize = 40034;
const MENU_ID_HELP_ABOUT: usize = 40035;
const ABOUT_DIALOG_VERSION_ID: i32 = 5019;
const ABOUT_DIALOG_OK_ID: usize = 5024;
const SINGLE_FIELD_DIALOG_EDIT_ID: i32 = 5010;
const SINGLE_FIELD_DIALOG_OK_ID: usize = 5011;
const SINGLE_FIELD_DIALOG_CANCEL_ID: usize = 5012;
const SYSMENU_ID_FIND: WPARAM = 0x0120;

/// Picks the Commands menu item at `index` as a click does, filling its
/// prompt with `input` when it has one.
fn run_commands_menu_item(
    app: &MirxApp,
    index: usize,
    input: Option<&str>,
) -> Result<(), Box<dyn std::error::Error>> {
    let frame_value = app.automation_frame_hwnd();
    let menu_thread = std::thread::spawn(move || unsafe {
        SendMessageW(
            frame_value as HWND,
            WM_COMMAND,
            (MENU_ID_COMMANDS_FIRST + index) as WPARAM,
            0,
        );
    });
    if let Some(input) = input {
        answer_commands_prompt(input)?;
    }
    menu_thread
        .join()
        .expect("Commands menu thread must not panic");
    app.wait_idle(Duration::from_secs(5))?;
    Ok(())
}

/// Types `input` into the open Commands prompt and presses OK.
fn answer_commands_prompt(input: &str) -> Result<(), Box<dyn std::error::Error>> {
    let class_name = wide_null("mirxWin32SingleFieldDialog");
    let dialog = wait_for_window(&class_name, "Commands prompt")?;
    let edit = unsafe { GetDlgItem(dialog, SINGLE_FIELD_DIALOG_EDIT_ID) };
    assert!(!edit.is_null(), "Commands prompt edit must exist");
    let input = wide_null(input);
    assert_ne!(unsafe { SetWindowTextW(edit, input.as_ptr()) }, 0);
    assert_ne!(
        unsafe { PostMessageW(dialog, WM_COMMAND, SINGLE_FIELD_DIALOG_OK_ID as WPARAM, 0) },
        0
    );
    wait_for_window_gone(dialog, "Commands prompt")
}

/// The Rooms List's list control, read from the real window.
fn room_list_control(app: &MirxApp) -> Result<HWND, Box<dyn std::error::Error>> {
    let child = view_child_hwnd(app, &ViewIdentity::RoomList)?;
    let list = unsafe { GetDlgItem(child, VIEW_CONTENT_ID) };
    assert!(!list.is_null(), "the Rooms List window must hold its list");
    assert_eq!(unsafe { class_name(list) }, "SysListView32");
    Ok(list)
}

fn mouse_lparam(point: &UiPoint) -> isize {
    ((point.y as u16 as isize) << 16) | point.x as u16 as isize
}

/// Clicks, or double-clicks, `hwnd` at `point` in its client area with the
/// left button, through the mouse messages the control itself handles.
fn post_left_click(hwnd: HWND, point: &UiPoint, double: bool) {
    let lparam = mouse_lparam(point);
    let mut messages = vec![(WM_LBUTTONDOWN, MK_LBUTTON), (WM_LBUTTONUP, 0)];
    if double {
        messages.extend([(WM_LBUTTONDBLCLK, MK_LBUTTON), (WM_LBUTTONUP, 0)]);
    }
    for (message, buttons) in messages {
        assert_ne!(
            unsafe { PostMessageW(hwnd, message, buttons as WPARAM, lparam) },
            0,
            "posting a mouse message must succeed"
        );
    }
}

/// Like `post_left_click`, but sent: the click is handled before this
/// returns, so a later automation request cannot overtake it
/// (`.cydo/memory/posted-input-vs-sent-automation-race.md`). Only for
/// clicks that open nothing modal.
fn send_left_click(hwnd: HWND, point: &UiPoint, double: bool) {
    let lparam = mouse_lparam(point);
    let mut messages = vec![(WM_LBUTTONDOWN, MK_LBUTTON), (WM_LBUTTONUP, 0)];
    if double {
        messages.extend([(WM_LBUTTONDBLCLK, MK_LBUTTON), (WM_LBUTTONUP, 0)]);
    }
    for (message, buttons) in messages {
        let mut result = 0;
        assert_ne!(
            unsafe {
                SendMessageTimeoutW(
                    hwnd,
                    message,
                    buttons as WPARAM,
                    lparam,
                    SMTO_NORMAL,
                    5_000,
                    &mut result,
                )
            },
            0,
            "the control must handle the click without blocking"
        );
    }
}

fn room_list_rows(ui_state: &UiState) -> Vec<Vec<String>> {
    ui_state
        .room_list
        .as_ref()
        .map(|room_list| room_list.rows.clone())
        .unwrap_or_default()
}

fn directory_row(room: &str, users: &str, name: &str) -> Vec<String> {
    vec![
        room.to_owned(),
        users.to_owned(),
        name.to_owned(),
        format!("About {name}"),
    ]
}

/// Signs in to the mock homeserver and lists its whole directory from the
/// Commands menu.
fn list_the_mock_directory(
    local_app_data: &Path,
    server: &MockBackfillHomeserver,
) -> Result<(GuiTestApp, UiState), Box<dyn std::error::Error>> {
    let app = launch_automation_app(local_app_data)?;
    app.restore_session(&server.base_url, stored_session())?;
    app.wait_for_lifecycle_state("ready", Duration::from_secs(30))?;
    app.wait_idle(Duration::from_secs(5))?;

    run_commands_menu_item(&app, COMMANDS_MENU_LIST, Some(""))?;
    let ui_state = app.wait_for_ui_state(
        "both directory pages listed",
        Duration::from_secs(10),
        |ui_state| {
            find_view(ui_state, &ViewIdentity::RoomList)
                .title
                .ends_with("5 rooms")
        },
    )?;
    Ok((app, ui_state))
}

#[test]
fn room_list_fills_sorts_and_joins_from_the_directory() -> Result<(), Box<dyn std::error::Error>> {
    let local_app_data = unique_local_app_data_path();
    let server = spawn_mock_homeserver(MockMessages::Hang, vec![])?;
    let (app, listed) = list_the_mock_directory(&local_app_data, &server)?;

    assert_eq!(listed.mdi.active, ViewIdentity::RoomList);
    assert_eq!(
        find_view(&listed, &ViewIdentity::RoomList).title,
        "Rooms List [example] - 5 rooms"
    );
    let room_list = listed.room_list.as_ref().expect("the Rooms List is open");
    assert_eq!(room_list.columns, ["Room", "Users", "Name", "Topic"]);
    assert_eq!(
        room_list.rows,
        vec![
            directory_row("#rust:example", "120", "Rust"),
            directory_row("#beta:example", "2", "Beta"),
            directory_row("!opaque:example", "7", "Opaque"),
            directory_row("#zed:example", "40", "Zed"),
            directory_row("#alpha:example", "1", "Alpha"),
        ]
    );
    assert_eq!(room_list.selected, None);
    assert_eq!(
        app.dump_model()?.submitted_room_list_requests,
        vec![
            SubmittedRoomListRequest {
                server: None,
                filter: None,
                since: None,
                limit: 100,
            },
            SubmittedRoomListRequest {
                server: None,
                filter: None,
                since: Some("page2".to_owned()),
                limit: 100,
            },
        ]
    );

    // Clicking the Users header sorts the busiest rooms first.
    let list = room_list_control(&app)?;
    let header = unsafe { child_window_with_class(list, "SysHeader32") };
    post_left_click(header, &room_list.column_click_points[1], false);
    let sorted = app.wait_for_ui_state(
        "the rooms sorted by users",
        Duration::from_secs(5),
        |ui_state| {
            room_list_rows(ui_state).first().map(|row| row[0].as_str()) == Some("#rust:example")
                && room_list_rows(ui_state).get(1).map(|row| row[0].as_str())
                    == Some("#zed:example")
        },
    )?;
    assert_eq!(
        room_list_rows(&sorted)
            .iter()
            .map(|row| row[1].as_str())
            .collect::<Vec<_>>(),
        ["120", "40", "7", "2", "1"]
    );

    // Double-clicking a room joins it and opens its window.
    let sorted_list = sorted.room_list.as_ref().expect("the Rooms List is open");
    post_left_click(list, &sorted_list.visible_row_click_points[0], true);
    let joined = app.wait_for_ui_state(
        "the joined room active",
        Duration::from_secs(10),
        |ui_state| ui_state.mdi.active == room_identity("!rust:example"),
    )?;
    find_view(&joined, &ViewIdentity::RoomList);
    let model = app.dump_model()?;
    assert_eq!(
        model.submitted_membership_commands,
        vec![SubmittedMembershipCommand {
            action: MembershipCommandAction::Join,
            target: "#rust:example".to_owned(),
        }]
    );
    assert_eq!(model.last_error, None);

    drop(app);
    let _ = fs::remove_dir_all(&local_app_data);
    Ok(())
}

#[test]
fn room_list_return_on_a_joined_room_activates_its_window() -> Result<(), Box<dyn std::error::Error>>
{
    let local_app_data = unique_local_app_data_path();
    let server = spawn_mock_homeserver(MockMessages::Hang, vec![])?;
    let (app, listed) = list_the_mock_directory(&local_app_data, &server)?;
    let room_list = listed.room_list.as_ref().expect("the Rooms List is open");
    assert_eq!(room_list.rows[1][0], "#beta:example");

    let list = room_list_control(&app)?;
    post_left_click(list, &room_list.visible_row_click_points[1], false);
    app.wait_for_ui_state(
        "the Beta row selected",
        Duration::from_secs(5),
        |ui_state| {
            ui_state
                .room_list
                .as_ref()
                .and_then(|room_list| room_list.selected)
                == Some(1)
        },
    )?;
    assert_ne!(
        unsafe { PostMessageW(list, WM_KEYDOWN, VK_RETURN as WPARAM, 0) },
        0
    );
    app.wait_for_ui_state(
        "the Beta window active",
        Duration::from_secs(5),
        |ui_state| ui_state.mdi.active == room_identity("!beta:example"),
    )?;
    assert!(app.dump_model()?.submitted_membership_commands.is_empty());

    drop(app);
    let _ = fs::remove_dir_all(&local_app_data);
    Ok(())
}

#[test]
fn alt_l_lists_matching_rooms_and_closing_the_window_drops_the_listing()
-> Result<(), Box<dyn std::error::Error>> {
    let local_app_data = unique_local_app_data_path();
    let server = spawn_mock_homeserver(MockMessages::Hang, vec![])?;
    let app = launch_automation_app(&local_app_data)?;
    let frame = app.automation_frame_hwnd() as HWND;

    // Signed out there is no directory to list.
    unsafe { SetForegroundWindow(frame) };
    press_keys(&[VK_MENU, u16::from(b'L')]);
    answer_commands_prompt("rust")?;
    app.wait_idle(Duration::from_secs(5))?;
    assert!(status_view_contains(
        &app,
        "* /list: not connected to server"
    )?);
    assert!(app.dump_ui_state()?.room_list.is_none());
    assert!(app.dump_model()?.submitted_room_list_requests.is_empty());

    app.restore_session(&server.base_url, stored_session())?;
    app.wait_for_lifecycle_state("ready", Duration::from_secs(30))?;
    app.wait_idle(Duration::from_secs(5))?;
    unsafe { SetForegroundWindow(frame) };
    press_keys(&[VK_MENU, u16::from(b'L')]);
    answer_commands_prompt("rust")?;
    let listed = app.wait_for_ui_state(
        "the matching rooms listed",
        Duration::from_secs(10),
        |ui_state| {
            ui_state
                .room_list
                .as_ref()
                .is_some_and(|room_list| room_list.rows.len() == 5)
        },
    )?;
    assert_eq!(listed.mdi.active, ViewIdentity::RoomList);
    assert_eq!(
        find_view(&listed, &ViewIdentity::RoomList).title,
        "Rooms List [example] \"rust\" - 5 rooms"
    );
    assert_eq!(
        app.dump_model()?.submitted_room_list_requests[0],
        SubmittedRoomListRequest {
            server: None,
            filter: Some("rust".to_owned()),
            since: None,
            limit: 100,
        }
    );

    close_view_without_prompt(&app, &ViewIdentity::RoomList)?;
    let closed = app.dump_ui_state()?;
    assert!(closed.room_list.is_none());
    assert!(
        closed
            .mdi
            .views
            .iter()
            .all(|view| view.identity != ViewIdentity::RoomList)
    );
    assert!(!serde_json::to_string(&closed.navigation)?.contains("Rooms List"));

    drop(app);
    let _ = fs::remove_dir_all(&local_app_data);
    Ok(())
}

#[test]
fn commands_menu_part_parts_the_active_room_and_empty_join_echoes_the_error()
-> Result<(), Box<dyn std::error::Error>> {
    let local_app_data = unique_local_app_data_path();
    let app = launch_automation_app(&local_app_data)?;
    app.load_events(&serde_json::to_string(&base_events())?)?;
    app.wait_idle(Duration::from_secs(5))?;
    let beta = room_identity("!beta:example");
    app.select_view(beta.clone())?;

    run_commands_menu_item(&app, COMMANDS_MENU_PART, None)?;
    let model = app.dump_model()?;
    assert_eq!(model.status, "part submitted: !beta:example");
    assert_eq!(
        model.submitted_membership_commands,
        vec![SubmittedMembershipCommand {
            action: MembershipCommandAction::Leave,
            target: "!beta:example".to_owned(),
        }]
    );

    run_commands_menu_item(&app, COMMANDS_MENU_JOIN, Some("  "))?;
    let ui_state = app.dump_ui_state()?;
    assert_eq!(ui_state.mdi.active, beta);
    assert!(view_contains(
        find_view(&ui_state, &beta),
        "* /join: insufficient parameters"
    ));

    drop(app);
    let _ = fs::remove_dir_all(&local_app_data);
    Ok(())
}

fn menu(label: &str, items: &[(&str, bool)]) -> UiMenu {
    UiMenu {
        label: label.to_owned(),
        items: items
            .iter()
            .map(|(label, enabled)| UiMenuItem {
                label: (*label).to_owned(),
                enabled: *enabled,
                checked: false,
            })
            .collect(),
    }
}

/// View, with each bar's item checked while the bar is shown.
fn view_menu(switchbar: bool, treebar: bool) -> UiMenu {
    UiMenu {
        label: "&View".to_owned(),
        items: [("&Switchbar", switchbar), ("&Treebar", treebar)]
            .into_iter()
            .map(|(label, checked)| UiMenuItem {
                label: label.to_owned(),
                enabled: true,
                checked,
            })
            .collect(),
    }
}

fn expected_menu_bar(file: [bool; 4]) -> Vec<UiMenu> {
    let [connect, disconnect, select_server, log_out] = file;
    vec![
        menu(
            "&File",
            &[
                ("&Connect", connect),
                ("&Disconnect", disconnect),
                ("Select &Server...\tAlt+E", select_server),
                ("&Log Out", log_out),
                ("E&xit", true),
            ],
        ),
        view_menu(true, true),
        menu(
            "&Tools",
            &[
                ("Set Up E2EE &Recovery...", true),
                ("Recover &E2EE...", true),
                ("&Accept Verification", false),
                ("Confirm &SAS", false),
                ("Re&ject SAS", false),
                ("&Cancel Verification", false),
                ("&Options...\tAlt+O", true),
            ],
        ),
        menu(
            "&Commands",
            &[
                ("&Join room...", true),
                ("&List rooms...\tAlt+L", true),
                ("&Part room", true),
                ("&Query user...", true),
                ("&Whois user...", true),
                ("&Invite user...", true),
                ("&Kick user...", true),
                ("&Ban user...", true),
                ("Change &nick...", true),
            ],
        ),
        menu(
            "&Window",
            &[
                ("Cascade", true),
                ("Tile Horizontal", true),
                ("Tile Vertical", true),
                ("Arrange Icons", true),
            ],
        ),
        menu("&Help", &[("&Help Topics", true), ("&About mirx", true)]),
    ]
}

#[test]
fn menu_bar_is_mirc_shaped() -> Result<(), Box<dyn std::error::Error>> {
    let local_app_data = unique_local_app_data_path();
    let app = launch_automation_app(&local_app_data)?;
    assert_eq!(
        app.dump_ui_state()?.menu_bar,
        expected_menu_bar([true, false, true, false])
    );

    // A maximized child adds MDI's bitmap items to the bar; they are no
    // menus.
    app.set_view_window_state(ViewIdentity::Status, MdiWindowState::Maximized)?;
    app.wait_idle(Duration::from_secs(5))?;
    assert!(
        app.dump_ui_state()?
            .mdi
            .views
            .iter()
            .any(|view| view.placement.state == MdiWindowState::Maximized)
    );
    // MDI marks the frame title and augments the bar together (Wine's
    // mdi.c, MDI_AugmentFrameMenu); menus are not readable cross-process.
    assert!(
        unsafe { read_window_text(app.automation_frame_hwnd() as HWND) }.ends_with("[Status]"),
        "the Status window must be MDI's maximized child"
    );
    assert_eq!(
        app.dump_ui_state()?.menu_bar,
        expected_menu_bar([true, false, true, false])
    );

    drop(app);
    let _ = fs::remove_dir_all(&local_app_data);
    Ok(())
}

const MENU_ID_VIEW_SWITCHBAR: usize = 40036;

#[test]
fn view_menu_hides_and_shows_the_bars_and_persists_them() -> Result<(), Box<dyn std::error::Error>>
{
    let exe_path =
        mirx_ui_exe_path().map_err(|error| format!("failed to locate mirx-ui.exe: {error}"))?;
    let local_app_data = unique_local_app_data_path();
    let launch = || {
        launch_app_with_options(
            &exe_path,
            &[OsString::from("--automation")],
            launch_options(Some(local_app_data.clone())),
        )
    };

    let app = launch()?;
    let shown = app.dump_ui_state()?.frame_layout;
    let frame_width = unsafe { client_rect(app.automation_frame_hwnd() as HWND) }.right;
    invoke_account_menu_command(&app, MENU_ID_VIEW_SWITCHBAR)?;
    let ui_state = app.dump_ui_state()?;
    assert_eq!(ui_state.frame_layout.switchbar, None);
    assert_eq!(ui_state.frame_layout.mdi_client.y, 0);
    assert_eq!(ui_state.status_bar.text, "switchbar: off");
    assert!(ui_state.menu_bar.contains(&view_menu(false, true)));
    // Its buttons still follow the windows.
    assert_eq!(ui_state.switchbar.items.len(), 1);

    type_commands(&app, &["/treebar off", "/treebar"])?;
    let ui_state = app.dump_ui_state()?;
    assert_eq!(ui_state.frame_layout.treebar, None);
    let mdi_client = ui_state.frame_layout.mdi_client;
    assert_eq!((mdi_client.x, mdi_client.width), (0, frame_width));
    assert!(ui_state.menu_bar.contains(&view_menu(false, false)));
    assert!(status_view_contains(&app, "* Treebar is off")?);
    let ini = fs::read_to_string(settings_file_path(&local_app_data))?;
    assert!(ini.contains("[switchbar]\r\nshow=off\r\n"), "{ini}");
    assert!(ini.contains("[treebar]\r\nshow=off\r\n"), "{ini}");
    drop(app);

    let app = launch()?;
    let ui_state = app.dump_ui_state()?;
    assert_eq!(
        (
            ui_state.frame_layout.switchbar,
            ui_state.frame_layout.treebar
        ),
        (None, None)
    );
    type_commands(&app, &["/switchbar on", "/treebar maybe", "/TREEBAR ON"])?;
    assert!(status_view_contains(
        &app,
        "* /treebar: usage: /treebar [on|off]"
    )?);
    let ui_state = app.dump_ui_state()?;
    assert_eq!(ui_state.frame_layout, shown);
    assert_eq!(
        ui_state.frame_layout.mdi_client.y,
        shown.switchbar.expect("the switchbar is shown").bottom()
    );
    assert!(ui_state.menu_bar.contains(&view_menu(true, true)));
    drop(app);
    let _ = fs::remove_dir_all(&local_app_data);
    Ok(())
}

const MENU_ID_TOOLS_OPTIONS: usize = 40038;
const OPTIONS_CONNECT_ON_STARTUP_ID: i32 = 6101;
const OPTIONS_TIMESTAMP_SHOW_ID: i32 = 6201;
const OPTIONS_TIMESTAMP_FORMAT_ID: i32 = 6202;
const OPTIONS_TIMESTAMP_TOKENS_ID: u16 = 6203;
const OPTIONS_SWITCHBAR_BUTTONS_ID: i32 = 6301;
const OPTIONS_HIGHLIGHT_LIST_ID: i32 = 6401;
const OPTIONS_HIGHLIGHT_WORD_ID: i32 = 6402;
const OPTIONS_HIGHLIGHT_ADD_ID: i32 = 6403;
const OPTIONS_HIGHLIGHT_DELETE_ID: i32 = 6404;
const OPTIONS_HIGHLIGHT_NOTE_ID: i32 = 6405;

/// Waits for the Options dialog to be open and shown, and returns it.
fn wait_for_options_dialog(app: &MirxApp) -> Result<HWND, Box<dyn std::error::Error>> {
    let dialog = wait_for_window(&wide_null("mirxWin32OptionsDialog"), "Options dialog")?;
    app.wait_for_ui_state("the Options dialog", Duration::from_secs(5), |ui_state| {
        ui_state.options_dialog.is_some()
    })?;
    assert_eq!(unsafe { read_window_text(dialog) }, "mirx Options");
    Ok(dialog)
}

/// Opens Tools > Options... from another thread, which the menu command
/// blocks until the dialog closes.
fn open_options_from_menu(app: &MirxApp) -> std::thread::JoinHandle<()> {
    let frame_value = app.automation_frame_hwnd();
    std::thread::spawn(move || unsafe {
        SendMessageW(
            frame_value as HWND,
            WM_COMMAND,
            MENU_ID_TOOLS_OPTIONS as WPARAM,
            0,
        );
    })
}

unsafe fn set_options_check(dialog: HWND, id: i32, checked: bool) {
    unsafe { SendMessageW(GetDlgItem(dialog, id), BM_SETCHECK, checked as WPARAM, 0) };
}

unsafe fn set_options_text(dialog: HWND, id: i32, text: &str) {
    let text = wide_null(text);
    unsafe { SetWindowTextW(GetDlgItem(dialog, id), text.as_ptr()) };
}

fn post_options_command(dialog: HWND, id: i32) -> Result<(), Box<dyn std::error::Error>> {
    assert_ne!(
        unsafe { PostMessageW(dialog, WM_COMMAND, id as WPARAM, 0) },
        0
    );
    Ok(())
}

#[test]
fn options_dialog_saves_connect_timestamp_and_buttons() -> Result<(), Box<dyn std::error::Error>> {
    let (app, local_app_data) = launch_close_lifecycle_app()?;
    let frame = app.automation_frame_hwnd() as HWND;
    unsafe { SetForegroundWindow(frame) };
    press_keys(&[VK_MENU, u16::from(b'O')]);
    let dialog = wait_for_options_dialog(&app)?;
    let options = app
        .dump_ui_state()?
        .options_dialog
        .expect("the dialog is open");
    assert_eq!(
        options.tree,
        [
            "Connect",
            "Connect/Options",
            "IRC",
            "IRC/Messages",
            "IRC/Highlight",
            "Display"
        ]
    );
    assert_eq!(options.selected, "Connect/Options");
    assert_eq!(
        options.visible_controls,
        [OPTIONS_CONNECT_ON_STARTUP_ID as u16]
    );

    // Down from Connect/Options is the IRC category, which shows its page.
    let tree = unsafe { GetDlgItem(dialog, 6001) };
    assert_ne!(
        unsafe { PostMessageW(tree, WM_KEYDOWN, VK_DOWN as WPARAM, 0) },
        0
    );
    let ui_state = app.wait_for_ui_state("IRC > Messages", Duration::from_secs(5), |ui_state| {
        ui_state
            .options_dialog
            .as_ref()
            .is_some_and(|options| options.selected == "IRC/Messages")
    })?;
    let options = ui_state.options_dialog.expect("the dialog is open");
    assert_eq!(
        options.visible_controls,
        [
            OPTIONS_TIMESTAMP_SHOW_ID as u16,
            OPTIONS_TIMESTAMP_FORMAT_ID as u16,
            OPTIONS_TIMESTAMP_TOKENS_ID,
        ]
    );
    assert_eq!(
        unsafe { read_window_text(GetDlgItem(dialog, OPTIONS_TIMESTAMP_FORMAT_ID)) },
        "[HH:nn]"
    );

    // Controls on hidden pages take their values too.
    unsafe {
        set_options_check(dialog, OPTIONS_CONNECT_ON_STARTUP_ID, false);
        set_options_check(dialog, OPTIONS_TIMESTAMP_SHOW_ID, false);
        set_options_text(dialog, OPTIONS_TIMESTAMP_FORMAT_ID, "[HH:nn:ss]");
        set_options_text(dialog, OPTIONS_SWITCHBAR_BUTTONS_ID, "90");
    }
    post_options_command(dialog, IDOK)?;
    wait_for_window_gone(dialog, "Options dialog")?;
    app.wait_idle(Duration::from_secs(5))?;

    let ui_state = app.dump_ui_state()?;
    assert_eq!(ui_state.options_dialog, None);
    assert_eq!(
        ui_state.timestamp,
        UiTimestamp {
            show: false,
            format: "[HH:nn:ss]".to_owned(),
        }
    );
    assert!(!ui_state.switchbar.items.is_empty());
    assert!(
        ui_state
            .switchbar
            .items
            .iter()
            .all(|item| item.rect.width == 90),
        "{:?}",
        ui_state.switchbar
    );
    assert_eq!(ui_state.status_bar.text, "options saved");
    let ini = fs::read_to_string(settings_file_path(&local_app_data))?;
    for line in [
        "[timestamp]\r\nshow=off\r\nformat=[HH:nn:ss]\r\n",
        "[connect]\r\nstartup=off\r\n",
        "buttons=90\r\n",
    ] {
        assert!(ini.contains(line), "{line:?} in {ini:?}");
    }

    // The menu item opens it too, with what was saved.
    let menu_thread = open_options_from_menu(&app);
    let dialog = wait_for_options_dialog(&app)?;
    assert_eq!(
        unsafe { read_window_text(GetDlgItem(dialog, OPTIONS_SWITCHBAR_BUTTONS_ID)) },
        "90"
    );
    post_options_command(dialog, IDCANCEL)?;
    wait_for_window_gone(dialog, "Options dialog")?;
    menu_thread.join().expect("the menu thread must not panic");

    drop(app);
    let _ = fs::remove_dir_all(&local_app_data);
    Ok(())
}

#[test]
fn options_dialog_rejects_a_bad_field_and_cancel_changes_nothing()
-> Result<(), Box<dyn std::error::Error>> {
    let (app, local_app_data) = launch_close_lifecycle_app()?;
    let before = app.dump_ui_state()?;
    let menu_thread = open_options_from_menu(&app);
    let dialog = wait_for_options_dialog(&app)?;
    unsafe { set_options_text(dialog, OPTIONS_SWITCHBAR_BUTTONS_ID, "20") };
    post_options_command(dialog, IDOK)?;
    let (message_box, text) = wait_for_owned_message_box(dialog);
    assert_eq!(text, "Switchbar buttons must be 40-400");
    assert_eq!(unsafe { read_window_text(message_box) }, "mirx Options");
    // The dialog shows the wrong field's page.
    let options = app
        .dump_ui_state()?
        .options_dialog
        .expect("the dialog stays open");
    assert_eq!(options.selected, "Display");
    assert_eq!(
        options.visible_controls,
        [OPTIONS_SWITCHBAR_BUTTONS_ID as u16]
    );
    answer_message_box(&app, message_box, IDOK)?;
    assert_ne!(unsafe { IsWindow(dialog) }, 0);
    assert!(!settings_file_path(&local_app_data).exists());

    post_options_command(dialog, IDCANCEL)?;
    wait_for_window_gone(dialog, "Options dialog")?;
    menu_thread.join().expect("the menu thread must not panic");

    // Esc in a field cancels too.
    let menu_thread = open_options_from_menu(&app);
    let dialog = wait_for_options_dialog(&app)?;
    let format = unsafe { GetDlgItem(dialog, OPTIONS_TIMESTAMP_FORMAT_ID) };
    unsafe { set_options_text(dialog, OPTIONS_TIMESTAMP_FORMAT_ID, "") };
    assert_ne!(
        unsafe { PostMessageW(format, WM_KEYDOWN, VK_ESCAPE as WPARAM, 0) },
        0
    );
    wait_for_window_gone(dialog, "Options dialog")?;
    menu_thread.join().expect("the menu thread must not panic");
    app.wait_idle(Duration::from_secs(5))?;

    let after = app.dump_ui_state()?;
    assert_eq!(after.timestamp, before.timestamp);
    assert_eq!(after.switchbar, before.switchbar);
    assert!(!settings_file_path(&local_app_data).exists());
    drop(app);
    let _ = fs::remove_dir_all(&local_app_data);
    Ok(())
}

/// Launches with the close-lifecycle rooms and, as a signed-in session's
/// push rules give, the highlight words `deploy` and the notify-only
/// `build`.
fn launch_highlight_words_app() -> Result<(GuiTestApp, PathBuf), Box<dyn std::error::Error>> {
    let (app, local_app_data) = launch_close_lifecycle_app()?;
    app.append_synthetic_events(&serde_json::to_string(&vec![Event::HighlightWords(vec![
        HighlightWord {
            word: "deploy".to_owned(),
            highlights: true,
        },
        HighlightWord {
            word: "build".to_owned(),
            highlights: false,
        },
    ])])?)?;
    app.wait_idle(Duration::from_secs(5))?;
    Ok((app, local_app_data))
}

/// Selects IRC > Highlight with the tree's arrow keys: Down to the IRC
/// category shows IRC > Messages, Down again is Highlight.
fn show_options_highlight_page(
    app: &MirxApp,
    dialog: HWND,
) -> Result<mirx_testkit::UiOptionsDialog, Box<dyn std::error::Error>> {
    let tree = unsafe { GetDlgItem(dialog, 6001) };
    for page in ["IRC/Messages", "IRC/Highlight"] {
        assert_ne!(
            unsafe { PostMessageW(tree, WM_KEYDOWN, VK_DOWN as WPARAM, 0) },
            0
        );
        app.wait_for_ui_state(page, Duration::from_secs(5), |ui_state| {
            ui_state
                .options_dialog
                .as_ref()
                .is_some_and(|options| options.selected == page)
        })?;
    }
    let options = app
        .dump_ui_state()?
        .options_dialog
        .expect("the dialog is open");
    assert_eq!(options.selected, "IRC/Highlight");
    assert_eq!(
        options.visible_controls,
        [
            OPTIONS_HIGHLIGHT_LIST_ID,
            OPTIONS_HIGHLIGHT_DELETE_ID,
            OPTIONS_HIGHLIGHT_WORD_ID,
            OPTIONS_HIGHLIGHT_ADD_ID,
            OPTIONS_HIGHLIGHT_NOTE_ID,
        ]
        .map(|id| id as u16)
    );
    Ok(options)
}

fn options_highlight(
    app: &MirxApp,
) -> Result<mirx_testkit::UiOptionsDialog, Box<dyn std::error::Error>> {
    Ok(app
        .dump_ui_state()?
        .options_dialog
        .expect("the dialog is open"))
}

/// Types `text` into the word box and presses Enter there.
fn enter_highlight_words(
    app: &MirxApp,
    dialog: HWND,
    text: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let word = unsafe { GetDlgItem(dialog, OPTIONS_HIGHLIGHT_WORD_ID) };
    unsafe { set_options_text(dialog, OPTIONS_HIGHLIGHT_WORD_ID, text) };
    send_left_click(word, &UiPoint { x: 4, y: 4 }, false);
    assert_eq!(unsafe { focused_hwnd(app) }, word);
    assert_ne!(
        unsafe { PostMessageW(word, WM_KEYDOWN, VK_RETURN as WPARAM, 0) },
        0
    );
    Ok(())
}

#[test]
fn options_highlight_page_lists_adds_deletes_and_submits_on_ok()
-> Result<(), Box<dyn std::error::Error>> {
    let (app, local_app_data) = launch_highlight_words_app()?;
    let menu_thread = open_options_from_menu(&app);
    let dialog = wait_for_options_dialog(&app)?;
    let options = show_options_highlight_page(&app, dialog)?;
    assert_eq!(options.highlight_items, ["deploy", "build (notify only)"]);
    assert!(options.highlight_controls_enabled);
    assert!(!options.highlight_delete_enabled);
    assert_eq!(
        unsafe { read_window_text(GetDlgItem(dialog, OPTIONS_HIGHLIGHT_NOTE_ID)) },
        "Words are matched as whole words; * and ? are wildcards. Highlight words are \
         kept on your Matrix server and shared with your other clients."
    );

    // Enter in the word box adds, several at once; `build` now highlights.
    enter_highlight_words(&app, dialog, "release, build")?;
    app.wait_for_ui_state("the added words", Duration::from_secs(5), |ui_state| {
        ui_state
            .options_dialog
            .as_ref()
            .is_some_and(|options| options.highlight_items == ["deploy", "build", "release"])
    })?;
    assert_eq!(
        unsafe { read_window_text(GetDlgItem(dialog, OPTIONS_HIGHLIGHT_WORD_ID)) },
        ""
    );
    assert_ne!(unsafe { IsWindow(dialog) }, 0, "Enter there is not OK");

    // A wrong word says why and keeps the text to fix.
    enter_highlight_words(&app, dialog, "ok, a/b")?;
    let (message_box, text) = wait_for_owned_message_box(dialog);
    assert_eq!(
        text,
        "Highlight \"a/b\": a highlight word cannot contain / or \\"
    );
    answer_message_box(&app, message_box, IDOK)?;
    assert_eq!(
        unsafe { read_window_text(GetDlgItem(dialog, OPTIONS_HIGHLIGHT_WORD_ID)) },
        "ok, a/b"
    );
    assert_eq!(
        options_highlight(&app)?.highlight_items,
        ["deploy", "build", "release"]
    );

    // Selecting an item enables Delete, which greys again once it deletes.
    let list = unsafe { GetDlgItem(dialog, OPTIONS_HIGHLIGHT_LIST_ID) };
    post_left_click(list, &UiPoint { x: 8, y: 4 }, false);
    app.wait_for_ui_state("Delete enabled", Duration::from_secs(5), |ui_state| {
        ui_state
            .options_dialog
            .as_ref()
            .is_some_and(|options| options.highlight_delete_enabled)
    })?;
    post_options_command(dialog, OPTIONS_HIGHLIGHT_DELETE_ID)?;
    app.wait_for_ui_state("deploy deleted", Duration::from_secs(5), |ui_state| {
        ui_state
            .options_dialog
            .as_ref()
            .is_some_and(|options| options.highlight_items == ["build", "release"])
    })?;
    assert!(!options_highlight(&app)?.highlight_delete_enabled);
    assert!(app.dump_model()?.submitted_notification_commands.is_empty());

    post_options_command(dialog, IDOK)?;
    wait_for_window_gone(dialog, "Options dialog")?;
    menu_thread.join().expect("the menu thread must not panic");
    app.wait_idle(Duration::from_secs(5))?;
    assert_eq!(
        app.dump_model()?.submitted_notification_commands,
        [
            SubmittedNotificationCommand::AddHighlightWord {
                word: "build".to_owned()
            },
            SubmittedNotificationCommand::AddHighlightWord {
                word: "release".to_owned()
            },
            SubmittedNotificationCommand::RemoveHighlightWord {
                word: "deploy".to_owned()
            },
        ]
    );

    drop(app);
    let _ = fs::remove_dir_all(&local_app_data);
    Ok(())
}

#[test]
fn options_highlight_page_cancel_sends_nothing() -> Result<(), Box<dyn std::error::Error>> {
    let (app, local_app_data) = launch_highlight_words_app()?;
    let menu_thread = open_options_from_menu(&app);
    let dialog = wait_for_options_dialog(&app)?;
    unsafe { set_options_text(dialog, OPTIONS_HIGHLIGHT_WORD_ID, "release") };
    post_options_command(dialog, OPTIONS_HIGHLIGHT_ADD_ID)?;
    app.wait_for_ui_state("release added", Duration::from_secs(5), |ui_state| {
        ui_state.options_dialog.as_ref().is_some_and(|options| {
            options.highlight_items == ["deploy", "build (notify only)", "release"]
        })
    })?;
    post_options_command(dialog, IDCANCEL)?;
    wait_for_window_gone(dialog, "Options dialog")?;
    menu_thread.join().expect("the menu thread must not panic");
    app.wait_idle(Duration::from_secs(5))?;
    assert!(app.dump_model()?.submitted_notification_commands.is_empty());

    // The next opening starts from the server's list again.
    let menu_thread = open_options_from_menu(&app);
    let dialog = wait_for_options_dialog(&app)?;
    assert_eq!(
        options_highlight(&app)?.highlight_items,
        ["deploy", "build (notify only)"]
    );
    // OK with no highlight changes sends nothing either.
    post_options_command(dialog, IDOK)?;
    wait_for_window_gone(dialog, "Options dialog")?;
    menu_thread.join().expect("the menu thread must not panic");
    app.wait_idle(Duration::from_secs(5))?;
    assert!(app.dump_model()?.submitted_notification_commands.is_empty());
    assert_eq!(app.dump_ui_state()?.status_bar.text, "options saved");

    drop(app);
    let _ = fs::remove_dir_all(&local_app_data);
    Ok(())
}

#[test]
fn options_highlight_page_is_greyed_while_signed_out() -> Result<(), Box<dyn std::error::Error>> {
    let (app, local_app_data) = launch_close_lifecycle_app()?;
    let menu_thread = open_options_from_menu(&app);
    let dialog = wait_for_options_dialog(&app)?;
    let options = show_options_highlight_page(&app, dialog)?;
    assert_eq!(options.highlight_items, Vec::<String>::new());
    assert!(!options.highlight_controls_enabled);
    assert!(!options.highlight_delete_enabled);
    assert_eq!(
        unsafe { read_window_text(GetDlgItem(dialog, OPTIONS_HIGHLIGHT_NOTE_ID)) },
        "Connect to edit highlight words."
    );
    post_options_command(dialog, IDOK)?;
    wait_for_window_gone(dialog, "Options dialog")?;
    menu_thread.join().expect("the menu thread must not panic");
    app.wait_idle(Duration::from_secs(5))?;
    assert!(app.dump_model()?.submitted_notification_commands.is_empty());

    drop(app);
    let _ = fs::remove_dir_all(&local_app_data);
    Ok(())
}

#[test]
fn help_topics_echoes_command_list() -> Result<(), Box<dyn std::error::Error>> {
    let local_app_data = unique_local_app_data_path();
    let app = launch_automation_app(&local_app_data)?;
    invoke_account_menu_command(&app, MENU_ID_HELP_TOPICS)?;
    assert!(status_view_contains(&app, "* Commands: /accept /ban")?);
    assert!(status_view_contains(&app, " /join ")?);

    drop(app);
    let _ = fs::remove_dir_all(&local_app_data);
    Ok(())
}

#[test]
fn help_about_dialog_shows_version_and_closes() -> Result<(), Box<dyn std::error::Error>> {
    let local_app_data = unique_local_app_data_path();
    let app = launch_automation_app(&local_app_data)?;
    let frame_value = app.automation_frame_hwnd();
    let menu_thread = std::thread::spawn(move || unsafe {
        SendMessageW(
            frame_value as HWND,
            WM_COMMAND,
            MENU_ID_HELP_ABOUT as WPARAM,
            0,
        );
    });
    let class_name = wide_null("mirxWin32AboutDialog");
    let dialog = wait_for_window(&class_name, "About dialog")?;
    assert_eq!(unsafe { read_window_text(dialog) }, "About mirx");
    let version = unsafe { GetDlgItem(dialog, ABOUT_DIALOG_VERSION_ID) };
    assert!(!version.is_null(), "About dialog version line must exist");
    assert_eq!(
        unsafe { read_window_text(version) },
        format!("mirx {}", env!("CARGO_PKG_VERSION"))
    );
    assert_ne!(
        unsafe { PostMessageW(dialog, WM_COMMAND, ABOUT_DIALOG_OK_ID as WPARAM, 0) },
        0
    );
    wait_for_window_gone(dialog, "About dialog")?;
    menu_thread
        .join()
        .expect("About menu command thread must not panic");

    drop(app);
    let _ = fs::remove_dir_all(&local_app_data);
    Ok(())
}

/// Cancels the open Find dialog.
fn cancel_find_dialog() -> Result<(), Box<dyn std::error::Error>> {
    let class_name = wide_null("mirxWin32SingleFieldDialog");
    let dialog = wait_for_window(&class_name, "Find dialog")?;
    assert_eq!(unsafe { read_window_text(dialog) }, "Find");
    assert_ne!(
        unsafe {
            PostMessageW(
                dialog,
                WM_COMMAND,
                SINGLE_FIELD_DIALOG_CANCEL_ID as WPARAM,
                0,
            )
        },
        0
    );
    wait_for_window_gone(dialog, "Find dialog")
}

#[test]
fn find_text_in_window_menu_and_ctrl_f() -> Result<(), Box<dyn std::error::Error>> {
    let local_app_data = unique_local_app_data_path();
    let app = launch_automation_app(&local_app_data)?;
    let ui_state = app.dump_ui_state()?;
    for label in ["Find &Text...\tCtrl+F", "&Font..."] {
        assert!(
            ui_state
                .active_system_menu_items
                .contains(&label.to_owned()),
            "{label:?} in {:?}",
            ui_state.active_system_menu_items
        );
    }

    let child = unsafe { active_mdi_child(&app) };
    assert_ne!(
        unsafe { PostMessageW(child, WM_SYSCOMMAND, SYSMENU_ID_FIND, 0) },
        0
    );
    cancel_find_dialog()?;
    app.wait_idle(Duration::from_secs(5))?;

    // Ctrl+F goes through the frame's accelerator table, which reads the
    // real keyboard state: type it.
    let frame = app.automation_frame_hwnd() as HWND;
    unsafe { SetForegroundWindow(frame) };
    assert_compose_has_focus(&app);
    press_keys(&[VK_CONTROL, u16::from(b'F')]);
    cancel_find_dialog()?;
    app.wait_idle(Duration::from_secs(5))?;

    drop(app);
    let _ = fs::remove_dir_all(&local_app_data);
    Ok(())
}

#[test]
fn alt_e_opens_select_server_only_while_signed_out() -> Result<(), Box<dyn std::error::Error>> {
    let local_app_data = unique_local_app_data_path();
    let server = spawn_mock_backfill_homeserver()?;
    let app = launch_automation_app(&local_app_data)?;
    let frame = app.automation_frame_hwnd() as HWND;
    unsafe { SetForegroundWindow(frame) };
    let class_name = wide_null("mirxWin32LoginDialog");

    press_keys(&[VK_MENU, u16::from(b'E')]);
    let dialog = wait_for_window(&class_name, "Select Server dialog")?;
    assert_ne!(
        unsafe { PostMessageW(dialog, WM_COMMAND, LOGIN_DIALOG_CANCEL_ID as WPARAM, 0) },
        0
    );
    wait_for_window_gone(dialog, "Select Server dialog")?;
    app.wait_idle(Duration::from_secs(5))?;

    // Signed in, Select Server is greyed, and an accelerator for a greyed
    // item sends nothing.
    app.restore_session(&server.base_url, stored_session())?;
    app.wait_for_lifecycle_state("ready", Duration::from_secs(30))?;
    unsafe { SetForegroundWindow(frame) };
    press_keys(&[VK_MENU, u16::from(b'E')]);
    app.wait_idle(Duration::from_secs(5))?;
    std::thread::sleep(Duration::from_millis(500));
    assert!(unsafe { FindWindowW(class_name.as_ptr(), std::ptr::null()) }.is_null());

    drop(app);
    let _ = fs::remove_dir_all(&local_app_data);
    Ok(())
}

/// Asserts that `result` failed with the protocol error `expected`.
fn assert_protocol_error(result: Result<(), TestkitError>, expected: &str) {
    match result {
        Err(TestkitError::Protocol { code, .. }) => assert_eq!(code, expected),
        other => panic!("expected the protocol error {expected}, got {other:?}"),
    }
}

/// Closes a mirx dialog as Esc does, with `IDCANCEL`, and waits until it
/// is gone.
fn close_dialog(app: &MirxApp, dialog: HWND) -> Result<(), Box<dyn std::error::Error>> {
    assert_ne!(
        unsafe { PostMessageW(dialog, WM_COMMAND, IDCANCEL as WPARAM, 0) },
        0
    );
    wait_for_window_gone(dialog, "dialog")?;
    app.wait_idle(Duration::from_secs(5))?;
    Ok(())
}

#[test]
fn open_dialog_opens_each_dialog_over_the_frame() -> Result<(), Box<dyn std::error::Error>> {
    let (app, local_app_data) = launch_close_lifecycle_app()?;
    let beta = room_identity("!beta:example");
    app.select_view(beta.clone())?;
    app.wait_idle(Duration::from_secs(5))?;
    for (dialog, class, title) in [
        (
            AutomationDialog::Options,
            "mirxWin32OptionsDialog",
            "mirx Options",
        ),
        (
            AutomationDialog::SelectServer,
            "mirxWin32LoginDialog",
            "Select Server",
        ),
        (
            AutomationDialog::About,
            "mirxWin32AboutDialog",
            "About mirx",
        ),
        (
            AutomationDialog::CommandsJoin,
            "mirxWin32SingleFieldDialog",
            "Join room",
        ),
        (
            AutomationDialog::RecoverE2ee,
            "mirxWin32RecoverE2eeDialog",
            "Recover E2EE",
        ),
        (
            AutomationDialog::SetupE2eeRecovery,
            "mirxWin32SetupE2eeDialog",
            "Set Up E2EE Recovery",
        ),
    ] {
        app.open_dialog(dialog)?;
        let open = app.wait_for_modal_dialog(Duration::from_secs(5))?;
        assert_eq!((open.class.as_str(), open.title.as_str()), (class, title));
        close_dialog(&app, open.hwnd as HWND)?;
    }

    // The close box asks before leaving the room; No keeps it.
    app.open_dialog(AutomationDialog::CloseActiveView)?;
    let open = app.wait_for_modal_dialog(Duration::from_secs(5))?;
    assert_eq!(
        (open.class.as_str(), open.title.as_str()),
        ("#32770", "mirx")
    );
    answer_message_box(&app, open.hwnd as HWND, IDNO)?;
    assert!(
        app.list_rooms()?
            .iter()
            .any(|room| room.room_id == "!beta:example")
    );
    assert!(app.dump_model()?.submitted_membership_commands.is_empty());

    // The Status window closes without asking.
    app.select_view(ViewIdentity::Status)?;
    assert_protocol_error(
        app.open_dialog(AutomationDialog::CloseActiveView),
        "no_close_confirmation",
    );
    app.wait_idle(Duration::from_secs(5))?;

    drop(app);
    let _ = fs::remove_dir_all(&local_app_data);
    Ok(())
}

#[test]
fn open_dialog_refuses_a_greyed_menu_item() -> Result<(), Box<dyn std::error::Error>> {
    let local_app_data = unique_local_app_data_path();
    let server = spawn_mock_backfill_homeserver()?;
    let app = launch_automation_app(&local_app_data)?;
    app.restore_session(&server.base_url, stored_session())?;
    app.wait_for_lifecycle_state("ready", Duration::from_secs(30))?;

    assert_protocol_error(
        app.open_dialog(AutomationDialog::SelectServer),
        "menu_item_disabled",
    );
    app.wait_idle(Duration::from_secs(5))?;
    std::thread::sleep(Duration::from_millis(500));
    assert!(
        unsafe { FindWindowW(wide_null("mirxWin32LoginDialog").as_ptr(), std::ptr::null()) }
            .is_null()
    );

    drop(app);
    let _ = fs::remove_dir_all(&local_app_data);
    Ok(())
}

#[test]
fn select_options_page_selects_each_page() -> Result<(), Box<dyn std::error::Error>> {
    let (app, local_app_data) = launch_close_lifecycle_app()?;
    assert_protocol_error(app.select_options_page("Display"), "no_options_dialog");

    app.open_dialog(AutomationDialog::Options)?;
    let dialog = app.wait_for_modal_dialog(Duration::from_secs(5))?;
    for (path, selected) in [
        ("IRC/Messages", "IRC/Messages"),
        ("IRC/Highlight", "IRC/Highlight"),
        ("Display", "Display"),
        ("Connect/Options", "Connect/Options"),
        // A category without a page of its own shows its first page.
        ("IRC", "IRC/Messages"),
    ] {
        app.select_options_page(path)?;
        let options = app
            .dump_ui_state()?
            .options_dialog
            .expect("the dialog is open");
        assert_eq!(options.selected, selected, "{path}");
    }
    for path in ["IRC/Logging", "irc/messages", ""] {
        assert_protocol_error(app.select_options_page(path), "unknown_options_page");
    }
    let options = app
        .dump_ui_state()?
        .options_dialog
        .expect("the dialog is open");
    assert_eq!(options.selected, "IRC/Messages");
    close_dialog(&app, dialog.hwnd as HWND)?;

    drop(app);
    let _ = fs::remove_dir_all(&local_app_data);
    Ok(())
}

/// Presses `keys` in order through `SendInput`, then releases them in
/// reverse: a chord such as Shift+Tab, or one key.
fn press_keys(keys: &[u16]) {
    let input = |virtual_key: u16, flags| INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: virtual_key,
                wScan: 0,
                dwFlags: flags,
                time: 0,
                dwExtraInfo: 0,
            },
        },
    };
    let inputs = keys
        .iter()
        .map(|key| input(*key, 0))
        .chain(keys.iter().rev().map(|key| input(*key, KEYEVENTF_KEYUP)))
        .collect::<Vec<_>>();
    assert_eq!(
        unsafe {
            SendInput(
                inputs.len() as u32,
                inputs.as_ptr(),
                std::mem::size_of::<INPUT>() as i32,
            )
        },
        inputs.len() as u32,
        "SendInput must queue every key event"
    );
}

/// Waits until the control `id` of `dialog` has the focus.
fn wait_for_dialog_focus(
    app: &MirxApp,
    dialog: HWND,
    id: i32,
) -> Result<(), Box<dyn std::error::Error>> {
    let control = unsafe { GetDlgItem(dialog, id) };
    assert!(!control.is_null(), "the dialog has the control {id}");
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let focused = unsafe { focused_hwnd(app) };
        if focused == control {
            return Ok(());
        }
        if Instant::now() >= deadline {
            return Err(format!(
                "the dialog's control {id} did not take the focus; {} has it",
                unsafe { GetDlgCtrlID(focused) }
            )
            .into());
        }
        std::thread::sleep(POLL_INTERVAL);
    }
}

#[test]
fn select_server_tab_moves_between_fields_and_esc_cancels() -> Result<(), Box<dyn std::error::Error>>
{
    let local_app_data = unique_local_app_data_path();
    let app = launch_automation_app(&local_app_data)?;
    app.open_dialog(AutomationDialog::SelectServer)?;
    let dialog = app.wait_for_modal_dialog(Duration::from_secs(5))?.hwnd as HWND;
    unsafe { SetForegroundWindow(dialog) };

    // The server field has the focus; Tab and Shift+Tab move through the
    // fields in order.
    wait_for_dialog_focus(&app, dialog, LOGIN_DIALOG_SERVER_ID)?;
    press_keys(&[VK_TAB]);
    wait_for_dialog_focus(&app, dialog, LOGIN_DIALOG_USER_ID)?;
    press_keys(&[VK_TAB]);
    wait_for_dialog_focus(&app, dialog, LOGIN_DIALOG_PASSWORD_ID)?;
    press_keys(&[VK_SHIFT, VK_TAB]);
    wait_for_dialog_focus(&app, dialog, LOGIN_DIALOG_USER_ID)?;

    press_keys(&[VK_ESCAPE]);
    wait_for_window_gone(dialog, "Select Server dialog")?;
    app.wait_idle(Duration::from_secs(5))?;
    assert!(app.dump_model()?.last_error.is_none());

    drop(app);
    let _ = fs::remove_dir_all(&local_app_data);
    Ok(())
}

#[test]
fn recover_e2ee_enter_submits() -> Result<(), Box<dyn std::error::Error>> {
    let exe_path =
        mirx_ui_exe_path().map_err(|error| format!("failed to locate mirx-ui.exe: {error}"))?;
    let fixture_path = fixture_state_path("e2ee-recovery-needed.json");
    let args = [
        OsString::from("--render-state"),
        fixture_path.as_os_str().to_os_string(),
    ];
    let app = launch_app(&exe_path, &args)?;
    app.wait_idle(Duration::from_secs(5))?;

    app.open_dialog(AutomationDialog::RecoverE2ee)?;
    let dialog = app.wait_for_modal_dialog(Duration::from_secs(5))?.hwnd as HWND;
    unsafe { SetForegroundWindow(dialog) };
    wait_for_dialog_focus(&app, dialog, RECOVER_E2EE_DIALOG_SECRET_ID)?;
    let secret = wide_null("sample recovery secret");
    assert_ne!(
        unsafe {
            SetWindowTextW(
                GetDlgItem(dialog, RECOVER_E2EE_DIALOG_SECRET_ID),
                secret.as_ptr(),
            )
        },
        0
    );
    press_keys(&[VK_RETURN]);
    wait_for_window_gone(dialog, "Recover E2EE dialog")?;
    app.wait_idle(Duration::from_secs(5))?;

    let model = app.dump_model()?;
    assert_eq!(model.status, "e2ee recovery submitted");
    assert_eq!(
        model.submitted_recovery_commands,
        vec![SubmittedRecoveryCommand {
            secret_redacted: true,
        }]
    );
    Ok(())
}

#[test]
fn commands_menu_join_records_submitted_membership_without_creating_room()
-> Result<(), Box<dyn std::error::Error>> {
    let exe_path =
        mirx_ui_exe_path().map_err(|error| format!("failed to locate mirx-ui.exe: {error}"))?;
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
    run_commands_menu_item(&app, COMMANDS_MENU_JOIN, Some("#ops:example.org"))?;

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
fn commands_menu_query_records_submitted_direct_room_without_creating_room()
-> Result<(), Box<dyn std::error::Error>> {
    let exe_path =
        mirx_ui_exe_path().map_err(|error| format!("failed to locate mirx-ui.exe: {error}"))?;
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
    run_commands_menu_item(&app, COMMANDS_MENU_QUERY, Some("@alice:example.org"))?;

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
    let exe_path =
        mirx_ui_exe_path().map_err(|error| format!("failed to locate mirx-ui.exe: {error}"))?;
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

    app.select_navigation_view(&room_identity("!invite:example"))?;
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
        app.selected_navigation_view()?,
        Some(room_identity("!invite:example"))
    );
    assert_eq!(
        find_switchbar_item(&invite_state, &room_identity("!invite:example")).title,
        "Lobby [invite]"
    );
    let invite_view = find_view(&invite_state, &room_identity("!invite:example"));
    assert_eq!(invite_view.title, "Lobby [invite]");
    assert!(view_contains(
        invite_view,
        "* You are invited to join Lobby"
    ));
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
    let exe_path =
        mirx_ui_exe_path().map_err(|error| format!("failed to locate mirx-ui.exe: {error}"))?;
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
    assert_eq!(
        after_topic.status,
        "* /topic: room is not joined: !invite:example"
    );
    assert!(after_topic.submitted_room_topic_commands.is_empty());

    app.set_compose_text("/reply $invite-root hello there")?;
    app.send_compose_key(ComposeKey::Return)?;
    app.wait_idle(Duration::from_secs(5))?;
    assert_eq!(app.compose_text()?, "");
    let after_reply = app.dump_model()?;
    assert_eq!(
        after_reply.status,
        "* /reply: room is not joined: !invite:example"
    );
    assert!(after_reply.submitted_reply_commands.is_empty());

    app.set_compose_text("/react $invite-root +1")?;
    app.send_compose_key(ComposeKey::Return)?;
    app.wait_idle(Duration::from_secs(5))?;
    assert_eq!(app.compose_text()?, "");
    let after_react = app.dump_model()?;
    assert_eq!(
        after_react.status,
        "* /react: room is not joined: !invite:example"
    );
    assert!(after_react.submitted_reaction_commands.is_empty());

    app.set_compose_text("/part")?;
    app.send_compose_key(ComposeKey::Return)?;
    app.wait_idle(Duration::from_secs(5))?;
    assert_eq!(app.compose_text()?, "");
    let after_part = app.dump_model()?;
    assert_eq!(
        after_part.status,
        "* /part: room is not joined: !invite:example"
    );
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
    let exe_path =
        mirx_ui_exe_path().map_err(|error| format!("failed to locate mirx-ui.exe: {error}"))?;
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
    assert!(view_contains(
        invite_view,
        "* You are invited to join Lobby"
    ));
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
    let exe_path =
        mirx_ui_exe_path().map_err(|error| format!("failed to locate mirx-ui.exe: {error}"))?;
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
    assert!(navigation_contains(&state.navigation, "Daria [invite]"));
    let invite_room = find_ui_room(&state, "!invite-direct:example");
    assert_eq!(invite_room.membership, RoomMembership::Invite);
    assert!(invite_room.is_direct);
    assert_eq!(
        invite_room.direct_user_ids,
        vec!["@daria:example".to_owned()]
    );
    assert_eq!(
        find_switchbar_item(&state, &room_identity("!invite-direct:example")).title,
        "Daria [invite]"
    );
    let invite_view = find_view(&state, &room_identity("!invite-direct:example"));
    assert!(invite_view.title.contains("Daria [invite]"));
    assert!(view_contains(
        invite_view,
        "* You are invited to join Daria"
    ));
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
    let exe_path =
        mirx_ui_exe_path().map_err(|error| format!("failed to locate mirx-ui.exe: {error}"))?;
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
    assert!(view_contains(
        joined_view,
        "* Now talking in Lobby (!invite:example)"
    ));
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
    let exe_path =
        mirx_ui_exe_path().map_err(|error| format!("failed to locate mirx-ui.exe: {error}"))?;
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
    let exe_path =
        mirx_ui_exe_path().map_err(|error| format!("failed to locate mirx-ui.exe: {error}"))?;
    let local_app_data = unique_local_app_data_path();
    let options = launch_options(Some(local_app_data.clone()));
    let args = [OsString::from("--automation")];

    let app = launch_app_with_options(&exe_path, &args, options)?;
    app.wait_for_lifecycle_state("core_started", Duration::from_secs(30))?;
    app.load_events(&serde_json::to_string(&hierarchical_invite_space_events())?)?;
    app.wait_idle(Duration::from_secs(5))?;

    let state = app.dump_ui_state()?;
    assert!(navigation_contains(&state.navigation, "Space"));
    assert!(navigation_contains(&state.navigation, "Beta"));
    assert_navigation_not_contains(&state.navigation, "Invite Space");

    drop(app);
    let _ = fs::remove_dir_all(&local_app_data);
    Ok(())
}

#[test]
fn automation_driver_slash_accept_keeps_invite_until_synced_rooms_join()
-> Result<(), Box<dyn std::error::Error>> {
    let exe_path =
        mirx_ui_exe_path().map_err(|error| format!("failed to locate mirx-ui.exe: {error}"))?;
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
        app.selected_navigation_view()?,
        Some(room_identity("!invite:example"))
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
    let exe_path =
        mirx_ui_exe_path().map_err(|error| format!("failed to locate mirx-ui.exe: {error}"))?;
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
        app.selected_navigation_view()?,
        Some(room_identity("!beta:example"))
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
    let exe_path =
        mirx_ui_exe_path().map_err(|error| format!("failed to locate mirx-ui.exe: {error}"))?;
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
        "* /accept: accept failed: ambiguous invited room target: Shared"
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
        "* /reject: reject failed: room is not invited: !beta:example"
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
        "* /accept: accept failed: invited room not found: Missing"
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
    assert_eq!(
        no_target.status,
        "* /reject: reject failed: no invited room selected"
    );
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
    let exe_path =
        mirx_ui_exe_path().map_err(|error| format!("failed to locate mirx-ui.exe: {error}"))?;
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
    let exe_path =
        mirx_ui_exe_path().map_err(|error| format!("failed to locate mirx-ui.exe: {error}"))?;
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
                notification_mode: Some(RoomNotificationMode::AllMessages),
                canonical_alias: None,
                alt_aliases: Vec::new(),
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
    let exe_path =
        mirx_ui_exe_path().map_err(|error| format!("failed to locate mirx-ui.exe: {error}"))?;
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
                notification_mode: Some(RoomNotificationMode::AllMessages),
                canonical_alias: None,
                alt_aliases: Vec::new(),
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
                notification_mode: Some(RoomNotificationMode::AllMessages),
                canonical_alias: None,
                alt_aliases: Vec::new(),
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
    assert_eq!(
        model.status,
        "* /query: ambiguous query target: @alice:example.org"
    );
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
    let exe_path =
        mirx_ui_exe_path().map_err(|error| format!("failed to locate mirx-ui.exe: {error}"))?;
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
    let exe_path =
        mirx_ui_exe_path().map_err(|error| format!("failed to locate mirx-ui.exe: {error}"))?;
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
    let exe_path =
        mirx_ui_exe_path().map_err(|error| format!("failed to locate mirx-ui.exe: {error}"))?;
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
    assert_eq!(
        model.status,
        "* /query: ambiguous query target: @alice:example.org"
    );
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
    let exe_path =
        mirx_ui_exe_path().map_err(|error| format!("failed to locate mirx-ui.exe: {error}"))?;
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
    let exe_path =
        mirx_ui_exe_path().map_err(|error| format!("failed to locate mirx-ui.exe: {error}"))?;
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
    assert_eq!(model.status, "* /part: no room selected");
    assert!(model.submitted_membership_commands.is_empty());
    assert_eq!(app.dump_ui_state()?.mdi.active, ViewIdentity::Status);

    drop(app);
    let _ = fs::remove_dir_all(&local_app_data);
    Ok(())
}

#[test]
fn automation_driver_slash_part_thread_targets_parent_and_removes_stale_thread()
-> Result<(), Box<dyn std::error::Error>> {
    let exe_path =
        mirx_ui_exe_path().map_err(|error| format!("failed to locate mirx-ui.exe: {error}"))?;
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
    let exe_path =
        mirx_ui_exe_path().map_err(|error| format!("failed to locate mirx-ui.exe: {error}"))?;
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
    assert!(!view_contains(beta_view, "* Topic is 'incident review'"));
    assert!(!view_contains(
        beta_view,
        "[22:16] * @alice:example changes topic to 'incident review'"
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
    assert!(view_contains(beta_view, "* Topic is 'incident review'"));
    assert!(view_contains(
        beta_view,
        "[22:16] * @alice:example changes topic to 'incident review'"
    ));

    drop(app);
    let _ = fs::remove_dir_all(&local_app_data);
    Ok(())
}

#[test]
fn slash_topic_shows_and_names_rooms_by_alias() -> Result<(), Box<dyn std::error::Error>> {
    let exe_path =
        mirx_ui_exe_path().map_err(|error| format!("failed to locate mirx-ui.exe: {error}"))?;
    let local_app_data = unique_local_app_data_path();
    let options = launch_options(Some(local_app_data.clone()));
    let app = launch_app_with_options(&exe_path, &[OsString::from("--automation")], options)?;
    app.wait_for_lifecycle_state("core_started", Duration::from_secs(30))?;
    let mut beta = room_summary("!beta:example", "Beta", true);
    beta.topic = Some("incident review".to_owned());
    beta.canonical_alias = Some("#beta:example".to_owned());
    let mut lobby = room_summary("!lobby:example", "Lobby", false);
    lobby.canonical_alias = Some("#lobby:example".to_owned());
    lobby.alt_aliases = vec!["#hall:example".to_owned()];
    app.load_events(&serde_json::to_string(&vec![
        Event::Started,
        Event::SyncedRooms(vec![beta, lobby]),
    ])?)?;
    app.wait_idle(Duration::from_secs(5))?;
    let beta = room_identity("!beta:example");
    app.select_view(beta.clone())?;

    type_commands(
        &app,
        &[
            "/topic",
            "/topic #hall",
            "/topic #lobby:example deploy at noon",
            "/topic #nope x",
            "/msg #lobby see ops",
        ],
    )?;

    let ui_state = app.dump_ui_state()?;
    assert_eq!(ui_state.mdi.active, beta);
    let view = find_view(&ui_state, &beta);
    // The window's own topic line, then /topic's.
    assert_eq!(
        view_lines_containing(view, "* Topic is 'incident review'").len(),
        2,
        "{:?}",
        view.lines
    );
    for line in [
        "] * No topic is set",
        "] * /topic: unknown room: #nope",
        "] -> *#lobby* see ops",
    ] {
        assert!(
            only_view_line_containing(view, line).ends_with(line),
            "{line:?} in {:?}",
            view.lines
        );
    }
    assert_eq!(
        app.dump_model()?.submitted_room_topic_commands,
        vec![SubmittedRoomTopicCommand {
            room_id: "!lobby:example".to_owned(),
            topic: "deploy at noon".to_owned(),
        }]
    );

    drop(app);
    let _ = fs::remove_dir_all(&local_app_data);
    Ok(())
}

#[test]
fn slash_ignore_round_trip() -> Result<(), Box<dyn std::error::Error>> {
    let exe_path =
        mirx_ui_exe_path().map_err(|error| format!("failed to locate mirx-ui.exe: {error}"))?;
    let local_app_data = unique_local_app_data_path();
    let options = launch_options(Some(local_app_data.clone()));
    let app = launch_app_with_options(&exe_path, &[OsString::from("--automation")], options)?;
    app.wait_for_lifecycle_state("core_started", Duration::from_secs(30))?;
    let mut events = base_events();
    events.insert(
        1,
        Event::LoggedIn {
            session: core_stored_session(),
            homeserver_url: "https://matrix.example/".to_owned(),
        },
    );
    events.push(room_members_event(
        "!beta:example",
        &[
            ("@alice:example", Some("Alice")),
            ("@bob:example", Some("Bob")),
        ],
    ));
    app.load_events(&serde_json::to_string(&events)?)?;
    app.wait_idle(Duration::from_secs(5))?;
    let beta = room_identity("!beta:example");
    app.select_view(beta.clone())?;

    type_commands(&app, &["/ignore"])?;
    app.append_synthetic_events(&serde_json::to_string(&vec![Event::IgnoredUsers(
        Vec::new(),
    )])?)?;
    app.wait_idle(Duration::from_secs(5))?;
    type_commands(&app, &["/ignore", "/ignore Bob", "/ignore Alice"])?;
    assert_eq!(
        app.dump_model()?.submitted_user_commands,
        vec![SubmittedUserCommand {
            action: UserCommandAction::Ignore,
            user_id: "@bob:example".to_owned(),
        }]
    );

    // The server's echo confirms the change.
    app.append_synthetic_events(&serde_json::to_string(&vec![Event::IgnoredUsers(vec![
        "@bob:example".to_owned(),
    ])])?)?;
    app.wait_idle(Duration::from_secs(5))?;
    assert_eq!(
        app.dump_ui_state()?.ignored_users,
        Some(vec!["@bob:example".to_owned()])
    );
    type_commands(&app, &["/ignore", "/ignore Bob", "/unignore Bob"])?;
    assert_eq!(
        app.dump_model()?.submitted_user_commands[1..],
        [SubmittedUserCommand {
            action: UserCommandAction::Unignore,
            user_id: "@bob:example".to_owned(),
        }]
    );

    let ui_state = app.dump_ui_state()?;
    let view = find_view(&ui_state, &beta);
    for line in [
        "] * /ignore: the ignore list has not been received yet",
        "] * Ignore list is empty",
        "] * /ignore: you cannot ignore yourself",
        "] * Added Bob (@bob:example) to ignore list",
        "] * Ignore list:",
        "] *   Bob (@bob:example)",
        "] * /ignore: Bob is already ignored",
    ] {
        assert!(
            only_view_line_containing(view, line).ends_with(line),
            "{line:?} in {:?}",
            view.lines
        );
    }

    drop(app);
    let _ = fs::remove_dir_all(&local_app_data);
    Ok(())
}

#[test]
fn slash_whois_prints_server_profile() -> Result<(), Box<dyn std::error::Error>> {
    let exe_path =
        mirx_ui_exe_path().map_err(|error| format!("failed to locate mirx-ui.exe: {error}"))?;
    let local_app_data = unique_local_app_data_path();
    let options = launch_options(Some(local_app_data.clone()));
    let app = launch_app_with_options(&exe_path, &[OsString::from("--automation")], options)?;
    app.wait_for_lifecycle_state("core_started", Duration::from_secs(30))?;
    app.load_events(&serde_json::to_string(&base_events())?)?;
    app.wait_idle(Duration::from_secs(5))?;
    let beta = room_identity("!beta:example");
    app.select_view(beta.clone())?;

    // Not a member of any known room: only the server knows them.
    type_commands(&app, &["/whois @stranger:remote"])?;
    // The automation core, which is not signed in, refuses it with an
    // error of its own; the UserProfile below stands in for the answer.
    assert_eq!(
        app.dump_model()?.submitted_user_commands,
        vec![SubmittedUserCommand {
            action: UserCommandAction::FetchProfile,
            user_id: "@stranger:remote".to_owned(),
        }]
    );

    app.append_synthetic_events(&serde_json::to_string(&vec![
        Event::UserProfile(UserProfile {
            user_id: "@stranger:remote".to_owned(),
            display_name: Some("Stranger".to_owned()),
        }),
        Event::Error(mirx_core::CoreErrorEvent {
            operation: mirx_core::Operation::FetchUserProfile,
            room_id: None,
            message: "no such user @nobody:remote".to_owned(),
        }),
    ])?)?;
    app.wait_idle(Duration::from_secs(5))?;

    let ui_state = app.dump_ui_state()?;
    let view = find_view(&ui_state, &beta);
    for line in [
        "] * Stranger is @stranger:remote * Stranger",
        "] * Stranger using remote",
        "] * Stranger End of /WHOIS list",
        "] * /whois: no such user @nobody:remote",
    ] {
        assert!(
            only_view_line_containing(view, line).ends_with(line),
            "{line:?} in {:?}",
            view.lines
        );
    }
    assert!(!view_contains(
        find_view(&ui_state, &ViewIdentity::Status),
        "Stranger"
    ));

    drop(app);
    let _ = fs::remove_dir_all(&local_app_data);
    Ok(())
}

#[test]
fn room_error_prints_in_room_window() -> Result<(), Box<dyn std::error::Error>> {
    let exe_path =
        mirx_ui_exe_path().map_err(|error| format!("failed to locate mirx-ui.exe: {error}"))?;
    let local_app_data = unique_local_app_data_path();
    let options = launch_options(Some(local_app_data.clone()));
    let app = launch_app_with_options(&exe_path, &[OsString::from("--automation")], options)?;
    app.wait_for_lifecycle_state("core_started", Duration::from_secs(30))?;
    app.load_events(&serde_json::to_string(&base_events())?)?;
    app.wait_idle(Duration::from_secs(5))?;
    let beta = room_identity("!beta:example");
    let alpha = room_identity("!alpha:example");
    app.select_view(beta.clone())?;

    app.append_synthetic_events(&serde_json::to_string(&vec![Event::Error(
        mirx_core::CoreErrorEvent {
            operation: mirx_core::Operation::SetRoomMemberPowerLevel,
            room_id: Some("!alpha:example".to_owned()),
            message: "not allowed".to_owned(),
        },
    )])?)?;
    app.wait_idle(Duration::from_secs(5))?;

    let line = "* Error during set room member power level: not allowed";
    let ui_state = app.dump_ui_state()?;
    assert_eq!(ui_state.mdi.active, beta);
    assert!(
        only_view_line_containing(find_view(&ui_state, &alpha), line).ends_with(line),
        "{:?}",
        find_view(&ui_state, &alpha).lines
    );
    assert!(!view_contains(find_view(&ui_state, &beta), line));
    assert!(!view_contains(
        find_view(&ui_state, &ViewIdentity::Status),
        line
    ));
    assert_eq!(
        app.dump_model()?.status,
        "error during set room member power level"
    );

    drop(app);
    let _ = fs::remove_dir_all(&local_app_data);
    Ok(())
}

#[test]
fn automation_driver_renders_synced_topic_without_topic_notice()
-> Result<(), Box<dyn std::error::Error>> {
    let exe_path =
        mirx_ui_exe_path().map_err(|error| format!("failed to locate mirx-ui.exe: {error}"))?;
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
    assert!(view_contains(beta_view, "* Topic is 'incident review'"));
    assert!(!view_contains(
        beta_view,
        "changes topic to 'incident review'"
    ));

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
    assert!(view_contains(beta_view, "* Topic is 'deployment window'"));
    assert!(view_contains(
        beta_view,
        "changes topic to 'deployment window'"
    ));
    assert!(!view_contains(beta_view, "* Topic is 'incident review'"));

    drop(app);
    let _ = fs::remove_dir_all(&local_app_data);
    Ok(())
}

#[test]
fn automation_driver_slash_react_records_submitted_reaction_without_appending_text()
-> Result<(), Box<dyn std::error::Error>> {
    let exe_path =
        mirx_ui_exe_path().map_err(|error| format!("failed to locate mirx-ui.exe: {error}"))?;
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
    let exe_path =
        mirx_ui_exe_path().map_err(|error| format!("failed to locate mirx-ui.exe: {error}"))?;
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
    let exe_path =
        mirx_ui_exe_path().map_err(|error| format!("failed to locate mirx-ui.exe: {error}"))?;
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
            sender_user_id: "@normal:example".to_owned(),
            body: "thanks for the detail".to_owned(),
            styles: Vec::new(),
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
    let exe_path =
        mirx_ui_exe_path().map_err(|error| format!("failed to locate mirx-ui.exe: {error}"))?;
    let local_app_data = unique_local_app_data_path();
    let options = launch_options(Some(local_app_data.clone()));
    let args = [OsString::from("--automation")];

    let app = launch_app_with_options(&exe_path, &args, options)?;
    app.wait_for_lifecycle_state("core_started", Duration::from_secs(30))?;
    app.load_events(&reactions_fixture_signed_in_as("@normal:example")?)?;
    app.wait_idle(Duration::from_secs(5))?;

    let before = app.room_activity(Some("!reactions:example"))?;
    let before_model = app.dump_model()?;
    assert!(before_model.submitted_edit_commands.is_empty());
    assert!(before_model.submitted_reaction_commands.is_empty());
    assert!(before_model.submitted_redaction_commands.is_empty());

    app.set_compose_text("/edit $react-normal replacement text")?;
    app.send_compose_key(ComposeKey::Return)?;
    app.wait_idle(Duration::from_secs(5))?;
    assert_eq!(app.compose_text()?, "");

    let model = app.dump_model()?;
    assert_eq!(
        model.submitted_edit_commands,
        vec![SubmittedEditCommand {
            room_id: "!reactions:example".to_owned(),
            target_event_id: "$react-normal".to_owned(),
            body: "replacement text".to_owned(),
            styles: Vec::new(),
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
    let exe_path =
        mirx_ui_exe_path().map_err(|error| format!("failed to locate mirx-ui.exe: {error}"))?;
    let fixture_path = fixture_state_path("reactions.json");
    let local_app_data = unique_local_app_data_path();
    let options = launch_options(Some(local_app_data.clone()));
    let args = [OsString::from("--automation")];

    let app = launch_app_with_options(&exe_path, &args, options)?;
    app.wait_for_lifecycle_state("core_started", Duration::from_secs(30))?;
    app.load_events(&fs::read_to_string(&fixture_path)?)?;
    app.wait_idle(Duration::from_secs(5))?;

    let before = app.room_activity(Some("!reactions:example"))?;

    app.set_compose_text("/reply")?;
    app.send_compose_key(ComposeKey::Return)?;
    app.wait_idle(Duration::from_secs(5))?;
    assert_eq!(app.compose_text()?, "");

    let model = app.dump_model()?;
    assert_eq!(model.status, "* /reply: insufficient parameters");
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
            .contains("* /reply: insufficient parameters")
    );

    drop(app);
    let _ = fs::remove_dir_all(&local_app_data);
    Ok(())
}

#[test]
fn automation_driver_slash_redact_records_submitted_redaction_without_appending_text()
-> Result<(), Box<dyn std::error::Error>> {
    let exe_path =
        mirx_ui_exe_path().map_err(|error| format!("failed to locate mirx-ui.exe: {error}"))?;
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
    let exe_path =
        mirx_ui_exe_path().map_err(|error| format!("failed to locate mirx-ui.exe: {error}"))?;
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
    let exe_path =
        mirx_ui_exe_path().map_err(|error| format!("failed to locate mirx-ui.exe: {error}"))?;
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
    assert_eq!(model.status, "* /redact: insufficient parameters");
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
            .contains("* /redact: insufficient parameters")
    );

    drop(app);
    let _ = fs::remove_dir_all(&local_app_data);
    Ok(())
}

#[test]
fn automation_driver_invalid_react_reports_usage_without_recording_commands()
-> Result<(), Box<dyn std::error::Error>> {
    let exe_path =
        mirx_ui_exe_path().map_err(|error| format!("failed to locate mirx-ui.exe: {error}"))?;
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
    assert_eq!(model.status, "* /react: insufficient parameters");
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
            .contains("* /react: insufficient parameters")
    );

    drop(app);
    let _ = fs::remove_dir_all(&local_app_data);
    Ok(())
}

#[test]
fn automation_driver_invalid_edit_reports_usage_without_recording_commands()
-> Result<(), Box<dyn std::error::Error>> {
    let exe_path =
        mirx_ui_exe_path().map_err(|error| format!("failed to locate mirx-ui.exe: {error}"))?;
    let fixture_path = fixture_state_path("reactions.json");
    let local_app_data = unique_local_app_data_path();
    let options = launch_options(Some(local_app_data.clone()));
    let args = [OsString::from("--automation")];

    let app = launch_app_with_options(&exe_path, &args, options)?;
    app.wait_for_lifecycle_state("core_started", Duration::from_secs(30))?;
    app.load_events(&fs::read_to_string(&fixture_path)?)?;
    app.wait_idle(Duration::from_secs(5))?;

    let before = app.room_activity(Some("!reactions:example"))?;

    app.set_compose_text("/edit")?;
    app.send_compose_key(ComposeKey::Return)?;
    app.wait_idle(Duration::from_secs(5))?;
    assert_eq!(app.compose_text()?, "");

    let model = app.dump_model()?;
    assert_eq!(model.status, "* /edit: insufficient parameters");
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
            .contains("* /edit: insufficient parameters")
    );

    app.set_compose_text("/edit $event-to-edit")?;
    app.send_compose_key(ComposeKey::Return)?;
    app.wait_idle(Duration::from_secs(5))?;
    assert_eq!(app.compose_text()?, "");
    let model = app.dump_model()?;
    assert_eq!(model.status, "* /edit: no such message in this window");
    assert!(model.submitted_edit_commands.is_empty());
    assert_eq!(app.dump_ui_state()?.compose_target, None);

    drop(app);
    let _ = fs::remove_dir_all(&local_app_data);
    Ok(())
}

#[test]
fn automation_driver_literal_react_submission_is_sent_as_text()
-> Result<(), Box<dyn std::error::Error>> {
    let exe_path =
        mirx_ui_exe_path().map_err(|error| format!("failed to locate mirx-ui.exe: {error}"))?;
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
    let exe_path =
        mirx_ui_exe_path().map_err(|error| format!("failed to locate mirx-ui.exe: {error}"))?;
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
    let exe_path =
        mirx_ui_exe_path().map_err(|error| format!("failed to locate mirx-ui.exe: {error}"))?;
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
    let exe_path =
        mirx_ui_exe_path().map_err(|error| format!("failed to locate mirx-ui.exe: {error}"))?;
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
    let exe_path =
        mirx_ui_exe_path().map_err(|error| format!("failed to locate mirx-ui.exe: {error}"))?;
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
            sender_user_id: "@reply:example".to_owned(),
            body: "threaded acknowledgement".to_owned(),
            styles: Vec::new(),
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
    let exe_path =
        mirx_ui_exe_path().map_err(|error| format!("failed to locate mirx-ui.exe: {error}"))?;
    let local_app_data = unique_local_app_data_path();
    let options = launch_options(Some(local_app_data.clone()));
    let args = [OsString::from("--automation")];

    let app = launch_app_with_options(&exe_path, &args, options)?;
    app.wait_for_lifecycle_state("core_started", Duration::from_secs(30))?;
    app.load_events(&reactions_fixture_signed_in_as("@reply:example")?)?;
    app.wait_idle(Duration::from_secs(5))?;

    let thread_view_identity = thread_identity("!reactions:example", "$react-thread-root");
    let before = app.room_activity(Some("!reactions:example"))?;
    app.select_view(thread_view_identity.clone())?;

    app.set_compose_text("/edit $react-thread-reply replacement text")?;
    app.send_compose_key(ComposeKey::Return)?;
    app.wait_idle(Duration::from_secs(5))?;
    assert_eq!(app.compose_text()?, "");

    let model = app.dump_model()?;
    assert_eq!(
        model.submitted_edit_commands,
        vec![SubmittedEditCommand {
            room_id: "!reactions:example".to_owned(),
            target_event_id: "$react-thread-reply".to_owned(),
            body: "replacement text".to_owned(),
            styles: Vec::new(),
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
    let exe_path =
        mirx_ui_exe_path().map_err(|error| format!("failed to locate mirx-ui.exe: {error}"))?;
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
    let exe_path =
        mirx_ui_exe_path().map_err(|error| format!("failed to locate mirx-ui.exe: {error}"))?;
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
fn room_menu_notifications_check_follows_summary_and_submits()
-> Result<(), Box<dyn std::error::Error>> {
    let exe_path =
        mirx_ui_exe_path().map_err(|error| format!("failed to locate mirx-ui.exe: {error}"))?;
    let local_app_data = unique_local_app_data_path();
    let options = launch_options(Some(local_app_data.clone()));
    let args = [OsString::from("--automation")];
    let quiet = |mode| RoomSummary {
        notification_mode: Some(mode),
        ..room_summary("!quiet:example", "Quiet", false)
    };
    let rooms = |mode| {
        Event::SyncedRooms(vec![
            room_summary("!beta:example", "Beta", false),
            quiet(mode),
            invited_room_summary("!invite:example", "Lobby", false),
        ])
    };

    let app = launch_app_with_options(&exe_path, &args, options)?;
    app.wait_for_lifecycle_state("core_started", Duration::from_secs(30))?;
    app.load_events(&serde_json::to_string(&vec![
        Event::Started,
        rooms(RoomNotificationMode::MentionsAndKeywordsOnly),
    ])?)?;
    app.wait_idle(Duration::from_secs(5))?;
    app.select_room("!quiet:example")?;

    let menu = app.dump_room_menu_state()?;
    assert_eq!(
        menu.notification_mode,
        Some(RoomNotificationMode::MentionsAndKeywordsOnly)
    );
    assert!(menu.notification_items_enabled);

    app.open_selected_navigation_context_menu(ContextMenuCommand::NotifyMute)?;
    app.wait_idle(Duration::from_secs(5))?;
    assert_eq!(
        app.dump_model()?.submitted_notification_commands,
        vec![SubmittedNotificationCommand::SetRoomNotificationMode {
            room_id: "!quiet:example".to_owned(),
            mode: RoomNotificationMode::Mute,
        }]
    );
    // The check waits for the server's echo.
    assert_eq!(
        app.dump_room_menu_state()?.notification_mode,
        Some(RoomNotificationMode::MentionsAndKeywordsOnly)
    );

    app.append_synthetic_events(&serde_json::to_string(&vec![rooms(
        RoomNotificationMode::Mute,
    )])?)?;
    app.wait_idle(Duration::from_secs(5))?;
    assert_eq!(
        app.dump_room_menu_state()?.notification_mode,
        Some(RoomNotificationMode::Mute)
    );

    app.select_room("!invite:example")?;
    let menu = app.dump_room_menu_state()?;
    assert_eq!(menu.notification_mode, None);
    assert!(!menu.notification_items_enabled);
    let error = app
        .open_selected_navigation_context_menu(ContextMenuCommand::NotifyAllMessages)
        .expect_err("Notifications is greyed for an invite");
    assert!(matches!(
        error,
        TestkitError::Protocol { message, .. }
            if message == "room context menu action is disabled"
    ));
    assert_eq!(app.dump_model()?.submitted_notification_commands.len(), 1);

    drop(app);
    let _ = fs::remove_dir_all(&local_app_data);
    Ok(())
}

#[test]
fn automation_driver_room_menu_part_waits_for_synced_rooms_removal()
-> Result<(), Box<dyn std::error::Error>> {
    let exe_path =
        mirx_ui_exe_path().map_err(|error| format!("failed to locate mirx-ui.exe: {error}"))?;
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
    let exe_path =
        mirx_ui_exe_path().map_err(|error| format!("failed to locate mirx-ui.exe: {error}"))?;
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
    let exe_path =
        mirx_ui_exe_path().map_err(|error| format!("failed to locate mirx-ui.exe: {error}"))?;
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
    assert!(
        !enabled.edit_enabled,
        "the fixture has no signed-in user, so no message is theirs to edit"
    );
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
    assert_eq!(app.compose_text()?, "");
    assert_eq!(
        app.dump_ui_state()?.compose_target,
        Some(UiComposeTarget {
            kind: UiComposeTargetKind::Reply,
            event_id: "$react-normal".to_owned(),
        })
    );
    let model = app.dump_model()?;
    assert!(model.submitted_reply_commands.is_empty());
    assert!(model.submitted_edit_commands.is_empty());
    assert!(model.submitted_reaction_commands.is_empty());
    assert!(model.submitted_redaction_commands.is_empty());

    let error = app
        .edit_selected_message_via_menu()
        .expect_err("edit must reject someone else's message");
    assert!(matches!(
        error,
        TestkitError::Protocol { code, message, .. }
            if code == "selected_message_unavailable"
                && message == "you can only edit your own messages"
    ));
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
    let exe_path =
        mirx_ui_exe_path().map_err(|error| format!("failed to locate mirx-ui.exe: {error}"))?;
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
    assert_eq!(app.compose_text()?, "");
    assert_eq!(
        app.dump_ui_state()?.compose_target,
        Some(UiComposeTarget {
            kind: UiComposeTargetKind::Reply,
            event_id: "$react-normal".to_owned(),
        })
    );
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
    let exe_path =
        mirx_ui_exe_path().map_err(|error| format!("failed to locate mirx-ui.exe: {error}"))?;
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
    app.open_selected_timeline_context_menu(ContextMenuCommand::ReplyMessage)?;
    assert_eq!(
        app.dump_ui_state()?.compose_target,
        Some(UiComposeTarget {
            kind: UiComposeTargetKind::Reply,
            event_id: "$react-normal".to_owned(),
        })
    );
    let error = app
        .open_selected_timeline_context_menu(ContextMenuCommand::EditMessage)
        .expect_err("edit is greyed out on someone else's message");
    assert!(matches!(
        error,
        TestkitError::Protocol { code, message, .. }
            if code == "selected_message_unavailable"
                && message == "message context menu action is disabled"
    ));
    drop(app);
    let _ = fs::remove_dir_all(&local_app_data);
    Ok(())
}

/// The replies fixture in a frame low enough that its room scrolls.
fn launch_replies_app() -> Result<(GuiTestApp, PathBuf), Box<dyn std::error::Error>> {
    let exe_path =
        mirx_ui_exe_path().map_err(|error| format!("failed to locate mirx-ui.exe: {error}"))?;
    let local_app_data = unique_local_app_data_path();
    let app = launch_app_with_options(
        &exe_path,
        &[OsString::from("--automation")],
        LaunchOptions {
            frame_size: Some(FrameSize {
                width: 800,
                height: 300,
            }),
            ..launch_options(Some(local_app_data.clone()))
        },
    )?;
    app.wait_for_lifecycle_state("core_started", Duration::from_secs(30))?;
    app.load_events(&fs::read_to_string(fixture_state_path("replies.json"))?)?;
    app.wait_idle(Duration::from_secs(5))?;
    app.select_room("!replies:example")?;
    // At the end, where the replies are.
    unsafe {
        SendMessageW(
            app.active_timeline_hwnd()? as HWND,
            WM_VSCROLL,
            SB_BOTTOM as WPARAM,
            0,
        )
    };
    Ok((app, local_app_data))
}

fn active_timeline(app: &MirxApp) -> Result<UiActiveTimeline, Box<dyn std::error::Error>> {
    Ok(app
        .dump_ui_state()?
        .active_timeline
        .expect("a room window has a timeline"))
}

/// Double-clicks the quote line above the reply `reply_event_id`.
fn double_click_quote_of(
    app: &MirxApp,
    reply_event_id: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let timeline = active_timeline(app)?;
    let quote = timeline
        .visible_lines
        .iter()
        .find(|line| {
            line.kind == UiTimelineLineKind::ReplyQuote
                && line.event_id.as_deref() == Some(reply_event_id)
        })
        .unwrap_or_else(|| panic!("the quote of {reply_event_id} is on screen: {timeline:?}"));
    // The jump opens nothing modal, so the click can be sent.
    send_left_click(
        app.active_timeline_hwnd()? as HWND,
        &quote.click_point,
        true,
    );
    app.wait_idle(Duration::from_secs(5))?;
    Ok(())
}

fn launch_render_state(fixture: &str) -> Result<GuiTestApp, Box<dyn std::error::Error>> {
    let exe_path =
        mirx_ui_exe_path().map_err(|error| format!("failed to locate mirx-ui.exe: {error}"))?;
    let fixture_path = fixture_state_path(fixture);
    let app = launch_app(
        &exe_path,
        &[
            OsString::from("--render-state"),
            fixture_path.as_os_str().to_os_string(),
        ],
    )?;
    app.wait_idle(Duration::from_secs(5))?;
    Ok(app)
}

/// The client point of the boundary before byte `offset` of the visible
/// line whose text contains `needle`, in the fixed-pitch text font.
fn text_point(
    app: &MirxApp,
    needle: &str,
    offset_in_needle: usize,
) -> Result<UiPoint, Box<dyn std::error::Error>> {
    let ui_state = app.dump_ui_state()?;
    assert_eq!(
        ui_state.text_font.face, "Fixedsys",
        "points are computed for a fixed-pitch face"
    );
    let width = ui_state.text_font.average_char_width;
    let timeline = ui_state.active_timeline.ok_or("a timeline is active")?;
    let line = timeline
        .visible_lines
        .iter()
        .find(|line| line.text.contains(needle))
        .ok_or_else(|| format!("{needle:?} is on screen: {:?}", timeline.visible_lines))?;
    let offset = line.text.find(needle).expect("found above") + offset_in_needle;
    assert!(line.text[..offset].is_ascii(), "ASCII before the point");
    Ok(UiPoint {
        x: line.text_left + offset as i32 * width + width / 4,
        y: line.click_point.y,
    })
}

/// Sends a mouse message to `hwnd` and waits for it to be handled.
fn send_mouse(hwnd: HWND, message: u32, buttons: u32, point: &UiPoint) {
    let mut result = 0;
    assert_ne!(
        unsafe {
            SendMessageTimeoutW(
                hwnd,
                message,
                buttons as WPARAM,
                mouse_lparam(point),
                SMTO_NORMAL,
                5_000,
                &mut result,
            )
        },
        0,
        "the timeline must handle the mouse without blocking"
    );
}

/// Presses at `from`, moves to `to` and releases there, with `release`
/// the button-up's key flags.
fn drag_text(hwnd: HWND, from: &UiPoint, to: &UiPoint, release: u32) {
    send_mouse(hwnd, WM_LBUTTONDOWN, MK_LBUTTON, from);
    send_mouse(hwnd, WM_MOUSEMOVE, MK_LBUTTON, to);
    send_mouse(hwnd, WM_LBUTTONUP, release, to);
}

fn clipboard_text() -> Result<String, String> {
    unsafe {
        if OpenClipboard(null_mut()) == 0 {
            return Err(format!(
                "OpenClipboard failed: {}",
                std::io::Error::last_os_error()
            ));
        }
        let memory = GetClipboardData(CF_UNICODETEXT);
        let text = if memory.is_null() {
            Err("the clipboard holds no text".to_owned())
        } else {
            let locked = GlobalLock(memory).cast::<u16>();
            let len = (0..).take_while(|&index| *locked.add(index) != 0).count();
            let text = String::from_utf16_lossy(std::slice::from_raw_parts(locked, len));
            GlobalUnlock(memory);
            Ok(text)
        };
        CloseClipboard();
        text
    }
}

#[test]
fn dragging_copies_the_marked_text_on_release() -> Result<(), Box<dyn std::error::Error>> {
    let app = launch_render_state("hotlinks.json")?;
    let timeline = app.active_timeline_hwnd()? as HWND;
    // From "spec" in Alice's wrapped message into Bob's next one.
    let from = text_point(&app, "the spec is", 4)?;
    let to = text_point(&app, "<Bob> mirror", "<Bob> mirror".len())?;
    drag_text(timeline, &from, &to, 0);
    assert_eq!(
        clipboard_text()?,
        "spec is at https://spec.matrix.org/latest/ for anyone asking\r\n[22:14] <Bob> mirror"
    );
    // The press selected its message, as a click does.
    assert_eq!(
        app.dump_message_menu_state()?.selected_event_id.as_deref(),
        Some("$links-0")
    );

    // Upwards and from inside a line works the same.
    let from = text_point(&app, "anyone asking", "anyone".len())?;
    let to = text_point(&app, "the spec is", 0)?;
    drag_text(timeline, &from, &to, 0);
    assert_eq!(
        clipboard_text()?,
        "the spec is at https://spec.matrix.org/latest/ for anyone"
    );
    Ok(())
}

#[test]
fn dragging_with_ctrl_copies_control_codes() -> Result<(), Box<dyn std::error::Error>> {
    let app = launch_render_state("rich-text.json")?;
    let timeline = app.active_timeline_hwnd()? as HWND;
    let from = text_point(&app, "<Alice> bold italic", "<Alice> ".len())?;
    let to = text_point(&app, "<Alice> bold italic", "<Alice> bold italic".len())?;
    drag_text(timeline, &from, &to, MK_CONTROL);
    assert_eq!(clipboard_text()?, "\x02bold\x0f \x1ditalic");
    // Without Ctrl, the text alone.
    drag_text(timeline, &from, &to, 0);
    assert_eq!(clipboard_text()?, "bold italic");
    Ok(())
}

#[test]
fn a_click_without_a_drag_copies_nothing() -> Result<(), Box<dyn std::error::Error>> {
    let app = launch_render_state("hotlinks.json")?;
    let timeline = app.active_timeline_hwnd()? as HWND;
    set_clipboard_text(null_mut(), "sentinel")?;
    let point = text_point(&app, "<Bob> mirror", 2)?;
    send_left_click(timeline, &point, false);
    // A wobble inside the drag box is still a click.
    send_mouse(timeline, WM_LBUTTONDOWN, MK_LBUTTON, &point);
    send_mouse(
        timeline,
        WM_MOUSEMOVE,
        MK_LBUTTON,
        &UiPoint {
            x: point.x + 1,
            y: point.y,
        },
    );
    send_mouse(timeline, WM_LBUTTONUP, 0, &point);
    assert_eq!(clipboard_text()?, "sentinel");
    assert_eq!(
        app.dump_message_menu_state()?.selected_event_id.as_deref(),
        Some("$links-1")
    );
    Ok(())
}

#[test]
fn new_lines_during_a_drag_keep_the_mark() -> Result<(), Box<dyn std::error::Error>> {
    let app = launch_render_state("hotlinks.json")?;
    let timeline = app.active_timeline_hwnd()? as HWND;
    let from = text_point(&app, "<Bob> mirror", "<Bob> ".len())?;
    let to = text_point(&app, "<Carol> see docs", "<Carol> see".len())?;

    send_mouse(timeline, WM_LBUTTONDOWN, MK_LBUTTON, &from);
    app.append_synthetic_events(&serde_json::to_string(&vec![text_event_with_event_id(
        "!links:example",
        "$late",
        "@alice:example",
        1_700_000_600_000,
        "a late line",
    )])?)?;
    app.wait_for_room_activity_contains(
        Some("!links:example"),
        "a late line",
        Duration::from_secs(5),
    )?;
    send_mouse(timeline, WM_MOUSEMOVE, MK_LBUTTON, &to);
    send_mouse(timeline, WM_LBUTTONUP, 0, &to);
    assert_eq!(
        clipboard_text()?,
        "mirror on www.example.org.\r\n[22:15] <Carol> see"
    );

    // An edit of the row the mark starts in drops the mark.
    set_clipboard_text(null_mut(), "sentinel")?;
    send_mouse(timeline, WM_LBUTTONDOWN, MK_LBUTTON, &from);
    app.append_synthetic_events(&serde_json::to_string(&vec![
        text_message_replaced_update_event(
            "!links:example",
            "$links-1",
            "$links-1-edit",
            "@bob:example",
            1_700_000_700_000,
            "mirror moved",
        ),
    ])?)?;
    app.wait_for_room_activity_contains(
        Some("!links:example"),
        "mirror moved",
        Duration::from_secs(5),
    )?;
    send_mouse(timeline, WM_MOUSEMOVE, MK_LBUTTON, &to);
    send_mouse(timeline, WM_LBUTTONUP, 0, &to);
    assert_eq!(clipboard_text()?, "sentinel");
    Ok(())
}

fn link(start: usize, end: usize, url: &str) -> UiTimelineLink {
    UiTimelineLink {
        start,
        end,
        url: url.to_owned(),
    }
}

#[test]
fn timeline_dump_lists_links() -> Result<(), Box<dyn std::error::Error>> {
    let app = launch_render_state("hotlinks.json")?;
    let timeline = app
        .dump_ui_state()?
        .active_timeline
        .ok_or("a timeline is active")?;
    let links_of = |needle: &str| {
        let line = timeline
            .visible_lines
            .iter()
            .find(|line| line.text.contains(needle))
            .unwrap_or_else(|| panic!("{needle:?} is on screen"));
        line.links
            .iter()
            .map(|link| (line.text[link.start..link.end].to_owned(), link.url.clone()))
            .collect::<Vec<_>>()
    };
    let pair = |shown: &str, url: &str| (shown.to_owned(), url.to_owned());
    assert_eq!(
        links_of("<Alice> the spec"),
        vec![pair(
            "https://spec.matrix.org/latest/",
            "https://spec.matrix.org/latest/"
        )]
    );
    assert_eq!(
        links_of("<Bob> mirror"),
        vec![pair("www.example.org", "www.example.org")]
    );
    assert_eq!(
        links_of("<Carol> see docs"),
        vec![pair("https://example.org/docs", "https://example.org/docs")]
    );
    // The wrapped address is in both of its lines, whole in each `url`.
    let long = "https://example.org/a/very/long/path/that/wraps/over/the/edge/of/the/window/to/the/next/line/index.html";
    let halves = timeline
        .visible_lines
        .iter()
        .flat_map(|line| {
            line.links
                .iter()
                .filter(|link| link.url == long)
                .map(|link| line.text[link.start..link.end].to_owned())
        })
        .collect::<Vec<_>>();
    assert!(halves.len() >= 2, "{halves:?}");
    assert_eq!(halves.concat(), long);
    assert_eq!(links_of("C# is not a link"), vec![]);
    // Offsets are in the line's text.
    let line = timeline
        .visible_lines
        .iter()
        .find(|line| line.text.contains("<Bob> mirror"))
        .expect("checked above");
    let start = line.text.find("www.").expect("on the line");
    assert_eq!(
        line.links,
        vec![link(
            start,
            start + "www.example.org".len(),
            "www.example.org"
        )]
    );
    Ok(())
}

/// Double-clicks the first character of `needle` on screen.
fn double_click_text(app: &MirxApp, needle: &str) -> Result<(), Box<dyn std::error::Error>> {
    let point = text_point(app, needle, 0)?;
    send_left_click(app.active_timeline_hwnd()? as HWND, &point, true);
    app.wait_idle(Duration::from_secs(5))?;
    Ok(())
}

#[test]
fn double_clicking_a_web_link_hands_it_to_the_shell() -> Result<(), Box<dyn std::error::Error>> {
    let app = launch_render_state("hotlinks.json")?;
    double_click_text(&app, "https://spec.matrix.org")?;
    double_click_text(&app, "www.example.org")?;
    assert_eq!(
        app.dump_model()?.opened_links,
        vec![
            "https://spec.matrix.org/latest/".to_owned(),
            "http://www.example.org".to_owned()
        ]
    );
    // winebrowser is disabled under test: the shell fails, and says so in
    // the window, without a box of its own.
    let ui_state = app.dump_ui_state()?;
    let view = find_view(&ui_state, &room_identity("!links:example"));
    for url in ["https://spec.matrix.org/latest/", "http://www.example.org"] {
        assert!(
            view_contains(
                view,
                &format!("* Unable to open {url}: ShellExecuteExW failed: ")
            ),
            "{:?}",
            view.lines
        );
    }
    assert!(
        ui_state
            .status_bar
            .text
            .starts_with("* Unable to open http://www.example.org: ")
    );
    assert!(
        app.wait_for_modal_dialog(Duration::from_millis(500))
            .is_err()
    );
    // A double-click on a link does not go anywhere else.
    assert_eq!(ui_state.mdi.active, room_identity("!links:example"));
    Ok(())
}

#[test]
fn double_clicking_a_room_link_activates_or_joins() -> Result<(), Box<dyn std::error::Error>> {
    let app = launch_render_state("hotlinks.json")?;
    // The Rust room is joined under the alias: its window comes forward.
    double_click_text(&app, "#rust:matrix.org,")?;
    assert_eq!(
        app.dump_ui_state()?.mdi.active,
        room_identity("!rust:example")
    );
    app.select_room("!links:example")?;
    double_click_text(&app, "https://matrix.to/#/%23rust")?;
    assert_eq!(
        app.dump_ui_state()?.mdi.active,
        room_identity("!rust:example")
    );
    app.select_room("!links:example")?;
    let model = app.dump_model()?;
    assert!(model.submitted_membership_commands.is_empty());

    // Any other room is joined.
    double_click_text(&app, "https://matrix.to/#/%23elsewhere")?;
    let model = app.dump_model()?;
    assert_eq!(
        model.submitted_membership_commands,
        vec![SubmittedMembershipCommand {
            action: MembershipCommandAction::Join,
            target: "#elsewhere:example.org".to_owned(),
        }]
    );
    assert_eq!(model.status, "join submitted: #elsewhere:example.org");

    // A user opens a query, as /query does.
    double_click_text(&app, "https://matrix.to/#/@bob")?;
    assert_eq!(
        app.dump_model()?.submitted_direct_room_commands,
        vec![SubmittedDirectRoomCommand {
            user_id: "@bob:example".to_owned(),
        }]
    );
    assert!(app.dump_model()?.opened_links.is_empty());
    Ok(())
}

#[test]
fn go_to_original_by_double_clicking_a_quote() -> Result<(), Box<dyn std::error::Error>> {
    let (app, local_app_data) = launch_replies_app()?;
    // Windows sends WM_LBUTTONDBLCLK only to a class that asks for it.
    let style = unsafe { GetClassLongPtrW(app.active_timeline_hwnd()? as HWND, GCL_STYLE) } as u32;
    assert_ne!(style & CS_DBLCLKS, 0);
    let before = active_timeline(&app)?;
    assert_ne!(before.top_line, 0, "the window starts scrolled to its end");
    assert_ne!(
        before.visible_lines[0].event_id.as_deref(),
        Some("$replies-question")
    );

    double_click_quote_of(&app, "$replies-fallback")?;
    assert_eq!(
        app.dump_message_menu_state()?.selected_event_id.as_deref(),
        Some("$replies-question")
    );
    let after = active_timeline(&app)?;
    assert_eq!(
        after.visible_lines[0].event_id.as_deref(),
        Some("$replies-question"),
        "{after:?}"
    );
    assert_eq!(app.dump_model()?.status, "original message found");

    drop(app);
    let _ = fs::remove_dir_all(&local_app_data);
    Ok(())
}

#[test]
fn go_to_original_via_the_message_menu() -> Result<(), Box<dyn std::error::Error>> {
    let (app, local_app_data) = launch_replies_app()?;
    app.select_timeline_event("$replies-question")?;
    assert!(!app.dump_message_menu_state()?.go_to_original_enabled);
    assert!(app.go_to_original_via_message_menu().is_err());

    // The emote replies to the answer, itself a reply: the answer's first
    // line is its own quote line.
    app.select_timeline_event("$replies-emote")?;
    assert!(app.dump_message_menu_state()?.go_to_original_enabled);
    app.go_to_original_via_message_menu()?;
    app.wait_idle(Duration::from_secs(5))?;
    assert_eq!(
        app.dump_message_menu_state()?.selected_event_id.as_deref(),
        Some("$replies-answer")
    );
    let top = &active_timeline(&app)?.visible_lines[0];
    assert_eq!(top.kind, UiTimelineLineKind::ReplyQuote);
    assert_eq!(top.event_id.as_deref(), Some("$replies-answer"));

    drop(app);
    let _ = fs::remove_dir_all(&local_app_data);
    Ok(())
}

#[test]
fn go_to_original_of_an_unloaded_message_asks_for_older_history()
-> Result<(), Box<dyn std::error::Error>> {
    let (app, local_app_data) = launch_replies_app()?;
    // A fetched quote (`$not-loaded`) is not a timeline line either; without
    // a session the core refuses the history at once.
    for reply in ["$replies-unknown-reply", "$replies-old-reply"] {
        double_click_quote_of(&app, reply)?;
        let deadline = Instant::now() + Duration::from_secs(5);
        let status = loop {
            let status = app.dump_model()?.status;
            if status != "finding the original message..." || Instant::now() >= deadline {
                break status;
            }
            std::thread::sleep(POLL_INTERVAL);
        };
        assert_eq!(
            status,
            "* could not load older messages: backfill room history requires an authenticated session",
            "{reply}"
        );
        assert_eq!(
            app.dump_message_menu_state()?.selected_event_id.as_deref(),
            Some(reply)
        );
    }

    drop(app);
    let _ = fs::remove_dir_all(&local_app_data);
    Ok(())
}

#[test]
fn automation_driver_reply_menu_arms_and_sends_reply() -> Result<(), Box<dyn std::error::Error>> {
    let exe_path =
        mirx_ui_exe_path().map_err(|error| format!("failed to locate mirx-ui.exe: {error}"))?;
    let local_app_data = unique_local_app_data_path();
    let app = launch_app_with_options(
        &exe_path,
        &[OsString::from("--automation")],
        launch_options(Some(local_app_data.clone())),
    )?;
    app.wait_for_lifecycle_state("core_started", Duration::from_secs(30))?;
    app.load_events(&fs::read_to_string(fixture_state_path("replies.json"))?)?;
    app.wait_idle(Duration::from_secs(5))?;
    app.select_room("!replies:example")?;
    app.select_timeline_event("$replies-question")?;
    app.reply_to_selected_message_via_menu()?;

    let ui_state = app.dump_ui_state()?;
    assert_eq!(
        ui_state.compose_target,
        Some(UiComposeTarget {
            kind: UiComposeTargetKind::Reply,
            event_id: "$replies-question".to_owned(),
        })
    );
    let lines = &find_view(&ui_state, &room_identity("!replies:example")).lines;
    assert_eq!(
        lines.last().map(String::as_str),
        Some(
            "* Replying to <Alice> are we deploying today? the build looks green on eve... (Esc cancels)"
        ),
        "{lines:?}"
    );
    assert_eq!(app.compose_text()?, "");

    app.set_compose_text("yes, at 14:00")?;
    app.send_compose_key(ComposeKey::Return)?;
    app.wait_idle(Duration::from_secs(5))?;
    assert_eq!(app.compose_text()?, "");
    assert_eq!(
        app.dump_model()?.submitted_reply_commands,
        vec![SubmittedReplyCommand {
            room_id: "!replies:example".to_owned(),
            target_event_id: "$replies-question".to_owned(),
            sender_user_id: "@alice:example".to_owned(),
            body: "yes, at 14:00".to_owned(),
            styles: Vec::new(),
            thread: None,
        }]
    );
    let ui_state = app.dump_ui_state()?;
    assert_eq!(ui_state.compose_target, None);
    assert!(
        !find_view(&ui_state, &room_identity("!replies:example"))
            .lines
            .iter()
            .any(|line| line.contains("Replying to")),
        "sending disarms the editbox"
    );

    drop(app);
    let _ = fs::remove_dir_all(&local_app_data);
    Ok(())
}

#[test]
fn automation_driver_edit_menu_prefills_own_message_and_escape_cancels()
-> Result<(), Box<dyn std::error::Error>> {
    let exe_path =
        mirx_ui_exe_path().map_err(|error| format!("failed to locate mirx-ui.exe: {error}"))?;
    let local_app_data = unique_local_app_data_path();
    let app = launch_app_with_options(
        &exe_path,
        &[OsString::from("--automation")],
        launch_options(Some(local_app_data.clone())),
    )?;
    app.wait_for_lifecycle_state("core_started", Duration::from_secs(30))?;
    app.load_events(&reactions_fixture_signed_in_as("@normal:example")?)?;
    app.wait_idle(Duration::from_secs(5))?;
    app.select_room("!reactions:example")?;

    app.select_timeline_event("$react-thread-root")?;
    assert!(!app.dump_message_menu_state()?.edit_enabled);
    let error = app
        .edit_selected_message_via_menu()
        .expect_err("edit must reject someone else's message");
    assert!(matches!(
        error,
        TestkitError::Protocol { code, message, .. }
            if code == "selected_message_unavailable"
                && message == "you can only edit your own messages"
    ));
    assert_eq!(app.dump_ui_state()?.compose_target, None);

    app.select_timeline_event("$react-normal")?;
    assert!(app.dump_message_menu_state()?.edit_enabled);
    app.edit_selected_message_via_menu()?;
    assert_eq!(app.compose_text()?, "normal reaction target");
    let edit_target = Some(UiComposeTarget {
        kind: UiComposeTargetKind::Edit,
        event_id: "$react-normal".to_owned(),
    });
    assert_eq!(app.dump_ui_state()?.compose_target, edit_target);

    app.send_compose_key(ComposeKey::Escape)?;
    assert_eq!(app.compose_text()?, "");
    assert_eq!(app.dump_ui_state()?.compose_target, None);

    app.edit_selected_message_via_menu()?;
    assert_eq!(app.dump_ui_state()?.compose_target, edit_target);
    app.set_compose_text("normal reaction target, fixed")?;
    app.send_compose_key(ComposeKey::Return)?;
    app.wait_idle(Duration::from_secs(5))?;
    assert_eq!(app.compose_text()?, "");
    assert_eq!(app.dump_ui_state()?.compose_target, None);
    assert_eq!(
        app.dump_model()?.submitted_edit_commands,
        vec![SubmittedEditCommand {
            room_id: "!reactions:example".to_owned(),
            target_event_id: "$react-normal".to_owned(),
            body: "normal reaction target, fixed".to_owned(),
            styles: Vec::new(),
        }]
    );

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
    let exe_path =
        mirx_ui_exe_path().map_err(|error| format!("failed to locate mirx-ui.exe: {error}"))?;
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
        .join("mirx")
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
    let exe_path =
        mirx_ui_exe_path().map_err(|error| format!("failed to locate mirx-ui.exe: {error}"))?;
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
    let exe_path =
        mirx_ui_exe_path().map_err(|error| format!("failed to locate mirx-ui.exe: {error}"))?;
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
    let exe_path =
        mirx_ui_exe_path().map_err(|error| format!("failed to locate mirx-ui.exe: {error}"))?;
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
    let exe_path =
        mirx_ui_exe_path().map_err(|error| format!("failed to locate mirx-ui.exe: {error}"))?;
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
            highlight: false,
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
fn automation_driver_slash_clear_in_status_view_empties_the_log()
-> Result<(), Box<dyn std::error::Error>> {
    let exe_path =
        mirx_ui_exe_path().map_err(|error| format!("failed to locate mirx-ui.exe: {error}"))?;
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
    let ui_state = app.dump_ui_state()?;
    assert_eq!(ui_state.mdi.active, ViewIdentity::Status);
    assert!(view_contains(
        find_view(&ui_state, &ViewIdentity::Status),
        "* Synced rooms: "
    ));

    app.set_compose_text("/clear")?;
    app.send_compose_key(ComposeKey::Return)?;
    app.wait_idle(Duration::from_secs(5))?;
    assert_eq!(app.compose_text()?, "");

    let ui_state = app.dump_ui_state()?;
    assert_eq!(ui_state.mdi.active, ViewIdentity::Status);
    assert!(ui_state.status_bar.text.contains("cleared window: status"));
    assert!(
        find_view(&ui_state, &ViewIdentity::Status)
            .lines
            .iter()
            .all(|line| line.is_empty())
    );
    assert_eq!(app.dump_model()?.last_error, None);

    app.send_compose_key(ComposeKey::Up)?;
    assert_eq!(app.compose_text()?, "/clear");

    drop(app);
    let _ = fs::remove_dir_all(&local_app_data);
    Ok(())
}

#[test]
fn automation_driver_status_window_is_a_timestamped_log() -> Result<(), Box<dyn std::error::Error>>
{
    let exe_path =
        mirx_ui_exe_path().map_err(|error| format!("failed to locate mirx-ui.exe: {error}"))?;
    let mut events = base_events();
    events.push(Event::VerificationRequest(mirx_core::VerificationRequest {
        flow_id: "flow-status-log".to_owned(),
        user_id: "@alice:example".to_owned(),
    }));
    let state_path = write_state_file(&events)?;
    let local_app_data = unique_local_app_data_path();
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

    app.set_compose_text("/status")?;
    app.send_compose_key(ComposeKey::Return)?;
    app.wait_idle(Duration::from_secs(5))?;
    let ui_state = app.dump_ui_state()?;
    assert_eq!(ui_state.mdi.active, ViewIdentity::Status);
    let status_view = find_view(&ui_state, &ViewIdentity::Status);
    // Render-state runs on a fixed clock, a minute before the fixtures.
    assert!(view_contains(
        status_view,
        "[22:12] * Synced rooms: 2 joined, 0 invited"
    ));
    assert!(view_contains(
        status_view,
        "[22:12] * Verification requested by @alice:example (flow flow-status-log)"
    ));

    let result = app.find_active_view("flow-status-log")?;
    assert_eq!(result.active, ViewIdentity::Status);
    assert_eq!(result.match_count, 1);
    assert_eq!(
        result.selected_text.as_deref(),
        Some("[22:12] * Verification requested by @alice:example (flow flow-status-log)")
    );

    // Status lines are not messages: the message menu has nothing to act on.
    let menu = app.dump_message_menu_state()?;
    assert!(!menu.reply_enabled && !menu.edit_enabled && !menu.react_enabled);
    assert!(!menu.redact_enabled && !menu.save_attachment_enabled);
    assert!(
        app.open_selected_timeline_context_menu(ContextMenuCommand::ReplyMessage)
            .is_err()
    );

    app.set_compose_text("/e2ee status")?;
    app.send_compose_key(ComposeKey::Return)?;
    app.wait_idle(Duration::from_secs(5))?;
    assert!(view_contains(
        find_view(&app.dump_ui_state()?, &ViewIdentity::Status),
        "[22:12] * E2EE status not yet known"
    ));

    drop(app);
    let _ = fs::remove_file(&state_path);
    let _ = fs::remove_dir_all(&local_app_data);
    Ok(())
}

#[test]
fn automation_driver_slash_find_updates_active_room_without_sending_text()
-> Result<(), Box<dyn std::error::Error>> {
    let exe_path =
        mirx_ui_exe_path().map_err(|error| format!("failed to locate mirx-ui.exe: {error}"))?;
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
        ExpectedFind {
            active: &room_identity("!beta:example"),
            query: "beta second",
            matched: true,
            match_count: 1,
            current_match_index: Some(0),
            selected_text: Some(selected_text),
            history_search: UiFindHistorySearchState::LocalOnly,
        },
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
    let exe_path =
        mirx_ui_exe_path().map_err(|error| format!("failed to locate mirx-ui.exe: {error}"))?;
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
        ExpectedFind {
            active: &search_view_identity,
            query: "needle",
            matched: true,
            match_count: 2,
            current_match_index: Some(0),
            selected_text: Some(matching_lines[0]),
            history_search: UiFindHistorySearchState::LocalOnly,
        },
    );
    let first_state = app.dump_ui_state()?;
    assert_eq!(first_state.mdi.active, search_view_identity);
    assert_ui_active_find(
        &first_state,
        ExpectedFind {
            active: &room_identity("!search:example"),
            query: "needle",
            matched: true,
            match_count: 2,
            current_match_index: Some(0),
            selected_text: Some(matching_lines[0]),
            history_search: UiFindHistorySearchState::LocalOnly,
        },
    );
    assert!(first_state.status_bar.text.contains("find: 1/2 \"needle\""));

    let second = app.find_active_view("needle")?;
    assert_find_result(
        &second,
        ExpectedFind {
            active: &search_view_identity,
            query: "needle",
            matched: true,
            match_count: 2,
            current_match_index: Some(1),
            selected_text: Some(matching_lines[1]),
            history_search: UiFindHistorySearchState::LocalOnly,
        },
    );
    let second_state = app.dump_ui_state()?;
    assert_eq!(second_state.mdi.active, search_view_identity);
    assert_ui_active_find(
        &second_state,
        ExpectedFind {
            active: &room_identity("!search:example"),
            query: "needle",
            matched: true,
            match_count: 2,
            current_match_index: Some(1),
            selected_text: Some(matching_lines[1]),
            history_search: UiFindHistorySearchState::LocalOnly,
        },
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
        ExpectedFind {
            active: &search_view_identity,
            query: "needle",
            matched: true,
            match_count: 2,
            current_match_index: Some(0),
            selected_text: Some(matching_lines[0]),
            history_search: UiFindHistorySearchState::LocalOnly,
        },
    );
    let wrapped_state = app.dump_ui_state()?;
    assert_eq!(wrapped_state.mdi.active, search_view_identity);
    assert_ui_active_find(
        &wrapped_state,
        ExpectedFind {
            active: &room_identity("!search:example"),
            query: "needle",
            matched: true,
            match_count: 2,
            current_match_index: Some(0),
            selected_text: Some(matching_lines[0]),
            history_search: UiFindHistorySearchState::LocalOnly,
        },
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
    let exe_path =
        mirx_ui_exe_path().map_err(|error| format!("failed to locate mirx-ui.exe: {error}"))?;
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
    let thread_view_identity = thread_identity("!ops:example", "$thread-root");
    app.select_navigation_view(&thread_view_identity)?;

    let thread_state = app.dump_ui_state()?;
    let thread_view = find_view(&thread_state, &thread_view_identity);
    // The pending draft replies to "thread rep", so its quote line matches
    // too, below the line itself.
    assert_eq!(
        view_lines_containing(thread_view, "thread rep"),
        [
            "[22:16] <@dave:example> thread rep",
            "    > <@dave:example> thread rep"
        ]
    );
    let selected_text = "[22:16] <@dave:example> thread rep";

    let positive = app.find_active_view("thread rep")?;
    assert_find_result(
        &positive,
        ExpectedFind {
            active: &thread_view_identity,
            query: "thread rep",
            matched: true,
            match_count: 2,
            current_match_index: Some(0),
            selected_text: Some(selected_text),
            history_search: UiFindHistorySearchState::LocalOnly,
        },
    );
    let positive_state = app.dump_ui_state()?;
    assert_eq!(positive_state.mdi.active, thread_view_identity);
    assert_ui_active_find(
        &positive_state,
        ExpectedFind {
            active: &thread_identity("!ops:example", "$thread-root"),
            query: "thread rep",
            matched: true,
            match_count: 2,
            current_match_index: Some(0),
            selected_text: Some(selected_text),
            history_search: UiFindHistorySearchState::LocalOnly,
        },
    );
    assert!(
        positive_state
            .status_bar
            .text
            .contains("find: 1/2 \"thread rep\"")
    );

    let scoped_miss = app.find_active_view("lobby")?;
    assert_find_result(
        &scoped_miss,
        ExpectedFind {
            active: &thread_view_identity,
            query: "lobby",
            matched: false,
            match_count: 0,
            current_match_index: None,
            selected_text: None,
            history_search: UiFindHistorySearchState::Loading,
        },
    );
    let miss_state = app.dump_ui_state()?;
    assert_eq!(miss_state.mdi.active, thread_view_identity);
    assert_ui_active_find(
        &miss_state,
        ExpectedFind {
            active: &thread_identity("!ops:example", "$thread-root"),
            query: "lobby",
            matched: false,
            match_count: 0,
            current_match_index: None,
            selected_text: None,
            history_search: UiFindHistorySearchState::Loading,
        },
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
    let exe_path =
        mirx_ui_exe_path().map_err(|error| format!("failed to locate mirx-ui.exe: {error}"))?;
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
        ExpectedFind {
            active: &ViewIdentity::Status,
            query: "cross-signing",
            matched: true,
            match_count: 1,
            current_match_index: Some(0),
            selected_text: Some(selected_text),
            history_search: UiFindHistorySearchState::LocalOnly,
        },
    );

    let ui_state = app.dump_ui_state()?;
    assert_eq!(ui_state.mdi.active, ViewIdentity::Status);
    assert_active_switchbar_item(&ui_state, &ViewIdentity::Status);
    assert_ui_active_find(
        &ui_state,
        ExpectedFind {
            active: &ViewIdentity::Status,
            query: "cross-signing",
            matched: true,
            match_count: 1,
            current_match_index: Some(0),
            selected_text: Some(selected_text),
            history_search: UiFindHistorySearchState::LocalOnly,
        },
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
    let exe_path =
        mirx_ui_exe_path().map_err(|error| format!("failed to locate mirx-ui.exe: {error}"))?;
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
        ExpectedFind {
            active: &beta_view_identity,
            query: "missing search term",
            matched: false,
            match_count: 0,
            current_match_index: None,
            selected_text: None,
            history_search: UiFindHistorySearchState::Loading,
        },
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
        ExpectedFind {
            active: &room_identity("!beta:example"),
            query: "missing search term",
            matched: false,
            match_count: 0,
            current_match_index: None,
            selected_text: None,
            history_search: UiFindHistorySearchState::Loading,
        },
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
    let exe_path =
        mirx_ui_exe_path().map_err(|error| format!("failed to locate mirx-ui.exe: {error}"))?;
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
            .contains("* /find: insufficient parameters")
    );

    let slash_model = app.dump_model()?;
    assert_eq!(slash_model.selected_room.as_deref(), Some("!beta:example"));
    assert_eq!(slash_model.status, "* /find: insufficient parameters");
    assert_eq!(slash_model.last_error, None);
    assert_no_submitted_commands(&slash_model);

    let result = app.find_active_view("")?;
    assert_find_result(
        &result,
        ExpectedFind {
            active: &beta_view_identity,
            query: "",
            matched: false,
            match_count: 0,
            current_match_index: None,
            selected_text: None,
            history_search: UiFindHistorySearchState::LocalOnly,
        },
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
    let exe_path =
        mirx_ui_exe_path().map_err(|error| format!("failed to locate mirx-ui.exe: {error}"))?;
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
        ExpectedFind {
            active: &alpha_view_identity,
            query: "alpha needle",
            matched: true,
            match_count: 1,
            current_match_index: Some(0),
            selected_text: Some(alpha_line),
            history_search: UiFindHistorySearchState::LocalOnly,
        },
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
        ExpectedFind {
            active: &beta_view_identity,
            query: "beta needle",
            matched: true,
            match_count: 2,
            current_match_index: Some(1),
            selected_text: Some(beta_lines[1]),
            history_search: UiFindHistorySearchState::LocalOnly,
        },
    );

    app.select_room("!alpha:example")?;
    let restored_alpha_state = app.dump_ui_state()?;
    assert_eq!(restored_alpha_state.mdi.active, alpha_view_identity);
    assert_ui_active_find(
        &restored_alpha_state,
        ExpectedFind {
            active: &room_identity("!alpha:example"),
            query: "alpha needle",
            matched: true,
            match_count: 1,
            current_match_index: Some(0),
            selected_text: Some(alpha_line),
            history_search: UiFindHistorySearchState::LocalOnly,
        },
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
        ExpectedFind {
            active: &room_identity("!beta:example"),
            query: "beta needle",
            matched: true,
            match_count: 2,
            current_match_index: Some(1),
            selected_text: Some(beta_lines[1]),
            history_search: UiFindHistorySearchState::LocalOnly,
        },
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
    let exe_path =
        mirx_ui_exe_path().map_err(|error| format!("failed to locate mirx-ui.exe: {error}"))?;
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
        ExpectedFind {
            active: &rebuild_view_identity,
            query: "needle",
            matched: true,
            match_count: 2,
            current_match_index: Some(1),
            selected_text: Some(selected_text),
            history_search: UiFindHistorySearchState::LocalOnly,
        },
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
        ExpectedFind {
            active: &room_identity("!rebuild:example"),
            query: "needle",
            matched: true,
            match_count: 3,
            current_match_index: Some(2),
            selected_text: Some(selected_text),
            history_search: UiFindHistorySearchState::LocalOnly,
        },
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
    let exe_path =
        mirx_ui_exe_path().map_err(|error| format!("failed to locate mirx-ui.exe: {error}"))?;
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
    let beta = room_identity("!beta:example");
    assert_eq!(ui_state.mdi.active, beta);
    // mIRC prints a command's error in the window it was typed in.
    let echo = only_view_line_containing(find_view(&ui_state, &beta), "/doesnotexist");
    assert!(
        echo.ends_with("] * /doesnotexist: unknown command"),
        "{echo:?}"
    );
    assert!(
        ui_state
            .status_bar
            .text
            .contains("* /doesnotexist: unknown command")
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
    let exe_path =
        mirx_ui_exe_path().map_err(|error| format!("failed to locate mirx-ui.exe: {error}"))?;
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
            .contains("* /me: insufficient parameters")
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
    let exe_path =
        mirx_ui_exe_path().map_err(|error| format!("failed to locate mirx-ui.exe: {error}"))?;
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
    let exe_path =
        mirx_ui_exe_path().map_err(|error| format!("failed to locate mirx-ui.exe: {error}"))?;
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
            .contains("* /me: insufficient parameters")
    );

    app.send_compose_key(ComposeKey::Up)?;
    assert_eq!(app.compose_text()?, "/me   ");

    drop(app);
    let _ = fs::remove_dir_all(&local_app_data);
    Ok(())
}

#[test]
fn live_matrix_ui_session() -> Result<(), Box<dyn std::error::Error>> {
    let Some(homeserver_url) = env::var("MIRX_HS").ok().filter(|value| !value.is_empty()) else {
        return Ok(());
    };
    let Some(username) = env::var("MIRX_USER").ok().filter(|value| !value.is_empty()) else {
        return Ok(());
    };
    let Some(password) = env::var("MIRX_PASS").ok().filter(|value| !value.is_empty()) else {
        return Ok(());
    };
    let room_id = env::var("MIRX_ROOM").ok().filter(|value| !value.is_empty());

    let exe_path =
        mirx_ui_exe_path().map_err(|error| format!("failed to locate mirx-ui.exe: {error}"))?;
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
            "mirx ui live {}",
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

#[test]
fn formatted_messages_render_styled_lines() -> Result<(), Box<dyn std::error::Error>> {
    let exe_path =
        mirx_ui_exe_path().map_err(|error| format!("failed to locate mirx-ui.exe: {error}"))?;
    let local_app_data = unique_local_app_data_path();
    let app = launch_app_with_options(
        &exe_path,
        &[OsString::from("--automation")],
        LaunchOptions {
            frame_size: Some(FrameSize {
                width: 800,
                height: 700,
            }),
            ..launch_options(Some(local_app_data.clone()))
        },
    )?;
    app.wait_for_lifecycle_state("core_started", Duration::from_secs(30))?;
    app.load_events(&fs::read_to_string(fixture_state_path("rich-text.json"))?)?;
    app.wait_idle(Duration::from_secs(5))?;
    app.select_room("!rich:example")?;
    let timeline = active_timeline(&app)?;
    let lines_of = |event_id: &str| {
        timeline
            .visible_lines
            .iter()
            .filter(|line| line.event_id.as_deref() == Some(event_id))
            .collect::<Vec<_>>()
    };
    // The styled text of each span across an event's lines.
    let styled_texts = |lines: &[&mirx_testkit::UiTimelineLine]| {
        lines
            .iter()
            .flat_map(|line| {
                line.styles
                    .iter()
                    .map(|span| line.text[span.start..span.end].to_owned())
            })
            .collect::<Vec<_>>()
    };

    // The styles sit on the body, past the timestamp and nick.
    let first = lines_of("$rich-1");
    assert!(
        first[0].text.starts_with("[22:14] <Alice> bold "),
        "{first:?}"
    );
    assert_eq!((first[0].styles[0].start, first[0].styles[0].end), (16, 20));
    assert!(first[0].styles[0].style.bold);
    assert_eq!(
        styled_texts(&first),
        ["bold", "italic курсив", "underline", "strike", " all"]
    );
    let last_span = first.last().unwrap().styles.last().unwrap();
    assert!(last_span.style.bold && last_span.style.italic && last_span.style.underline);

    // A span crossing a wrap is split between the lines, each part on its own text.
    let wrapped = lines_of("$rich-7");
    assert!(wrapped.len() >= 2, "{wrapped:?}");
    let red = mirx_core::TextColor {
        red: 0xff,
        green: 0,
        blue: 0,
    };
    let red_text = wrapped
        .iter()
        .flat_map(|line| {
            line.styles.iter().map(|span| {
                assert!(span.style.underline);
                assert_eq!(span.style.foreground, Some(red));
                line.text[span.start..span.end].trim().to_owned()
            })
        })
        .collect::<Vec<_>>();
    assert!(red_text.len() >= 2, "{wrapped:?}");
    assert_eq!(
        red_text.join(" "),
        "its red underlined span carries on past the break onto the next line"
    );

    // An edit shows the replacement's styles.
    let edited = lines_of("$rich-10");
    assert_eq!(edited.len(), 1, "{timeline:?}");
    assert_eq!(edited[0].text, "[22:23] <Alice> final text (edited)");
    assert_eq!(styled_texts(&edited), ["text"]);
    assert!(edited[0].styles[0].style.bold);
    assert!(!edited[0].styles[0].style.italic);

    drop(app);
    let _ = fs::remove_dir_all(&local_app_data);
    Ok(())
}

#[test]
fn editbox_format_keys_insert_mirc_codes() -> Result<(), Box<dyn std::error::Error>> {
    let (app, local_app_data) = launch_replies_app()?;
    let edit = unsafe { compose_edit_hwnd(&app) };
    let type_text = |text: &str| {
        for unit in text.encode_utf16() {
            unsafe { SendMessageW(edit, WM_CHAR, WPARAM::from(unit), 0) };
        }
    };
    let format = |key: MircFormatKey| app.send_compose_key(ComposeKey::Format { key });

    for key in [
        MircFormatKey::Bold,
        MircFormatKey::Color,
        MircFormatKey::Underline,
        MircFormatKey::Italic,
        MircFormatKey::Reset,
        MircFormatKey::Reverse,
    ] {
        format(key)?;
    }
    assert_eq!(app.compose_text()?, "\x02\x03\x1f\x1d\x0f\x16");

    app.set_compose_text("")?;
    format(MircFormatKey::Bold)?;
    type_text("hi");
    format(MircFormatKey::Bold)?;
    type_text(" ");
    format(MircFormatKey::Color)?;
    type_text("4,1red");
    assert_eq!(app.compose_text()?, "\x02hi\x02 \x034,1red");
    app.send_compose_key(ComposeKey::Return)?;
    app.wait_idle(Duration::from_secs(5))?;
    assert_eq!(app.compose_text()?, "");

    let style = |bold: bool, colors: Option<(u32, u32)>| {
        let color = |hex: u32| mirx_core::TextColor {
            red: (hex >> 16) as u8,
            green: (hex >> 8) as u8,
            blue: hex as u8,
        };
        mirx_core::TextStyle {
            bold,
            foreground: colors.map(|(foreground, _)| color(foreground)),
            background: colors.map(|(_, background)| color(background)),
            ..mirx_core::TextStyle::default()
        }
    };
    assert_eq!(
        app.dump_model()?.submitted_message_commands,
        vec![SubmittedMessageCommand {
            room_id: "!replies:example".to_owned(),
            presentation: RoomMessagePresentationKind::Text,
            body: "hi red".to_owned(),
            styles: vec![
                mirx_core::TextStyleSpan {
                    start: 0,
                    end: 2,
                    style: style(true, None),
                },
                mirx_core::TextStyleSpan {
                    start: 3,
                    end: 6,
                    style: style(false, Some((0xff0000, 0x000000))),
                },
            ],
            thread: None,
        }]
    );
    // History recalls the line as typed, codes and all.
    app.send_compose_key(ComposeKey::Up)?;
    assert_eq!(app.compose_text()?, "\x02hi\x02 \x034,1red");

    drop(app);
    let _ = fs::remove_dir_all(&local_app_data);
    Ok(())
}
