#![cfg(not(windows))]

use std::process::{Command, Output};

fn run_ui(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_mirx-ui"))
        .args(args)
        .output()
        .expect("run mirx-ui")
}

fn assert_output(output: Output, code: i32, stdout: &str, stderr: &str) {
    assert_eq!(output.status.code(), Some(code));
    assert_eq!(
        String::from_utf8(output.stdout).expect("stdout utf-8"),
        stdout
    );
    assert_eq!(
        String::from_utf8(output.stderr).expect("stderr utf-8"),
        stderr
    );
}

#[test]
fn no_args_runs_non_windows_stub() {
    assert_output(run_ui(&[]), 0, "mirx-ui non-windows stub\n", "");
}

#[test]
fn auto_close_runs_non_windows_stub() {
    assert_output(
        run_ui(&["--auto-close"]),
        0,
        "mirx-ui auto-close stub\n",
        "",
    );
}

#[test]
fn invalid_arguments_exit_with_usage_error() {
    let output = run_ui(&["--unknown"]);
    assert_eq!(output.status.code(), Some(2));
    assert_eq!(String::from_utf8(output.stdout).expect("stdout utf-8"), "");
    assert!(
        String::from_utf8(output.stderr)
            .expect("stderr utf-8")
            .starts_with("usage: mirx-ui ")
    );
}

/// The build a user gets: no automation harness, so none of its launches.
#[cfg(not(feature = "automation"))]
mod release {
    use super::{assert_output, run_ui};

    const USAGE: &str = "usage: mirx-ui [--auto-close|--help]\n";

    #[test]
    fn help_lists_only_the_releases_flags() {
        assert_output(
            run_ui(&["--help"]),
            0,
            "\
usage: mirx-ui [--auto-close|--help]
  --auto-close  start, then close by itself (a start-up check)
  --help        show this help
",
            "",
        );
    }

    #[test]
    fn automation_flags_are_refused_without_the_feature() {
        for args in [
            &["--automation"][..],
            &["--render-state", "state.json"],
            &["--frame-size", "640x768"],
            &["--self-test"],
        ] {
            assert_output(run_ui(args), 2, "", USAGE);
        }
    }
}

/// A build with the automation harness (the `automation` feature).
#[cfg(feature = "automation")]
mod automation {
    use super::{assert_output, run_ui};

    const USAGE: &str = "usage: mirx-ui [--automation] [--render-state <file.json>] [--frame-size WIDTHxHEIGHT] | [--self-test|--auto-close|--help]\n";

    #[test]
    fn automation_launches_run_non_windows_stub() {
        for args in [
            &["--automation"][..],
            &["--render-state", "state.json"],
            &["--frame-size", "640x768"],
        ] {
            assert_output(run_ui(args), 0, "mirx-ui non-windows stub\n", "");
        }
    }

    #[test]
    fn self_test_prints_core_self_test_line() {
        assert_output(run_ui(&["--self-test"]), 0, "mirx-ui self-test ok\n", "");
    }

    #[test]
    fn help_prints_extended_usage() {
        assert_output(
            run_ui(&["--help"]),
            0,
            "\
usage: mirx-ui [--automation] [--render-state <file.json>] [--frame-size WIDTHxHEIGHT]
       mirx-ui [--self-test|--auto-close|--help]
  --automation                enable automation control surface
  --render-state <file.json>  load render-state events at startup
  --frame-size WIDTHxHEIGHT   create the frame at an explicit launch size
  --self-test                 create the Win32 shell, check it through automation, and exit
  --auto-close                start, then close by itself (a start-up check)
  --help                      show this help
",
            "",
        );
    }

    #[test]
    fn invalid_frame_size_exits_with_usage_error() {
        assert_output(run_ui(&["--frame-size", "640"]), 2, "", USAGE);
    }
}
