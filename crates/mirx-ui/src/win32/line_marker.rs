//! mIRC's line marker on the room's Matrix read marker (`m.fully_read`):
//! a rule below the last line the user read, which the server keeps for
//! all their clients.
//!
//! As in mIRC, the marker moves when the user leaves a window, and only
//! once they saw it: "The line marker is only updated after you have
//! scrolled back to view it." It moves to the window's last line, by a
//! request to the server, whose echo moves the rule; mirx keeps no marker
//! of its own.

use super::*;

/// Where the user is with a room window's line marker.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum MarkerSight {
    /// The window is not the one the user is in: never entered, left, or
    /// mirx minimized. Leaving it moves nothing. The active window is away
    /// only while mirx is minimized, since activation enters it, or when
    /// it became active without MDI activating it (`unregister_view_by_child`
    /// guessing the next window).
    Away,
    /// Entered, its rule not on screen yet.
    Unseen,
    /// Entered, and its rule was on screen.
    Seen,
}

/// Where the read marker moves when the user leaves a room window: its
/// last line `last_line_event`, unless the marker is there already, or the
/// window showed its marker (`marker_row_shown`) and the user never saw it
/// (`sight`). A window without a marker row has nothing to see: its
/// marker is not set, or on a line the window does not show. A window the
/// user was not in is not left.
pub(super) fn read_marker_move<'a>(
    server_marker: Option<&str>,
    last_line_event: Option<&'a str>,
    marker_row_shown: bool,
    sight: MarkerSight,
) -> Option<&'a str> {
    let last_line_event = last_line_event?;
    let seen = match sight {
        MarkerSight::Away => return None,
        MarkerSight::Unseen => false,
        MarkerSight::Seen => true,
    };
    (server_marker != Some(last_line_event) && (seen || !marker_row_shown))
        .then_some(last_line_event)
}

/// The user leaves `view_id`'s window, which exists: another window is
/// activated, or the active window closes. The window is away then. A room
/// window whose marker was seen moves the room's read marker to its last
/// line, while mirx is connected: the move shows only once the next sync
/// echoes it, and offline the request could only fail, with an error about
/// something the user did not ask for. The marker then stays where it was.
pub(super) unsafe fn leave_window(hwnd: HWND, view_id: &ViewId) -> Result<(), String> {
    let ViewId::Room(room_id) = view_id else {
        return Ok(());
    };
    let state = state_ref(hwnd);
    let timeline_hwnd = view_timeline(view_id, state.views[view_id].content);
    let sight = std::mem::replace(
        &mut timeline_state_mut(timeline_hwnd).line_marker_sight,
        MarkerSight::Away,
    );
    if state.model.connection != ConnectionState::Syncing {
        return Ok(());
    }
    let Some(room_view) = state.model.room_views.get(room_id) else {
        return Ok(());
    };
    let timeline = timeline_state_ref(timeline_hwnd);
    let marker_row_shown = timeline
        .source_rows
        .iter()
        .any(|row| row.kind == TimelineRowKind::LineMarker);
    let server_marker = state
        .model
        .room_read_states
        .get(room_id)
        .and_then(|read_state| read_state.fully_read_event_id.as_deref());
    let Some(event_id) = read_marker_move(
        server_marker,
        read_receipts::window_read_line(room_view, view_id),
        marker_row_shown,
        sight,
    )
    .map(str::to_owned) else {
        return Ok(());
    };
    let room_id = room_id.clone();
    submit_set_read_marker(state_mut(hwnd), &room_id, event_id)
}

/// The user enters `view_id`'s window, which becomes the active one: its
/// marker is not seen yet. The refresh that follows repaints it, and a
/// marker on screen is seen then.
pub(super) unsafe fn enter_window(hwnd: HWND, view_id: &ViewId) {
    if let ViewId::Room(_) = view_id {
        let state = state_ref(hwnd);
        timeline_state_mut(view_timeline(view_id, state.views[view_id].content))
            .line_marker_sight = MarkerSight::Unseen;
    }
}

/// What Ctrl+L says in a window without a line marker.
const NO_LINE_MARKER_STATUS: &str = "* this window has no line marker";

/// Ctrl+L: scrolls the active room window back to its line marker, loading
/// older history until the marked line is there, as mIRC's "Scrolls back
/// text in a window to the Line Marker". It scrolls to the marked line even
/// where no rule is drawn.
pub(super) unsafe fn go_to_line_marker(hwnd: HWND) -> Result<(), String> {
    let state = state_ref(hwnd);
    let view_id = state.active_view.clone();
    let target_event_id = match &view_id {
        ViewId::Room(room_id) => state
            .model
            .room_read_states
            .get(room_id)
            .and_then(|read_state| read_state.fully_read_event_id.clone()),
        // Matrix keeps one read marker per room, for its room window.
        ViewId::Status | ViewId::Thread { .. } | ViewId::RoomList => None,
    };
    let Some(target_event_id) = target_event_id else {
        return set_local_command_status(hwnd, NO_LINE_MARKER_STATUS.to_owned());
    };
    start_event_jump(
        hwnd,
        replies::EventJump {
            view_id,
            target_event_id,
            purpose: replies::JumpPurpose::LineMarker,
        },
    )
}

#[cfg(test)]
mod tests {
    use super::super::test_fixtures::*;
    use super::*;

    /// A frame showing Ops, 40 lines `$m0`..`$m39` with the read marker
    /// on `fully_read`, and a query with Bob; connected, with a sender
    /// whose commands wait.
    unsafe fn with_marker_frame(fully_read: Option<&str>, test: impl FnOnce(HWND)) {
        with_test_frame(600, 400, |hwnd| {
            let (sender, commands) = inert_command_sender();
            std::mem::forget(commands);
            let state = state_mut(hwnd);
            state.command_sender = Some(sender);
            state.model.connection = ConnectionState::Syncing;
            let mut query = room("!bob:example", "Bob", false);
            query.is_direct = true;
            state.model.rooms = vec![room("!ops:example", "Ops", false), query];
            sync_views_and_tree(hwnd).unwrap();
            let mut events = (0..40)
                .map(|index| {
                    timeline_with_transaction_id(
                        "!ops:example",
                        &format!("$m{index}"),
                        None,
                        &format!("line {index}"),
                        1_700_000_000_000 + index * 1_000,
                    )
                })
                .collect::<Vec<_>>();
            events.push(timeline_with_transaction_id(
                "!bob:example",
                "$q0",
                None,
                "hi",
                1_700_000_000_000,
            ));
            for room_id in ["!ops:example", "!bob:example"] {
                events.push(room_read_state(room_id, None, fully_read, false, 0, 0, 0));
            }
            apply_events_and_refresh(hwnd, events).unwrap();
            activate_view(hwnd, &room_view_id("!ops:example"), None).unwrap();
            test(hwnd);
        });
    }

    fn read_marker_moves(state: &WindowState) -> Vec<(String, String)> {
        state
            .recorded_commands
            .iter()
            .filter_map(|command| match command {
                Command::SetReadMarker(request) => {
                    Some((request.room_id.clone(), request.event_id.clone()))
                }
                _ => None,
            })
            .collect()
    }

    unsafe fn ops_timeline(hwnd: HWND) -> HWND {
        let view_id = room_view_id("!ops:example");
        view_timeline(&view_id, state_ref(hwnd).views[&view_id].content)
    }

    fn moved_to(event_id: &str) -> Vec<(String, String)> {
        vec![("!ops:example".to_owned(), event_id.to_owned())]
    }

    #[test]
    fn leaving_a_window_whose_marker_was_seen_moves_it_to_the_last_line() {
        unsafe {
            with_marker_frame(Some("$m1"), |hwnd| {
                let timeline = ops_timeline(hwnd);
                SendMessageW(timeline, WM_PAINT, 0, 0);
                assert_eq!(
                    timeline_state_ref(timeline).line_marker_sight,
                    MarkerSight::Unseen
                );
                // At the end, the marker far above was never on screen.
                activate_view(hwnd, &ViewId::Status, None).unwrap();
                assert_eq!(read_marker_moves(state_ref(hwnd)), Vec::new());

                activate_view(hwnd, &room_view_id("!ops:example"), None).unwrap();
                SendMessageW(timeline, WM_VSCROLL, SB_TOP as WPARAM, 0);
                SendMessageW(timeline, WM_PAINT, 0, 0);
                assert_eq!(
                    timeline_state_ref(timeline).line_marker_sight,
                    MarkerSight::Seen
                );
                // Back at the end: seen is seen.
                SendMessageW(timeline, WM_VSCROLL, SB_BOTTOM as WPARAM, 0);
                SendMessageW(timeline, WM_PAINT, 0, 0);
                activate_view(hwnd, &ViewId::Status, None).unwrap();
                assert_eq!(read_marker_moves(state_ref(hwnd)), moved_to("$m39"));

                // The echo has not come: the next activation starts unseen.
                activate_view(hwnd, &room_view_id("!ops:example"), None).unwrap();
                assert_eq!(
                    timeline_state_ref(timeline).line_marker_sight,
                    MarkerSight::Unseen
                );
            });
        }
    }

    #[test]
    fn a_window_without_a_marker_row_moves_it_and_one_at_the_end_does_not() {
        unsafe {
            with_marker_frame(None, |hwnd| {
                activate_view(hwnd, &ViewId::Status, None).unwrap();
                assert_eq!(read_marker_moves(state_ref(hwnd)), moved_to("$m39"));
            });
            // On a line the window does not show.
            with_marker_frame(Some("$elsewhere"), |hwnd| {
                activate_view(hwnd, &ViewId::Status, None).unwrap();
                assert_eq!(read_marker_moves(state_ref(hwnd)), moved_to("$m39"));
            });
            with_marker_frame(Some("$m39"), |hwnd| {
                activate_view(hwnd, &ViewId::Status, None).unwrap();
                assert_eq!(read_marker_moves(state_ref(hwnd)), Vec::new());
            });
        }
    }

    #[test]
    fn only_leaving_a_connected_room_window_moves_the_marker() {
        unsafe {
            with_marker_frame(None, |hwnd| {
                // Activating the active window again leaves nothing.
                activate_view(hwnd, &room_view_id("!ops:example"), None).unwrap();
                assert_eq!(read_marker_moves(state_ref(hwnd)), Vec::new());
                // Status has no marker.
                activate_view(hwnd, &ViewId::Status, None).unwrap();
                activate_view(hwnd, &room_view_id("!ops:example"), None).unwrap();
                assert_eq!(read_marker_moves(state_ref(hwnd)), moved_to("$m39"));
                // Offline the request could only fail.
                state_mut(hwnd).model.connection = ConnectionState::SignedIn;
                activate_view(hwnd, &ViewId::Status, None).unwrap();
                assert_eq!(read_marker_moves(state_ref(hwnd)), moved_to("$m39"));
            });
        }
    }

    /// The kind of the timeline's top line, and the event of its row.
    unsafe fn top_line(timeline: HWND) -> (TimelineRowKind, Option<String>) {
        let state = timeline_state_ref(timeline);
        let line = &state.paint_lines[state.top_line];
        let row = &state.source_rows[line.source_row_index.unwrap()];
        (row.kind.clone(), row.event_id.clone())
    }

    fn backfill_requests(state: &WindowState) -> usize {
        state
            .recorded_commands
            .iter()
            .filter(|command| matches!(command, Command::BackfillRoomHistory(_)))
            .count()
    }

    #[test]
    fn ctrl_l_scrolls_the_marker_to_the_top_without_selecting() {
        unsafe {
            with_marker_frame(Some("$m1"), |hwnd| {
                let timeline = ops_timeline(hwnd);
                assert_ne!(top_line(timeline).0, TimelineRowKind::LineMarker);
                go_to_line_marker(hwnd).unwrap();
                assert_eq!(top_line(timeline), (TimelineRowKind::LineMarker, None));
                assert_eq!(state_ref(hwnd).model.status, "line marker found");
                assert_eq!(state_ref(hwnd).selected_timeline_event, None);
                assert_eq!(state_ref(hwnd).pending_jump, None);
                assert_eq!(backfill_requests(state_ref(hwnd)), 0);
            });
        }
    }

    #[test]
    fn ctrl_l_scrolls_to_the_marked_line_where_no_rule_is_drawn() {
        unsafe {
            with_marker_frame(Some("$m1"), |hwnd| {
                state_mut(hwnd)
                    .settings
                    .update(|settings| settings.marker.show = false)
                    .unwrap();
                sync_views_and_tree(hwnd).unwrap();
                let timeline = ops_timeline(hwnd);
                go_to_line_marker(hwnd).unwrap();
                assert_eq!(top_line(timeline).1.as_deref(), Some("$m1"));
                assert_eq!(state_ref(hwnd).model.status, "line marker found");
            });
            // The last line: as low as the end allows.
            with_marker_frame(Some("$m39"), |hwnd| {
                let timeline = ops_timeline(hwnd);
                SendMessageW(timeline, WM_VSCROLL, SB_TOP as WPARAM, 0);
                go_to_line_marker(hwnd).unwrap();
                let state = timeline_state_ref(timeline);
                assert_eq!(
                    state.top_line,
                    timeline_max_top_line(state.paint_lines.len(), timeline_visible_rows(timeline))
                );
                assert_eq!(state_ref(hwnd).model.status, "line marker found");
            });
        }
    }

    #[test]
    fn ctrl_l_pages_history_until_the_marked_line_arrives() {
        unsafe {
            with_marker_frame(Some("$old"), |hwnd| {
                go_to_line_marker(hwnd).unwrap();
                assert_eq!(backfill_requests(state_ref(hwnd)), 1);
                assert_eq!(state_ref(hwnd).model.status, "finding the line marker...");
                let old = match timeline_with_transaction_id(
                    "!ops:example",
                    "$old",
                    None,
                    "the old line",
                    1_699_999_000_000,
                ) {
                    Event::RoomActivity(activity) => RoomHistoryBackfillEvent::Activity(activity),
                    _ => unreachable!("a timeline event is an activity"),
                };
                apply_events_and_refresh(
                    hwnd,
                    vec![room_history_backfill("!ops:example", Some("t1"), vec![old])],
                )
                .unwrap();
                let timeline = ops_timeline(hwnd);
                assert_eq!(top_line(timeline), (TimelineRowKind::LineMarker, None));
                assert_eq!(state_ref(hwnd).model.status, "line marker found");
            });
            with_marker_frame(Some("$old"), |hwnd| {
                go_to_line_marker(hwnd).unwrap();
                apply_events_and_refresh(
                    hwnd,
                    vec![room_history_backfill("!ops:example", None, vec![])],
                )
                .unwrap();
                assert_eq!(state_ref(hwnd).pending_jump, None);
                assert_eq!(
                    state_ref(hwnd).model.status,
                    "* the line marker is not in this room's history"
                );
            });
        }
    }

    #[test]
    fn ctrl_l_without_a_marker_says_so() {
        unsafe {
            with_marker_frame(None, |hwnd| {
                go_to_line_marker(hwnd).unwrap();
                assert_eq!(state_ref(hwnd).model.status, NO_LINE_MARKER_STATUS);
            });
            with_marker_frame(Some("$m1"), |hwnd| {
                activate_view(hwnd, &ViewId::Status, None).unwrap();
                go_to_line_marker(hwnd).unwrap();
                assert_eq!(state_ref(hwnd).model.status, NO_LINE_MARKER_STATUS);
                assert_eq!(state_ref(hwnd).pending_jump, None);
            });
        }
    }

    #[test]
    fn closing_the_active_query_moves_its_marker_once() {
        unsafe {
            with_marker_frame(None, |hwnd| {
                let bob = room_view_id("!bob:example");
                activate_view(hwnd, &bob, None).unwrap();
                state_mut(hwnd).recorded_commands.clear();
                let child = state_ref(hwnd).views[&bob].child;
                SendMessageW(child, WM_CLOSE, 0, 0);
                assert!(!state_ref(hwnd).views.contains_key(&bob));
                // Whatever the next window is, the closed one left once.
                activate_view(hwnd, &ViewId::Status, None).unwrap();
                let moves = read_marker_moves(state_ref(hwnd))
                    .into_iter()
                    .filter(|(room_id, _)| room_id == "!bob:example")
                    .collect::<Vec<_>>();
                assert_eq!(moves, vec![("!bob:example".to_owned(), "$q0".to_owned())]);
            });
        }
    }

    #[test]
    fn a_window_not_entered_is_never_left() {
        unsafe {
            with_marker_frame(None, |hwnd| {
                let ops = room_view_id("!ops:example");
                let timeline = ops_timeline(hwnd);
                activate_view(hwnd, &room_view_id("!bob:example"), None).unwrap();
                assert_eq!(read_marker_moves(state_ref(hwnd)), moved_to("$m39"));
                assert_eq!(
                    timeline_state_ref(timeline).line_marker_sight,
                    MarkerSight::Away
                );
                // Painted beside the active window, it is not being read.
                SendMessageW(timeline, WM_VSCROLL, SB_TOP as WPARAM, 0);
                SendMessageW(timeline, WM_PAINT, 0, 0);
                assert_eq!(
                    timeline_state_ref(timeline).line_marker_sight,
                    MarkerSight::Away
                );
                // Made the active one without being entered, as when the
                // active window closes and mirx guesses the next one, and
                // then left for the window MDI activates.
                state_mut(hwnd).recorded_commands.clear();
                state_mut(hwnd).active_view = ops;
                activate_view(hwnd, &ViewId::Status, None).unwrap();
                assert_eq!(read_marker_moves(state_ref(hwnd)), Vec::new());
            });
        }
    }

    #[test]
    fn the_marker_moves_to_the_last_line_once_seen() {
        let last = Some("$last");
        // Seen: it moves.
        assert_eq!(
            read_marker_move(Some("$old"), last, true, MarkerSight::Seen),
            last
        );
        // Shown and never seen: it stays, however much is below it.
        assert_eq!(
            read_marker_move(Some("$old"), last, true, MarkerSight::Unseen),
            None
        );
        // No marker row to see (none set, or on a line not shown).
        assert_eq!(
            read_marker_move(None, last, false, MarkerSight::Unseen),
            last
        );
        assert_eq!(
            read_marker_move(Some("$unshown"), last, false, MarkerSight::Unseen),
            last
        );
        // Already there.
        assert_eq!(read_marker_move(last, last, false, MarkerSight::Seen), None);
        assert_eq!(
            read_marker_move(last, last, false, MarkerSight::Unseen),
            None
        );
        // A window without a line the marker can name.
        assert_eq!(
            read_marker_move(Some("$old"), None, true, MarkerSight::Seen),
            None
        );
        assert_eq!(
            read_marker_move(None, None, false, MarkerSight::Unseen),
            None
        );
        // A window the user was not in.
        assert_eq!(
            read_marker_move(Some("$old"), last, true, MarkerSight::Away),
            None
        );
        assert_eq!(read_marker_move(None, last, false, MarkerSight::Away), None);
    }
}
