//! Replies in a timeline. mIRC has no replies, so mirx shows what a reply
//! answers on a dim quote line above it, as `    > <Alice> are we...`.
//!
//! Pure code: which relation quotes which event, and the one-line excerpt
//! of the quoted line.

use mirx_core::RoomActivityRelation;

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

#[cfg(test)]
mod tests {
    use super::*;

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
