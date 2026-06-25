use std::env;
use std::ffi::OsString;
use std::process::ExitCode;

fn main() -> ExitCode {
    let mut args = env::args_os();
    let _program = args.next();

    match (args.next(), args.next()) {
        (None, None) => {
            println!("mmatrix-ui foundation stub");
            ExitCode::SUCCESS
        }
        (Some(flag), None) if flag == OsString::from("--self-test") => {
            println!("{}", mmatrix_core::ui_self_test_line());
            ExitCode::SUCCESS
        }
        (Some(flag), None) if flag == OsString::from("--help") => {
            println!("usage: mmatrix-ui [--self-test]");
            ExitCode::SUCCESS
        }
        _ => {
            eprintln!("usage: mmatrix-ui [--self-test]");
            ExitCode::from(2)
        }
    }
}
