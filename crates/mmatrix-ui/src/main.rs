use std::env;
use std::ffi::{OsStr, OsString};
use std::process::ExitCode;

#[cfg(not(windows))]
mod non_windows;
#[cfg(windows)]
mod win32;

#[cfg(not(windows))]
use non_windows as platform;
#[cfg(windows)]
use win32 as platform;

const USAGE: &str = "usage: mmatrix-ui [--self-test|--auto-close|--help]";
const HELP: &str = "\
usage: mmatrix-ui [--self-test|--auto-close|--help]
  --self-test   create the Win32 shell, verify the edit control, and exit
  --auto-close  create the Win32 shell and close it automatically
  --help        show this help";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AppCommand {
    Run,
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
    let args: Vec<OsString> = args.into_iter().collect();

    match args.as_slice() {
        [] => Ok(AppCommand::Run),
        [flag] if flag == OsStr::new("--self-test") => Ok(AppCommand::SelfTest),
        [flag] if flag == OsStr::new("--auto-close") => Ok(AppCommand::AutoClose),
        [flag] if flag == OsStr::new("--help") => Ok(AppCommand::Help),
        _ => Err(()),
    }
}

#[cfg(test)]
mod tests {
    use std::ffi::OsString;

    use super::{AppCommand, parse_args};

    #[test]
    fn parses_supported_commands() {
        assert_eq!(parse_args(Vec::<OsString>::new()), Ok(AppCommand::Run));
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
        assert_eq!(
            parse_args([OsString::from("--self-test"), OsString::from("--extra")]),
            Err(())
        );
    }
}
