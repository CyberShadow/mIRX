//! mIRC-style Tab nick completion for the compose edit box.
//!
//! Tab replaces the word before the caret with the first member whose nick
//! starts with it, and further Tabs cycle through the other matches; Shift+Tab
//! cycles backwards. Like mIRC, the nick is inserted bare, without a `: `
//! suffix. Positions are UTF-16 code units, as an EDIT control reports them.

use std::char::REPLACEMENT_CHARACTER;
use std::ops::Range;

use mirx_core::RoomMemberSummary;

/// A replacement of `replace` in the edit text by `text`; the caret goes
/// after `text`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct CompletionEdit {
    pub(super) replace: Range<usize>,
    pub(super) text: String,
}

/// A completion in progress: the edit text around the completed word, and
/// the matches it cycles through. It only continues while the edit text is
/// exactly what it last left there, with the caret after the inserted nick;
/// any other edit or caret move starts a new completion.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct NickCompletion {
    before: Vec<u16>,
    after: Vec<u16>,
    /// Never empty.
    candidates: Vec<String>,
    index: usize,
}

impl NickCompletion {
    /// Completes the word that ends at `caret` against `members`, taken in
    /// nick list order. `None` when there is no word or no member matches.
    ///
    /// A word matches a member whose nick, or user ID with or without its
    /// `@`, starts with it, ignoring case. A word starting with `@`
    /// completes to user IDs, so `/query @al` and `/kick @bo` stay useful;
    /// any other word completes to the member's nick. Matches that would
    /// insert the same text appear once: two members both named Sam
    /// complete to one `Sam`, and typing `@sam` picks between them. Our own
    /// user is never offered, since naming ourselves is pointless.
    pub(super) fn start(
        text: &[u16],
        caret: usize,
        members: &[RoomMemberSummary],
        reverse: bool,
    ) -> Option<(Self, CompletionEdit)> {
        let word_start = word_start(&text[..caret]);
        let word = String::from_utf16_lossy(&text[word_start..caret]);
        if word.is_empty() {
            return None;
        }
        let word = word.to_lowercase();
        let wants_user_id = word.starts_with('@');

        let mut candidates = Vec::<String>::new();
        for member in members.iter().filter(|member| !member.is_own_user) {
            let user_id = member.user_id.to_lowercase();
            let matches = if wants_user_id {
                user_id.starts_with(&word)
            } else {
                user_id.trim_start_matches('@').starts_with(&word)
                    || member.nick.to_lowercase().starts_with(&word)
            };
            if !matches {
                continue;
            }
            let inserted = if wants_user_id {
                member.user_id.as_str()
            } else {
                member.nick.as_str()
            };
            if !candidates.iter().any(|candidate| candidate == inserted) {
                candidates.push(inserted.to_owned());
            }
        }
        if candidates.is_empty() {
            return None;
        }

        let index = if reverse { candidates.len() - 1 } else { 0 };
        let completion = Self {
            before: text[..word_start].to_vec(),
            after: text[caret..].to_vec(),
            candidates,
            index,
        };
        let edit = CompletionEdit {
            replace: word_start..caret,
            text: completion.candidates[index].clone(),
        };
        Some((completion, edit))
    }

    /// Whether `text` with the caret at `caret` is what this completion last
    /// left in the edit box.
    pub(super) fn continues(&self, text: &[u16], caret: usize) -> bool {
        let applied = self.applied_range();
        caret == applied.end
            && text.len() == applied.end + self.after.len()
            && text[..applied.start] == self.before[..]
            && text[applied.clone()]
                .iter()
                .copied()
                .eq(self.candidates[self.index].encode_utf16())
            && text[applied.end..] == self.after[..]
    }

    /// Cycles to the next match, or the previous one when `reverse`, wrapping
    /// around, and replaces the nick inserted last.
    pub(super) fn advance(&mut self, reverse: bool) -> CompletionEdit {
        let replace = self.applied_range();
        let count = self.candidates.len();
        self.index = if reverse {
            (self.index + count - 1) % count
        } else {
            (self.index + 1) % count
        };
        CompletionEdit {
            replace,
            text: self.candidates[self.index].clone(),
        }
    }

    fn applied_range(&self) -> Range<usize> {
        let start = self.before.len();
        start..start + self.candidates[self.index].encode_utf16().count()
    }
}

/// The start of the run of non-whitespace characters that ends `text`.
fn word_start(text: &[u16]) -> usize {
    let mut start = 0;
    let mut end = 0;
    for unit in char::decode_utf16(text.iter().copied()) {
        // A lone surrogate is one unit and not whitespace.
        let character = unit.unwrap_or(REPLACEMENT_CHARACTER);
        end += character.len_utf16();
        if character.is_whitespace() {
            start = end;
        }
    }
    start
}

#[cfg(test)]
mod tests {
    use mirx_core::RoomMemberRank;

    use super::*;

    fn member(user_id: &str, nick: &str) -> RoomMemberSummary {
        RoomMemberSummary {
            user_id: user_id.to_owned(),
            nick: nick.to_owned(),
            nick_ambiguous: false,
            is_own_user: false,
            rank: RoomMemberRank::Member,
            can_kick: false,
            can_ban: false,
        }
    }

    fn members() -> Vec<RoomMemberSummary> {
        vec![
            member("@zed:example", "Alfred"),
            RoomMemberSummary {
                is_own_user: true,
                ..member("@alex:example", "Alex")
            },
            member("@alice:example", "Alice"),
            member("@alan:example", "Alan"),
            member("@casey:example", "Casey"),
            member("@sam-one:example", "Sam"),
            member("@sam-two:example", "Sam"),
            // Without a display name, the core names them by localpart.
            member("@ghost:example", "ghost"),
            member("@zoë:example", "Zoë 🦊"),
        ]
    }

    fn utf16(text: &str) -> Vec<u16> {
        text.encode_utf16().collect()
    }

    /// The edit text and caret after applying `edit` to `text`.
    fn apply(text: &[u16], edit: &CompletionEdit) -> (Vec<u16>, usize) {
        let mut result = text[..edit.replace.start].to_vec();
        result.extend(edit.text.encode_utf16());
        let caret = result.len();
        result.extend_from_slice(&text[edit.replace.end..]);
        (result, caret)
    }

    /// Starts at the end of `text` and returns each completed text for the
    /// first `tabs` presses, checking that every press continues the last.
    fn complete(text: &str, caret: usize, tabs: &[bool]) -> Vec<String> {
        let members = members();
        let text = utf16(text);
        let (mut completion, edit) =
            NickCompletion::start(&text, caret, &members, tabs[0]).expect("completion starts");
        let (mut text, mut caret) = apply(&text, &edit);
        let mut results = vec![String::from_utf16(&text).expect("utf-16")];
        for &reverse in &tabs[1..] {
            assert!(completion.continues(&text, caret));
            let edit = completion.advance(reverse);
            (text, caret) = apply(&text, &edit);
            results.push(String::from_utf16(&text).expect("utf-16"));
        }
        assert!(completion.continues(&text, caret));
        results
    }

    #[test]
    fn completes_nick_prefixes_case_insensitively_in_member_order() {
        assert_eq!(
            complete("hi al", 5, &[false, false, false, false]),
            vec!["hi Alfred", "hi Alice", "hi Alan", "hi Alfred"]
        );
        assert_eq!(complete("CAS", 3, &[false]), vec!["Casey"]);
    }

    #[test]
    fn shift_tab_cycles_backwards_and_wraps() {
        assert_eq!(
            complete("al", 2, &[true, true, false, false]),
            vec!["Alan", "Alice", "Alan", "Alfred"]
        );
        assert_eq!(complete("al", 2, &[false, true]), vec!["Alfred", "Alan"]);
    }

    #[test]
    fn user_id_localparts_match_and_insert_the_nick() {
        // `zed` is Alfred's localpart.
        assert_eq!(complete("ze", 2, &[false]), vec!["Alfred"]);
        // A member without a display name goes by their localpart.
        assert_eq!(complete("gh", 2, &[false]), vec!["ghost"]);
        assert_eq!(complete("@gh", 3, &[false]), vec!["@ghost:example"]);
    }

    /// Nicks differing in case are distinct texts to insert, so both are
    /// offered; the core names both users for either.
    #[test]
    fn nicks_differing_in_case_are_both_offered() {
        let members = [
            member("@casper:example", "Ghost"),
            member("@ghost:example", "ghost"),
        ];
        let text = utf16("gh");
        let (mut completion, edit) =
            NickCompletion::start(&text, 2, &members, false).expect("completion starts");
        assert_eq!(edit.text, "Ghost");
        assert_eq!(completion.advance(false).text, "ghost");
    }

    #[test]
    fn at_sign_words_complete_to_user_ids() {
        assert_eq!(
            complete("/query @ca", 10, &[false]),
            vec!["/query @casey:example"]
        );
        assert_eq!(
            complete("@SAM", 4, &[false, false, false]),
            vec!["@sam-one:example", "@sam-two:example", "@sam-one:example"]
        );
        // `@` words match user IDs only, not display names.
        assert!(NickCompletion::start(&utf16("@alf"), 4, &members(), false).is_none());
    }

    #[test]
    fn members_sharing_a_display_name_complete_once() {
        assert_eq!(complete("sa", 2, &[false, false]), vec!["Sam", "Sam"]);
    }

    #[test]
    fn own_user_is_never_offered() {
        assert!(NickCompletion::start(&utf16("alex"), 4, &members(), false).is_none());
        assert!(NickCompletion::start(&utf16("@alex"), 5, &members(), false).is_none());
    }

    #[test]
    fn empty_words_and_unmatched_words_do_not_complete() {
        let members = members();
        assert!(NickCompletion::start(&utf16(""), 0, &members, false).is_none());
        assert!(NickCompletion::start(&utf16("hi "), 3, &members, false).is_none());
        assert!(NickCompletion::start(&utf16("hi xyz"), 6, &members, false).is_none());
        assert!(NickCompletion::start(&utf16("al"), 2, &[], false).is_none());
    }

    #[test]
    fn only_the_word_before_the_caret_is_replaced() {
        // Caret after "ca" in "ca there": the rest is kept.
        assert_eq!(complete("ca there", 2, &[false]), vec!["Casey there"]);
        // Caret inside a word completes the part before the caret.
        assert_eq!(complete("hi casXYZ", 6, &[false]), vec!["hi CaseyXYZ"]);
        let members = members();
        let text = utf16("x ca y");
        let (completion, edit) =
            NickCompletion::start(&text, 4, &members, false).expect("completion starts");
        assert_eq!(edit.replace, 2..4);
        let (text, caret) = apply(&text, &edit);
        assert_eq!(String::from_utf16(&text).expect("utf-16"), "x Casey y");
        assert_eq!(caret, 7);
        assert!(completion.continues(&text, caret));
    }

    #[test]
    fn positions_are_utf16_units_around_astral_and_non_ascii_text() {
        // "🦊" is two UTF-16 units, "é" one.
        assert_eq!(complete("🦊 é zo", 7, &[false]), vec!["🦊 é Zoë 🦊"]);
        let members = members();
        let text = utf16("🦊zo");
        // The fox is part of the word, which then matches nobody.
        assert!(NickCompletion::start(&text, 4, &members, false).is_none());
        let text = utf16("é al é");
        let (mut completion, edit) =
            NickCompletion::start(&text, 4, &members, false).expect("completion starts");
        assert_eq!(edit.replace, 2..4);
        let (text, caret) = apply(&text, &edit);
        assert_eq!(caret, 8);
        let edit = completion.advance(false);
        assert_eq!(edit.replace, 2..8);
        assert_eq!(edit.text, "Alice");
        let (text, caret) = apply(&text, &edit);
        assert_eq!(String::from_utf16(&text).expect("utf-16"), "é Alice é");
        assert!(completion.continues(&text, caret));
    }

    #[test]
    fn lone_surrogates_are_one_unit_of_the_word() {
        assert_eq!(word_start(&[0xD83E, u16::from(b' '), u16::from(b'a')]), 2);
        assert_eq!(word_start(&[u16::from(b' '), 0xDD8A, u16::from(b'a')]), 1);
    }

    #[test]
    fn advancing_replaces_the_inserted_nick_not_the_original_word() {
        let members = members();
        let text = utf16("hi al!");
        let (mut completion, edit) =
            NickCompletion::start(&text, 5, &members, false).expect("completion starts");
        assert_eq!(edit.replace, 3..5);
        let (text, _) = apply(&text, &edit);
        let edit = completion.advance(false);
        assert_eq!(edit.replace, 3..9, "covers the inserted \"Alfred\"");
        assert_eq!(apply(&text, &edit).0, utf16("hi Alice!"));
    }

    #[test]
    fn any_other_edit_or_caret_move_ends_the_completion() {
        let members = members();
        let text = utf16("hi al");
        let (completion, edit) =
            NickCompletion::start(&text, 5, &members, false).expect("completion starts");
        let (text, caret) = apply(&text, &edit);
        assert!(completion.continues(&text, caret));
        assert!(!completion.continues(&text, caret - 1));
        assert!(!completion.continues(&utf16("hi Alfredx"), caret));
        assert!(!completion.continues(&utf16("ho Alfred"), caret));
        assert!(!completion.continues(&utf16("hi Alfrex"), caret));
        assert!(!completion.continues(&utf16("hi Alfred "), caret));
    }
}
