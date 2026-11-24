//! The launches that turn the automation harness on: `--automation`, which a
//! render-state snapshot (`--render-state <file.json>`) and an explicit
//! frame size (`--frame-size WIDTHxHEIGHT`) imply, and `--self-test`.

use std::ffi::{OsStr, OsString};
use std::path::PathBuf;

use crate::{AppCommand, FrameSize};

pub(crate) const USAGE: &str = "usage: mirx-ui [--automation] [--render-state <file.json>] [--frame-size WIDTHxHEIGHT] | [--self-test|--auto-close|--help]";
pub(crate) const HELP: &str = "\
usage: mirx-ui [--automation] [--render-state <file.json>] [--frame-size WIDTHxHEIGHT]
       mirx-ui [--self-test|--auto-close|--help]
  --automation                enable automation control surface
  --render-state <file.json>  load render-state events at startup
  --frame-size WIDTHxHEIGHT   create the frame at an explicit launch size
  --self-test                 create the Win32 shell, check it through automation, and exit
  --auto-close                start, then close by itself (a start-up check)
  --help                      show this help";

/// A run the automation protocol drives.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct AutomationLaunch {
    /// The render-state file whose events load at startup, into a frame
    /// without a core or a profile.
    pub(crate) render_state: Option<PathBuf>,
    /// The frame's launch size, in physical pixels.
    pub(crate) frame_size: Option<FrameSize>,
}

/// The harness's launches; `crate::parse_args` has taken the release's.
pub(crate) fn parse_args(args: &[OsString]) -> Result<AppCommand, ()> {
    if let [flag] = args
        && flag == "--self-test"
    {
        return Ok(AppCommand::SelfTest);
    }

    let mut automation = false;
    let mut launch = AutomationLaunch::default();
    let mut args = args.iter();
    while let Some(arg) = args.next() {
        match arg.to_str().ok_or(())? {
            "--automation" if !automation => automation = true,
            "--render-state" if launch.render_state.is_none() => {
                let path = args.next().ok_or(())?;
                if !is_valid_render_state_path(path) {
                    return Err(());
                }
                launch.render_state = Some(PathBuf::from(path));
            }
            "--frame-size" if launch.frame_size.is_none() => {
                let value = args.next().ok_or(())?;
                if is_known_option(value) {
                    return Err(());
                }
                launch.frame_size = Some(parse_frame_size_arg(value)?);
            }
            _ => return Err(()),
        }
    }
    Ok(AppCommand::Automation(launch))
}

fn parse_frame_size_arg(arg: &OsStr) -> Result<FrameSize, ()> {
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

fn is_valid_render_state_path(arg: &OsStr) -> bool {
    if is_known_option(arg) {
        return false;
    }

    let path = arg.to_string_lossy();
    if path.starts_with("--") {
        return path.ends_with(".json");
    }

    true
}

fn is_known_option(arg: &OsStr) -> bool {
    [
        "--automation",
        "--render-state",
        "--frame-size",
        "--self-test",
        "--auto-close",
        "--help",
    ]
    .iter()
    .any(|option| arg == *option)
}

#[cfg(test)]
mod tests {
    use std::ffi::OsString;
    use std::path::PathBuf;

    use super::AutomationLaunch;
    use crate::{AppCommand, FrameSize, parse_args};

    fn parse(args: &[&str]) -> Result<AppCommand, ()> {
        parse_args(args.iter().map(OsString::from))
    }

    fn launch(render_state: Option<&str>, frame_size: Option<(i32, i32)>) -> AppCommand {
        AppCommand::Automation(AutomationLaunch {
            render_state: render_state.map(PathBuf::from),
            frame_size: frame_size.map(|(width, height)| FrameSize { width, height }),
        })
    }

    #[test]
    fn parses_automation_launches() {
        assert_eq!(parse(&["--automation"]), Ok(launch(None, None)));
        assert_eq!(
            parse(&["--render-state", "state.json"]),
            Ok(launch(Some("state.json"), None))
        );
        assert_eq!(
            parse(&["--render-state", "--state.json"]),
            Ok(launch(Some("--state.json"), None))
        );
        assert_eq!(
            parse(&["--automation", "--render-state", "state.json"]),
            Ok(launch(Some("state.json"), None))
        );
        assert_eq!(
            parse(&["--render-state", "state.json", "--automation"]),
            Ok(launch(Some("state.json"), None))
        );
        assert_eq!(
            parse(&["--frame-size", "640x768"]),
            Ok(launch(None, Some((640, 768))))
        );
        assert_eq!(
            parse(&["--render-state", "state.json", "--frame-size", "640x768"]),
            Ok(launch(Some("state.json"), Some((640, 768))))
        );
        assert_eq!(parse(&["--self-test"]), Ok(AppCommand::SelfTest));
    }

    #[test]
    fn rejects_unsupported_automation_arguments() {
        for args in [
            &["--unknown"][..],
            &["--automation", "--automation"],
            &["--render-state"],
            &["--render-state", "--unknown"],
            &["--render-state", "--render-state"],
            &["--render-state", "--automation"],
            &["--render-state", "--self-test"],
            &["--render-state", "--auto-close"],
            &["--render-state", "--help"],
            &[
                "--render-state",
                "state-a.json",
                "--render-state",
                "state-b.json",
            ],
            &["--frame-size"],
            &["--frame-size", "--automation"],
            &["--frame-size", "640"],
            &["--frame-size", "640X768"],
            &["--frame-size", "x768"],
            &["--frame-size", "640x"],
            &["--frame-size", "0x768"],
            &["--frame-size", "640x0"],
            &["--frame-size", "-1x768", "--automation"],
            &["--frame-size", "640x768x1"],
            &["--frame-size", "640x768", "--frame-size", "800x600"],
            &["--self-test", "--automation"],
            &["--self-test", "--render-state", "state.json"],
            &["--self-test", "--frame-size", "640x768"],
            &["--self-test", "--self-test"],
            &["--auto-close", "--automation"],
            &["--auto-close", "--render-state", "state.json"],
            &["--auto-close", "--frame-size", "640x768"],
            &["--frame-size", "640x768", "--help"],
        ] {
            assert_eq!(parse(args), Err(()), "{args:?}");
        }
    }
}
