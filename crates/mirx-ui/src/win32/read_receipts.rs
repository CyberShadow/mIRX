//! Read receipts (`m.read`): what the user's window shows tells the server
//! how far they read, so other clients and mirx's next start agree on what
//! is unread.
//!
//! The receipt names the newest line a window shows that the server can be
//! told about (`window_read_line`), decided from the model and the server's
//! read state at the time; mirx keeps no list of unread lines of its own.

use super::*;

/// The newest line of `view_id`'s window that a receipt can name: the last
/// activity with a read target (`read_target_for_room_activity`), which
/// pending sends are not; among the thread's own for a thread window. A
/// room window shows every activity of its room.
pub(super) fn window_read_line<'a>(
    room_view: &'a RoomViewModel,
    view_id: &ViewId,
) -> Option<&'a str> {
    let mut activities = room_view.activities.iter().rev();
    match view_id {
        ViewId::Room(_) => activities.find_map(read_target_for_room_activity),
        ViewId::Thread {
            thread_root_event_id,
            ..
        } => activities
            .filter(|activity| room_activity_belongs_to_thread(activity, thread_root_event_id))
            .find_map(read_target_for_room_activity),
        ViewId::Status | ViewId::RoomList => {
            unreachable!("only room and thread windows show room lines")
        }
    }
}

/// Whether the server counts the room unread, rather than only marked so.
fn counted_unread(read_state: &RoomReadState) -> bool {
    read_state.num_unread > 0 || read_state.num_notifications > 0 || read_state.num_mentions > 0
}

/// Whether the server's latest receipt, its own or implied by the user's
/// own events, is already on `line`.
fn read_up_to(read_state: &RoomReadState, line: &str) -> bool {
    read_state.latest_active_event_id.as_deref() == Some(line)
}

/// The receipt activating `view_id` sends, its newest line `line`. A room
/// window sends it when the server says the room is unread, whether from
/// the initial sync or from lines that arrived since: marked unread, or
/// counted unread up to before `line`. Without its read state the room's
/// unread state is unknown, and nothing is sent. A thread window sends it
/// when it had activity (`view_had_activity`), the only unread state a
/// thread has, unless the server's receipt is already there.
pub(super) fn activation_receipt<'a>(
    view_id: &ViewId,
    line: Option<&'a str>,
    read_state: Option<&RoomReadState>,
    view_had_activity: bool,
) -> Option<&'a str> {
    let line = line?;
    let sends = match view_id {
        ViewId::Room(_) => read_state.is_some_and(|read_state| {
            read_state.marked_unread
                || (counted_unread(read_state) && !read_up_to(read_state, line))
        }),
        ViewId::Thread { .. } => {
            view_had_activity && !read_state.is_some_and(|read_state| read_up_to(read_state, line))
        }
        ViewId::Status | ViewId::RoomList => {
            unreachable!("only room and thread windows send read receipts")
        }
    };
    sends.then_some(line)
}

/// The receipt activating `view_id`'s window would send now, had it the
/// activity `view_had_activity`.
pub(super) fn view_activation_receipt<'a>(
    state: &'a WindowState,
    view_id: &ViewId,
    view_had_activity: bool,
) -> Option<&'a str> {
    let room_id = view_id
        .room_id()
        .expect("only room and thread windows send read receipts");
    // A room without lines yet has no view model.
    let line = state
        .model
        .room_views
        .get(room_id)
        .and_then(|room_view| window_read_line(room_view, view_id));
    activation_receipt(
        view_id,
        line,
        state.model.room_read_states.get(room_id),
        view_had_activity,
    )
}

/// The read receipt mirx last sent per room: in flight until the server's
/// echo moves the room's `latest_active_event_id`, which takes a sync.
/// The active window's new lines are read once each meanwhile.
#[derive(Debug, Default)]
pub(super) struct SentReceipts {
    by_room: BTreeMap<String, String>,
}

impl SentReceipts {
    pub(super) fn record(&mut self, room_id: &str, event_id: &str) {
        self.by_room.insert(room_id.to_owned(), event_id.to_owned());
    }

    fn is_sent(&self, room_id: &str, event_id: &str) -> bool {
        self.by_room.get(room_id).map(String::as_str) == Some(event_id)
    }

    pub(super) fn retain_rooms(&mut self, mut keep: impl FnMut(&str) -> bool) {
        self.by_room.retain(|room_id, _| keep(room_id));
    }

    pub(super) fn clear(&mut self) {
        self.by_room.clear();
    }
}

/// The receipt the active room window's newest line `line` wants while
/// the user is in front of it: the server counts the room unread up to
/// before it, and mirx has not sent it already (`already_sent`). A room
/// only marked unread stays so until the user activates its window.
fn live_receipt<'a>(
    line: Option<&'a str>,
    read_state: Option<&RoomReadState>,
    already_sent: bool,
) -> Option<&'a str> {
    let line = line?;
    let read_state = read_state?;
    (counted_unread(read_state) && !read_up_to(read_state, line) && !already_sent).then_some(line)
}

/// Reads the active room window's new lines while the user is in front of
/// it (`in_front`): the receipt for its newest line, if it wants one. A
/// thread window gets none: its receipt is unthreaded, and would mark main
/// timeline lines the window does not show as read too. Only while
/// connected, as the read marker's move, and never for a replayed load.
fn read_active_room(state: &mut WindowState, in_front: bool) -> Result<(), String> {
    let ViewId::Room(room_id) = &state.active_view else {
        return Ok(());
    };
    if !in_front
        || state.model.connection != ConnectionState::Syncing
        || state.synthetic_load_in_progress
    {
        return Ok(());
    }
    let line = state
        .model
        .room_views
        .get(room_id)
        .and_then(|room_view| window_read_line(room_view, &state.active_view));
    let Some(event_id) = live_receipt(
        line,
        state.model.room_read_states.get(room_id),
        line.is_some_and(|line| state.sent_receipts.is_sent(room_id, line)),
    )
    .map(str::to_owned) else {
        return Ok(());
    };
    let room_id = room_id.clone();
    submit_mark_room_read(state, &room_id, event_id)
}

/// `read_active_room` while mirx is the application in front and its
/// frame `hwnd` is not minimized: after every core event batch, and when
/// the user comes back to mirx.
pub(super) unsafe fn read_active_window(hwnd: HWND) -> Result<(), String> {
    let in_front = mirx_is_foreground() && IsIconic(hwnd) == 0;
    read_active_room(state_mut(hwnd), in_front)
}

#[cfg(test)]
mod tests {
    use super::super::test_fixtures::*;
    use super::*;

    fn read_state(
        latest_active: Option<&str>,
        marked_unread: bool,
        num_unread: u64,
        num_notifications: u64,
        num_mentions: u64,
    ) -> RoomReadState {
        let Event::RoomReadState(read_state) = room_read_state(
            "!room:example",
            latest_active,
            None,
            marked_unread,
            num_unread,
            num_notifications,
            num_mentions,
        ) else {
            unreachable!("a read state event");
        };
        read_state
    }

    fn activities(activities: Vec<RoomActivity>) -> RoomViewModel {
        let mut room_view = RoomViewModel::default();
        for activity in activities {
            room_view.apply_activity(activity);
        }
        room_view
    }

    fn room_line(activity: RoomActivity) -> Option<String> {
        window_read_line(
            &activities(vec![
                sent_message_activity("!room:example", "$before", None, "hi", 1),
                activity,
            ]),
            &room_view_id("!room:example"),
        )
        .map(str::to_owned)
    }

    fn activity(event: Event) -> RoomActivity {
        let Event::RoomActivity(activity) = event else {
            unreachable!("an activity event");
        };
        *activity
    }

    #[test]
    fn a_window_read_line_is_its_newest_event_backed_line() {
        let named = |event_id: &str| Some(event_id.to_owned());
        assert_eq!(
            room_line(sent_message_activity("!room:example", "$m", None, "hi", 2)),
            named("$m")
        );
        assert_eq!(
            room_line(activity(image_message(
                "!room:example",
                "$image",
                "@bob:example",
                2,
                "diagram.png",
                None,
                None,
            ))),
            named("$image")
        );
        assert_eq!(
            room_line(file_activity(
                "!room:example",
                "$file",
                "@bob:example",
                2,
                "report",
                None,
                None,
            )),
            named("$file")
        );
        assert_eq!(
            room_line(audio_activity(
                "!room:example",
                "$audio",
                "@bob:example",
                2,
                "voice",
                None,
                None,
            )),
            named("$audio")
        );
        assert_eq!(
            room_line(activity(video_message("!room:example", "$video"))),
            named("$video")
        );
        assert_eq!(
            room_line(sticker_activity(
                "!room:example",
                "$sticker",
                "@bob:example",
                2,
                "a waving cat"
            )),
            named("$sticker")
        );
        assert_eq!(
            room_line(poll_activity(
                "!room:example",
                "$poll",
                2,
                poll_start("Lunch?", &["Pizza", "Sushi"])
            )),
            named("$poll")
        );
        assert_eq!(
            room_line(poll_activity(
                "!room:example",
                "$poll-end",
                2,
                poll_end("$poll", "Pizza won")
            )),
            named("$poll-end")
        );
        assert_eq!(
            room_line(verification_request_activity(
                "!room:example",
                "$verify",
                2,
                "@carol:example"
            )),
            named("$verify")
        );
        assert_eq!(
            room_line(unsupported_message_activity(
                "!room:example",
                "$custom",
                "@bob:example",
                2,
                "com.example.custom",
                "clip",
            )),
            named("$custom")
        );
        assert_eq!(
            room_line(activity(unable_to_decrypt_activity(
                "!room:example",
                "$utd",
                "@bob:example",
                2
            ))),
            named("$utd")
        );
        assert_eq!(
            room_line(activity(topic_state("!room:example", "incident review"))),
            named("$topic:!room:example:incident review")
        );
        // A pending send has no event yet: the line before it.
        assert_eq!(
            room_line(pending_message_activity("!room:example", "txn1", "sending")),
            named("$before")
        );
    }

    #[test]
    fn a_redacted_line_is_a_read_line() {
        let mut room_view = activities(vec![
            sent_message_activity("!room:example", "$a", None, "hi", 1),
            sent_message_activity("!room:example", "$b", None, "oops", 2),
        ]);
        let Event::RoomActivityUpdate(redacted) =
            incoming_redacted("!room:example", "$b", "$redaction")
        else {
            unreachable!("a redaction is an activity update");
        };
        room_view.apply_update(&redacted);
        assert_eq!(
            window_read_line(&room_view, &room_view_id("!room:example")),
            Some("$b")
        );
    }

    #[test]
    fn a_thread_window_read_line_is_its_thread_newest() {
        let room_view = activities(vec![
            sent_message_activity("!room:example", "$root", None, "root", 1),
            activity(thread_timeline("!room:example", "$t1", "$root", "reply")),
            sent_message_activity("!room:example", "$main", None, "elsewhere", 3),
        ]);
        assert_eq!(
            window_read_line(&room_view, &thread_view_id("!room:example", "$root")),
            Some("$t1")
        );
        assert_eq!(
            window_read_line(&room_view, &thread_view_id("!room:example", "$other")),
            None
        );
        assert_eq!(
            window_read_line(&room_view, &room_view_id("!room:example")),
            Some("$main")
        );
    }

    #[test]
    fn activating_a_room_window_sends_its_line_when_the_server_says_unread() {
        let room = room_view_id("!room:example");
        let line = Some("$b");
        let receipt = |read_state: Option<RoomReadState>| {
            activation_receipt(&room, line, read_state.as_ref(), false)
        };
        // Unread from the initial sync, or from lines since.
        assert_eq!(receipt(Some(read_state(None, false, 2, 0, 0))), line);
        assert_eq!(receipt(Some(read_state(Some("$a"), false, 0, 1, 0))), line);
        assert_eq!(receipt(Some(read_state(Some("$a"), false, 0, 0, 1))), line);
        // Read.
        assert_eq!(receipt(Some(read_state(Some("$a"), false, 0, 0, 0))), None);
        // Counted, but up to after the line the window shows.
        assert_eq!(receipt(Some(read_state(Some("$b"), false, 1, 0, 0))), None);
        // Marked unread: the receipt clears the mark.
        assert_eq!(receipt(Some(read_state(Some("$b"), true, 0, 0, 0))), line);
        // Unknown.
        assert_eq!(receipt(None), None);
        // A window without a line a receipt can name.
        assert_eq!(
            activation_receipt(&room, None, Some(&read_state(None, true, 2, 0, 0)), true),
            None
        );
    }

    /// A frame with Ops, its lines `$m0`..`$m3` unread from the first
    /// sync; connected, with a sender whose commands wait.
    unsafe fn with_unread_frame(test: impl FnOnce(HWND)) {
        with_test_frame(600, 400, |hwnd| {
            let (sender, commands) = inert_command_sender();
            std::mem::forget(commands);
            let state = state_mut(hwnd);
            state.command_sender = Some(sender);
            state.model.connection = ConnectionState::Syncing;
            let mut events = (0..4)
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
            events.push(Event::SyncedRooms(vec![room("!ops:example", "Ops", false)]));
            events.push(room_read_state("!ops:example", None, None, false, 4, 0, 0));
            apply_events_and_refresh(hwnd, events).unwrap();
            test(hwnd);
        });
    }

    fn receipts(state: &WindowState) -> Vec<&str> {
        state
            .recorded_commands
            .iter()
            .filter_map(|command| match command {
                Command::MarkRoomRead(request) => Some(request.event_id.as_str()),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn activating_a_window_unread_from_the_first_sync_reads_it_once() {
        unsafe {
            with_unread_frame(|hwnd| {
                let ops = room_view_id("!ops:example");
                activate_view(hwnd, &ops, None).unwrap();
                assert_eq!(receipts(state_ref(hwnd)), ["$m3"]);
                // Re-applying the active window after a model change is
                // no activation.
                show_view(hwnd, &ops, None).unwrap();
                apply_events_and_refresh(hwnd, vec![typing_event("!ops:example", &[])]).unwrap();
                assert_eq!(receipts(state_ref(hwnd)), ["$m3"]);
                // Activating it again is one, while the server still says
                // unread.
                activate_view(hwnd, &ViewId::Status, None).unwrap();
                activate_view(hwnd, &ops, None).unwrap();
                assert_eq!(receipts(state_ref(hwnd)), ["$m3", "$m3"]);
            });
        }
    }

    #[test]
    fn the_active_window_reads_its_new_line_once_while_counted_unread() {
        let line = Some("$b");
        let receipt = |read_state: Option<RoomReadState>, already_sent: bool| {
            live_receipt(line, read_state.as_ref(), already_sent)
        };
        assert_eq!(
            receipt(Some(read_state(Some("$a"), false, 1, 0, 0)), false),
            line
        );
        assert_eq!(receipt(Some(read_state(None, false, 0, 1, 0)), false), line);
        // Read, or read up to the line already.
        assert_eq!(
            receipt(Some(read_state(Some("$a"), false, 0, 0, 0)), false),
            None
        );
        assert_eq!(
            receipt(Some(read_state(Some("$b"), false, 1, 0, 0)), false),
            None
        );
        // Marked unread only: it stays so until the window is activated.
        assert_eq!(
            receipt(Some(read_state(Some("$b"), true, 0, 0, 0)), false),
            None
        );
        assert_eq!(
            receipt(Some(read_state(Some("$a"), true, 0, 0, 0)), false),
            None
        );
        // In flight.
        assert_eq!(
            receipt(Some(read_state(Some("$a"), false, 1, 0, 0)), true),
            None
        );
        assert_eq!(receipt(None, false), None);
        assert_eq!(
            live_receipt(None, Some(&read_state(None, false, 1, 0, 0)), false),
            None
        );
    }

    /// Ops active and connected, its line `$a` read, and a sender whose
    /// commands wait.
    fn active_ops_state() -> WindowState {
        let mut state = test_state();
        let (sender, commands) = inert_command_sender();
        std::mem::forget(commands);
        state.command_sender = Some(sender);
        state.model.connection = ConnectionState::Syncing;
        state.model.rooms = vec![room("!ops:example", "Ops", false)];
        state.active_view = room_view_id("!ops:example");
        apply_core_event(
            &mut state,
            Event::RoomActivity(Box::new(sent_message_activity(
                "!ops:example",
                "$a",
                None,
                "hi",
                1,
            ))),
        )
        .unwrap();
        apply_core_event(
            &mut state,
            room_read_state("!ops:example", Some("$a"), None, false, 0, 0, 0),
        )
        .unwrap();
        state
    }

    fn bob_says(event_id: &str) -> Event {
        Event::RoomActivity(Box::new(sent_message_activity(
            "!ops:example",
            event_id,
            None,
            "news",
            2,
        )))
    }

    #[test]
    fn the_active_window_in_front_reads_new_lines_once() {
        // The line first, then the server's count of it.
        let mut state = active_ops_state();
        apply_core_event(&mut state, bob_says("$b")).unwrap();
        read_active_room(&mut state, true).unwrap();
        assert_eq!(receipts(&state), Vec::<&str>::new());
        apply_core_event(
            &mut state,
            room_read_state("!ops:example", Some("$a"), None, false, 1, 1, 0),
        )
        .unwrap();
        read_active_room(&mut state, false).unwrap();
        assert_eq!(receipts(&state), Vec::<&str>::new());
        read_active_room(&mut state, true).unwrap();
        assert_eq!(receipts(&state), ["$b"]);
        // Until the echo, it is in flight.
        read_active_room(&mut state, true).unwrap();
        assert_eq!(receipts(&state), ["$b"]);
        // The next line, its count with it.
        apply_core_event(&mut state, bob_says("$c")).unwrap();
        apply_core_event(
            &mut state,
            room_read_state("!ops:example", Some("$a"), None, false, 2, 2, 0),
        )
        .unwrap();
        read_active_room(&mut state, true).unwrap();
        assert_eq!(receipts(&state), ["$b", "$c"]);
        // The echo.
        apply_core_event(
            &mut state,
            room_read_state("!ops:example", Some("$c"), None, false, 0, 0, 0),
        )
        .unwrap();
        read_active_room(&mut state, true).unwrap();
        assert_eq!(receipts(&state), ["$b", "$c"]);

        // The count first, then the line: the line before it is counted.
        let mut state = active_ops_state();
        apply_core_event(
            &mut state,
            room_read_state("!ops:example", Some("$a"), None, false, 1, 1, 0),
        )
        .unwrap();
        read_active_room(&mut state, true).unwrap();
        assert_eq!(receipts(&state), Vec::<&str>::new());
        apply_core_event(&mut state, bob_says("$b")).unwrap();
        read_active_room(&mut state, true).unwrap();
        assert_eq!(receipts(&state), ["$b"]);
    }

    #[test]
    fn only_a_connected_live_room_window_reads_new_lines() {
        let unread = |state: &mut WindowState| {
            apply_core_event(state, bob_says("$b")).unwrap();
            apply_core_event(
                state,
                room_read_state("!ops:example", Some("$a"), None, false, 1, 1, 0),
            )
            .unwrap();
        };
        let mut state = active_ops_state();
        unread(&mut state);
        state.model.connection = ConnectionState::SignedIn;
        read_active_room(&mut state, true).unwrap();
        assert_eq!(receipts(&state), Vec::<&str>::new());

        let mut state = active_ops_state();
        unread(&mut state);
        state.synthetic_load_in_progress = true;
        read_active_room(&mut state, true).unwrap();
        assert_eq!(receipts(&state), Vec::<&str>::new());

        // A thread window's receipt would read the room's lines too.
        let mut state = active_ops_state();
        unread(&mut state);
        state.active_view = thread_view_id("!ops:example", "$a");
        read_active_room(&mut state, true).unwrap();
        assert_eq!(receipts(&state), Vec::<&str>::new());
    }

    #[test]
    fn sent_receipts_go_with_the_session_and_the_rooms() {
        let mut state = active_ops_state();
        state.sent_receipts.record("!ops:example", "$a");
        state.sent_receipts.record("!gone:example", "$g");
        apply_core_event(
            &mut state,
            Event::SyncedRooms(vec![room("!ops:example", "Ops", false)]),
        )
        .unwrap();
        assert!(state.sent_receipts.is_sent("!ops:example", "$a"));
        assert!(!state.sent_receipts.is_sent("!gone:example", "$g"));
        with_local_app_data(|_| apply_core_event(&mut state, Event::LoggedOut).unwrap());
        assert!(!state.sent_receipts.is_sent("!ops:example", "$a"));
    }

    #[test]
    fn activating_a_thread_window_sends_its_line_after_activity() {
        let thread = thread_view_id("!room:example", "$root");
        let line = Some("$t1");
        assert_eq!(activation_receipt(&thread, line, None, true), line);
        assert_eq!(activation_receipt(&thread, line, None, false), None);
        assert_eq!(
            activation_receipt(
                &thread,
                line,
                Some(&read_state(Some("$a"), false, 1, 0, 0)),
                true
            ),
            line
        );
        assert_eq!(
            activation_receipt(
                &thread,
                line,
                Some(&read_state(Some("$t1"), false, 1, 0, 0)),
                true
            ),
            None
        );
    }
}
