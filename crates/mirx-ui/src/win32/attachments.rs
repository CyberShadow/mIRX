//! Saving message attachments to the profile's attachments directory.

use super::*;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct PendingAttachmentSave {
    room_id: String,
    activity_id: String,
    event_id: String,
    filename: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum AttachmentSaveResult {
    Saved {
        room_id: String,
        activity_id: String,
        event_id: String,
        filename: String,
        path: PathBuf,
    },
    Failed {
        room_id: String,
        activity_id: String,
        event_id: String,
        filename: Option<String>,
        message: String,
    },
}

fn attachments_dir() -> Result<PathBuf, String> {
    Ok(profile_dir()?.join("attachments"))
}

fn sanitize_attachment_path_component(value: &str) -> Result<String, String> {
    if value.is_empty()
        || matches!(value, "." | "..")
        || value.ends_with([' ', '.'])
        || value
            .chars()
            .any(|c| matches!(c, '/' | '\\' | ':' | '\0') || c.is_ascii_control())
    {
        return Err(format!("invalid attachment path component: {value:?}"));
    }
    let upper = value.to_ascii_uppercase();
    if matches!(upper.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || (upper.len() == 4
            && (upper.starts_with("COM") || upper.starts_with("LPT"))
            && matches!(upper.as_bytes()[3], b'1'..=b'9'))
    {
        return Err(format!("invalid attachment path component: {value:?}"));
    }
    Ok(value.to_owned())
}

fn attachment_directory_component(value: &str) -> String {
    value
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.') {
                c
            } else {
                '_'
            }
        })
        .collect()
}

fn attachment_save_path(room_id: &str, event_id: &str, filename: &str) -> Result<PathBuf, String> {
    let filename = sanitize_attachment_path_component(filename)?;
    Ok(attachments_dir()?
        .join(attachment_directory_component(room_id))
        .join(attachment_directory_component(event_id))
        .join(filename))
}

fn write_attachment_file(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let parent = path
        .parent()
        .ok_or_else(|| format!("attachment path {} has no parent directory", path.display()))?;
    fs::create_dir_all(parent).map_err(|err| {
        format!(
            "failed to create attachment directory {}: {err}",
            parent.display()
        )
    })?;
    let temp_path = path.with_extension("tmp");
    fs::write(&temp_path, bytes)
        .map_err(|err| format!("failed to write attachment {}: {err}", temp_path.display()))?;
    match fs::remove_file(path) {
        Ok(()) => {}
        Err(err) if err.kind() == ErrorKind::NotFound => {}
        Err(err) => {
            return Err(format!(
                "failed to replace attachment {}: {err}",
                path.display()
            ));
        }
    };
    fs::rename(&temp_path, path)
        .map_err(|err| format!("failed to save attachment {}: {err}", path.display()))
}

fn attachment_save_key(room_id: &str, activity_id: &str, event_id: &str) -> String {
    format!("{room_id}\u{1f}{activity_id}\u{1f}{event_id}")
}

pub(super) fn apply_attachment_save_media_event(
    state: &mut WindowState,
    room_id: String,
    activity_id: String,
    event_id: String,
    kind: RoomMediaKind,
) {
    let key = attachment_save_key(&room_id, &activity_id, &event_id);
    let Some(pending) = state.pending_attachment_saves.remove(&key) else {
        return;
    };
    match kind {
        RoomMediaKind::Loaded { bytes, .. } => match pending
            .filename
            .as_deref()
            .ok_or_else(|| "attachment has no filename".to_owned())
            .and_then(|filename| {
                attachment_save_path(&room_id, &event_id, filename).and_then(|path| {
                    write_attachment_file(&path, &bytes)?;
                    Ok((filename.to_owned(), path))
                })
            }) {
            Ok((filename, path)) => {
                state.last_attachment_save = Some(AttachmentSaveResult::Saved {
                    room_id,
                    activity_id,
                    event_id,
                    filename,
                    path: path.clone(),
                });
                state.model.status = format!("attachment saved: {}", path.display());
            }
            Err(message) => {
                state.last_attachment_save = Some(AttachmentSaveResult::Failed {
                    room_id,
                    activity_id,
                    event_id,
                    filename: pending.filename,
                    message: message.clone(),
                });
                state.model.status = format!("attachment save failed: {message}");
            }
        },
        RoomMediaKind::Failed { message, .. } => {
            state.last_attachment_save = Some(AttachmentSaveResult::Failed {
                room_id,
                activity_id,
                event_id,
                filename: pending.filename,
                message: message.clone(),
            });
            state.model.status = format!("attachment save failed: {message}");
        }
    }
}

pub(super) unsafe fn save_selected_message_attachment(hwnd: HWND) -> Result<(), String> {
    let attachment = selected_timeline_media_attachment(state_ref(hwnd))
        .ok_or_else(|| "no media attachment selected".to_owned())?;
    let filename = attachment
        .filename
        .clone()
        .ok_or_else(|| "attachment has no filename".to_owned())?;
    sanitize_attachment_path_component(&filename)?;
    let command = Command::FetchRoomMedia(FetchRoomMediaRequest {
        room_id: attachment.room_id.clone(),
        activity_id: attachment.activity_id.clone(),
        event_id: attachment.event_id.clone(),
        source: attachment.source.clone(),
        format: RoomMediaFormat::File,
    });
    let key = attachment_save_key(
        &attachment.room_id,
        &attachment.activity_id,
        &attachment.event_id,
    );
    let state = state_mut(hwnd);
    state.pending_attachment_saves.insert(
        key.clone(),
        PendingAttachmentSave {
            room_id: attachment.room_id,
            activity_id: attachment.activity_id,
            event_id: attachment.event_id,
            filename: Some(filename.clone()),
        },
    );
    let sender = state
        .command_sender
        .as_ref()
        .cloned()
        .ok_or_else(|| "command sender is unavailable".to_owned())?;
    if let Err(err) = sender.send(command.clone()) {
        state.pending_attachment_saves.remove(&key);
        return Err(format!("failed to enqueue fetch room media command: {err}"));
    }
    #[cfg(test)]
    state.recorded_commands.push(command);
    state.model.status = format!("saving attachment: {filename}");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::super::test_fixtures::*;
    use super::*;

    #[test]
    fn save_attachment_sanitizer_rejects_invalid_windows_names() {
        for value in [
            "",
            ".",
            "..",
            "../file",
            "dir\\file",
            "C:file",
            "NUL",
            "com1",
            "name.",
            "name ",
        ] {
            assert!(
                sanitize_attachment_path_component(value).is_err(),
                "{value:?} must be rejected"
            );
        }
        assert_eq!(
            sanitize_attachment_path_component("report.pdf").unwrap(),
            "report.pdf"
        );
    }

    #[test]
    fn attachment_save_path_uses_profile_attachment_root() {
        with_local_app_data(|root| {
            assert_eq!(
                attachment_save_path("!room:example", "$event:example", "report.pdf").unwrap(),
                root.join(PROFILE_SUBDIR)
                    .join("attachments")
                    .join("_room_example")
                    .join("_event_example")
                    .join("report.pdf")
            );
        });
    }
}
