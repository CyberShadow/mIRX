//! The latest edit of each line that has one.

use super::room_model::MessageReplacementFields;
use super::*;

/// Which of two edits of a line is the later: the one the server received
/// later, then the greater event ID. Equal means the same edit again (a
/// bundled edit and the edit itself, or an edit reported decrypted twice).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(super) struct EditOrder {
    pub(super) timestamp_ms: u64,
    pub(super) replacement_event_id: String,
}

/// An edit of a line that could not be decrypted: it applies once the line
/// decrypts to a message of its sender.
#[derive(Debug, Clone)]
pub(super) struct HeldEdit {
    pub(super) order: EditOrder,
    pub(super) sender: String,
    pub(super) fields: MessageReplacementFields,
}

/// The latest edit of each shown line that has one, by the line's event ID.
/// A message line shows it (and `(edited)`); a line that could not be
/// decrypted holds it until it decrypts (`RoomViewModel::apply_decrypted`).
#[derive(Debug, Default, Clone)]
pub(super) struct LineEdits {
    by_target: BTreeMap<String, LineEdit>,
}

#[derive(Debug, Clone)]
enum LineEdit {
    Applied(EditOrder),
    Held(HeldEdit),
}

impl LineEdit {
    fn order(&self) -> &EditOrder {
        match self {
            Self::Applied(order) => order,
            Self::Held(held) => &held.order,
        }
    }
}

impl LineEdits {
    /// Whether an edit of `target` is later than the one it has, if any.
    pub(super) fn is_newer(&self, target: &str, order: &EditOrder) -> bool {
        self.by_target
            .get(target)
            .is_none_or(|edit| order > edit.order())
    }

    /// The message `target` shows the edit `order` now.
    pub(super) fn applied(&mut self, target: &str, order: EditOrder) {
        self.by_target
            .insert(target.to_owned(), LineEdit::Applied(order));
    }

    /// `target` could not be decrypted yet: it keeps the edit until it is.
    pub(super) fn hold(&mut self, target: &str, held: HeldEdit) {
        self.by_target
            .insert(target.to_owned(), LineEdit::Held(held));
    }

    /// `target` decrypted: the edit it held, which the caller applies (and
    /// marks `applied`) or drops.
    pub(super) fn take_held(&mut self, target: &str) -> Option<HeldEdit> {
        match self.by_target.remove(target)? {
            LineEdit::Held(held) => Some(held),
            LineEdit::Applied(order) => {
                panic!("{target} decrypted with an edit applied: {order:?}")
            }
        }
    }

    /// `target` was redacted, or left the window.
    pub(super) fn forget(&mut self, target: &str) {
        self.by_target.remove(target);
    }

    /// The held edit `replacement_event_id` was redacted: whether there was
    /// one. An edit already applied stays: its line no longer has the text
    /// it replaced.
    pub(super) fn forget_held_by_replacement(&mut self, replacement_event_id: &str) -> bool {
        let Some(target) = self.by_target.iter().find_map(|(target, edit)| {
            matches!(edit, LineEdit::Held(held)
                if held.order.replacement_event_id == replacement_event_id)
            .then(|| target.clone())
        }) else {
            return false;
        };
        self.by_target.remove(&target);
        true
    }

    /// Whether the message `target` shows an edit: `(edited)`.
    pub(super) fn shows_edited(&self, target: &str) -> bool {
        matches!(self.by_target.get(target), Some(LineEdit::Applied(_)))
    }

    #[cfg(test)]
    pub(super) fn is_empty(&self) -> bool {
        self.by_target.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn order(timestamp_ms: u64, replacement_event_id: &str) -> EditOrder {
        EditOrder {
            timestamp_ms,
            replacement_event_id: replacement_event_id.to_owned(),
        }
    }

    fn held(timestamp_ms: u64, replacement_event_id: &str) -> HeldEdit {
        HeldEdit {
            order: order(timestamp_ms, replacement_event_id),
            sender: "@alice:example".to_owned(),
            fields: MessageReplacementFields {
                presentation: RoomMessagePresentationKind::Text,
                body: "fixed".to_owned(),
                styles: Vec::new(),
                user_links: Vec::new(),
            },
        }
    }

    #[test]
    fn a_later_edit_is_newer_by_time_then_by_event_id() {
        let mut edits = LineEdits::default();
        assert!(edits.is_newer("$m", &order(1, "$a")));
        edits.applied("$m", order(2, "$b"));
        assert!(!edits.is_newer("$m", &order(1, "$z")));
        assert!(!edits.is_newer("$m", &order(2, "$a")));
        assert!(!edits.is_newer("$m", &order(2, "$b")), "the same edit");
        assert!(edits.is_newer("$m", &order(2, "$c")));
        assert!(edits.is_newer("$m", &order(3, "$a")));
        assert!(edits.is_newer("$other", &order(1, "$a")));

        edits.hold("$u", held(5, "$h"));
        assert!(!edits.is_newer("$u", &order(4, "$z")));
        assert!(edits.is_newer("$u", &order(6, "$a")));
    }

    #[test]
    fn only_an_applied_edit_shows_and_a_held_one_is_taken_once() {
        let mut edits = LineEdits::default();
        edits.applied("$m", order(1, "$a"));
        edits.hold("$u", held(1, "$h"));
        assert!(edits.shows_edited("$m"));
        assert!(!edits.shows_edited("$u"));

        assert_eq!(
            edits.take_held("$u").map(|held| held.order),
            Some(order(1, "$h"))
        );
        assert!(edits.take_held("$u").is_none());
        assert!(!edits.shows_edited("$u"));
    }

    #[test]
    fn forgetting_drops_a_lines_edit_and_a_redacted_held_edit() {
        let mut edits = LineEdits::default();
        edits.applied("$m", order(1, "$a"));
        edits.hold("$u", held(1, "$h"));

        assert!(!edits.forget_held_by_replacement("$a"), "applied stays");
        assert!(edits.shows_edited("$m"));
        assert!(edits.forget_held_by_replacement("$h"));
        assert!(!edits.forget_held_by_replacement("$h"));
        edits.forget("$m");
        assert!(edits.is_empty());
    }
}
