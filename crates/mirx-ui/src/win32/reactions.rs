//! The reaction summary row under a message. mIRC has no reactions, so
//! mirx shows them on one dim, indented line that names who reacted, the
//! way mIRC lists nicks: `    reactions: 👍 Alice, Bob  heart Carol and 2 more`.
//!
//! Pure code: the row's text from the message's reaction groups.

use crate::automation::DumpRoomReactionSummary;

/// How many reactors a group names before it counts the rest as
/// `and N more` (`+N` would read as a reaction key).
pub(super) const REACTORS_SHOWN: usize = 3;

/// The reaction summary row's text. Each group is its key and its first
/// [`REACTORS_SHOWN`] reactors' nicks, in the order the summaries give
/// (reaction time); groups are separated by two spaces. `nick` names a
/// reactor by user ID.
pub(super) fn summary_line(
    summaries: &[DumpRoomReactionSummary],
    nick: impl Fn(&str) -> String,
) -> String {
    assert!(
        !summaries.is_empty(),
        "a message without reactions has no reaction row"
    );
    let groups = summaries
        .iter()
        .map(|summary| {
            let shown = summary
                .senders
                .iter()
                .take(REACTORS_SHOWN)
                .map(|sender| nick(sender))
                .collect::<Vec<_>>()
                .join(", ");
            match summary.senders.len().saturating_sub(REACTORS_SHOWN) {
                0 => format!("{} {shown}", summary.key),
                rest => format!("{} {shown} and {rest} more", summary.key),
            }
        })
        .collect::<Vec<_>>();
    format!("    reactions: {}", groups.join("  "))
}

#[cfg(test)]
mod tests {
    use super::summary_line;
    use crate::automation::DumpRoomReactionSummary;

    fn summary(key: &str, senders: &[&str]) -> DumpRoomReactionSummary {
        DumpRoomReactionSummary {
            key: key.to_owned(),
            count: senders.len() as u64,
            senders: senders.iter().map(|sender| (*sender).to_owned()).collect(),
            reaction_event_ids: (0..senders.len())
                .map(|index| format!("$r{index}"))
                .collect(),
        }
    }

    /// Alice, Bob, Carol, Dave and Erin are members; the user is "me".
    fn nick(user_id: &str) -> String {
        match user_id {
            "@alice:example" => "Alice",
            "@bob:example" => "Bob",
            "@carol:example" => "Carol",
            "@dave:example" => "Dave",
            "@erin:example" => "Erin",
            "@me:example" => "me",
            other => other,
        }
        .to_owned()
    }

    #[test]
    fn one_reactor_is_named() {
        assert_eq!(
            summary_line(&[summary("👍", &["@alice:example"])], nick),
            "    reactions: 👍 Alice"
        );
    }

    #[test]
    fn up_to_three_reactors_are_named() {
        assert_eq!(
            summary_line(
                &[summary(
                    "heart",
                    &["@carol:example", "@dave:example", "@erin:example"]
                )],
                nick
            ),
            "    reactions: heart Carol, Dave, Erin"
        );
    }

    #[test]
    fn reactors_past_three_are_counted() {
        assert_eq!(
            summary_line(
                &[summary(
                    "heart",
                    &[
                        "@carol:example",
                        "@dave:example",
                        "@erin:example",
                        "@alice:example",
                        "@bob:example",
                    ]
                )],
                nick
            ),
            "    reactions: heart Carol, Dave, Erin and 2 more"
        );
    }

    #[test]
    fn groups_keep_their_order_two_spaces_apart() {
        assert_eq!(
            summary_line(
                &[
                    summary("👍", &["@alice:example", "@bob:example"]),
                    summary("heart", &["@carol:example"]),
                ],
                nick
            ),
            "    reactions: 👍 Alice, Bob  heart Carol"
        );
    }

    #[test]
    fn a_reactor_outside_the_members_shows_by_user_id() {
        assert_eq!(
            summary_line(&[summary("+1", &["@ghost:example"])], nick),
            "    reactions: +1 @ghost:example"
        );
    }

    #[test]
    fn the_users_own_reaction_shows_their_nick() {
        assert_eq!(
            summary_line(&[summary("+1", &["@me:example", "@alice:example"])], nick),
            "    reactions: +1 me, Alice"
        );
    }
}
