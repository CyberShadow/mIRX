#![cfg(not(windows))]

use std::process::{Command, Output};

fn run_ui(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_mmatrix-ui"))
        .args(args)
        .output()
        .expect("run mmatrix-ui")
}

#[test]
fn no_args_runs_non_windows_stub() {
    let output = run_ui(&[]);

    assert_eq!(output.status.code(), Some(0));
    assert_eq!(
        String::from_utf8(output.stdout).expect("stdout utf-8"),
        "mmatrix-ui non-windows stub\n"
    );
    assert_eq!(String::from_utf8(output.stderr).expect("stderr utf-8"), "");
}

#[test]
fn automation_flag_runs_non_windows_stub() {
    let output = run_ui(&["--automation"]);

    assert_eq!(output.status.code(), Some(0));
    assert_eq!(
        String::from_utf8(output.stdout).expect("stdout utf-8"),
        "mmatrix-ui non-windows stub\n"
    );
    assert_eq!(String::from_utf8(output.stderr).expect("stderr utf-8"), "");
}

#[test]
fn render_state_runs_non_windows_stub() {
    let output = run_ui(&["--render-state", "state.json"]);

    assert_eq!(output.status.code(), Some(0));
    assert_eq!(
        String::from_utf8(output.stdout).expect("stdout utf-8"),
        "mmatrix-ui non-windows stub\n"
    );
    assert_eq!(String::from_utf8(output.stderr).expect("stderr utf-8"), "");
}

#[test]
fn frame_size_runs_non_windows_stub() {
    let output = run_ui(&["--frame-size", "640x768"]);

    assert_eq!(output.status.code(), Some(0));
    assert_eq!(
        String::from_utf8(output.stdout).expect("stdout utf-8"),
        "mmatrix-ui non-windows stub\n"
    );
    assert_eq!(String::from_utf8(output.stderr).expect("stderr utf-8"), "");
}

#[test]
fn self_test_prints_core_self_test_line() {
    let output = run_ui(&["--self-test"]);

    assert_eq!(output.status.code(), Some(0));
    assert_eq!(
        String::from_utf8(output.stdout).expect("stdout utf-8"),
        "mmatrix-ui self-test ok\n"
    );
    assert_eq!(String::from_utf8(output.stderr).expect("stderr utf-8"), "");
}

#[test]
fn auto_close_runs_non_windows_stub() {
    let output = run_ui(&["--auto-close"]);

    assert_eq!(output.status.code(), Some(0));
    assert_eq!(
        String::from_utf8(output.stdout).expect("stdout utf-8"),
        "mmatrix-ui auto-close stub\n"
    );
    assert_eq!(String::from_utf8(output.stderr).expect("stderr utf-8"), "");
}

#[test]
fn help_prints_extended_usage() {
    let output = run_ui(&["--help"]);

    assert_eq!(output.status.code(), Some(0));
    assert_eq!(
        String::from_utf8(output.stdout).expect("stdout utf-8"),
        "\
usage: mmatrix-ui [--automation] [--render-state <file.json>] [--frame-size WIDTHxHEIGHT]
       mmatrix-ui [--self-test|--auto-close|--help]
  --automation                enable automation control surface
  --render-state <file.json>  load render-state events at startup
  --frame-size WIDTHxHEIGHT   create the frame at an explicit launch size
  --self-test                 create the Win32 shell, verify the edit control, and exit
  --auto-close                create the Win32 shell and close it automatically
  --help                      show this help
"
    );
    assert_eq!(String::from_utf8(output.stderr).expect("stderr utf-8"), "");
}

#[test]
fn invalid_arguments_exit_with_usage_error() {
    let output = run_ui(&["--unknown"]);

    assert_eq!(output.status.code(), Some(2));
    assert_eq!(String::from_utf8(output.stdout).expect("stdout utf-8"), "");
    assert_eq!(
        String::from_utf8(output.stderr).expect("stderr utf-8"),
        "usage: mmatrix-ui [--automation] [--render-state <file.json>] [--frame-size WIDTHxHEIGHT] | [--self-test|--auto-close|--help]\n"
    );
}

#[test]
fn invalid_frame_size_exits_with_usage_error() {
    let output = run_ui(&["--frame-size", "640"]);

    assert_eq!(output.status.code(), Some(2));
    assert_eq!(String::from_utf8(output.stdout).expect("stdout utf-8"), "");
    assert_eq!(
        String::from_utf8(output.stderr).expect("stderr utf-8"),
        "usage: mmatrix-ui [--automation] [--render-state <file.json>] [--frame-size WIDTHxHEIGHT] | [--self-test|--auto-close|--help]\n"
    );
}
