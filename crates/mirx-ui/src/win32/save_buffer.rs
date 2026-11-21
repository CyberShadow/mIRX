//! mIRC's window menu Buffer > Save... and `/savebuf`: a window's buffer,
//! its lines as it shows them, saved to a text file.

use std::fs;
use std::path::{Path, PathBuf};

use windows_sys::Win32::UI::Controls::Dialogs::{
    CommDlgExtendedError, GetSaveFileNameW, OFN_EXPLORER, OFN_HIDEREADONLY, OFN_NOCHANGEDIR,
    OFN_OVERWRITEPROMPT, OFN_PATHMUSTEXIST, OPENFILENAMEW,
};

use super::logging::{log_file_stem, logs_dir};
use super::timeline_rows::TimelineSyntheticRowLabel;
use super::*;

/// The dialog's file types: pairs of a label and a pattern, each
/// NUL-terminated, and the list ends with an empty string.
const FILTER: &str = "Text Files (*.txt)\0*.txt\0All Files (*.*)\0*.*\0\0";

/// Room for a long (`\\?\`) path, in UTF-16 units.
const PATH_CAPACITY: usize = 32768;

/// The text a window's buffer saves as: its lines, plain, as it shows them,
/// each ending in CRLF. The lines about the window's state now (who is
/// typing, older history loading, what the editbox replies to) are not
/// lines of its buffer.
fn save_buffer_text(rows: &[TimelineRow]) -> String {
    rows.iter()
        .filter(|row| {
            !matches!(
                row.identity,
                TimelineRowIdentity::Synthetic {
                    label: TimelineSyntheticRowLabel::Typing
                        | TimelineSyntheticRowLabel::Backfill
                        | TimelineSyntheticRowLabel::ComposeTarget,
                    ..
                }
            )
        })
        .flat_map(|row| row.logical_text.split('\n'))
        .map(|line| format!("{}\r\n", line.strip_suffix('\r').unwrap_or(line)))
        .collect()
}

/// Saves the buffer of `view_id`'s window to `path`, relative to the logs
/// folder, as `/savebuf` names it; returns the status bar's feedback.
pub(super) unsafe fn save_window_buffer(
    frame: HWND,
    view_id: &ViewId,
    path: &Path,
) -> Result<String, String> {
    let logs = logs_dir()?;
    fs::create_dir_all(&logs)
        .map_err(|error| format!("cannot create {}: {error}", logs.display()))?;
    let path = logs.join(path);
    let state = state_ref(frame);
    let timeline = view_timeline(view_id, state.views[view_id].content);
    fs::write(
        &path,
        save_buffer_text(&timeline_state_ref(timeline).source_rows),
    )
    .map_err(|error| format!("cannot write {}: {error}", path.display()))?;
    Ok(format!("saved buffer: {}", path.display()))
}

/// Buffer > Save...: the Save dialog for `view_id`'s window, which the
/// caller captured before it opens, then the save. `None` when cancelled.
pub(super) unsafe fn run_save_buffer_dialog(
    frame: HWND,
    view_id: &ViewId,
) -> Result<Option<String>, String> {
    let file_name = format!(
        "{}.txt",
        log_file_stem(&state_ref(frame).model, &log_view(view_id)?)
    );
    // Modal: core events keep arriving, so nothing is held across it.
    match choose_buffer_file(frame, &file_name, &logs_dir()?)? {
        Some(path) => save_window_buffer(frame, view_id, &path).map(Some),
        None => Ok(None),
    }
}

/// Runs the Save dialog owned by `owner`, in `folder` with `file_name`
/// filled in, and returns the chosen file, or `None` when the user cancels.
/// The dialog is modal and pumps messages, so the caller must hold no
/// window state borrow across it.
unsafe fn choose_buffer_file(
    owner: HWND,
    file_name: &str,
    folder: &Path,
) -> Result<Option<PathBuf>, String> {
    let filter = FILTER.encode_utf16().collect::<Vec<_>>();
    let title = wide_null("Save Buffer");
    let folder = wide_null(&folder.display().to_string());
    let default_extension = wide_null("txt");
    let mut path = file_name.encode_utf16().collect::<Vec<_>>();
    assert!(
        path.len() < PATH_CAPACITY,
        "a log name fits the path buffer"
    );
    path.resize(PATH_CAPACITY, 0);
    let mut save = OPENFILENAMEW {
        lStructSize: size_of::<OPENFILENAMEW>() as u32,
        hwndOwner: owner,
        lpstrFilter: filter.as_ptr(),
        nFilterIndex: 1,
        lpstrFile: path.as_mut_ptr(),
        nMaxFile: PATH_CAPACITY as u32,
        lpstrInitialDir: folder.as_ptr(),
        lpstrTitle: title.as_ptr(),
        lpstrDefExt: default_extension.as_ptr(),
        Flags: OFN_EXPLORER
            | OFN_OVERWRITEPROMPT
            | OFN_PATHMUSTEXIST
            | OFN_HIDEREADONLY
            | OFN_NOCHANGEDIR,
        ..OPENFILENAMEW::default()
    };
    if GetSaveFileNameW(&mut save) == 0 {
        return match CommDlgExtendedError() {
            0 => Ok(None),
            code => Err(format!("GetSaveFileNameW failed: {code:#x}")),
        };
    }
    let len = path
        .iter()
        .position(|&unit| unit == 0)
        .expect("GetSaveFileNameW NUL-terminates the path");
    Ok(Some(PathBuf::from(
        String::from_utf16(&path[..len])
            .map_err(|err| format!("the chosen path is not valid UTF-16: {err}"))?,
    )))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_buffer_saves_its_lines_but_not_the_windows_state() {
        let rows = vec![
            TimelineRow::full_width(TimelineRowKind::Plain, "* Now talking in Ops".to_owned())
                .with_identity(TimelineRowIdentity::synthetic(
                    TimelineSyntheticRowLabel::NowTalking,
                    "* Now talking in Ops",
                )),
            TimelineRow::full_width(
                TimelineRowKind::Plain,
                "* Loading older history...".to_owned(),
            )
            .with_identity(TimelineRowIdentity::synthetic(
                TimelineSyntheticRowLabel::Backfill,
                "* Loading older history...",
            )),
            TimelineRow::full_width(
                TimelineRowKind::Info,
                "<Alice> one\ntwo\r\nthree".to_owned(),
            ),
            TimelineRow::full_width(TimelineRowKind::Typing, "* Alice is typing".to_owned())
                .with_identity(TimelineRowIdentity::synthetic(
                    TimelineSyntheticRowLabel::Typing,
                    "* Alice is typing",
                )),
            TimelineRow::full_width(TimelineRowKind::Plain, "* Replying to Bob".to_owned())
                .with_identity(TimelineRowIdentity::synthetic(
                    TimelineSyntheticRowLabel::ComposeTarget,
                    "* Replying to Bob",
                )),
        ];
        assert_eq!(
            save_buffer_text(&rows),
            "* Now talking in Ops\r\n<Alice> one\r\ntwo\r\nthree\r\n"
        );
        assert_eq!(save_buffer_text(&[]), "");
    }

    #[test]
    fn filter_is_two_pairs_ending_in_an_empty_string() {
        assert_eq!(
            FILTER.split('\0').collect::<Vec<_>>(),
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
