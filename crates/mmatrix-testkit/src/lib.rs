#![cfg_attr(windows, allow(unsafe_op_in_unsafe_fn))]

use std::ffi::{OsStr, OsString};
use std::fmt;
use std::path::Path;
use std::time::Duration;

#[cfg(windows)]
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

#[cfg(windows)]
const FRAME_CLASS_NAME: &str = "mmatrixWin32Frame";
#[cfg(windows)]
const COMMAND_ID_PING: u32 = 1;
#[cfg(windows)]
const COMMAND_ID_GET_VERSION: u32 = 2;
#[cfg(windows)]
const COMMAND_ID_WAIT_IDLE: u32 = 3;
#[cfg(windows)]
const COMMAND_ID_DUMP_MODEL: u32 = 4;
#[cfg(windows)]
const COMMAND_ID_DUMP_TIMELINE: u32 = 5;
#[cfg(windows)]
const COMMAND_ID_LIST_ROOMS: u32 = 6;
#[cfg(windows)]
const COMMAND_ID_SELECT_ROOM: u32 = 7;
#[cfg(windows)]
const COMMAND_ID_SEND_TEXT: u32 = 8;
#[cfg(windows)]
const COMMAND_ID_LOAD_SYNTHETIC_STATE: u32 = 9;
#[cfg(windows)]
const RESPONSE_MAPPING_CAPACITY: usize = 4 * 1024 * 1024;
#[cfg(windows)]
const RESPONSE_JSON_CAPACITY: usize = RESPONSE_MAPPING_CAPACITY - std::mem::size_of::<u32>();
const DEFAULT_LAUNCH_TIMEOUT: Duration = Duration::from_secs(10);
#[cfg(windows)]
const DEFAULT_POLL_INTERVAL: Duration = Duration::from_millis(50);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LaunchOptions {
    pub startup_timeout: Duration,
}

impl Default for LaunchOptions {
    fn default() -> Self {
        Self {
            startup_timeout: DEFAULT_LAUNCH_TIMEOUT,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Model {
    pub status: String,
    pub rooms: Vec<Room>,
    pub last_timeline: Option<TimelineLine>,
    pub last_verification: Option<Verification>,
    pub last_error: Option<UiError>,
    pub selected_room: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Room {
    pub room_id: String,
    pub display_name: String,
    pub encrypted: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TimelineLine {
    pub room_id: String,
    pub body: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Verification {
    pub flow_id: String,
    pub user_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UiError {
    pub operation: String,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProtocolVersion {
    pub protocol_version: u32,
    pub app_version: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SyntheticState {
    pub rooms: Vec<SyntheticRoom>,
    pub timelines: Vec<SyntheticTimeline>,
    pub selected_room: Option<String>,
    pub verification: Option<SyntheticVerification>,
    pub error: Option<SyntheticError>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SyntheticRoom {
    pub room_id: String,
    pub display_name: String,
    pub encrypted: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SyntheticTimeline {
    pub room_id: String,
    pub lines: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SyntheticVerification {
    pub flow_id: String,
    pub user_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SyntheticError {
    pub operation: String,
    pub message: String,
}

#[derive(Debug)]
pub enum TestkitError {
    UnsupportedPlatform(&'static str),
    Timeout(String),
    Launch(String),
    Transport(String),
    Protocol { code: String, message: String },
    InvalidResponse(String),
    Io(std::io::Error),
    Json(serde_json::Error),
}

impl fmt::Display for TestkitError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnsupportedPlatform(message) => f.write_str(message),
            Self::Timeout(message)
            | Self::Launch(message)
            | Self::Transport(message)
            | Self::InvalidResponse(message) => f.write_str(message),
            Self::Protocol { code, message } => write!(f, "protocol error {code}: {message}"),
            Self::Io(error) => write!(f, "io error: {error}"),
            Self::Json(error) => write!(f, "json error: {error}"),
        }
    }
}

impl std::error::Error for TestkitError {}

impl From<std::io::Error> for TestkitError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error)
    }
}

impl From<serde_json::Error> for TestkitError {
    fn from(error: serde_json::Error) -> Self {
        Self::Json(error)
    }
}

#[derive(Debug)]
pub struct MmatrixApp {
    inner: imp::AppHandle,
}

impl MmatrixApp {
    pub fn find(timeout: Duration) -> Result<Self, TestkitError> {
        imp::AppHandle::find(timeout).map(|inner| Self { inner })
    }

    pub fn launch(
        exe_path: impl AsRef<Path>,
        args: &[impl AsRef<OsStr>],
    ) -> Result<Self, TestkitError> {
        let options = LaunchOptions::default();
        let owned_args = args
            .iter()
            .map(|arg| arg.as_ref().to_os_string())
            .collect::<Vec<_>>();
        imp::AppHandle::launch(exe_path.as_ref(), &owned_args, options.startup_timeout)
            .map(|inner| Self { inner })
    }

    pub fn ping(&self) -> Result<(), TestkitError> {
        self.inner.ping()
    }

    pub fn get_version(&self) -> Result<ProtocolVersion, TestkitError> {
        self.inner.get_version()
    }

    pub fn wait_idle(&self, timeout: Duration) -> Result<(), TestkitError> {
        self.inner.wait_idle(timeout)
    }

    pub fn dump_model(&self) -> Result<Model, TestkitError> {
        self.inner.dump_model()
    }

    pub fn timeline(&self, room_id: Option<&str>) -> Result<Vec<String>, TestkitError> {
        self.inner.timeline(room_id)
    }

    pub fn list_rooms(&self) -> Result<Vec<Room>, TestkitError> {
        self.inner.list_rooms()
    }

    pub fn select_room(&self, room_id: &str) -> Result<(), TestkitError> {
        self.inner.select_room(room_id)
    }

    pub fn send_text(&self, room_id: Option<&str>, body: &str) -> Result<(), TestkitError> {
        self.inner.send_text(room_id, body)
    }

    pub fn load_synthetic_state(&self, state: &SyntheticState) -> Result<(), TestkitError> {
        self.inner.load_synthetic_state(state)
    }

    pub fn wait_for_timeline_contains(
        &self,
        room_id: Option<&str>,
        substr: &str,
        timeout: Duration,
    ) -> Result<(), TestkitError> {
        self.inner
            .wait_for_timeline_contains(room_id, substr, timeout)
    }

    pub fn screenshot(&self, path: impl AsRef<Path>) -> Result<(), TestkitError> {
        self.inner.screenshot(path.as_ref())
    }
}

#[cfg(windows)]
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct EmptyArgs {}

#[cfg(windows)]
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct RequestEnvelope<'a, T> {
    id: &'a str,
    args: T,
}

#[cfg(windows)]
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct WaitIdleArgs {
    timeout_ms: u64,
}

#[cfg(windows)]
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct DumpTimelineArgs<'a> {
    room_id: Option<&'a str>,
}

#[cfg(windows)]
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct SelectRoomArgs<'a> {
    room_id: &'a str,
}

#[cfg(windows)]
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct SendTextArgs<'a> {
    room_id: Option<&'a str>,
    body: &'a str,
}

#[cfg(windows)]
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct LoadSyntheticStateArgs<'a> {
    json: &'a SyntheticState,
}

#[cfg(windows)]
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawResponseEnvelope<T> {
    id: Option<String>,
    ok: bool,
    json: Option<T>,
    err: Option<ProtocolFailure>,
}

#[cfg(windows)]
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
struct ProtocolFailure {
    code: String,
    message: String,
}

#[cfg(windows)]
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
struct PongResponse {
    pong: bool,
}

#[cfg(windows)]
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
struct WaitIdleResponse {
    idle: bool,
    event_generation: u64,
    pending_core_events: u64,
    synthetic_load_in_progress: bool,
}

#[cfg(windows)]
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
struct DumpTimelineResponse {
    room_id: String,
    lines: Vec<String>,
}

#[cfg(windows)]
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
struct ListRoomsResponse {
    rooms: Vec<Room>,
}

#[cfg(windows)]
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
struct SelectRoomResponse {
    selected_room: String,
}

#[cfg(windows)]
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
struct SendTextResponse {
    accepted: bool,
    room_id: String,
}

#[cfg(windows)]
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
struct LoadSyntheticStateResponse {
    loaded: bool,
    rooms: usize,
    timeline_events: usize,
}

#[cfg(not(windows))]
mod imp {
    use super::*;

    #[derive(Debug)]
    pub(crate) struct AppHandle;

    impl AppHandle {
        pub(crate) fn find(_timeout: Duration) -> Result<Self, TestkitError> {
            Err(TestkitError::UnsupportedPlatform(
                "mmatrix-testkit requires the Windows target",
            ))
        }

        pub(crate) fn launch(
            _exe_path: &Path,
            _args: &[OsString],
            _timeout: Duration,
        ) -> Result<Self, TestkitError> {
            Err(TestkitError::UnsupportedPlatform(
                "mmatrix-testkit requires the Windows target",
            ))
        }

        pub(crate) fn ping(&self) -> Result<(), TestkitError> {
            Err(TestkitError::UnsupportedPlatform(
                "mmatrix-testkit requires the Windows target",
            ))
        }

        pub(crate) fn get_version(&self) -> Result<ProtocolVersion, TestkitError> {
            Err(TestkitError::UnsupportedPlatform(
                "mmatrix-testkit requires the Windows target",
            ))
        }

        pub(crate) fn wait_idle(&self, _timeout: Duration) -> Result<(), TestkitError> {
            Err(TestkitError::UnsupportedPlatform(
                "mmatrix-testkit requires the Windows target",
            ))
        }

        pub(crate) fn dump_model(&self) -> Result<Model, TestkitError> {
            Err(TestkitError::UnsupportedPlatform(
                "mmatrix-testkit requires the Windows target",
            ))
        }

        pub(crate) fn timeline(&self, _room_id: Option<&str>) -> Result<Vec<String>, TestkitError> {
            Err(TestkitError::UnsupportedPlatform(
                "mmatrix-testkit requires the Windows target",
            ))
        }

        pub(crate) fn list_rooms(&self) -> Result<Vec<Room>, TestkitError> {
            Err(TestkitError::UnsupportedPlatform(
                "mmatrix-testkit requires the Windows target",
            ))
        }

        pub(crate) fn select_room(&self, _room_id: &str) -> Result<(), TestkitError> {
            Err(TestkitError::UnsupportedPlatform(
                "mmatrix-testkit requires the Windows target",
            ))
        }

        pub(crate) fn send_text(
            &self,
            _room_id: Option<&str>,
            _body: &str,
        ) -> Result<(), TestkitError> {
            Err(TestkitError::UnsupportedPlatform(
                "mmatrix-testkit requires the Windows target",
            ))
        }

        pub(crate) fn load_synthetic_state(
            &self,
            _state: &SyntheticState,
        ) -> Result<(), TestkitError> {
            Err(TestkitError::UnsupportedPlatform(
                "mmatrix-testkit requires the Windows target",
            ))
        }

        pub(crate) fn wait_for_timeline_contains(
            &self,
            _room_id: Option<&str>,
            _substr: &str,
            _timeout: Duration,
        ) -> Result<(), TestkitError> {
            Err(TestkitError::UnsupportedPlatform(
                "mmatrix-testkit requires the Windows target",
            ))
        }

        pub(crate) fn screenshot(&self, _path: &Path) -> Result<(), TestkitError> {
            Err(TestkitError::UnsupportedPlatform(
                "mmatrix-testkit requires the Windows target",
            ))
        }
    }
}

#[cfg(windows)]
mod imp {
    use std::mem::size_of;
    use std::os::windows::ffi::OsStrExt;
    use std::process::{Child, Command};
    use std::ptr::null_mut;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::thread;
    use std::time::Instant;

    use windows_sys::Win32::Foundation::{CloseHandle, HANDLE, HWND, LPARAM};
    use windows_sys::Win32::System::DataExchange::COPYDATASTRUCT;
    use windows_sys::Win32::System::Memory::{
        FILE_MAP_READ, MEMORY_MAPPED_VIEW_ADDRESS, MapViewOfFile, OpenFileMappingW, UnmapViewOfFile,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        EnumWindows, GetClassNameW, GetWindowThreadProcessId, SendMessageW, WM_COPYDATA,
    };

    use super::*;

    static REQUEST_COUNTER: AtomicU64 = AtomicU64::new(1);

    #[derive(Debug)]
    pub(crate) struct AppHandle {
        hwnd: HWND,
        pid: u32,
        response_mapping: HANDLE,
        response_view: *mut u8,
        owned_child: Option<Child>,
    }

    impl Drop for AppHandle {
        fn drop(&mut self) {
            unsafe {
                if !self.response_view.is_null() {
                    UnmapViewOfFile(MEMORY_MAPPED_VIEW_ADDRESS {
                        Value: self.response_view.cast(),
                    });
                }
                if !self.response_mapping.is_null() {
                    CloseHandle(self.response_mapping);
                }
            }

            if let Some(child) = self.owned_child.as_mut() {
                match child.try_wait() {
                    Ok(Some(_)) => {}
                    Ok(None) => {
                        let _ = child.kill();
                        let _ = child.wait();
                    }
                    Err(_) => {
                        let _ = child.kill();
                        let _ = child.wait();
                    }
                }
            }
        }
    }

    impl AppHandle {
        pub(crate) fn find(timeout: Duration) -> Result<Self, TestkitError> {
            let deadline = Instant::now() + timeout;
            let mut last_error = None;
            while Instant::now() < deadline {
                match attach_to_window(None) {
                    Ok(app) => match app.ping() {
                        Ok(()) => return Ok(app),
                        Err(error) => last_error = Some(error),
                    },
                    Err(error) => last_error = Some(error),
                }
                thread::sleep(DEFAULT_POLL_INTERVAL);
            }

            Err(TestkitError::Timeout(match last_error {
                Some(error) => format!("timed out waiting for automation window: {error}"),
                None => "timed out waiting for automation window".to_owned(),
            }))
        }

        pub(crate) fn launch(
            exe_path: &Path,
            args: &[OsString],
            timeout: Duration,
        ) -> Result<Self, TestkitError> {
            let mut final_args = args.to_vec();
            if !final_args
                .iter()
                .any(|arg| arg == OsStr::new("--automation") || arg == OsStr::new("--render-state"))
            {
                final_args.push(OsString::from("--automation"));
            }

            let mut child = Command::new(exe_path)
                .args(&final_args)
                .spawn()
                .map_err(|error| {
                    TestkitError::Launch(format!(
                        "failed to launch {}: {error}",
                        exe_path.display()
                    ))
                })?;
            let child_pid = child.id();
            let deadline = Instant::now() + timeout;
            let mut last_error = None;

            while Instant::now() < deadline {
                match attach_to_window(Some(child_pid)) {
                    Ok(mut app) => match app.ping() {
                        Ok(()) => {
                            app.owned_child = Some(child);
                            return Ok(app);
                        }
                        Err(error) => last_error = Some(error),
                    },
                    Err(error) => last_error = Some(error),
                }

                if let Some(status) = child.try_wait()? {
                    return Err(TestkitError::Launch(format!(
                        "mmatrix-ui exited before automation was ready with status {status}"
                    )));
                }

                thread::sleep(DEFAULT_POLL_INTERVAL);
            }

            let _ = child.kill();
            let _ = child.wait();
            Err(TestkitError::Timeout(match last_error {
                Some(error) => {
                    format!("timed out waiting for launched mmatrix-ui automation window: {error}")
                }
                None => "timed out waiting for launched mmatrix-ui automation window".to_owned(),
            }))
        }

        pub(crate) fn ping(&self) -> Result<(), TestkitError> {
            let response: PongResponse = self.send_command(COMMAND_ID_PING, EmptyArgs {})?;
            if response.pong {
                Ok(())
            } else {
                Err(TestkitError::InvalidResponse(
                    "PING response did not contain pong=true".to_owned(),
                ))
            }
        }

        pub(crate) fn get_version(&self) -> Result<ProtocolVersion, TestkitError> {
            self.send_command(COMMAND_ID_GET_VERSION, EmptyArgs {})
        }

        pub(crate) fn wait_idle(&self, timeout: Duration) -> Result<(), TestkitError> {
            let deadline = Instant::now() + timeout;
            loop {
                let remaining = deadline.saturating_duration_since(Instant::now());
                let response: WaitIdleResponse = self.send_command(
                    COMMAND_ID_WAIT_IDLE,
                    WaitIdleArgs {
                        timeout_ms: duration_millis_u64(remaining),
                    },
                )?;
                if response.idle {
                    return Ok(());
                }
                if Instant::now() >= deadline {
                    return Err(TestkitError::Timeout(format!(
                        "WAIT_IDLE did not report idle for pid {} hwnd 0x{:x}",
                        self.pid, self.hwnd as usize
                    )));
                }
                thread::sleep(DEFAULT_POLL_INTERVAL);
            }
        }

        pub(crate) fn dump_model(&self) -> Result<Model, TestkitError> {
            self.send_command(COMMAND_ID_DUMP_MODEL, EmptyArgs {})
        }

        pub(crate) fn timeline(&self, room_id: Option<&str>) -> Result<Vec<String>, TestkitError> {
            let response: DumpTimelineResponse =
                self.send_command(COMMAND_ID_DUMP_TIMELINE, DumpTimelineArgs { room_id })?;
            Ok(response.lines)
        }

        pub(crate) fn list_rooms(&self) -> Result<Vec<Room>, TestkitError> {
            let response: ListRoomsResponse =
                self.send_command(COMMAND_ID_LIST_ROOMS, EmptyArgs {})?;
            Ok(response.rooms)
        }

        pub(crate) fn select_room(&self, room_id: &str) -> Result<(), TestkitError> {
            let response: SelectRoomResponse =
                self.send_command(COMMAND_ID_SELECT_ROOM, SelectRoomArgs { room_id })?;
            if response.selected_room == room_id {
                Ok(())
            } else {
                Err(TestkitError::InvalidResponse(format!(
                    "SELECT_ROOM returned selected_room={}, expected {}",
                    response.selected_room, room_id
                )))
            }
        }

        pub(crate) fn send_text(
            &self,
            room_id: Option<&str>,
            body: &str,
        ) -> Result<(), TestkitError> {
            let response: SendTextResponse =
                self.send_command(COMMAND_ID_SEND_TEXT, SendTextArgs { room_id, body })?;
            if response.accepted {
                Ok(())
            } else {
                Err(TestkitError::InvalidResponse(format!(
                    "SEND_TEXT was not accepted for room {}",
                    response.room_id
                )))
            }
        }

        pub(crate) fn load_synthetic_state(
            &self,
            state: &SyntheticState,
        ) -> Result<(), TestkitError> {
            let response: LoadSyntheticStateResponse = self.send_command(
                COMMAND_ID_LOAD_SYNTHETIC_STATE,
                LoadSyntheticStateArgs { json: state },
            )?;
            if response.loaded {
                Ok(())
            } else {
                Err(TestkitError::InvalidResponse(format!(
                    "LOAD_SYNTHETIC_STATE reported loaded=false after {} room(s) and {} timeline event(s)",
                    response.rooms, response.timeline_events
                )))
            }
        }

        pub(crate) fn wait_for_timeline_contains(
            &self,
            room_id: Option<&str>,
            substr: &str,
            timeout: Duration,
        ) -> Result<(), TestkitError> {
            let deadline = Instant::now() + timeout;
            loop {
                let remaining = deadline.saturating_duration_since(Instant::now());
                self.wait_idle(remaining)?;
                let lines = self.timeline(room_id)?;
                if lines.iter().any(|line| line.contains(substr)) {
                    return Ok(());
                }
                if Instant::now() >= deadline {
                    return Err(TestkitError::Timeout(format!(
                        "timeline never contained {:?} for pid {} hwnd 0x{:x}",
                        substr, self.pid, self.hwnd as usize
                    )));
                }
                thread::sleep(DEFAULT_POLL_INTERVAL);
            }
        }

        pub(crate) fn screenshot(&self, path: &Path) -> Result<(), TestkitError> {
            // PrintWindow captures black frames under Wine/Xvfb in the validated probe setup.
            // Keep screenshot capture on the proven host-side ImageMagick import path for now.
            let status = Command::new("import")
                .arg("-window")
                .arg("root")
                .arg(path)
                .status()
                .map_err(|error| {
                    TestkitError::Launch(format!(
                        "failed to launch ImageMagick import for {}: {error}",
                        path.display()
                    ))
                })?;
            if status.success() {
                Ok(())
            } else {
                Err(TestkitError::Launch(format!(
                    "ImageMagick import exited with status {status} while writing {}",
                    path.display()
                )))
            }
        }

        fn send_command<Args: Serialize, Response: DeserializeOwned>(
            &self,
            command_id: u32,
            args: Args,
        ) -> Result<Response, TestkitError> {
            let request_id = next_request_id();
            let request = RequestEnvelope {
                id: &request_id,
                args,
            };
            let request_bytes = serde_json::to_vec(&request)?;
            let cb_data = u32::try_from(request_bytes.len()).map_err(|_| {
                TestkitError::Transport(format!(
                    "request payload for command id {command_id} exceeded WM_COPYDATA size range"
                ))
            })?;
            let mut request = COPYDATASTRUCT {
                dwData: command_id as usize,
                cbData: cb_data,
                lpData: request_bytes.as_ptr().cast_mut().cast(),
            };

            let send_result = unsafe {
                SendMessageW(
                    self.hwnd,
                    WM_COPYDATA,
                    0,
                    (&mut request as *mut COPYDATASTRUCT) as LPARAM,
                )
            };
            if send_result == 0 {
                return Err(TestkitError::Transport(format!(
                    "SendMessageW returned zero for command id {command_id} to pid {} hwnd 0x{:x}",
                    self.pid, self.hwnd as usize
                )));
            }

            let response_bytes = self.read_response_bytes()?;
            let response: RawResponseEnvelope<Response> = serde_json::from_slice(response_bytes)?;
            if let Some(response_id) = response.id.as_deref()
                && response_id != request_id
            {
                return Err(TestkitError::InvalidResponse(format!(
                    "response id {} did not match request id {}",
                    response_id, request_id
                )));
            }

            if response.ok {
                response.json.ok_or_else(|| {
                    TestkitError::InvalidResponse(format!(
                        "successful response for command id {command_id} did not include json"
                    ))
                })
            } else {
                let failure = response.err.ok_or_else(|| {
                    TestkitError::InvalidResponse(format!(
                        "failed response for command id {command_id} did not include err"
                    ))
                })?;
                Err(TestkitError::Protocol {
                    code: failure.code,
                    message: failure.message,
                })
            }
        }

        fn read_response_bytes(&self) -> Result<&[u8], TestkitError> {
            if self.response_view.is_null() {
                return Err(TestkitError::Transport(format!(
                    "response mapping was not mapped for pid {} hwnd 0x{:x}",
                    self.pid, self.hwnd as usize
                )));
            }

            let prefix_len = size_of::<u32>();
            let buffer = unsafe {
                std::slice::from_raw_parts(self.response_view, RESPONSE_MAPPING_CAPACITY)
            };
            let response_len =
                u32::from_le_bytes(buffer[..prefix_len].try_into().unwrap()) as usize;
            if response_len == 0 {
                return Err(TestkitError::InvalidResponse(
                    "response envelope length prefix was zero".to_owned(),
                ));
            }
            if response_len > RESPONSE_JSON_CAPACITY {
                return Err(TestkitError::InvalidResponse(format!(
                    "response envelope length {} exceeded maximum {}",
                    response_len, RESPONSE_JSON_CAPACITY
                )));
            }

            Ok(&buffer[prefix_len..prefix_len + response_len])
        }
    }

    struct WindowSearch {
        pid_filter: Option<u32>,
        hwnd: HWND,
        pid: u32,
        mapping: HANDLE,
    }

    fn attach_to_window(pid_filter: Option<u32>) -> Result<AppHandle, TestkitError> {
        let mut search = WindowSearch {
            pid_filter,
            hwnd: null_mut(),
            pid: 0,
            mapping: null_mut(),
        };

        unsafe {
            EnumWindows(
                Some(enum_windows_callback),
                (&mut search as *mut WindowSearch) as LPARAM,
            );
        }

        if search.hwnd.is_null() || search.mapping.is_null() {
            return Err(TestkitError::Transport(match pid_filter {
                Some(pid) => format!("no automation-enabled mmatrix window found for pid {pid}"),
                None => "no automation-enabled mmatrix window found".to_owned(),
            }));
        }

        let view = unsafe {
            MapViewOfFile(
                search.mapping,
                FILE_MAP_READ,
                0,
                0,
                RESPONSE_MAPPING_CAPACITY,
            )
        };
        if view.Value.is_null() {
            unsafe {
                CloseHandle(search.mapping);
            }
            return Err(TestkitError::Transport(last_os_error("MapViewOfFile")));
        }

        Ok(AppHandle {
            hwnd: search.hwnd,
            pid: search.pid,
            response_mapping: search.mapping,
            response_view: view.Value.cast(),
            owned_child: None,
        })
    }

    unsafe extern "system" fn enum_windows_callback(hwnd: HWND, lparam: LPARAM) -> i32 {
        let search = &mut *(lparam as *mut WindowSearch);
        let Some(class_name) = class_name(hwnd) else {
            return 1;
        };
        if class_name != FRAME_CLASS_NAME {
            return 1;
        }

        let mut pid = 0;
        GetWindowThreadProcessId(hwnd, &mut pid);
        if pid == 0 {
            return 1;
        }
        if let Some(expected_pid) = search.pid_filter
            && pid != expected_pid
        {
            return 1;
        }

        let mapping_name = wide_null(&response_mapping_name(pid, hwnd));
        let mapping = OpenFileMappingW(FILE_MAP_READ, 0, mapping_name.as_ptr());
        if mapping.is_null() {
            return 1;
        }

        search.hwnd = hwnd;
        search.pid = pid;
        search.mapping = mapping;
        0
    }

    unsafe fn class_name(hwnd: HWND) -> Option<String> {
        let mut buffer = [0u16; 128];
        let len = GetClassNameW(hwnd, buffer.as_mut_ptr(), buffer.len() as i32);
        if len <= 0 {
            return None;
        }
        Some(String::from_utf16_lossy(&buffer[..len as usize]))
    }

    fn response_mapping_name(pid: u32, hwnd: HWND) -> String {
        format!(
            "Local\\mmatrix-ui-automation-response-{pid}-{:x}",
            hwnd as usize
        )
    }

    fn next_request_id() -> String {
        format!(
            "request-{}",
            REQUEST_COUNTER.fetch_add(1, Ordering::Relaxed)
        )
    }

    fn duration_millis_u64(duration: Duration) -> u64 {
        duration.as_millis().min(u64::MAX as u128) as u64
    }

    fn wide_null(text: &str) -> Vec<u16> {
        OsStr::new(text).encode_wide().chain([0]).collect()
    }

    fn last_os_error(operation: &str) -> String {
        let error = std::io::Error::last_os_error();
        format!("{operation} failed: {error}")
    }
}
