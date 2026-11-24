// A GUI program: started from Explorer or a shortcut, no console opens beside
// it. The bin's unit-test harness stays a console program so `cargo test`
// prints its results.
#![cfg_attr(all(windows, not(test)), windows_subsystem = "windows")]
// The UI-state dump tests compare whole `json!` literals, deeper than the
// default limit expands.
#![recursion_limit = "256"]

use std::env;
use std::ffi::OsString;
use std::process::ExitCode;

// The automation harness (the `automation` feature; the unit tests always
// have it): the protocol, and the launches that turn it on.
#[cfg(any(test, feature = "automation"))]
mod automation;
#[cfg(any(test, feature = "automation"))]
mod automation_launch;
// What the frame and the protocol share; the non-Windows stub has no frame.
#[cfg(any(windows, test, feature = "automation"))]
mod shared;

#[cfg(windows)]
mod dpapi;
#[cfg(not(windows))]
mod non_windows;
#[cfg(windows)]
mod win32;

#[cfg(any(test, feature = "automation"))]
use automation_launch::{AutomationLaunch, HELP, USAGE};
#[cfg(not(windows))]
use non_windows as platform;
#[cfg(windows)]
use win32 as platform;

#[cfg(not(any(test, feature = "automation")))]
const USAGE: &str = "usage: mirx-ui [--auto-close|--help]";
#[cfg(not(any(test, feature = "automation")))]
const HELP: &str = "\
usage: mirx-ui [--auto-close|--help]
  --auto-close  start, then close by itself (a start-up check)
  --help        show this help";

/// The frame's size, in physical pixels.
#[cfg(any(windows, test, feature = "automation"))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct FrameSize {
    width: i32,
    height: i32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum AppCommand {
    /// mirx, for the person at the screen.
    Run,
    /// mirx driven by the automation protocol.
    #[cfg(any(test, feature = "automation"))]
    Automation(AutomationLaunch),
    /// The frame checks itself through the protocol, then closes.
    #[cfg(any(test, feature = "automation"))]
    SelfTest,
    AutoClose,
    Help,
}

fn main() -> ExitCode {
    match parse_args(env::args_os().skip(1)) {
        Ok(command) => platform::run(command),
        Err(()) => platform::usage_error(USAGE),
    }
}

fn parse_args(args: impl IntoIterator<Item = OsString>) -> Result<AppCommand, ()> {
    let args = args.into_iter().collect::<Vec<_>>();
    match args.as_slice() {
        [] => Ok(AppCommand::Run),
        [flag] if flag == "--auto-close" => Ok(AppCommand::AutoClose),
        [flag] if flag == "--help" => Ok(AppCommand::Help),
        #[cfg(any(test, feature = "automation"))]
        args => automation_launch::parse_args(args),
        #[cfg(not(any(test, feature = "automation")))]
        _ => Err(()),
    }
}

#[cfg(test)]
mod tests {
    use std::ffi::OsString;

    use super::{AppCommand, parse_args};

    fn parse(args: &[&str]) -> Result<AppCommand, ()> {
        parse_args(args.iter().map(OsString::from))
    }

    #[test]
    fn parses_the_releases_commands() {
        assert_eq!(parse(&[]), Ok(AppCommand::Run));
        assert_eq!(parse(&["--auto-close"]), Ok(AppCommand::AutoClose));
        assert_eq!(parse(&["--help"]), Ok(AppCommand::Help));
    }

    #[test]
    fn the_releases_commands_take_nothing_more() {
        for args in [
            &["--auto-close", "--help"][..],
            &["--help", "--auto-close"],
            &["--auto-close", "--auto-close"],
            &["--help", "--automation"],
            &["--auto-close", "--self-test"],
            &["--unknown"],
            &["run"],
        ] {
            assert_eq!(parse(args), Err(()), "{args:?}");
        }
    }
}
