use std::process::ExitCode;

use crate::{AppCommand, HELP};

pub(crate) fn run(command: AppCommand) -> ExitCode {
    match command {
        AppCommand::Run(_) => {
            println!("mmatrix-ui non-windows stub");
            ExitCode::SUCCESS
        }
        AppCommand::SelfTest => {
            println!("{}", mmatrix_core::ui_self_test_line());
            ExitCode::SUCCESS
        }
        AppCommand::AutoClose => {
            println!("mmatrix-ui auto-close stub");
            ExitCode::SUCCESS
        }
        AppCommand::Help => {
            println!("{HELP}");
            ExitCode::SUCCESS
        }
    }
}
