// A GUI program: started from Explorer or a shortcut, no console opens beside
// it. The bin's unit-test harness stays a console program so `cargo test`
// prints its results.
#![cfg_attr(all(windows, not(test)), windows_subsystem = "windows")]
// The UI-state dump tests compare whole `json!` literals, deeper than the
// default limit expands.
#![recursion_limit = "256"]

use std::env;
use std::ffi::{OsStr, OsString};
use std::path::PathBuf;
use std::process::ExitCode;

mod automation;

#[cfg(windows)]
mod dpapi;
#[cfg(not(windows))]
mod non_windows;
#[cfg(windows)]
mod win32;

#[cfg(not(windows))]
use non_windows as platform;
#[cfg(windows)]
use win32 as platform;

const USAGE: &str = "usage: mirx-ui [--automation] [--render-state <file.json>] [--frame-size WIDTHxHEIGHT] | [--self-test|--auto-close|--help]";
const HELP: &str = "\
usage: mirx-ui [--automation] [--render-state <file.json>] [--frame-size WIDTHxHEIGHT]
       mirx-ui [--self-test|--auto-close|--help]
  --automation                enable automation control surface
  --render-state <file.json>  load render-state events at startup
  --frame-size WIDTHxHEIGHT   create the frame at an explicit launch size
  --self-test                 create the Win32 shell, verify the edit control, and exit
  --auto-close                create the Win32 shell and close it automatically
  --help                      show this help";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct FrameSize {
    width: i32,
    height: i32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct RunOptions {
    automation: bool,
    render_state: Option<PathBuf>,
    frame_size: Option<FrameSize>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum AppCommand {
    Run(RunOptions),
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
    let mut run_options = RunOptions {
        automation: false,
        render_state: None,
        frame_size: None,
    };
    let mut lifecycle_command = None;
    let mut args = args.into_iter();

    while let Some(arg) = args.next() {
        match arg.as_os_str() {
            flag if flag == OsStr::new("--automation") => {
                if lifecycle_command.is_some() {
                    return Err(());
                }
                run_options.automation = true;
            }
            flag if flag == OsStr::new("--render-state") => {
                if lifecycle_command.is_some() || run_options.render_state.is_some() {
                    return Err(());
                }

                let path = args.next().ok_or(())?;
                if !is_valid_render_state_path(&path) {
                    return Err(());
                }
                run_options.automation = true;
                run_options.render_state = Some(PathBuf::from(path));
            }
            flag if flag == OsStr::new("--frame-size") => {
                if lifecycle_command.is_some() || run_options.frame_size.is_some() {
                    return Err(());
                }

                let value = args.next().ok_or(())?;
                if is_known_option(&value) {
                    return Err(());
                }
                run_options.frame_size = Some(parse_frame_size_arg(&value)?);
            }
            flag if flag == OsStr::new("--self-test") => {
                if lifecycle_command.is_some()
                    || run_options.automation
                    || run_options.render_state.is_some()
                    || run_options.frame_size.is_some()
                {
                    return Err(());
                }
                lifecycle_command = Some(AppCommand::SelfTest);
            }
            flag if flag == OsStr::new("--auto-close") => {
                if lifecycle_command.is_some()
                    || run_options.automation
                    || run_options.render_state.is_some()
                    || run_options.frame_size.is_some()
                {
                    return Err(());
                }
                lifecycle_command = Some(AppCommand::AutoClose);
            }
            flag if flag == OsStr::new("--help") => {
                if lifecycle_command.is_some()
                    || run_options.automation
                    || run_options.render_state.is_some()
                    || run_options.frame_size.is_some()
                {
                    return Err(());
                }
                lifecycle_command = Some(AppCommand::Help);
            }
            _ => return Err(()),
        }
    }

    Ok(lifecycle_command.unwrap_or(AppCommand::Run(run_options)))
}

fn parse_frame_size_arg(arg: &OsString) -> Result<FrameSize, ()> {
    let value = arg.to_str().ok_or(())?;
    let mut parts = value.split('x');
    let width = parts.next().ok_or(())?;
    let height = parts.next().ok_or(())?;
    if width.is_empty() || height.is_empty() || parts.next().is_some() {
        return Err(());
    }

    let width = width.parse::<i32>().map_err(|_| ())?;
    let height = height.parse::<i32>().map_err(|_| ())?;
    if width <= 0 || height <= 0 {
        return Err(());
    }

    Ok(FrameSize { width, height })
}

fn is_valid_render_state_path(arg: &OsString) -> bool {
    if is_known_option(arg) {
        return false;
    }

    let path = arg.to_string_lossy();
    if path.starts_with("--") {
        return path.ends_with(".json");
    }

    true
}

fn is_known_option(arg: &OsString) -> bool {
    matches!(
        arg.as_os_str(),
        flag
            if flag == OsStr::new("--automation")
                || flag == OsStr::new("--render-state")
                || flag == OsStr::new("--frame-size")
                || flag == OsStr::new("--self-test")
                || flag == OsStr::new("--auto-close")
                || flag == OsStr::new("--help")
    )
}

#[cfg(test)]
mod tests {
    use std::ffi::OsString;
    use std::path::PathBuf;

    use super::{AppCommand, FrameSize, RunOptions, parse_args};

    #[test]
    fn parses_run_modes() {
        assert_eq!(
            parse_args(Vec::<OsString>::new()),
            Ok(AppCommand::Run(RunOptions {
                automation: false,
                render_state: None,
                frame_size: None,
            }))
        );
        assert_eq!(
            parse_args([OsString::from("--automation")]),
            Ok(AppCommand::Run(RunOptions {
                automation: true,
                render_state: None,
                frame_size: None,
            }))
        );
        assert_eq!(
            parse_args([
                OsString::from("--render-state"),
                OsString::from("state.json")
            ]),
            Ok(AppCommand::Run(RunOptions {
                automation: true,
                render_state: Some(PathBuf::from("state.json")),
                frame_size: None,
            }))
        );
        assert_eq!(
            parse_args([
                OsString::from("--render-state"),
                OsString::from("--state.json"),
            ]),
            Ok(AppCommand::Run(RunOptions {
                automation: true,
                render_state: Some(PathBuf::from("--state.json")),
                frame_size: None,
            }))
        );
        let expected = Ok(AppCommand::Run(RunOptions {
            automation: true,
            render_state: Some(PathBuf::from("state.json")),
            frame_size: None,
        }));
        assert_eq!(
            parse_args([
                OsString::from("--automation"),
                OsString::from("--render-state"),
                OsString::from("state.json"),
            ]),
            expected.clone()
        );
        assert_eq!(
            parse_args([
                OsString::from("--render-state"),
                OsString::from("state.json"),
                OsString::from("--automation"),
            ]),
            expected
        );
        assert_eq!(
            parse_args([OsString::from("--frame-size"), OsString::from("640x768"),]),
            Ok(AppCommand::Run(RunOptions {
                automation: false,
                render_state: None,
                frame_size: Some(FrameSize {
                    width: 640,
                    height: 768,
                }),
            }))
        );
        assert_eq!(
            parse_args([
                OsString::from("--render-state"),
                OsString::from("state.json"),
                OsString::from("--frame-size"),
                OsString::from("640x768"),
            ]),
            Ok(AppCommand::Run(RunOptions {
                automation: true,
                render_state: Some(PathBuf::from("state.json")),
                frame_size: Some(FrameSize {
                    width: 640,
                    height: 768,
                }),
            }))
        );
    }

    #[test]
    fn parses_single_lifecycle_commands() {
        assert_eq!(
            parse_args([OsString::from("--self-test")]),
            Ok(AppCommand::SelfTest)
        );
        assert_eq!(
            parse_args([OsString::from("--auto-close")]),
            Ok(AppCommand::AutoClose)
        );
        assert_eq!(parse_args([OsString::from("--help")]), Ok(AppCommand::Help));
    }

    #[test]
    fn rejects_unsupported_arguments() {
        assert_eq!(parse_args([OsString::from("--unknown")]), Err(()));
        assert_eq!(parse_args([OsString::from("--render-state")]), Err(()));
        assert_eq!(
            parse_args([
                OsString::from("--render-state"),
                OsString::from("--unknown"),
            ]),
            Err(())
        );
        assert_eq!(
            parse_args([
                OsString::from("--render-state"),
                OsString::from("--render-state"),
            ]),
            Err(())
        );
        assert_eq!(
            parse_args([
                OsString::from("--render-state"),
                OsString::from("--automation")
            ]),
            Err(())
        );
        assert_eq!(
            parse_args([
                OsString::from("--render-state"),
                OsString::from("--self-test")
            ]),
            Err(())
        );
        assert_eq!(
            parse_args([
                OsString::from("--render-state"),
                OsString::from("--auto-close")
            ]),
            Err(())
        );
        assert_eq!(
            parse_args([OsString::from("--render-state"), OsString::from("--help")]),
            Err(())
        );
        assert_eq!(
            parse_args([
                OsString::from("--render-state"),
                OsString::from("state-a.json"),
                OsString::from("--render-state"),
                OsString::from("state-b.json"),
            ]),
            Err(())
        );
        assert_eq!(parse_args([OsString::from("--frame-size")]), Err(()));
        assert_eq!(
            parse_args([
                OsString::from("--frame-size"),
                OsString::from("--automation"),
            ]),
            Err(())
        );
        assert_eq!(
            parse_args([OsString::from("--frame-size"), OsString::from("640"),]),
            Err(())
        );
        assert_eq!(
            parse_args([OsString::from("--frame-size"), OsString::from("640X768"),]),
            Err(())
        );
        assert_eq!(
            parse_args([OsString::from("--frame-size"), OsString::from("x768"),]),
            Err(())
        );
        assert_eq!(
            parse_args([OsString::from("--frame-size"), OsString::from("640x"),]),
            Err(())
        );
        assert_eq!(
            parse_args([OsString::from("--frame-size"), OsString::from("0x768"),]),
            Err(())
        );
        assert_eq!(
            parse_args([OsString::from("--frame-size"), OsString::from("640x0"),]),
            Err(())
        );
        assert_eq!(
            parse_args([
                OsString::from("--frame-size"),
                OsString::from("-1x768"),
                OsString::from("--automation"),
            ]),
            Err(())
        );
        assert_eq!(
            parse_args([OsString::from("--frame-size"), OsString::from("640x768x1"),]),
            Err(())
        );
        assert_eq!(
            parse_args([
                OsString::from("--frame-size"),
                OsString::from("640x768"),
                OsString::from("--frame-size"),
                OsString::from("800x600"),
            ]),
            Err(())
        );
        assert_eq!(
            parse_args([
                OsString::from("--self-test"),
                OsString::from("--automation")
            ]),
            Err(())
        );
        assert_eq!(
            parse_args([
                OsString::from("--self-test"),
                OsString::from("--render-state"),
                OsString::from("state.json"),
            ]),
            Err(())
        );
        assert_eq!(
            parse_args([
                OsString::from("--self-test"),
                OsString::from("--frame-size"),
                OsString::from("640x768"),
            ]),
            Err(())
        );
        assert_eq!(
            parse_args([
                OsString::from("--auto-close"),
                OsString::from("--automation")
            ]),
            Err(())
        );
        assert_eq!(
            parse_args([
                OsString::from("--auto-close"),
                OsString::from("--render-state"),
                OsString::from("state.json"),
            ]),
            Err(())
        );
        assert_eq!(
            parse_args([
                OsString::from("--auto-close"),
                OsString::from("--frame-size"),
                OsString::from("640x768"),
            ]),
            Err(())
        );
        assert_eq!(
            parse_args([
                OsString::from("--frame-size"),
                OsString::from("640x768"),
                OsString::from("--help"),
            ]),
            Err(())
        );
    }
}
