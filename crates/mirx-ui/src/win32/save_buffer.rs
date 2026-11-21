//! mIRC's window menu Buffer > Save... and `/savebuf`: a window's buffer,
//! its lines as it shows them, saved to a text file.

use std::fs;
use std::path::Path;

use super::file_dialogs::{self, FileDialog};
use super::logging::{log_file_stem, logs_dir};
use super::timeline_rows::TimelineSyntheticRowLabel;
use super::*;

/// The text a window's buffer saves as: its lines, plain, as it shows them,
/// each ending in CRLF. The lines about the window's state now (who is
/// typing, older history loading, what the editbox replies to, where the
/// user stopped reading) are not lines of its buffer.
fn save_buffer_text(rows: &[TimelineRow]) -> String {
    rows.iter()
        .filter(|row| {
            !matches!(
                row.identity,
                TimelineRowIdentity::Synthetic {
                    label: TimelineSyntheticRowLabel::Typing
                        | TimelineSyntheticRowLabel::Backfill
                        | TimelineSyntheticRowLabel::ComposeTarget
                        | TimelineSyntheticRowLabel::LineMarker,
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
    let logs = logs_dir()?;
    let dialog = FileDialog::Save {
        file_name: &file_name,
        folder: Some(&logs),
    };
    match file_dialogs::choose_file(frame, "Save Buffer", file_dialogs::TEXT_FILES, dialog)? {
        Some(path) => save_window_buffer(frame, view_id, &path).map(Some),
        None => Ok(None),
    }
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
            TimelineRow::line_marker(),
            TimelineRow::full_width(TimelineRowKind::Info, "<Bob> four".to_owned()),
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
            "* Now talking in Ops\r\n<Alice> one\r\ntwo\r\nthree\r\n<Bob> four\r\n"
        );
        assert_eq!(save_buffer_text(&[]), "");
    }
}
