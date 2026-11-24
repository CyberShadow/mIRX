//! Crash reports: a panic anywhere in mirx appends a report to
//! `crash.log` in the profile, shows an error box in an interactive run, and
//! ends the process, in debug builds as in release (`panic = "abort"`).
//!
//! The hook aborts before any unwinding, so a `catch_unwind` would never see
//! a panic: adding one means revisiting this module first.
//!
//! The hook never touches window state (no `state_ref`/`state_mut`, no
//! messages to mirx's windows): everything it needs is captured when it is
//! installed, and the box runs on a thread of its own, because a box on the
//! panicking UI thread would pump its messages and re-enter mirx's window
//! procedures mid-panic.

use std::backtrace::Backtrace;
use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::panic::{self, Location};
use std::path::{Path, PathBuf};
use std::ptr::{null, null_mut};
use std::sync::Mutex;
use std::thread;
use std::time::{SystemTime, UNIX_EPOCH};

use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    MB_ICONERROR, MB_OK, MB_SETFOREGROUND, MB_TOPMOST, MessageBoxW,
};

use super::{FRAME_TITLE, is_interactive, system_local_time, wide_null};
use crate::AppCommand;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum CrashPresentation {
    LogOnly,
    LogAndMessageBox,
}

/// Only a run with a person at the screen gets a box: in automation,
/// render-state, `--self-test` and `--auto-close` runs nothing would close
/// it, and the test would hang instead of seeing the process end.
pub(super) fn crash_presentation(command: &AppCommand) -> CrashPresentation {
    if is_interactive(command) {
        CrashPresentation::LogAndMessageBox
    } else {
        CrashPresentation::LogOnly
    }
}

/// Held, never released, by the first panicking thread: another thread
/// that panics meanwhile waits here until the process ends, so there is
/// one report and one box.
static REPORTING: Mutex<()> = Mutex::new(());

pub(super) fn install(presentation: CrashPresentation, log_path: PathBuf) {
    let default_hook = panic::take_hook();
    panic::set_hook(Box::new(move |info| {
        let _first = REPORTING.lock();
        let now_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock must be after the unix epoch")
            .as_millis() as u64;
        let current = thread::current();
        let report = crash_report(&CrashFacts {
            time: system_local_time(now_ms).asctime(),
            version: env!("CARGO_PKG_VERSION"),
            module_base: unsafe { GetModuleHandleW(null()) } as usize,
            // Unnamed threads and `panic_any` payloads exist.
            thread: current.name().unwrap_or("<unnamed>"),
            message: info
                .payload_as_str()
                .unwrap_or("<non-string panic payload>"),
            location: info.location(),
            backtrace: &Backtrace::force_capture().to_string(),
        });
        let written = append_crash_log(&log_path, &report);
        // stderr as before: tests and Wine logs keep showing panics.
        default_hook(info);
        if presentation == CrashPresentation::LogAndMessageBox {
            let text = box_text(
                info.payload_as_str()
                    .unwrap_or("<non-string panic payload>"),
                &log_path,
                &written,
            );
            // Without a thread there is no box; the log stands.
            if let Ok(shown) = thread::Builder::new().spawn(move || show_box(&text)) {
                let _ = shown.join();
            }
        }
        std::process::abort();
    }));
}

pub(super) struct CrashFacts<'a> {
    pub(super) time: String,
    pub(super) version: &'a str,
    pub(super) module_base: usize,
    pub(super) thread: &'a str,
    pub(super) message: &'a str,
    pub(super) location: Option<&'a Location<'a>>,
    pub(super) backtrace: &'a str,
}

/// The report, CRLF lines. The module base maps a stripped release's raw
/// backtrace addresses onto a same-commit build with symbols.
pub(super) fn crash_report(facts: &CrashFacts) -> String {
    let location = facts.location.map_or_else(
        || "<unknown>".to_owned(),
        |location| {
            format!(
                "{}:{}:{}",
                location.file(),
                location.line(),
                location.column()
            )
        },
    );
    let mut lines = vec![
        format!("mirx {} crashed: {}", facts.version, facts.time),
        format!("Thread: {}", facts.thread),
        format!("Panic: {}", facts.message),
        format!("At: {location}"),
        format!("Module base: 0x{:x}", facts.module_base),
        "Backtrace:".to_owned(),
    ];
    lines.extend(facts.backtrace.lines().map(str::to_owned));
    lines.iter().map(|line| format!("{line}\r\n")).collect()
}

/// Appends `report`, after a blank line if the log already has reports.
pub(super) fn append_crash_log(path: &Path, report: &str) -> io::Result<()> {
    fs::create_dir_all(
        path.parent()
            .expect("the crash log is in the profile folder"),
    )?;
    let mut file = OpenOptions::new().create(true).append(true).open(path)?;
    if file.metadata()?.len() > 0 {
        file.write_all(b"\r\n")?;
    }
    file.write_all(report.as_bytes())
}

fn box_text(message: &str, log_path: &Path, written: &io::Result<()>) -> String {
    let details = match written {
        Ok(()) => format!("Details were written to {}.", log_path.display()),
        Err(error) => format!(
            "The details could not be written to {}: {error}.",
            log_path.display()
        ),
    };
    format!("mirx has stopped because of an internal error.\r\n\r\n{message}\r\n\r\n{details}")
}

fn show_box(text: &str) {
    let title = wide_null(FRAME_TITLE);
    let text = wide_null(text);
    unsafe {
        MessageBoxW(
            null_mut(),
            text.as_ptr(),
            title.as_ptr(),
            MB_OK | MB_ICONERROR | MB_SETFOREGROUND | MB_TOPMOST,
        );
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use crate::automation_launch::AutomationLaunch;

    #[test]
    fn crash_report_has_every_fact_on_crlf_lines() {
        let location = Location::caller();
        let report = crash_report(&CrashFacts {
            time: "Fri Nov 20 14:03:09 2026".to_owned(),
            version: "1.2.3",
            module_base: 0x1_4000_0000,
            thread: "main",
            message: "boom",
            location: Some(location),
            backtrace: "   0: first\n   1: second",
        });
        assert_eq!(
            report,
            format!(
                "mirx 1.2.3 crashed: Fri Nov 20 14:03:09 2026\r\n\
                 Thread: main\r\n\
                 Panic: boom\r\n\
                 At: {}:{}:{}\r\n\
                 Module base: 0x140000000\r\n\
                 Backtrace:\r\n   0: first\r\n   1: second\r\n",
                location.file(),
                location.line(),
                location.column()
            )
        );
    }

    #[test]
    fn crash_log_is_created_with_its_folder_and_appended_after_a_blank_line() {
        let dir = std::env::temp_dir().join(format!(
            "mirx-crash-log-test-{}-{:?}",
            std::process::id(),
            thread::current().id()
        ));
        let _ = fs::remove_dir_all(&dir);
        let path = dir.join("mirx").join("crash.log");
        append_crash_log(&path, "first\r\n").unwrap();
        append_crash_log(&path, "second\r\n").unwrap();
        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            "first\r\n\r\nsecond\r\n"
        );
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn only_an_interactive_run_shows_a_crash_box() {
        let automation = |render_state: Option<&str>| {
            AppCommand::Automation(AutomationLaunch {
                render_state: render_state.map(PathBuf::from),
                frame_size: None,
            })
        };
        assert_eq!(
            crash_presentation(&AppCommand::Run),
            CrashPresentation::LogAndMessageBox
        );
        for command in [
            automation(None),
            automation(Some("state.json")),
            AppCommand::SelfTest,
            AppCommand::AutoClose,
            AppCommand::Help,
        ] {
            assert_eq!(crash_presentation(&command), CrashPresentation::LogOnly);
        }
    }
}
