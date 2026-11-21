//! The Send File dialog: the standard Windows Open dialog, opened from a
//! room window's window menu (`Send File...`) and from `/sendfile` without
//! a path, as mIRC's DCC Send dialog opens from `/dcc send`.

use std::path::PathBuf;

use windows_sys::Win32::Foundation::HWND;

use super::file_dialogs::{self, FileDialog};

fn dialog_title(room_name: &str) -> String {
    format!("Send File to {room_name}")
}

/// Runs the Open dialog owned by `owner` and returns the chosen file, or
/// `None` when the user cancels. The dialog is modal and pumps messages, so
/// the caller must hold no window state borrow across it.
pub(super) unsafe fn choose_file_to_send(
    owner: HWND,
    room_name: &str,
) -> Result<Option<PathBuf>, String> {
    file_dialogs::choose_file(
        owner,
        &dialog_title(room_name),
        file_dialogs::ALL_FILES,
        FileDialog::Open,
    )
}

#[cfg(test)]
mod tests {
    use super::dialog_title;

    #[test]
    fn dialog_title_names_the_room() {
        assert_eq!(dialog_title("Ops"), "Send File to Ops");
    }
}
