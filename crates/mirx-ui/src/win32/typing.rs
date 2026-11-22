//! Typing notices sent: a room or thread window whose editbox holds a
//! message tells the room the user is typing (`m.typing`), and stops when
//! it no longer does.
//!
//! The decisions are pure (`TypingNotices`, `composes_a_message`); the
//! editbox's `EN_CHANGE` runs them (`editbox_changed`).

use std::collections::HashMap;

use mirx_core::{Command, SetTypingRequest};
use windows_sys::Win32::Foundation::HWND;

use super::aliases::Aliases;
use super::command_runner::enqueue_command;
use super::commands::{ComposeSubmission, classify_input};
use super::room_model::sendable_room_by_id;
use super::{ConnectionState, ViewId, mdi_child_edit, read_window_text, state_mut};

/// How often a typist's notice is repeated, at most. The SDK itself sends
/// a start only when its last one is more than 3 s old, inside the
/// server's 4 s timeout; asking every second keeps a typist's notice
/// from lapsing without a command for every key.
const TYPING_REPEAT_MS: u64 = 1_000;

/// The typing notices mirx has said are live, by room: when it last said
/// the user types there.
#[derive(Debug, Default)]
pub(super) struct TypingNotices {
    live: HashMap<String, u64>,
}

impl TypingNotices {
    /// What to tell `room_id` now that its editbox does (not) compose a
    /// message: `Some(true)` at most every `TYPING_REPEAT_MS` while it
    /// does, `Some(false)` once when it stops; `None` otherwise.
    pub(super) fn update(&mut self, room_id: &str, composing: bool, now_ms: u64) -> Option<bool> {
        if !composing {
            return self.live.remove(room_id).map(|_| false);
        }
        match self.live.get(room_id) {
            Some(&said_ms) if now_ms.saturating_sub(said_ms) < TYPING_REPEAT_MS => None,
            _ => {
                self.live.insert(room_id.to_owned(), now_ms);
                Some(true)
            }
        }
    }

    /// What to send for an editbox change, given whether notices may be
    /// sent at all (the option on, connected, a joined room). A closed
    /// gate forgets the room's notice and sends nothing: the server lets
    /// it lapse.
    pub(super) fn notice_to_send(
        &mut self,
        room_id: &str,
        composing: bool,
        may_send: bool,
        now_ms: u64,
    ) -> Option<bool> {
        self.update(room_id, composing && may_send, now_ms)
            .filter(|_| may_send)
    }
}

/// Whether the editbox's text would go out as a message on Enter: text,
/// `//text`, `/me action` or `/say text`, not a command, an alias or
/// nothing.
pub(super) fn composes_a_message(text: &str, aliases: &Aliases) -> bool {
    matches!(
        classify_input(text, aliases),
        ComposeSubmission::Message { body, .. } if !body.is_empty()
    )
}

/// The editbox of the MDI child `child` changed (typed, pasted, cleared on
/// Enter, recalled with Up): tells its room, if it is a room's or a
/// thread's.
pub(super) unsafe fn editbox_changed(frame: HWND, child: HWND) -> Result<(), String> {
    let state = state_mut(frame);
    // The editbox is made empty, and changes only once the view is known.
    let room_id = match &state.hwnd_views[&(child as isize)] {
        ViewId::Room(room_id) | ViewId::Thread { room_id, .. } => room_id.clone(),
        ViewId::Status => return Ok(()),
        ViewId::RoomList => unreachable!("the Rooms List has no editbox"),
    };
    let composing = composes_a_message(
        &read_window_text(mdi_child_edit(child)),
        state.aliases.aliases(),
    );
    let may_send = state.settings.settings().typing.send
        && state.model.connection == ConnectionState::Syncing
        && sendable_room_by_id(&state.model.rooms, &room_id).is_some();
    let now_ms = state.clock.now_ms();
    match state
        .typing_notices
        .notice_to_send(&room_id, composing, may_send, now_ms)
    {
        Some(typing) => enqueue_command(
            state,
            Command::SetTyping(SetTypingRequest { room_id, typing }),
        ),
        None => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_typist_is_told_at_most_every_second_and_stopped_once() {
        let mut notices = TypingNotices::default();
        assert_eq!(notices.update("!a", true, 10_000), Some(true));
        assert_eq!(notices.update("!a", true, 10_999), None);
        assert_eq!(notices.update("!a", true, 11_000), Some(true));
        assert_eq!(notices.update("!a", true, 11_500), None);
        assert_eq!(notices.update("!a", false, 11_600), Some(false));
        assert_eq!(notices.update("!a", false, 11_700), None);
        // Typing again starts again at once.
        assert_eq!(notices.update("!a", true, 11_800), Some(true));
    }

    #[test]
    fn rooms_are_told_apart() {
        let mut notices = TypingNotices::default();
        assert_eq!(notices.update("!a", true, 0), Some(true));
        assert_eq!(notices.update("!b", true, 100), Some(true));
        assert_eq!(notices.update("!a", false, 200), Some(false));
        assert_eq!(notices.update("!b", true, 300), None);
        assert_eq!(notices.update("!b", false, 400), Some(false));
    }

    #[test]
    fn a_closed_gate_sends_nothing_and_forgets_the_notice() {
        let mut notices = TypingNotices::default();
        assert_eq!(notices.notice_to_send("!a", true, false, 0), None);
        assert_eq!(notices.notice_to_send("!a", true, true, 100), Some(true));
        // Turned off (or disconnected) with a notice live: no stop is sent,
        // and none is owed after.
        assert_eq!(notices.notice_to_send("!a", true, false, 200), None);
        assert_eq!(notices.notice_to_send("!a", false, true, 300), None);
        assert_eq!(notices.notice_to_send("!a", true, true, 400), Some(true));
        assert_eq!(notices.notice_to_send("!a", false, true, 500), Some(false));
    }

    #[test]
    fn only_what_would_be_sent_as_a_message_is_typing() {
        let defaults = Aliases::defaults();
        for text in ["hi", "//x", "/me x", "/say x", "/!me x", " "] {
            assert!(composes_a_message(text, &defaults), "{text:?}");
        }
        for text in [
            "",
            "/m",
            "/me",
            "/me ",
            "/say",
            "/msg bob hi",
            "/away",
            "/",
            "/j x",
        ] {
            assert!(!composes_a_message(text, &defaults), "{text:?}");
        }
        let me_alias = Aliases::from_source(vec!["/me say me".to_owned()]).unwrap();
        assert!(!composes_a_message("/me x", &me_alias));
        assert!(composes_a_message("/!me x", &me_alias));
    }
}
