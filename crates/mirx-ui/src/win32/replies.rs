//! Replies in a timeline. mIRC has no replies, so mirx shows what a reply
//! answers on a dim quote line above it, as `    > <Alice> are we...`.
//!
//! Replying to or editing a message arms the window's editbox with it, so
//! the next line typed there is the reply or the new text.
//!
//! Go to Original (double-click a quote line, or the message menu) scrolls
//! to the message a reply quotes, paging older room history until it is
//! loaded. Ctrl+L's scroll back to the line marker is the same event jump.
//!
//! Pure code: which relation quotes which event, the one-line excerpt of
//! the quoted line, which messages the user can edit, and the next step of
//! an event jump.

use mirx_core::{
    RoomActivity, RoomActivityKind, RoomActivityRelation, RoomMessageActivity,
    RoomMessagePresentationKind,
};

use super::ViewId;
use super::mirc_format::mirc_input_from_styled;

/// How many characters of the quoted line a quote shows.
const QUOTE_EXCERPT_CHARS: usize = 60;

/// The quote line's indent, the same as the reaction summary row's.
const QUOTE_LINE_PREFIX: &str = "    > ";

/// The event a relation quotes, if any. A thread message replying to the
/// thread's root is how every thread message starts, and the root is the
/// thread window's first line, so it gets no quote; nor does the reply a
/// thread message carries only as a fallback for clients without threads.
pub(super) fn reply_quote_target(relation: &RoomActivityRelation) -> Option<&str> {
    match relation {
        RoomActivityRelation::Reply {
            in_reply_to_event_id,
        } => Some(in_reply_to_event_id),
        RoomActivityRelation::Thread {
            thread_root_event_id,
            in_reply_to_event_id: Some(in_reply_to_event_id),
            is_falling_back: false,
        } if in_reply_to_event_id != thread_root_event_id => Some(in_reply_to_event_id),
        RoomActivityRelation::Thread { .. } => None,
    }
}

/// The first line of `line`, cut to 60 characters with `...` when longer.
pub(super) fn quote_excerpt(line: &str) -> String {
    let first_line = line.lines().next().unwrap_or_default();
    let mut chars = first_line.chars();
    let excerpt = chars.by_ref().take(QUOTE_EXCERPT_CHARS).collect::<String>();
    if chars.next().is_some() {
        format!("{excerpt}...")
    } else {
        excerpt
    }
}

/// The text of a quote line.
pub(super) fn quote_line(quoted: &str) -> String {
    format!("{QUOTE_LINE_PREFIX}{quoted}")
}

/// A quoted message that was redacted.
pub(super) const QUOTE_REDACTED_TEXT: &str = "message redacted";

/// A quoted message not loaded yet.
pub(super) const QUOTE_LOADING_TEXT: &str = "(loading the replied-to message...)";

/// A quoted message the server could not give.
pub(super) const QUOTE_UNAVAILABLE_TEXT: &str = "(the replied-to message is unavailable)";

/// What a window's editbox is armed with: the message the next line typed
/// there replies to, or replaces.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum ComposeTarget {
    Reply { event_id: String },
    Edit { event_id: String },
}

impl ComposeTarget {
    pub(super) fn event_id(&self) -> &str {
        match self {
            Self::Reply { event_id } | Self::Edit { event_id } => event_id,
        }
    }
}

/// The line below the timeline while `target` is armed; `quoted` is the
/// target as its quote shows it.
pub(super) fn compose_target_indicator(target: &ComposeTarget, quoted: &str) -> String {
    let verb = match target {
        ComposeTarget::Reply { .. } => "Replying to",
        ComposeTarget::Edit { .. } => "Editing",
    };
    format!("{verb} {quoted} (Esc cancels)")
}

/// The message `activity` is, if the user can edit it: their own text or
/// emote. `own_user_id` is `None` while signed out.
pub(super) fn editable_message<'a>(
    activity: &'a RoomActivity,
    own_user_id: Option<&str>,
) -> Result<&'a RoomMessageActivity, &'static str> {
    let RoomActivityKind::Message(message) = &activity.kind else {
        return Err("only text messages can be edited");
    };
    if own_user_id != Some(message.sender.user_id.as_str()) {
        return Err("you can only edit your own messages");
    }
    match message.presentation {
        RoomMessagePresentationKind::Text | RoomMessagePresentationKind::Emote => Ok(message),
        RoomMessagePresentationKind::Notice => Err("only text messages can be edited"),
    }
}

/// The editbox text that would send `message` as it is: its styles as mIRC
/// codes, `/me` for an emote, and `/say` before a text that starts with `/`.
pub(super) fn compose_text_for_edit(message: &RoomMessageActivity) -> String {
    let input = mirc_input_from_styled(&message.body, &message.styles);
    match message.presentation {
        // Codes before a `/` already keep it from reading as a command.
        RoomMessagePresentationKind::Text if input.starts_with('/') => format!("/say {input}"),
        RoomMessagePresentationKind::Text => input,
        RoomMessagePresentationKind::Emote => format!("/me {input}"),
        RoomMessagePresentationKind::Notice => {
            panic!("notices are not editable; editable_message rejects them")
        }
    }
}

/// What an event jump looks for: Go to Original's message, or Ctrl+L's
/// line marker, which the room's read marker places on a line only once
/// the loaded history reaches it (`line_marker::marker_place`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum JumpTarget {
    Original(String),
    LineMarker,
}

impl JumpTarget {
    /// The status bar while the jump pages older history.
    pub(super) fn finding_status(&self) -> &'static str {
        match self {
            Self::Original(_) => "finding the original message...",
            Self::LineMarker => "finding the line marker...",
        }
    }

    pub(super) fn found_status(&self) -> &'static str {
        match self {
            Self::Original(_) => "original message found",
            Self::LineMarker => "line marker found",
        }
    }

    pub(super) fn not_found_status(&self) -> &'static str {
        match self {
            Self::Original(_) => "* the original message is not in this room's history",
            Self::LineMarker => "* the line marker is not in this room's history",
        }
    }
}

/// An event jump waiting for older history: the window it started in, and
/// what it looks for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct EventJump {
    pub(super) view_id: ViewId,
    pub(super) target: JumpTarget,
}

/// The room's history paging, as an event jump sees it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum JumpHistory {
    Idle,
    Loading,
    Exhausted,
    /// The last request for older messages failed. `just_now` when the
    /// failure is the result the jump was waiting for, rather than one
    /// left from before, which the jump retries.
    Failed {
        message: String,
        just_now: bool,
    },
}

/// What an event jump does next.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum EventJumpStep {
    /// Show the event in this window: the one the jump started in, or its
    /// room's.
    Show(ViewId),
    /// Ask for older messages and wait for them.
    LoadOlder,
    /// Wait for the older messages already asked for, by this room or
    /// another (one request is in flight at a time).
    Wait,
    /// The whole history is loaded without the event.
    NotFound,
    /// Older messages could not be loaded.
    Failed(String),
}

/// The next step of an event jump that started in `view_id`.
/// `in_view_rows`: the window shows the event. `in_room_model`: the room's
/// timeline holds it, maybe outside this (thread) window.
pub(super) fn event_jump_step(
    view_id: &ViewId,
    in_view_rows: bool,
    in_room_model: bool,
    history: JumpHistory,
    another_backfill_loading: bool,
) -> EventJumpStep {
    if in_view_rows {
        return EventJumpStep::Show(view_id.clone());
    }
    if in_room_model {
        return match view_id {
            ViewId::Thread { room_id, .. } => EventJumpStep::Show(ViewId::Room(room_id.clone())),
            ViewId::Room(_) => unreachable!("a room window shows every activity of its room"),
            ViewId::Status | ViewId::RoomList => {
                unreachable!("only room and thread windows jump to events")
            }
        };
    }
    match history {
        JumpHistory::Exhausted => EventJumpStep::NotFound,
        JumpHistory::Failed {
            message,
            just_now: true,
        } => EventJumpStep::Failed(message),
        JumpHistory::Loading => EventJumpStep::Wait,
        _ if another_backfill_loading => EventJumpStep::Wait,
        JumpHistory::Idle
        | JumpHistory::Failed {
            just_now: false, ..
        } => EventJumpStep::LoadOlder,
    }
}

#[cfg(test)]
mod tests {
    use mirx_core::{RoomMessageSendState, RoomRedactedActivity, RoomUserProfile};

    use super::*;

    fn message(
        sender: &str,
        presentation: RoomMessagePresentationKind,
        body: &str,
    ) -> RoomActivity {
        RoomActivity {
            room_id: "!room:example".to_owned(),
            activity_id: "event:$m".to_owned(),
            timestamp_ms: 0,
            highlight: false,
            relation: None,
            kind: RoomActivityKind::Message(RoomMessageActivity {
                event_id: Some("$m".to_owned()),
                transaction_id: None,
                sender: RoomUserProfile {
                    user_id: sender.to_owned(),
                    display_name: None,
                    display_name_ambiguous: false,
                },
                presentation,
                body: body.to_owned(),
                styles: Vec::new(),
                user_links: Vec::new(),
                send_state: RoomMessageSendState::Sent,
            }),
        }
    }

    fn edit_text(activity: &RoomActivity) -> Result<String, &'static str> {
        editable_message(activity, Some("@me:example")).map(compose_text_for_edit)
    }

    #[test]
    fn own_text_and_emotes_edit_as_they_would_be_typed() {
        use RoomMessagePresentationKind::{Emote, Text};
        assert_eq!(
            edit_text(&message("@me:example", Text, "hi")),
            Ok("hi".to_owned())
        );
        assert_eq!(
            edit_text(&message("@me:example", Text, "/not a command")),
            Ok("/say /not a command".to_owned())
        );
        assert_eq!(
            edit_text(&message("@me:example", Emote, "waves")),
            Ok("/me waves".to_owned())
        );
    }

    fn bolded(mut activity: RoomActivity, start: usize, end: usize) -> RoomActivity {
        let RoomActivityKind::Message(message) = &mut activity.kind else {
            unreachable!("message() makes messages")
        };
        message.styles = vec![mirx_core::TextStyleSpan {
            start,
            end,
            style: mirx_core::TextStyle {
                bold: true,
                ..mirx_core::TextStyle::default()
            },
        }];
        activity
    }

    #[test]
    fn styled_messages_edit_as_their_mirc_codes() {
        use RoomMessagePresentationKind::{Emote, Text};
        assert_eq!(
            edit_text(&bolded(message("@me:example", Text, "hi there"), 0, 2)),
            Ok("\x02hi\x0f there".to_owned())
        );
        assert_eq!(
            edit_text(&bolded(message("@me:example", Emote, "waves hard"), 6, 10)),
            Ok("/me waves \x02hard".to_owned())
        );
        // A code before the `/` keeps it from being a command already.
        assert_eq!(
            edit_text(&bolded(
                message("@me:example", Text, "/not a command"),
                0,
                4
            )),
            Ok("\x02/not\x0f a command".to_owned())
        );
        assert_eq!(
            edit_text(&bolded(
                message("@me:example", Text, "/not a command"),
                5,
                6
            )),
            Ok("/say /not \x02a\x0f command".to_owned())
        );
    }

    #[test]
    fn only_own_text_messages_are_editable() {
        use RoomMessagePresentationKind::{Notice, Text};
        assert_eq!(
            edit_text(&message("@alice:example", Text, "hi")),
            Err("you can only edit your own messages")
        );
        assert_eq!(
            editable_message(&message("@me:example", Text, "hi"), None).map(|_| ()),
            Err("you can only edit your own messages"),
            "signed out, no message is ours"
        );
        assert_eq!(
            edit_text(&message("@me:example", Notice, "bot says")),
            Err("only text messages can be edited")
        );
        let redacted = RoomActivity {
            kind: RoomActivityKind::Redacted(RoomRedactedActivity {
                event_id: "$m".to_owned(),
            }),
            ..message("@me:example", Text, "gone")
        };
        assert_eq!(
            edit_text(&redacted),
            Err("only text messages can be edited")
        );
    }

    #[test]
    fn the_indicator_names_what_is_armed() {
        assert_eq!(
            compose_target_indicator(
                &ComposeTarget::Reply {
                    event_id: "$q".to_owned()
                },
                "<Alice> hi"
            ),
            "Replying to <Alice> hi (Esc cancels)"
        );
        assert_eq!(
            compose_target_indicator(
                &ComposeTarget::Edit {
                    event_id: "$q".to_owned()
                },
                "<Bob> yes"
            ),
            "Editing <Bob> yes (Esc cancels)"
        );
    }

    #[test]
    fn excerpt_is_the_first_line_cut_to_sixty_characters() {
        assert_eq!(quote_excerpt("<Alice> one\nsecond line"), "<Alice> one");
        assert_eq!(quote_excerpt(""), "");
        let sixty = "a".repeat(60);
        assert_eq!(quote_excerpt(&sixty), sixty);
        assert_eq!(quote_excerpt(&format!("{sixty}b")), format!("{sixty}..."));
        // Characters, not bytes: a multi-byte character counts once.
        let wide = "é".repeat(61);
        assert_eq!(
            quote_excerpt(&wide),
            format!("{}...", "é".repeat(60)),
            "61 two-byte characters"
        );
    }

    #[test]
    fn replies_quote_their_target_but_thread_roots_and_fallbacks_do_not() {
        assert_eq!(
            reply_quote_target(&RoomActivityRelation::Reply {
                in_reply_to_event_id: "$q".to_owned(),
            }),
            Some("$q")
        );
        let thread = |in_reply_to: Option<&str>, is_falling_back| RoomActivityRelation::Thread {
            thread_root_event_id: "$root".to_owned(),
            in_reply_to_event_id: in_reply_to.map(str::to_owned),
            is_falling_back,
        };
        assert_eq!(reply_quote_target(&thread(Some("$q"), false)), Some("$q"));
        assert_eq!(reply_quote_target(&thread(Some("$root"), false)), None);
        assert_eq!(reply_quote_target(&thread(Some("$q"), true)), None);
        assert_eq!(reply_quote_target(&thread(None, false)), None);
    }

    #[test]
    fn quote_lines_are_indented_like_reaction_rows() {
        assert_eq!(quote_line("<Alice> hi"), "    > <Alice> hi");
    }

    fn thread_view() -> ViewId {
        ViewId::Thread {
            room_id: "!ops:example".to_owned(),
            thread_root_event_id: "$root".to_owned(),
        }
    }

    fn room_view() -> ViewId {
        ViewId::Room("!ops:example".to_owned())
    }

    fn failed(just_now: bool) -> JumpHistory {
        JumpHistory::Failed {
            message: "offline".to_owned(),
            just_now,
        }
    }

    #[test]
    fn a_loaded_original_shows_in_its_window_or_the_thread_s_room() {
        for view_id in [room_view(), thread_view()] {
            assert_eq!(
                event_jump_step(&view_id, true, true, JumpHistory::Loading, true),
                EventJumpStep::Show(view_id.clone())
            );
        }
        // A thread message replying to a message outside the thread.
        assert_eq!(
            event_jump_step(&thread_view(), false, true, JumpHistory::Exhausted, false),
            EventJumpStep::Show(room_view())
        );
    }

    #[test]
    #[should_panic(expected = "a room window shows every activity of its room")]
    fn a_room_window_shows_every_loaded_original() {
        event_jump_step(&room_view(), false, true, JumpHistory::Idle, false);
    }

    #[test]
    fn an_original_not_loaded_pages_history_until_it_is_exhausted() {
        let step = |history, another_loading| {
            event_jump_step(&room_view(), false, false, history, another_loading)
        };
        assert_eq!(step(JumpHistory::Idle, false), EventJumpStep::LoadOlder);
        assert_eq!(step(JumpHistory::Loading, false), EventJumpStep::Wait);
        // One request at a time: another room's comes first.
        assert_eq!(step(JumpHistory::Idle, true), EventJumpStep::Wait);
        assert_eq!(step(JumpHistory::Exhausted, false), EventJumpStep::NotFound);
        assert_eq!(step(JumpHistory::Exhausted, true), EventJumpStep::NotFound);
    }

    #[test]
    fn an_earlier_failure_is_retried_and_the_awaited_one_reported() {
        let step = |history, another_loading| {
            event_jump_step(&thread_view(), false, false, history, another_loading)
        };
        assert_eq!(step(failed(false), false), EventJumpStep::LoadOlder);
        assert_eq!(step(failed(false), true), EventJumpStep::Wait);
        assert_eq!(
            step(failed(true), false),
            EventJumpStep::Failed("offline".to_owned())
        );
        // Even when another request started meanwhile.
        assert_eq!(
            step(failed(true), true),
            EventJumpStep::Failed("offline".to_owned())
        );
    }
}
