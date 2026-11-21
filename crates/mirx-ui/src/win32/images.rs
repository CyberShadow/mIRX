//! GDI+ image decoding and the lazy preview media of image and video rows.

use super::*;

pub(super) struct GdiplusSession {
    token: usize,
}

impl GdiplusSession {
    fn startup() -> Result<Self, String> {
        let input = GdiplusStartupInput {
            GdiplusVersion: 1,
            DebugEventCallback: 0,
            SuppressBackgroundThread: 0,
            SuppressExternalCodecs: 0,
        };
        let mut token = 0usize;
        let status = unsafe { GdiplusStartup(&mut token, &input, null_mut()) };
        if status != 0 {
            return Err(format_gdiplus_status("GdiplusStartup", status));
        }
        Ok(Self { token })
    }
}

impl Drop for GdiplusSession {
    fn drop(&mut self) {
        unsafe { GdiplusShutdown(self.token) };
    }
}

pub(super) struct GdiplusImage {
    pub(super) raw: *mut GpImage,
    width: u32,
    height: u32,
}

impl Drop for GdiplusImage {
    fn drop(&mut self) {
        unsafe {
            GdipDisposeImage(self.raw);
        }
    }
}

pub(super) fn initialize_gdiplus_state() -> (Option<GdiplusSession>, Option<String>) {
    match GdiplusSession::startup() {
        Ok(session) => (Some(session), None),
        Err(message) => (None, Some(message)),
    }
}

/// The thumbnail asked of the server: the timeline's 96-dpi box, whatever
/// the DPI. It is also the media's storage key. A server answers with its
/// nearest size at or above the request (Synapse's are 320x240 and larger
/// for this), an encrypted thumbnail comes at its sender's size, and GDI+
/// draws the decoded image into the box at the window's DPI, so asking for
/// more at a higher DPI would gain nothing.
pub(super) fn timeline_thumbnail_format() -> RoomMediaFormat {
    RoomMediaFormat::Thumbnail {
        width: TIMELINE_IMAGE_THUMBNAIL_MAX_WIDTH as u32,
        height: TIMELINE_IMAGE_THUMBNAIL_MAX_HEIGHT as u32,
    }
}

pub(super) fn room_preview_media_storage_key(
    room_id: &str,
    activity_id: &str,
    event_id: &str,
    format: &RoomMediaFormat,
) -> String {
    match format {
        RoomMediaFormat::Thumbnail { width, height } => {
            format!("{room_id}|{activity_id}|{event_id}|thumbnail:{width}x{height}")
        }
        RoomMediaFormat::File => {
            format!("{room_id}|{activity_id}|{event_id}|file")
        }
    }
}

fn format_gdiplus_status(operation: &str, status: i32) -> String {
    format!("{operation} failed with GDI+ status {status}")
}

unsafe fn release_com_interface(ptr: *mut c_void) {
    if ptr.is_null() {
        return;
    }
    let vtable = *(ptr as *mut *mut windows_sys::core::IUnknown_Vtbl);
    ((*vtable).Release)(ptr);
}

fn decode_image_bytes(bytes: &[u8]) -> Result<GdiplusImage, String> {
    unsafe {
        let hglobal = GlobalAlloc(GMEM_MOVEABLE, bytes.len());
        if hglobal.is_null() {
            return Err(last_os_error("GlobalAlloc"));
        }

        let mut stream = null_mut();
        let copy_result = (|| -> Result<GdiplusImage, String> {
            let locked = GlobalLock(hglobal);
            if locked.is_null() {
                return Err(last_os_error("GlobalLock"));
            }
            copy_nonoverlapping(bytes.as_ptr(), locked.cast::<u8>(), bytes.len());
            GlobalUnlock(hglobal);

            let hr = CreateStreamOnHGlobal(hglobal, 1, &mut stream);
            if hr < 0 {
                return Err(format!(
                    "CreateStreamOnHGlobal failed with HRESULT 0x{:08x}",
                    hr as u32
                ));
            }

            let decode_result = (|| -> Result<GdiplusImage, String> {
                let mut image = null_mut::<GpImage>();
                let status = GdipLoadImageFromStream(stream, &mut image);
                if status != 0 {
                    return Err(format_gdiplus_status("GdipLoadImageFromStream", status));
                }

                let mut width = 0u32;
                let status = GdipGetImageWidth(image, &mut width);
                if status != 0 {
                    GdipDisposeImage(image);
                    return Err(format_gdiplus_status("GdipGetImageWidth", status));
                }

                let mut height = 0u32;
                let status = GdipGetImageHeight(image, &mut height);
                if status != 0 {
                    GdipDisposeImage(image);
                    return Err(format_gdiplus_status("GdipGetImageHeight", status));
                }

                if width == 0 || height == 0 {
                    GdipDisposeImage(image);
                    return Err("decoded image reported zero width or height".to_owned());
                }

                Ok(GdiplusImage {
                    raw: image,
                    width,
                    height,
                })
            })();

            release_com_interface(stream);
            decode_result
        })();

        if copy_result.is_err() && stream.is_null() {
            GlobalFree(hglobal);
        }
        copy_result
    }
}

fn room_preview_media_loaded_key(preview_media: &RoomPreviewMediaState) -> Option<&str> {
    match preview_media {
        RoomPreviewMediaState::Loaded { image_key, .. } => Some(image_key.as_str()),
        _ => None,
    }
}

pub(super) fn prune_decoded_images(state: &mut WindowState) {
    let referenced_keys = state
        .model
        .room_views
        .values()
        .flat_map(|room_view| room_view.preview_media.values())
        .filter_map(room_preview_media_loaded_key)
        .map(str::to_owned)
        .collect::<BTreeSet<_>>();
    state
        .decoded_images
        .retain(|image_key, _| referenced_keys.contains(image_key));
}

fn room_preview_activity_matches(
    room_view: &RoomViewModel,
    activity_id: &str,
    event_id: &str,
) -> bool {
    room_view.activities.iter().any(|activity| {
        activity.activity_id == activity_id
            && (matches!(
                &activity.kind,
                RoomActivityKind::Image(message) | RoomActivityKind::Sticker(message)
                    if message.event_id == event_id
            ) || matches!(
                &activity.kind,
                RoomActivityKind::Video(message) if message.event_id == event_id
            ))
    })
}

fn decoded_image_from_loaded_media(
    state: &mut WindowState,
    room_id: &str,
    activity_id: &str,
    event_id: &str,
    format: &RoomMediaFormat,
    bytes: &[u8],
) -> Result<RoomPreviewMediaState, String> {
    let Some(_session) = state.gdiplus_session.as_ref() else {
        return Err(state
            .gdiplus_startup_error
            .clone()
            .unwrap_or_else(|| "GDI+ startup did not complete".to_owned()));
    };
    let image = decode_image_bytes(bytes)?;
    let key = room_preview_media_storage_key(room_id, activity_id, event_id, format);
    let width = image.width;
    let height = image.height;
    state.decoded_images.insert(key.clone(), image);
    Ok(RoomPreviewMediaState::Loaded {
        event_id: event_id.to_owned(),
        format: format.clone(),
        image_key: key,
        width,
        height,
    })
}

pub(super) fn apply_room_media_event(state: &mut WindowState, media: RoomMedia) {
    let RoomMedia {
        room_id,
        activity_id,
        event_id,
        kind,
    } = media;
    let format = match &kind {
        RoomMediaKind::Loaded { format, .. } | RoomMediaKind::Failed { format, .. } => format,
    };
    if format == &RoomMediaFormat::File {
        apply_attachment_save_media_event(state, room_id, activity_id, event_id, kind);
        return;
    }
    if format != &timeline_thumbnail_format() {
        return;
    }
    let Some(room_view) = state.model.room_views.get(&room_id) else {
        return;
    };
    if !room_preview_activity_matches(room_view, &activity_id, &event_id) {
        return;
    }

    let previous = {
        state
            .model
            .room_views
            .get_mut(&room_id)
            .expect("room image media updates require an existing room view")
            .remove_preview_media(&activity_id)
    };
    if let Some(image_key) = previous
        .as_ref()
        .and_then(room_preview_media_loaded_key)
        .map(str::to_owned)
    {
        state.decoded_images.remove(&image_key);
    }

    let next_state = match kind {
        RoomMediaKind::Failed { format, message } => RoomPreviewMediaState::Failed {
            event_id,
            format,
            message,
        },
        RoomMediaKind::Loaded { format, bytes } => {
            match decoded_image_from_loaded_media(
                state,
                &room_id,
                &activity_id,
                &event_id,
                &format,
                &bytes,
            ) {
                Ok(preview_media) => preview_media,
                Err(message) => RoomPreviewMediaState::Failed {
                    event_id,
                    format,
                    message,
                },
            }
        }
    };

    state
        .model
        .room_views
        .get_mut(&room_id)
        .expect("room image media updates require an existing room view")
        .insert_preview_media(activity_id, next_state);
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct RoomPreviewMediaDispatch {
    activity_id: String,
    event_id: String,
    source: RoomMediaSource,
    format: RoomMediaFormat,
}

fn timeline_image_preview_source(
    message: &mirx_core::RoomImageActivity,
) -> Option<&RoomMediaSource> {
    if let Some(thumbnail_source) = message
        .info
        .as_ref()
        .and_then(|info| info.thumbnail_source.as_ref())
    {
        return Some(thumbnail_source);
    }

    match &message.source {
        RoomMediaSource::Plain { .. } => Some(&message.source),
        RoomMediaSource::Encrypted { .. } => None,
    }
}

pub(super) fn timeline_video_preview_source(
    message: &mirx_core::RoomVideoActivity,
) -> Option<&RoomMediaSource> {
    message
        .info
        .as_ref()
        .and_then(|info| info.thumbnail_source.as_ref())
}

pub(super) fn collect_lazy_room_preview_media_requests<'a>(
    room_view: &RoomViewModel,
    activities: impl IntoIterator<Item = &'a RoomActivity>,
) -> Vec<RoomPreviewMediaDispatch> {
    let format = timeline_thumbnail_format();
    activities
        .into_iter()
        .filter_map(|activity| {
            let (event_id, source) = match &activity.kind {
                RoomActivityKind::Image(message) | RoomActivityKind::Sticker(message) => {
                    (&message.event_id, timeline_image_preview_source(message))
                }
                RoomActivityKind::Video(message) => {
                    (&message.event_id, timeline_video_preview_source(message))
                }
                _ => return None,
            };
            if !room_view.should_request_preview_media(&activity.activity_id, event_id, &format) {
                return None;
            }
            let source = source?;
            Some(RoomPreviewMediaDispatch {
                activity_id: activity.activity_id.clone(),
                event_id: event_id.clone(),
                source: source.clone(),
                format: format.clone(),
            })
        })
        .collect()
}

/// Whether a refresh may ask the core for preview media: not while a
/// synthetic load replays its events.
pub(super) fn can_fetch_room_media(state: &WindowState) -> bool {
    !state.synthetic_load_in_progress
}

pub(super) fn dispatch_lazy_room_preview_media_requests(
    state: &mut WindowState,
    room_id: &str,
    requests: Vec<RoomPreviewMediaDispatch>,
) -> Result<(), String> {
    if !can_fetch_room_media(state) || requests.is_empty() {
        return Ok(());
    }

    for request in requests {
        let should_request = state
            .model
            .room_views
            .get(room_id)
            .expect("image media dispatch requires an existing room view")
            .should_request_preview_media(&request.activity_id, &request.event_id, &request.format);
        if !should_request {
            continue;
        }

        let previous = state
            .model
            .room_views
            .get_mut(room_id)
            .expect("image media dispatch requires an existing room view")
            .mark_preview_media_pending(
                request.activity_id.clone(),
                request.event_id.clone(),
                request.format.clone(),
            );
        if let Some(image_key) = previous
            .as_ref()
            .and_then(room_preview_media_loaded_key)
            .map(str::to_owned)
        {
            state.decoded_images.remove(&image_key);
        }

        let command = Command::FetchRoomMedia(FetchRoomMediaRequest {
            room_id: room_id.to_owned(),
            activity_id: request.activity_id.clone(),
            event_id: request.event_id.clone(),
            source: request.source.clone(),
            format: request.format.clone(),
        });
        let sender = match state.command_sender.as_ref().cloned() {
            Some(sender) => sender,
            None => continue,
        };
        if let Err(err) = sender.send(command.clone()) {
            state
                .model
                .room_views
                .get_mut(room_id)
                .expect("image media dispatch requires an existing room view")
                .remove_preview_media(&request.activity_id);
            return Err(format!(
                "failed to enqueue fetch room image media command: {err}"
            ));
        }
        #[cfg(test)]
        state.recorded_commands.push(command);
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::super::test_fixtures::*;
    use super::*;

    fn encrypted_image_activity(
        room_id: &str,
        event_id: &str,
        sender: &str,
        timestamp_ms: u64,
        body: &str,
        thumbnail_source: Option<RoomMediaSource>,
    ) -> RoomActivity {
        RoomActivity {
            room_id: room_id.to_owned(),
            activity_id: format!("event:{event_id}"),
            timestamp_ms,
            highlight: false,
            relation: None,
            kind: RoomActivityKind::Image(Box::new(mirx_core::RoomImageActivity {
                event_id: event_id.to_owned(),
                sender: user_profile(sender),
                body: body.to_owned(),
                filename: None,
                source: encrypted_media_source("mxc://example.org/encrypted"),
                info: Some(RoomImageInfo {
                    mimetype: Some("image/png".to_owned()),
                    size: Some(12_345),
                    width: Some(640),
                    height: Some(480),
                    is_animated: Some(false),
                    thumbnail_source,
                }),
            })),
        }
    }

    fn encrypted_media_source(url: &str) -> RoomMediaSource {
        RoomMediaSource::Encrypted {
            url: url.to_owned(),
            key: mirx_core::RoomEncryptedFileKey {
                kty: "oct".to_owned(),
                key_ops: vec!["encrypt".to_owned(), "decrypt".to_owned()],
                alg: "A256CTR".to_owned(),
                k: "base64url-key".to_owned(),
                ext: true,
            },
            iv: "base64-iv".to_owned(),
            hashes: mirx_core::RoomEncryptedFileHashes {
                sha256: "base64-sha256".to_owned(),
            },
            v: "v2".to_owned(),
        }
    }

    #[test]
    fn clear_room_timeline_removes_stale_preview_media_state() {
        let mut state = test_state();
        state.model.rooms = vec![room("!ops:example", "Ops", false)];

        apply_core_event(
            &mut state,
            image_message(
                "!ops:example",
                "$image1:example.org",
                "@alice:example.org",
                1_710_000_000_000,
                "diagram.png",
                None,
                None,
            ),
        )
        .unwrap();
        apply_core_event(
            &mut state,
            room_preview_media_event(
                "!ops:example",
                "event:$image1:example.org",
                "$image1:example.org",
                RoomMediaKind::Failed {
                    format: timeline_thumbnail_format(),
                    message: "unavailable".to_owned(),
                },
            ),
        )
        .unwrap();
        apply_core_event(
            &mut state,
            outgoing_queued("!ops:example", "txn-1", "hello"),
        )
        .unwrap();

        state
            .model
            .room_views
            .get_mut("!ops:example")
            .expect("room view must exist after timeline update")
            .clear_visible_entries();

        let room_view = state
            .model
            .room_views
            .get("!ops:example")
            .expect("room view must exist after clearing");
        assert_eq!(
            room_view.activities,
            vec![pending_message_activity("!ops:example", "txn-1", "hello")]
        );
        assert!(room_view.preview_media.is_empty());
    }

    #[test]
    fn stale_or_mismatched_room_preview_media_event_does_not_replace_current_state() {
        let mut state = test_state();
        state.model.rooms = vec![room("!ops:example", "Ops", false)];
        apply_core_event(
            &mut state,
            image_message(
                "!ops:example",
                "$image1:example.org",
                "@alice:example.org",
                1_710_000_000_000,
                "diagram.png",
                None,
                None,
            ),
        )
        .unwrap();
        apply_core_event(
            &mut state,
            room_preview_media_event(
                "!ops:example",
                "event:$image1:example.org",
                "$image1:example.org",
                RoomMediaKind::Failed {
                    format: timeline_thumbnail_format(),
                    message: "current".to_owned(),
                },
            ),
        )
        .unwrap();

        apply_core_event(
            &mut state,
            room_preview_media_event(
                "!ops:example",
                "event:$image1:example.org",
                "$different:example.org",
                RoomMediaKind::Failed {
                    format: timeline_thumbnail_format(),
                    message: "stale".to_owned(),
                },
            ),
        )
        .unwrap();
        apply_core_event(
            &mut state,
            room_preview_media_event(
                "!ops:example",
                "event:$image1:example.org",
                "$image1:example.org",
                RoomMediaKind::Failed {
                    format: RoomMediaFormat::File,
                    message: "wrong format".to_owned(),
                },
            ),
        )
        .unwrap();

        assert_eq!(
            state
                .model
                .room_views
                .get("!ops:example")
                .and_then(|room_view| room_view.preview_media.get("event:$image1:example.org"))
                .cloned(),
            Some(RoomPreviewMediaState::Failed {
                event_id: "$image1:example.org".to_owned(),
                format: timeline_thumbnail_format(),
                message: "current".to_owned(),
            })
        );
    }

    #[test]
    fn video_preview_media_event_with_stale_activity_id_is_ignored() {
        let mut state = test_state();
        state.model.rooms = vec![room("!ops:example", "Ops", false)];
        apply_core_event(
            &mut state,
            video_message("!ops:example", "$video1:example.org"),
        )
        .unwrap();

        apply_core_event(
            &mut state,
            room_preview_media_event(
                "!ops:example",
                "event:$stale-video:example.org",
                "$video1:example.org",
                RoomMediaKind::Failed {
                    format: timeline_thumbnail_format(),
                    message: "stale".to_owned(),
                },
            ),
        )
        .unwrap();

        assert!(
            state
                .model
                .room_views
                .get("!ops:example")
                .expect("room view must exist")
                .preview_media
                .is_empty()
        );
    }

    #[test]
    fn video_preview_dispatch_uses_thumbnail_source_and_skips_missing_thumbnail() {
        let (mut state, handle) = test_state_with_command_sender();
        state.model.rooms = vec![room("!ops:example", "Ops", false)];
        apply_core_event(
            &mut state,
            video_message("!ops:example", "$video1:example.org"),
        )
        .unwrap();
        let requests = {
            let view = state.model.room_views.get("!ops:example").unwrap();
            collect_lazy_room_preview_media_requests(view, view.activities.iter())
        };
        dispatch_lazy_room_preview_media_requests(&mut state, "!ops:example", requests).unwrap();
        assert_eq!(
            state.recorded_commands[0],
            Command::FetchRoomMedia(FetchRoomMediaRequest {
                room_id: "!ops:example".to_owned(),
                activity_id: "event:$video1:example.org".to_owned(),
                event_id: "$video1:example.org".to_owned(),
                source: RoomMediaSource::Plain {
                    url: "mxc://example.org/video-thumbnail".to_owned()
                },
                format: timeline_thumbnail_format(),
            })
        );
        stop_test_core(handle);

        let mut state = test_state();
        state.model.rooms = vec![room("!ops:example", "Ops", false)];
        apply_core_event(
            &mut state,
            video_message_with_thumbnail("!ops:example", "$video2:example.org", false),
        )
        .unwrap();
        let view = state.model.room_views.get("!ops:example").unwrap();
        assert!(collect_lazy_room_preview_media_requests(view, view.activities.iter()).is_empty());
    }

    #[test]
    fn a_sticker_previews_as_an_image_does() {
        let (mut state, handle) = test_state_with_command_sender();
        state.model.rooms = vec![room("!ops:example", "Ops", false)];
        apply_core_event(
            &mut state,
            Event::RoomActivity(Box::new(sticker_activity(
                "!ops:example",
                "$sticker",
                "@bob:example",
                1,
                "a waving cat",
            ))),
        )
        .unwrap();
        let requests = {
            let view = state.model.room_views.get("!ops:example").unwrap();
            collect_lazy_room_preview_media_requests(view, view.activities.iter())
        };
        assert_eq!(
            requests,
            vec![RoomPreviewMediaDispatch {
                activity_id: "event:$sticker".to_owned(),
                event_id: "$sticker".to_owned(),
                source: RoomMediaSource::Plain {
                    url: "mxc://example.org/sticker".to_owned()
                },
                format: timeline_thumbnail_format(),
            }]
        );
        dispatch_lazy_room_preview_media_requests(&mut state, "!ops:example", requests).unwrap();
        apply_core_event(
            &mut state,
            room_preview_media_event(
                "!ops:example",
                "event:$sticker",
                "$sticker",
                RoomMediaKind::Loaded {
                    format: timeline_thumbnail_format(),
                    bytes: tiny_png_bytes(),
                },
            ),
        )
        .unwrap();
        let rows = build_room_timeline_rows(&state.model, "!ops:example", state.timestamps(), None);
        let sticker = rows.last().expect("the sticker's row");
        assert_eq!(sticker.kind, TimelineRowKind::Image { highlight: false });
        assert!(
            sticker
                .logical_text
                .ends_with("* @bob:example sent sticker: a waving cat")
        );
        assert!(matches!(
            sticker.image_state,
            Some(TimelineImageState::Loaded {
                width: 1,
                height: 1,
                ..
            })
        ));
        stop_test_core(handle);
    }

    #[test]
    fn loaded_video_preview_projects_and_dedupes_dispatch() {
        let (mut state, handle) = test_state_with_command_sender();
        state.model.rooms = vec![room("!ops:example", "Ops", false)];
        apply_core_event(
            &mut state,
            video_message("!ops:example", "$video1:example.org"),
        )
        .unwrap();
        let requests = {
            let view = state.model.room_views.get("!ops:example").unwrap();
            collect_lazy_room_preview_media_requests(view, view.activities.iter())
        };
        dispatch_lazy_room_preview_media_requests(&mut state, "!ops:example", requests).unwrap();
        apply_core_event(
            &mut state,
            room_preview_media_event(
                "!ops:example",
                "event:$video1:example.org",
                "$video1:example.org",
                RoomMediaKind::Loaded {
                    format: timeline_thumbnail_format(),
                    bytes: tiny_png_bytes(),
                },
            ),
        )
        .unwrap();
        assert!(
            build_room_timeline_rows(&state.model, "!ops:example", state.timestamps(), None)
                .iter()
                .any(|row| matches!(
                    row.image_state,
                    Some(TimelineImageState::Loaded {
                        width: 1,
                        height: 1,
                        ..
                    })
                ))
        );
        let view = state.model.room_views.get("!ops:example").unwrap();
        assert!(collect_lazy_room_preview_media_requests(view, view.activities.iter()).is_empty());
        stop_test_core(handle);
    }

    #[test]
    fn redacted_video_preview_drops_state_and_ignores_late_media() {
        let mut state = test_state();
        state.model.rooms = vec![room("!ops:example", "Ops", false)];
        apply_core_event(
            &mut state,
            video_message("!ops:example", "$video1:example.org"),
        )
        .unwrap();
        apply_core_event(
            &mut state,
            room_preview_media_event(
                "!ops:example",
                "event:$video1:example.org",
                "$video1:example.org",
                RoomMediaKind::Failed {
                    format: timeline_thumbnail_format(),
                    message: "current".to_owned(),
                },
            ),
        )
        .unwrap();
        apply_core_event(
            &mut state,
            incoming_redacted(
                "!ops:example",
                "$video1:example.org",
                "$redaction:example.org",
            ),
        )
        .unwrap();
        apply_core_event(
            &mut state,
            room_preview_media_event(
                "!ops:example",
                "event:$video1:example.org",
                "$video1:example.org",
                RoomMediaKind::Failed {
                    format: timeline_thumbnail_format(),
                    message: "late".to_owned(),
                },
            ),
        )
        .unwrap();
        assert!(
            state.model.room_views["!ops:example"]
                .preview_media
                .is_empty()
        );
    }

    #[test]
    fn redaction_image_row_clears_media_state_and_ignores_late_media_updates() {
        let mut state = test_state();
        state.model.rooms = vec![room("!ops:example", "Ops", false)];
        apply_core_event(
            &mut state,
            image_message(
                "!ops:example",
                "$image1:example.org",
                "@alice:example.org",
                1_710_000_000_000,
                "body fallback",
                Some("diagram.png"),
                Some(RoomImageInfo {
                    mimetype: Some("image/png".to_owned()),
                    size: Some(12_345),
                    width: Some(640),
                    height: Some(480),
                    is_animated: Some(false),
                    thumbnail_source: None,
                }),
            ),
        )
        .unwrap();
        apply_core_event(
            &mut state,
            room_preview_media_event(
                "!ops:example",
                "event:$image1:example.org",
                "$image1:example.org",
                RoomMediaKind::Failed {
                    format: timeline_thumbnail_format(),
                    message: "current".to_owned(),
                },
            ),
        )
        .unwrap();
        apply_core_event(
            &mut state,
            incoming_redacted(
                "!ops:example",
                "$image1:example.org",
                "$redaction:example.org",
            ),
        )
        .unwrap();

        let room_view = state
            .model
            .room_views
            .get("!ops:example")
            .expect("room view must exist");
        assert!(
            !room_view
                .preview_media
                .contains_key("event:$image1:example.org"),
            "redaction should drop stored image media state"
        );
        assert!(
            collect_lazy_room_preview_media_requests(room_view, room_view.activities.iter())
                .is_empty(),
            "redacted image rows must not trigger lazy image fetches"
        );

        let rendered = render_room_lines(&state, "!ops:example").join("\n");
        assert!(rendered.contains("message redacted"));
        assert!(!rendered.contains("diagram.png"));
        assert!(!rendered.contains("body fallback"));
        assert!(!rendered.contains("mxc://example.org/diagram"));

        apply_core_event(
            &mut state,
            room_preview_media_event(
                "!ops:example",
                "event:$image1:example.org",
                "$image1:example.org",
                RoomMediaKind::Failed {
                    format: timeline_thumbnail_format(),
                    message: "late".to_owned(),
                },
            ),
        )
        .unwrap();

        let room_view = state
            .model
            .room_views
            .get("!ops:example")
            .expect("room view must exist");
        assert!(room_view.preview_media.is_empty());
        let rendered = render_room_lines(&state, "!ops:example").join("\n");
        assert!(rendered.contains("message redacted"));
        assert!(!rendered.contains("late"));
    }

    #[test]
    fn room_preview_media_dispatch_records_one_thumbnail_fetch_and_dedupes_pending() {
        let (mut state, handle) = test_state_with_command_sender();
        state.model.rooms = vec![room("!ops:example", "Ops", false)];
        apply_core_event(
            &mut state,
            image_message(
                "!ops:example",
                "$image1:example.org",
                "@alice:example.org",
                1_710_000_000_000,
                "diagram.png",
                None,
                None,
            ),
        )
        .unwrap();

        let requests = {
            let room_view = state
                .model
                .room_views
                .get("!ops:example")
                .expect("room view must exist");
            collect_lazy_room_preview_media_requests(room_view, room_view.activities.iter())
        };
        dispatch_lazy_room_preview_media_requests(&mut state, "!ops:example", requests).unwrap();
        assert_eq!(
            state.recorded_commands,
            vec![Command::FetchRoomMedia(FetchRoomMediaRequest {
                room_id: "!ops:example".to_owned(),
                activity_id: "event:$image1:example.org".to_owned(),
                event_id: "$image1:example.org".to_owned(),
                source: RoomMediaSource::Plain {
                    url: "mxc://example.org/diagram".to_owned(),
                },
                format: timeline_thumbnail_format(),
            })]
        );

        let requests = {
            let room_view = state
                .model
                .room_views
                .get("!ops:example")
                .expect("room view must exist");
            collect_lazy_room_preview_media_requests(room_view, room_view.activities.iter())
        };
        assert!(requests.is_empty());

        stop_test_core(handle);
    }

    #[test]
    fn room_preview_media_dispatch_without_command_sender_keeps_pending_state() {
        let mut state = test_state();
        state.model.rooms = vec![room("!ops:example", "Ops", false)];
        apply_core_event(
            &mut state,
            image_message(
                "!ops:example",
                "$image1:example.org",
                "@alice:example.org",
                1_710_000_000_000,
                "diagram.png",
                None,
                None,
            ),
        )
        .unwrap();

        let requests = {
            let room_view = state
                .model
                .room_views
                .get("!ops:example")
                .expect("room view must exist");
            collect_lazy_room_preview_media_requests(room_view, room_view.activities.iter())
        };
        dispatch_lazy_room_preview_media_requests(&mut state, "!ops:example", requests).unwrap();

        assert_eq!(
            state
                .model
                .room_views
                .get("!ops:example")
                .and_then(|room_view| room_view.preview_media.get("event:$image1:example.org"))
                .cloned(),
            Some(RoomPreviewMediaState::Pending {
                event_id: "$image1:example.org".to_owned(),
                format: timeline_thumbnail_format(),
            })
        );
        assert!(state.recorded_commands.is_empty());
    }

    #[test]
    fn thread_preview_media_dispatch_uses_room_backed_pending_state() {
        let (mut state, handle) = test_state_with_command_sender();
        state.model.rooms = vec![room("!ops:example", "Ops", false)];
        let mut activity = image_activity(
            "!ops:example",
            "$thread-image:example.org",
            "@alice:example.org",
            1_710_000_000_000,
            "diagram.png",
            None,
            None,
        );
        activity.relation = Some(thread_relation("$thread-root:example.org"));
        apply_core_event(&mut state, Event::RoomActivity(Box::new(activity))).unwrap();

        let requests = {
            let room_view = state
                .model
                .room_views
                .get("!ops:example")
                .expect("room view must exist");
            collect_lazy_room_preview_media_requests(
                room_view,
                room_view.activities.iter().filter(|activity| {
                    room_activity_belongs_to_thread(activity, "$thread-root:example.org")
                }),
            )
        };
        dispatch_lazy_room_preview_media_requests(&mut state, "!ops:example", requests).unwrap();
        assert_eq!(state.recorded_commands.len(), 1);

        let room_requests = {
            let room_view = state
                .model
                .room_views
                .get("!ops:example")
                .expect("room view must exist");
            collect_lazy_room_preview_media_requests(room_view, room_view.activities.iter())
        };
        assert!(room_requests.is_empty());

        stop_test_core(handle);
    }

    #[test]
    fn thread_video_preview_media_dispatch_uses_room_backed_pending_state() {
        let (mut state, handle) = test_state_with_command_sender();
        state.model.rooms = vec![room("!ops:example", "Ops", false)];
        let mut event = video_message("!ops:example", "$thread-video:example.org");
        let Event::RoomActivity(activity) = &mut event else {
            unreachable!("video message must create a room activity");
        };
        activity.relation = Some(thread_relation("$thread-root:example.org"));
        apply_core_event(&mut state, event).unwrap();

        let requests = {
            let room_view = state
                .model
                .room_views
                .get("!ops:example")
                .expect("room view must exist");
            collect_lazy_room_preview_media_requests(
                room_view,
                room_view.activities.iter().filter(|activity| {
                    room_activity_belongs_to_thread(activity, "$thread-root:example.org")
                }),
            )
        };
        dispatch_lazy_room_preview_media_requests(&mut state, "!ops:example", requests).unwrap();
        assert_eq!(
            state.recorded_commands,
            vec![Command::FetchRoomMedia(FetchRoomMediaRequest {
                room_id: "!ops:example".to_owned(),
                activity_id: "event:$thread-video:example.org".to_owned(),
                event_id: "$thread-video:example.org".to_owned(),
                source: RoomMediaSource::Plain {
                    url: "mxc://example.org/video-thumbnail".to_owned(),
                },
                format: timeline_thumbnail_format(),
            })]
        );
        assert_eq!(
            state
                .model
                .room_views
                .get("!ops:example")
                .and_then(|room_view| room_view
                    .preview_media
                    .get("event:$thread-video:example.org"))
                .cloned(),
            Some(RoomPreviewMediaState::Pending {
                event_id: "$thread-video:example.org".to_owned(),
                format: timeline_thumbnail_format(),
            })
        );

        let room_requests = {
            let room_view = state
                .model
                .room_views
                .get("!ops:example")
                .expect("room view must exist");
            collect_lazy_room_preview_media_requests(room_view, room_view.activities.iter())
        };
        assert!(room_requests.is_empty());

        stop_test_core(handle);
    }

    #[test]
    fn duplicate_lazy_image_requests_in_single_dispatch_send_once() {
        let (mut state, handle) = test_state_with_command_sender();
        state.model.rooms = vec![room("!ops:example", "Ops", false)];
        apply_core_event(
            &mut state,
            image_message(
                "!ops:example",
                "$image1:example.org",
                "@alice:example.org",
                1_710_000_000_000,
                "diagram.png",
                None,
                None,
            ),
        )
        .unwrap();

        let duplicate_request = RoomPreviewMediaDispatch {
            activity_id: "event:$image1:example.org".to_owned(),
            event_id: "$image1:example.org".to_owned(),
            source: RoomMediaSource::Plain {
                url: "mxc://example.org/diagram".to_owned(),
            },
            format: timeline_thumbnail_format(),
        };
        dispatch_lazy_room_preview_media_requests(
            &mut state,
            "!ops:example",
            vec![duplicate_request.clone(), duplicate_request],
        )
        .unwrap();

        assert_eq!(state.recorded_commands.len(), 1);

        stop_test_core(handle);
    }

    #[test]
    fn loaded_or_failed_preview_media_state_skips_additional_fetch_dispatch() {
        let (mut state, handle) = test_state_with_command_sender();
        state.model.rooms = vec![room("!ops:example", "Ops", false)];
        apply_core_event(
            &mut state,
            image_message(
                "!ops:example",
                "$image1:example.org",
                "@alice:example.org",
                1_710_000_000_000,
                "diagram.png",
                None,
                None,
            ),
        )
        .unwrap();
        state
            .model
            .room_views
            .get_mut("!ops:example")
            .expect("room view must exist")
            .insert_preview_media(
                "event:$image1:example.org".to_owned(),
                RoomPreviewMediaState::Loaded {
                    event_id: "$image1:example.org".to_owned(),
                    format: timeline_thumbnail_format(),
                    image_key: "loaded-key".to_owned(),
                    width: 64,
                    height: 48,
                },
            );

        let requests = {
            let room_view = state
                .model
                .room_views
                .get("!ops:example")
                .expect("room view must exist");
            collect_lazy_room_preview_media_requests(room_view, room_view.activities.iter())
        };
        assert!(requests.is_empty());

        apply_core_event(
            &mut state,
            room_preview_media_event(
                "!ops:example",
                "event:$image1:example.org",
                "$image1:example.org",
                RoomMediaKind::Failed {
                    format: timeline_thumbnail_format(),
                    message: "unavailable".to_owned(),
                },
            ),
        )
        .unwrap();
        let requests = {
            let room_view = state
                .model
                .room_views
                .get("!ops:example")
                .expect("room view must exist");
            collect_lazy_room_preview_media_requests(room_view, room_view.activities.iter())
        };
        assert!(requests.is_empty());

        stop_test_core(handle);
    }

    #[test]
    fn encrypted_image_with_thumbnail_source_queues_thumbnail_request() {
        let (mut state, handle) = test_state_with_command_sender();
        state.model.rooms = vec![room("!ops:example", "Ops", false)];
        state
            .model
            .room_views
            .entry_or_default("!ops:example".to_owned())
            .apply_activity(encrypted_image_activity(
                "!ops:example",
                "$image1:example.org",
                "@alice:example.org",
                1_710_000_000_000,
                "diagram.png",
                Some(encrypted_media_source("mxc://example.org/encrypted-thumb")),
            ));

        let requests = {
            let room_view = state
                .model
                .room_views
                .get("!ops:example")
                .expect("room view must exist");
            collect_lazy_room_preview_media_requests(room_view, room_view.activities.iter())
        };
        dispatch_lazy_room_preview_media_requests(&mut state, "!ops:example", requests).unwrap();

        assert_eq!(
            state.recorded_commands,
            vec![Command::FetchRoomMedia(FetchRoomMediaRequest {
                room_id: "!ops:example".to_owned(),
                activity_id: "event:$image1:example.org".to_owned(),
                event_id: "$image1:example.org".to_owned(),
                source: encrypted_media_source("mxc://example.org/encrypted-thumb"),
                format: timeline_thumbnail_format(),
            })]
        );

        stop_test_core(handle);
    }

    #[test]
    fn encrypted_image_without_thumbnail_source_queues_no_request() {
        let mut state = test_state();
        state.model.rooms = vec![room("!ops:example", "Ops", false)];
        state
            .model
            .room_views
            .entry_or_default("!ops:example".to_owned())
            .apply_activity(encrypted_image_activity(
                "!ops:example",
                "$image1:example.org",
                "@alice:example.org",
                1_710_000_000_000,
                "diagram.png",
                None,
            ));

        let room_view = state
            .model
            .room_views
            .get("!ops:example")
            .expect("room view must exist");
        let requests =
            collect_lazy_room_preview_media_requests(room_view, room_view.activities.iter());
        assert!(requests.is_empty());
    }

    #[test]
    fn plain_image_without_thumbnail_source_queues_original_source_request() {
        let (mut state, handle) = test_state_with_command_sender();
        state.model.rooms = vec![room("!ops:example", "Ops", false)];
        apply_core_event(
            &mut state,
            image_message(
                "!ops:example",
                "$image1:example.org",
                "@alice:example.org",
                1_710_000_000_000,
                "diagram.png",
                Some("diagram.png"),
                Some(RoomImageInfo {
                    mimetype: Some("image/png".to_owned()),
                    size: Some(12_345),
                    width: Some(640),
                    height: Some(480),
                    is_animated: Some(false),
                    thumbnail_source: None,
                }),
            ),
        )
        .unwrap();

        let requests = {
            let room_view = state
                .model
                .room_views
                .get("!ops:example")
                .expect("room view must exist");
            collect_lazy_room_preview_media_requests(room_view, room_view.activities.iter())
        };
        dispatch_lazy_room_preview_media_requests(&mut state, "!ops:example", requests).unwrap();

        assert_eq!(
            state.recorded_commands,
            vec![Command::FetchRoomMedia(FetchRoomMediaRequest {
                room_id: "!ops:example".to_owned(),
                activity_id: "event:$image1:example.org".to_owned(),
                event_id: "$image1:example.org".to_owned(),
                source: RoomMediaSource::Plain {
                    url: "mxc://example.org/diagram".to_owned(),
                },
                format: timeline_thumbnail_format(),
            })]
        );

        stop_test_core(handle);
    }

    #[test]
    fn plain_image_with_thumbnail_source_queues_thumbnail_request() {
        let (mut state, handle) = test_state_with_command_sender();
        state.model.rooms = vec![room("!ops:example", "Ops", false)];
        apply_core_event(
            &mut state,
            image_message(
                "!ops:example",
                "$image1:example.org",
                "@alice:example.org",
                1_710_000_000_000,
                "diagram.png",
                Some("diagram.png"),
                Some(RoomImageInfo {
                    mimetype: Some("image/png".to_owned()),
                    size: Some(12_345),
                    width: Some(640),
                    height: Some(480),
                    is_animated: Some(false),
                    thumbnail_source: Some(RoomMediaSource::Plain {
                        url: "mxc://example.org/diagram-thumb".to_owned(),
                    }),
                }),
            ),
        )
        .unwrap();

        let requests = {
            let room_view = state
                .model
                .room_views
                .get("!ops:example")
                .expect("room view must exist");
            collect_lazy_room_preview_media_requests(room_view, room_view.activities.iter())
        };
        dispatch_lazy_room_preview_media_requests(&mut state, "!ops:example", requests).unwrap();

        assert_eq!(
            state.recorded_commands,
            vec![Command::FetchRoomMedia(FetchRoomMediaRequest {
                room_id: "!ops:example".to_owned(),
                activity_id: "event:$image1:example.org".to_owned(),
                event_id: "$image1:example.org".to_owned(),
                source: RoomMediaSource::Plain {
                    url: "mxc://example.org/diagram-thumb".to_owned(),
                },
                format: timeline_thumbnail_format(),
            })]
        );

        stop_test_core(handle);
    }
}
