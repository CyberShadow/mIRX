//! Whom a message names, for its `m.mentions` (MSC3952, required since
//! MSC4210 removed the legacy body-matching push rules) and its pills.
//! Pure, over the room's members.

use std::collections::{BTreeMap, BTreeSet};

use matrix_sdk::ruma::{OwnedUserId, UserId};

use crate::api::UserLinkSpan;

/// What a message body names: whom it mentions, where to link them, and
/// whether it mentions the room.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct BodyMentions {
    pub(crate) user_ids: BTreeSet<OwnedUserId>,
    /// Sorted, non-overlapping, on one line each; one user per pill. They
    /// pass `check_user_links`.
    pub(crate) pills: Vec<UserLinkSpan>,
    pub(crate) room: bool,
}

/// The members `body` names, the way the legacy push rules noticed names
/// (`.m.rule.contains_display_name`, `.m.rule.roomnotif`) and the way mIRC
/// users name people: by typing a nick.
///
/// - **Candidates.** Every member but the user (`own_user_id`) is named by
///   their full user ID, and by their nick (display name, else localpart;
///   never empty) unless it has a line break, which no line typed can
///   match.
/// - **Matching.** A candidate matches where the body's text equals it,
///   ignoring case, between word boundaries: the start or end of the body,
///   or a character outside `[A-Za-z0-9_]`, as push rules define them.
/// - **Overlaps.** At each position the longest match is taken, and
///   matches are taken left to right, skipping any that overlaps one
///   taken: `Bob Smith` wins over `Bob` there, and `Bob` is not named.
/// - **Ambiguity.** A taken match that several members' candidates equal
///   (two members named `Sam`) names them all, but is no pill, since a
///   pill names one user. Every other taken match is a pill over the text
///   as typed.
/// - **`@room`** between word boundaries mentions the room. Whether the
///   sender may notify the room is the server's call
///   (`.m.rule.is_room_mention` checks the power level), as in Element.
pub(crate) fn body_mentions<'a>(
    body: &str,
    members: impl IntoIterator<Item = (&'a UserId, &'a str)>,
    own_user_id: &UserId,
) -> BodyMentions {
    let candidates: Vec<(&str, &UserId)> = members
        .into_iter()
        .filter(|(user_id, _)| *user_id != own_user_id)
        .flat_map(|(user_id, nick)| {
            assert!(!nick.is_empty(), "a nick names someone");
            let nick = (!nick.contains('\n')).then_some(nick);
            [Some(user_id.as_str()), nick]
                .into_iter()
                .flatten()
                .map(move |text| (text, user_id))
        })
        .collect();

    let mut user_ids = BTreeSet::new();
    let mut pills = Vec::new();
    let mut taken_end = 0;
    for (start, _) in body.char_indices() {
        if start < taken_end || !starts_word(body, start) {
            continue;
        }
        // The users each match length at `start` names.
        let mut matches: BTreeMap<usize, BTreeSet<&UserId>> = BTreeMap::new();
        for (text, user_id) in &candidates {
            if let Some(len) = word_match_len(body, start, text) {
                matches.entry(len).or_default().insert(user_id);
            }
        }
        let Some((len, named)) = matches.pop_last() else {
            continue;
        };
        let end = start + len;
        taken_end = end;
        if let [user_id] = named.iter().collect::<Vec<_>>()[..] {
            assert!(
                !body[start..end].contains('\n'),
                "no candidate has a line break"
            );
            pills.push(UserLinkSpan {
                start,
                end,
                user_id: user_id.to_string(),
            });
        }
        user_ids.extend(named.into_iter().map(UserId::to_owned));
    }

    let room = body.char_indices().any(|(start, _)| {
        starts_word(body, start) && word_match_len(body, start, "@room").is_some()
    });
    BodyMentions {
        user_ids,
        pills,
        room,
    }
}

/// Whether `c` is a word character as push rules define it.
fn is_word_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_'
}

/// Whether a word boundary precedes `body[start..]`.
fn starts_word(body: &str, start: usize) -> bool {
    !body[..start].chars().next_back().is_some_and(is_word_char)
}

/// The byte length of `needle` at `body[start..]`, ignoring case, when a
/// word boundary follows it.
fn word_match_len(body: &str, start: usize, needle: &str) -> Option<usize> {
    let len = match_len_ignoring_case(&body[start..], needle)?;
    (!body[start + len..].chars().next().is_some_and(is_word_char)).then_some(len)
}

/// The byte length of the start of `text` that equals `needle` ignoring
/// case, char by char. Lowercasing can change a text's byte length, so
/// matching in a lowercased copy would give wrong offsets into `text`.
fn match_len_ignoring_case(text: &str, needle: &str) -> Option<usize> {
    let mut text_chars = text.char_indices();
    for needle_char in needle.chars() {
        let (_, text_char) = text_chars.next()?;
        if !text_char.to_lowercase().eq(needle_char.to_lowercase()) {
            return None;
        }
    }
    Some(text_chars.next().map_or(text.len(), |(index, _)| index))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn user(id: &str) -> OwnedUserId {
        OwnedUserId::try_from(id).expect("valid user id")
    }

    fn pill(start: usize, end: usize, user_id: &str) -> UserLinkSpan {
        UserLinkSpan {
            start,
            end,
            user_id: user_id.to_owned(),
        }
    }

    const ME: &str = "@me:example.org";

    /// The mentions of `body` among `members` (user ID, nick), as user ID
    /// strings, with each pill's text.
    fn named(body: &str, members: &[(&str, &str)]) -> (Vec<String>, Vec<(String, String)>, bool) {
        let members: Vec<(OwnedUserId, &str)> =
            members.iter().map(|(id, nick)| (user(id), *nick)).collect();
        let mentions = body_mentions(
            body,
            members.iter().map(|(id, name)| (id.as_ref(), *name)),
            &user(ME),
        );
        assert_eq!(crate::api::check_user_links(body, &mentions.pills), Ok(()));
        (
            mentions.user_ids.iter().map(ToString::to_string).collect(),
            mentions
                .pills
                .iter()
                .map(|pill| (body[pill.start..pill.end].to_owned(), pill.user_id.clone()))
                .collect(),
            mentions.room,
        )
    }

    fn ids(ids: &[&str]) -> Vec<String> {
        ids.iter().map(|id| (*id).to_owned()).collect()
    }

    fn pills(pills: &[(&str, &str)]) -> Vec<(String, String)> {
        pills
            .iter()
            .map(|(text, id)| ((*text).to_owned(), (*id).to_owned()))
            .collect()
    }

    const ALICE: (&str, &str) = ("@alice:example.org", "Alice");
    const BOB: (&str, &str) = ("@bob:example.org", "Bob");

    #[test]
    fn nicks_match_as_whole_words_in_any_case() {
        assert_eq!(
            named("alice: hi, BOB!", &[ALICE, BOB]),
            (
                ids(&["@alice:example.org", "@bob:example.org"]),
                pills(&[("alice", "@alice:example.org"), ("BOB", "@bob:example.org")]),
                false
            )
        );
        assert_eq!(
            named("Bobby bob_ xbob Bob2", &[BOB]),
            (Vec::new(), Vec::new(), false),
            "a name inside a longer word is no mention"
        );
        assert_eq!(named("sure", &[ALICE]), (Vec::new(), Vec::new(), false));
    }

    #[test]
    fn names_at_the_start_and_end_and_on_other_lines_match() {
        assert_eq!(
            named("Alice\nask bob", &[ALICE, BOB]),
            (
                ids(&["@alice:example.org", "@bob:example.org"]),
                pills(&[("Alice", "@alice:example.org"), ("bob", "@bob:example.org")]),
                false
            )
        );
        let mentions = body_mentions(
            "one\ntwo Bob",
            [(user("@bob:example.org").as_ref(), "Bob")],
            &user(ME),
        );
        assert_eq!(mentions.pills, vec![pill(8, 11, "@bob:example.org")]);
    }

    #[test]
    fn full_user_ids_match_and_pill() {
        assert_eq!(
            named("ping @Carol:example.org!", &[("@carol:example.org", "Caz")]),
            (
                ids(&["@carol:example.org"]),
                pills(&[("@Carol:example.org", "@carol:example.org")]),
                false
            )
        );
        assert_eq!(
            named("x@carol:example.org", &[("@carol:example.org", "Caz")]),
            (Vec::new(), Vec::new(), false)
        );
    }

    #[test]
    fn the_user_is_never_named() {
        assert_eq!(
            named("Me and @me:example.org", &[(ME, "Me"), ALICE]),
            (Vec::new(), Vec::new(), false)
        );
    }

    #[test]
    fn multi_line_nicks_name_nobody() {
        assert_eq!(
            named("a b", &[("@nl:example.org", "a\nb")]),
            (Vec::new(), Vec::new(), false)
        );
    }

    #[test]
    fn a_nameless_member_is_named_by_localpart() {
        assert_eq!(
            named("ghost: hi", &[("@ghost:example.org", "ghost"), ALICE]),
            (
                ids(&["@ghost:example.org"]),
                pills(&[("ghost", "@ghost:example.org")]),
                false
            )
        );
    }

    #[test]
    fn a_localpart_equal_to_another_members_name_names_both_without_a_pill() {
        assert_eq!(
            named(
                "ghost: hi",
                &[
                    ("@ghost:example.org", "ghost"),
                    ("@casper:example.org", "Ghost")
                ]
            ),
            (
                ids(&["@casper:example.org", "@ghost:example.org"]),
                Vec::new(),
                false
            )
        );
    }

    #[test]
    fn the_longest_match_at_a_position_wins() {
        let bob_smith = ("@smith:example.org", "Bob Smith");
        assert_eq!(
            named("Bob Smith, Bob", &[BOB, bob_smith]),
            (
                ids(&["@bob:example.org", "@smith:example.org"]),
                pills(&[
                    ("Bob Smith", "@smith:example.org"),
                    ("Bob", "@bob:example.org")
                ]),
                false
            )
        );
        assert_eq!(
            named("Bob Smith", &[BOB, bob_smith]),
            (
                ids(&["@smith:example.org"]),
                pills(&[("Bob Smith", "@smith:example.org")]),
                false
            ),
            "the shorter name inside a taken match is not named"
        );
    }

    #[test]
    fn an_ambiguous_name_names_everyone_with_it_without_a_pill() {
        let sam1 = ("@sam1:example.org", "Sam");
        let sam2 = ("@sam2:example.org", "sam");
        assert_eq!(
            named("Sam: hi", &[sam1, sam2]),
            (
                ids(&["@sam1:example.org", "@sam2:example.org"]),
                Vec::new(),
                false
            )
        );
        assert_eq!(
            named("@sam1:example.org: hi", &[sam1, sam2]),
            (
                ids(&["@sam1:example.org"]),
                pills(&[("@sam1:example.org", "@sam1:example.org")]),
                false
            ),
            "a user ID is never ambiguous"
        );
    }

    #[test]
    fn case_folding_keeps_byte_offsets_into_the_typed_text() {
        let aerger = ("@aerger:example.org", "Ärger");
        let mentions = body_mentions(
            "größe ärger",
            [(user(aerger.0).as_ref(), aerger.1)],
            &user(ME),
        );
        assert_eq!(mentions.pills, vec![pill(8, 14, "@aerger:example.org")]);
        assert_eq!(
            named("ÄRGER? ärger", &[aerger]),
            (
                ids(&["@aerger:example.org"]),
                pills(&[
                    ("ÄRGER", "@aerger:example.org"),
                    ("ärger", "@aerger:example.org")
                ]),
                false
            )
        );
    }

    #[test]
    fn at_room_mentions_the_room() {
        assert!(named("@room", &[]).2);
        assert!(named("@room: deploy", &[]).2);
        assert!(named("hey @ROOM!", &[]).2);
        assert!(!named("x@room", &[]).2);
        assert!(!named("@roomy", &[]).2);
        assert!(!named("room", &[]).2);
        assert_eq!(named("@room", &[]).1, Vec::new(), "no pill");
    }
}
