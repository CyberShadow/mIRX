//! The Send File dialog: the standard Windows Open dialog, opened from a
//! room window's window menu (`Send File...`) and from `/sendfile` without
//! a path, as mIRC's DCC Send dialog opens from `/dcc send`.

use std::path::PathBuf;

use windows_sys::Win32::Foundation::HWND;
use windows_sys::Win32::UI::Controls::Dialogs::{
    CommDlgExtendedError, GetOpenFileNameW, OFN_EXPLORER, OFN_FILEMUSTEXIST, OFN_HIDEREADONLY,
    OFN_NOCHANGEDIR, OFN_PATHMUSTEXIST, OPENFILENAMEW,
};

use super::wide_null;

/// The dialog's file types: pairs of a label and a pattern, each
/// NUL-terminated, and the list ends with an empty string.
const FILTER: &str = "All Files (*.*)\0*.*\0\0";

/// Room for a long (`\\?\`) path, in UTF-16 units.
const PATH_CAPACITY: usize = 32768;

fn dialog_title(room_name: &str) -> String {
    format!("Send File to {room_name}")
}

/// Runs the Open dialog owned by `owner` and returns the chosen file, or
/// `None` when the user cancels. The dialog is modal and pumps messages, so
/// the caller must hold no window state borrow across it. It starts in the
/// folder last used, which comdlg32 remembers, and leaves the process's
/// current directory alone.
pub(super) unsafe fn choose_file_to_send(
    owner: HWND,
    room_name: &str,
) -> Result<Option<PathBuf>, String> {
    let filter = FILTER.encode_utf16().collect::<Vec<_>>();
    let title = wide_null(&dialog_title(room_name));
    let mut path = vec![0u16; PATH_CAPACITY];
    let mut open = OPENFILENAMEW {
        lStructSize: size_of::<OPENFILENAMEW>() as u32,
        hwndOwner: owner,
        lpstrFilter: filter.as_ptr(),
        nFilterIndex: 1,
        lpstrFile: path.as_mut_ptr(),
        nMaxFile: PATH_CAPACITY as u32,
        lpstrTitle: title.as_ptr(),
        Flags: OFN_EXPLORER
            | OFN_FILEMUSTEXIST
            | OFN_PATHMUSTEXIST
            | OFN_HIDEREADONLY
            | OFN_NOCHANGEDIR,
        ..OPENFILENAMEW::default()
    };
    if GetOpenFileNameW(&mut open) == 0 {
        return match CommDlgExtendedError() {
            0 => Ok(None),
            code => Err(format!("GetOpenFileNameW failed: {code:#x}")),
        };
    }
    let len = path
        .iter()
        .position(|&unit| unit == 0)
        .expect("GetOpenFileNameW NUL-terminates the path");
    Ok(Some(PathBuf::from(
        String::from_utf16(&path[..len])
            .map_err(|err| format!("the chosen path is not valid UTF-16: {err}"))?,
    )))
}

#[cfg(test)]
mod tests {
    use super::{FILTER, dialog_title};

    #[test]
    fn dialog_title_names_the_room() {
        assert_eq!(dialog_title("Ops"), "Send File to Ops");
    }

    #[test]
    fn filter_is_one_pair_ending_in_an_empty_string() {
        assert!(FILTER.ends_with("\0\0"));
        assert_eq!(
            FILTER.split('\0').collect::<Vec<_>>(),
            ["All Files (*.*)", "*.*", "", ""]
        );
    }
}
