//! Replies in a timeline. mIRC has no replies, so mirx shows what a reply
//! answers on a dim quote line above it, as `    > <Alice> are we...`.
//!
//! Replying to or editing a message arms the window's editbox with it, so
//! the next line typed there is the reply or the new text.
//!
//! Pure code: which relation quotes which event, the one-line excerpt of
//! the quoted line, and which messages the user can edit.

use mirx_core::{
    RoomActivity, RoomActivityKind, RoomActivityRelation, RoomMessageActivity,
    RoomMessagePresentationKind,
};

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

/// The editbox text that would send `message` as it is: `/me` for an emote,
/// and `//` escaping a text's leading `/`.
pub(super) fn compose_text_for_edit(message: &RoomMessageActivity) -> String {
    match message.presentation {
        RoomMessagePresentationKind::Text if message.body.starts_with('/') => {
            format!("/{}", message.body)
        }
        RoomMessagePresentationKind::Text => message.body.clone(),
        RoomMessagePresentationKind::Emote => format!("/me {}", message.body),
        RoomMessagePresentationKind::Notice => {
            panic!("notices are not editable; editable_message rejects them")
        }
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
            Ok("//not a command".to_owned())
        );
        assert_eq!(
            edit_text(&message("@me:example", Emote, "waves")),
            Ok("/me waves".to_owned())
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
}
