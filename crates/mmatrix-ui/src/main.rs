use std::env;
use std::ffi::{OsStr, OsString};
use std::path::PathBuf;
use std::process::ExitCode;

mod automation;

#[cfg(not(windows))]
mod non_windows;
#[cfg(windows)]
mod win32;

#[cfg(not(windows))]
use non_windows as platform;
#[cfg(windows)]
use win32 as platform;

const USAGE: &str = "usage: mmatrix-ui [--automation] [--render-state <file.json>] | [--self-test|--auto-close|--help]";
const HELP: &str = "\
usage: mmatrix-ui [--automation] [--render-state <file.json>]
       mmatrix-ui [--self-test|--auto-close|--help]
  --automation                enable automation control surface
  --render-state <file.json>  load synthetic render state at startup
  --self-test                 create the Win32 shell, verify the edit control, and exit
  --auto-close                create the Win32 shell and close it automatically
  --help                      show this help";

#[derive(Debug, Clone, PartialEq, Eq)]
struct RunOptions {
    automation: bool,
    render_state: Option<PathBuf>,
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
        Err(()) => {
            eprintln!("{USAGE}");
            ExitCode::from(2)
        }
    }
}

fn parse_args(args: impl IntoIterator<Item = OsString>) -> Result<AppCommand, ()> {
    let mut run_options = RunOptions {
        automation: false,
        render_state: None,
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
                run_options.automation = true;
                run_options.render_state = Some(PathBuf::from(path));
            }
            flag if flag == OsStr::new("--self-test") => {
                if lifecycle_command.is_some()
                    || run_options.automation
                    || run_options.render_state.is_some()
                {
                    return Err(());
                }
                lifecycle_command = Some(AppCommand::SelfTest);
            }
            flag if flag == OsStr::new("--auto-close") => {
                if lifecycle_command.is_some()
                    || run_options.automation
                    || run_options.render_state.is_some()
                {
                    return Err(());
                }
                lifecycle_command = Some(AppCommand::AutoClose);
            }
            flag if flag == OsStr::new("--help") => {
                if lifecycle_command.is_some()
                    || run_options.automation
                    || run_options.render_state.is_some()
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

#[cfg(test)]
mod tests {
    use std::ffi::OsString;
    use std::path::PathBuf;

    use super::{AppCommand, RunOptions, parse_args};

    #[test]
    fn parses_run_modes() {
        assert_eq!(
            parse_args(Vec::<OsString>::new()),
            Ok(AppCommand::Run(RunOptions {
                automation: false,
                render_state: None,
            }))
        );
        assert_eq!(
            parse_args([OsString::from("--automation")]),
            Ok(AppCommand::Run(RunOptions {
                automation: true,
                render_state: None,
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
            }))
        );
        let expected = Ok(AppCommand::Run(RunOptions {
            automation: true,
            render_state: Some(PathBuf::from("state.json")),
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
                OsString::from("state-a.json"),
                OsString::from("--render-state"),
                OsString::from("state-b.json"),
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
    }
}
