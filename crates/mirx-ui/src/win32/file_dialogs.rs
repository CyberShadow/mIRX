//! The standard Windows Open and Save dialogs, as Send File, Buffer >
//! Save... and the room key files open them.

use std::path::{Path, PathBuf};

use windows_sys::Win32::Foundation::HWND;
use windows_sys::Win32::UI::Controls::Dialogs::{
    CommDlgExtendedError, GetOpenFileNameW, GetSaveFileNameW, OFN_EXPLORER, OFN_FILEMUSTEXIST,
    OFN_HIDEREADONLY, OFN_NOCHANGEDIR, OFN_OVERWRITEPROMPT, OFN_PATHMUSTEXIST, OPENFILENAMEW,
};

use super::wide_null;

/// A dialog's file types: pairs of a label and a pattern, each
/// NUL-terminated, and the list ends with an empty string.
pub(super) const ALL_FILES: &str = "All Files (*.*)\0*.*\0\0";
pub(super) const TEXT_FILES: &str = "Text Files (*.txt)\0*.txt\0All Files (*.*)\0*.*\0\0";

/// Room for a long (`\\?\`) path, in UTF-16 units.
const PATH_CAPACITY: usize = 32768;

/// Which dialog, and what it starts with. Either starts in the folder
/// comdlg32 last used unless told one, and leaves the process's current
/// directory alone.
pub(super) enum FileDialog<'a> {
    /// A file that exists.
    Open,
    /// A text file to write, `file_name` filled in: a name typed without
    /// an extension gets `.txt`, and an existing file is overwritten only
    /// once the user says so.
    Save {
        file_name: &'a str,
        folder: Option<&'a Path>,
    },
}

/// Runs `dialog`, owned by `owner`, and returns the chosen file, or `None`
/// when the user cancels. The dialog is modal and pumps messages, so the
/// caller must hold no window state borrow across it.
pub(super) unsafe fn choose_file(
    owner: HWND,
    title: &str,
    filter: &str,
    dialog: FileDialog,
) -> Result<Option<PathBuf>, String> {
    let filter = filter.encode_utf16().collect::<Vec<_>>();
    let title = wide_null(title);
    let default_extension = wide_null("txt");
    let (mut path, folder) = match &dialog {
        FileDialog::Open => (Vec::new(), None),
        FileDialog::Save { file_name, folder } => (
            file_name.encode_utf16().collect::<Vec<_>>(),
            folder.map(|folder| wide_null(&folder.display().to_string())),
        ),
    };
    assert!(
        path.len() < PATH_CAPACITY,
        "a file name fits the path buffer"
    );
    path.resize(PATH_CAPACITY, 0);
    let mut file = OPENFILENAMEW {
        lStructSize: size_of::<OPENFILENAMEW>() as u32,
        hwndOwner: owner,
        lpstrFilter: filter.as_ptr(),
        nFilterIndex: 1,
        lpstrFile: path.as_mut_ptr(),
        nMaxFile: PATH_CAPACITY as u32,
        lpstrInitialDir: folder
            .as_ref()
            .map_or(std::ptr::null(), |folder| folder.as_ptr()),
        lpstrTitle: title.as_ptr(),
        Flags: OFN_EXPLORER | OFN_PATHMUSTEXIST | OFN_HIDEREADONLY | OFN_NOCHANGEDIR,
        ..OPENFILENAMEW::default()
    };
    let (chosen, function) = match dialog {
        FileDialog::Open => {
            file.Flags |= OFN_FILEMUSTEXIST;
            (GetOpenFileNameW(&mut file), "GetOpenFileNameW")
        }
        FileDialog::Save { .. } => {
            file.Flags |= OFN_OVERWRITEPROMPT;
            file.lpstrDefExt = default_extension.as_ptr();
            (GetSaveFileNameW(&mut file), "GetSaveFileNameW")
        }
    };
    if chosen == 0 {
        return match CommDlgExtendedError() {
            0 => Ok(None),
            code => Err(format!("{function} failed: {code:#x}")),
        };
    }
    let len = path
        .iter()
        .position(|&unit| unit == 0)
        .expect("the dialog NUL-terminates the path");
    Ok(Some(PathBuf::from(
        String::from_utf16(&path[..len])
            .map_err(|err| format!("the chosen path is not valid UTF-16: {err}"))?,
    )))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filters_are_pairs_ending_in_an_empty_string() {
        assert_eq!(
            ALL_FILES.split('\0').collect::<Vec<_>>(),
            ["All Files (*.*)", "*.*", "", ""]
        );
        assert_eq!(
            TEXT_FILES.split('\0').collect::<Vec<_>>(),
            [
                "Text Files (*.txt)",
                "*.txt",
                "All Files (*.*)",
                "*.*",
                "",
                ""
            ]
        );
    }
}
