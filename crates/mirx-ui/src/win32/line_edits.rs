//! Every edit of each line that has one, and what the line showed before
//! them, so that a redacted edit gives back the text it replaced.

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

/// An edit of a line: who sent it, and the text it gives the line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Edit {
    pub(super) sender: String,
    pub(super) fields: MessageReplacementFields,
}

/// The line an edit is for, as the window shows it.
pub(super) enum EditedLine<'a> {
    /// A message, showing its latest edit if it has one.
    Message(&'a RoomMessageActivity),
    /// A line that could not be decrypted: it holds its edits until it does.
    Undecrypted,
}

/// What a line shows once one of its edits was redacted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Restore {
    /// The latest edit left, still edited.
    Edit(MessageReplacementFields),
    /// No edit left: the message's own text, not edited.
    Original(MessageReplacementFields),
    /// What it shows: the edit was not the latest, or the line holds it.
    Unchanged,
}

/// Every edit seen of each shown line that has any, by the line's event ID.
/// A message line shows the latest (and `(edited)`); a line that could not
/// be decrypted holds them until it decrypts (`RoomViewModel::apply_decrypted`).
#[derive(Debug, Default, Clone)]
pub(super) struct LineEdits {
    by_target: BTreeMap<String, LineEdit>,
}

/// The edits of one line, never none, and what it showed before them.
#[derive(Debug, Clone)]
struct LineEdit {
    /// The message's own fields; `None` while the line could not be
    /// decrypted (its edits are held, their senders checked when it does).
    original: Option<MessageReplacementFields>,
    edits: BTreeMap<EditOrder, Edit>,
}

impl LineEdit {
    fn latest(&self) -> &Edit {
        self.edits
            .last_key_value()
            .map(|(_, edit)| edit)
            .expect("a line's entry has an edit")
    }
}

impl LineEdits {
    /// Keeps an edit of `target` (once: the same order again is the same
    /// edit), whose sender the caller checked against the line's. Returns
    /// what the message line must show now, when the edit is its latest;
    /// a held one shows nothing yet.
    pub(super) fn edit(
        &mut self,
        target: &str,
        order: EditOrder,
        edit: Edit,
        line: EditedLine<'_>,
    ) -> Option<MessageReplacementFields> {
        let entry = self
            .by_target
            .entry(target.to_owned())
            .or_insert_with(|| LineEdit {
                original: match line {
                    EditedLine::Message(message) => Some(MessageReplacementFields::of(message)),
                    EditedLine::Undecrypted => None,
                },
                edits: BTreeMap::new(),
            });
        assert_eq!(
            entry.original.is_some(),
            matches!(line, EditedLine::Message(_)),
            "{target}'s edits are held exactly while it could not be decrypted"
        );
        let latest = entry
            .edits
            .last_key_value()
            .is_none_or(|(latest, _)| order > *latest);
        entry.edits.entry(order).or_insert(edit);
        (latest && entry.original.is_some()).then(|| entry.latest().fields.clone())
    }

    /// `target`, which held its edits, decrypted to `message`: the edits of
    /// its sender stay, and the latest is what it must show, if any.
    pub(super) fn decrypted(
        &mut self,
        target: &str,
        message: &RoomMessageActivity,
    ) -> Option<MessageReplacementFields> {
        let mut entry = self.by_target.remove(target)?;
        assert!(
            entry.original.is_none(),
            "{target} decrypted with its edits shown"
        );
        entry
            .edits
            .retain(|_, edit| edit.sender == message.sender.user_id);
        if entry.edits.is_empty() {
            return None;
        }
        entry.original = Some(MessageReplacementFields::of(message));
        let fields = entry.latest().fields.clone();
        self.by_target.insert(target.to_owned(), entry);
        Some(fields)
    }

    /// The edit `replacement_event_id` was redacted: the line that has it,
    /// and what that line must show now; `None` when no line has it (the
    /// redaction is of something else).
    pub(super) fn redact_edit(&mut self, replacement_event_id: &str) -> Option<(String, Restore)> {
        let (target, order) = self.find_edit(replacement_event_id)?;
        let entry = self.by_target.get_mut(&target).expect("found it above");
        let was_latest = entry
            .edits
            .last_key_value()
            .is_some_and(|(latest, _)| *latest == order);
        entry.edits.remove(&order);
        let restore = match &entry.original {
            // Held: the line shows none of its edits yet.
            None => Restore::Unchanged,
            Some(_) if !was_latest => Restore::Unchanged,
            Some(original) => match entry.edits.last_key_value() {
                Some((_, edit)) => Restore::Edit(edit.fields.clone()),
                None => Restore::Original(original.clone()),
            },
        };
        if entry.edits.is_empty() {
            self.by_target.remove(&target);
        }
        Some((target, restore))
    }

    /// Whether a line has the edit `replacement_event_id`.
    pub(super) fn has_edit(&self, replacement_event_id: &str) -> bool {
        self.find_edit(replacement_event_id).is_some()
    }

    /// The line that has the edit `replacement_event_id`, and its order.
    fn find_edit(&self, replacement_event_id: &str) -> Option<(String, EditOrder)> {
        self.by_target.iter().find_map(|(target, entry)| {
            entry
                .edits
                .keys()
                .find(|order| order.replacement_event_id == replacement_event_id)
                .map(|order| (target.clone(), order.clone()))
        })
    }

    /// `target` was redacted, or left the window, or decrypted to a line
    /// that has no text to edit.
    pub(super) fn forget(&mut self, target: &str) {
        self.by_target.remove(target);
    }

    /// Whether the message `target` shows an edit: `(edited)`.
    pub(super) fn shows_edited(&self, target: &str) -> bool {
        self.by_target
            .get(target)
            .is_some_and(|entry| entry.original.is_some())
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

    fn fields(body: &str) -> MessageReplacementFields {
        MessageReplacementFields {
            presentation: RoomMessagePresentationKind::Text,
            body: body.to_owned(),
            styles: Vec::new(),
            user_links: Vec::new(),
        }
    }

    fn edit(sender: &str, body: &str) -> Edit {
        Edit {
            sender: sender.to_owned(),
            fields: fields(body),
        }
    }

    fn message(sender: &str, body: &str) -> RoomMessageActivity {
        let Event::RoomActivity(activity) = test_fixtures::timeline_with_transaction_id(
            "!ops:example",
            "$m",
            None,
            body,
            1_700_000_000_000,
        ) else {
            unreachable!()
        };
        let RoomActivityKind::Message(mut message) = activity.kind else {
            unreachable!()
        };
        message.sender = test_fixtures::user_profile(sender);
        message
    }

    const ALICE: &str = "@alice:example";

    /// `$m` reads `first`, edited `second` at 2 and `third` at 3.
    fn edited() -> LineEdits {
        let original = message(ALICE, "first");
        let mut edits = LineEdits::default();
        let line = || EditedLine::Message(&original);
        assert_eq!(
            edits.edit("$m", order(3, "$e3"), edit(ALICE, "third"), line()),
            Some(fields("third"))
        );
        assert_eq!(
            edits.edit("$m", order(2, "$e2"), edit(ALICE, "second"), line()),
            None,
            "an older edit arriving late shows nothing"
        );
        assert!(edits.shows_edited("$m"));
        edits
    }

    #[test]
    fn edits_in_any_order_show_the_latest_and_the_same_edit_again_is_one() {
        let original = message(ALICE, "first");
        let mut edits = LineEdits::default();
        let line = || EditedLine::Message(&original);
        assert!(!edits.shows_edited("$m"));
        assert_eq!(
            edits.edit("$m", order(2, "$b"), edit(ALICE, "b"), line()),
            Some(fields("b"))
        );
        assert_eq!(
            edits.edit("$m", order(1, "$z"), edit(ALICE, "z"), line()),
            None
        );
        assert_eq!(
            edits.edit("$m", order(2, "$a"), edit(ALICE, "a"), line()),
            None,
            "an equal time orders by event ID"
        );
        assert_eq!(
            edits.edit("$m", order(2, "$b"), edit(ALICE, "b"), line()),
            None,
            "the same edit"
        );
        assert_eq!(
            edits.edit("$m", order(2, "$c"), edit(ALICE, "c"), line()),
            Some(fields("c"))
        );
        // Redacting the latest twice over shows each edit in turn, then the
        // message's own text: every edit was kept, once.
        for (redacted, restore) in [
            ("$c", Restore::Edit(fields("b"))),
            ("$b", Restore::Edit(fields("a"))),
            ("$a", Restore::Edit(fields("z"))),
            ("$z", Restore::Original(fields("first"))),
        ] {
            assert_eq!(
                edits.redact_edit(redacted),
                Some(("$m".to_owned(), restore)),
                "{redacted}"
            );
        }
        assert!(edits.is_empty());
    }

    #[test]
    fn redacting_the_latest_edit_restores_the_previous_then_the_original() {
        let mut edits = edited();

        assert_eq!(
            edits.redact_edit("$e3"),
            Some(("$m".to_owned(), Restore::Edit(fields("second"))))
        );
        assert!(edits.shows_edited("$m"));
        assert_eq!(
            edits.redact_edit("$e2"),
            Some(("$m".to_owned(), Restore::Original(fields("first"))))
        );
        assert!(!edits.shows_edited("$m"));
        assert!(edits.is_empty());
        assert_eq!(edits.redact_edit("$e2"), None, "gone");
    }

    #[test]
    fn redacting_an_older_edit_changes_nothing_shown() {
        let mut edits = edited();

        assert_eq!(
            edits.redact_edit("$e2"),
            Some(("$m".to_owned(), Restore::Unchanged))
        );
        assert_eq!(
            edits.redact_edit("$e3"),
            Some(("$m".to_owned(), Restore::Original(fields("first"))))
        );
    }

    #[test]
    fn a_held_line_keeps_every_edit_and_keeps_its_senders_when_it_decrypts() {
        let mut edits = LineEdits::default();
        for (timestamp_ms, id, sender, body) in [
            (1, "$own", ALICE, "own"),
            (2, "$theirs", "@mallory:example", "theirs"),
            (3, "$dropped", ALICE, "dropped"),
        ] {
            assert_eq!(
                edits.edit(
                    "$u",
                    order(timestamp_ms, id),
                    edit(sender, body),
                    EditedLine::Undecrypted
                ),
                None,
                "a held edit shows nothing"
            );
        }
        assert!(!edits.shows_edited("$u"));
        assert_eq!(
            edits.redact_edit("$dropped"),
            Some(("$u".to_owned(), Restore::Unchanged)),
            "a redacted held edit is dropped alone"
        );

        assert_eq!(
            edits.decrypted("$u", &message(ALICE, "decrypted")),
            Some(fields("own"))
        );
        assert!(edits.shows_edited("$u"));
        assert_eq!(
            edits.redact_edit("$own"),
            Some(("$u".to_owned(), Restore::Original(fields("decrypted"))))
        );
        assert_eq!(edits.redact_edit("$theirs"), None, "not its sender's");
    }

    #[test]
    fn a_held_line_whose_edits_are_all_someone_elses_shows_none() {
        let mut edits = LineEdits::default();
        edits.edit(
            "$u",
            order(1, "$theirs"),
            edit("@mallory:example", "theirs"),
            EditedLine::Undecrypted,
        );

        assert_eq!(edits.decrypted("$u", &message(ALICE, "decrypted")), None);
        assert!(edits.is_empty());
        assert_eq!(edits.decrypted("$other", &message(ALICE, "plain")), None);
    }

    #[test]
    fn an_unknown_edit_or_a_forgotten_line_has_nothing_to_redact() {
        let mut edits = edited();
        assert_eq!(edits.redact_edit("$unknown"), None);
        assert!(!edits.has_edit("$unknown"));
        assert!(edits.has_edit("$e2"));

        edits.forget("$m");
        assert!(edits.is_empty());
        assert_eq!(edits.redact_edit("$e3"), None);
    }
}
