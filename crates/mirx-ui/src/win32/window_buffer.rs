//! mIRC's window buffer (Options > Other): a window keeps at most so many
//! lines, dropping its oldest as new ones arrive. As in mIRC, a window
//! scrolled back keeps every line until it returns to its end, and lowering
//! the setting shortens the windows at their end at once. Lines a room
//! window dropped load back from the server when it is scrolled to the top.

use super::*;

/// Trims every window at its end to the buffer. Runs after the windows
/// were synced, so each line it drops was shown, and written to the
/// window's log, first. Returns whether it dropped anything: the trimmed
/// windows' rows need building again.
pub(super) unsafe fn trim_window_buffers(hwnd: HWND) -> bool {
    let (cap, rooms, status) = {
        let state = state_ref(hwnd);
        let cap = state.settings.settings().other.window_buffer.lines();
        let rooms = state
            .views
            .iter()
            .filter_map(|(view_id, view)| match view_id {
                ViewId::Room(room_id)
                    if timeline_shows_end(view_timeline(view_id, view.content)) =>
                {
                    room_buffer_trim(state, room_id, cap).map(|trim| (room_id.clone(), trim))
                }
                _ => None,
            })
            .collect::<Vec<_>>();
        let status = &state.views[&ViewId::Status];
        (
            cap,
            rooms,
            timeline_shows_end(view_timeline(&ViewId::Status, status.content)),
        )
    };
    let state = state_mut(hwnd);
    // Only the rooms that trim: a mutable access moves a room's revision,
    // which rebuilds its windows.
    for (room_id, trim) in &rooms {
        state
            .model
            .room_views
            .get_mut(room_id)
            .expect("a trimmed room has a view model")
            .trim_buffer(*trim);
    }
    if !rooms.is_empty() {
        prune_decoded_images(state);
    }
    let status_trimmed = status && state.model.status_log.trim_to(cap);
    !rooms.is_empty() || status_trimmed
}

/// What the room's window, which is at its end, drops to keep `cap` lines;
/// `None` when it keeps them all. A room paging older history keeps them
/// all: a page loading would go above a gap, and a Find or a Go to Original
/// paging back would drop each page it loaded and ask for it again.
fn room_buffer_trim(state: &WindowState, room_id: &str, cap: usize) -> Option<BufferTrim> {
    let room_view = state.model.room_views.get(room_id)?;
    if room_view.buffer_lines() <= cap
        || matches!(room_view.history, RoomHistoryBackfillState::Loading { .. })
        || pending_timeline_find_history_views(state)
            .iter()
            .any(|view_id| view_id.room_id() == Some(room_id))
        || state
            .pending_jump
            .as_ref()
            .is_some_and(|jump| jump.view_id.room_id() == Some(room_id))
    {
        return None;
    }
    let open_threads = state
        .views
        .keys()
        .filter_map(|view_id| match view_id {
            ViewId::Thread {
                room_id: thread_room_id,
                thread_root_event_id,
            } if thread_room_id == room_id => Some(thread_root_event_id.clone()),
            _ => None,
        })
        .collect::<BTreeSet<_>>();
    room_view.buffer_trim(cap, &open_threads)
}
