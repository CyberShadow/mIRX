//! The room a command's room argument names, as mIRC users name channels.
//!
//! A target names a room by, in this order:
//! 1. its room ID (`!abc:example.org`);
//! 2. one of its aliases (`#ops:example.org`): canonical, alternative, or
//!    one it was joined by this session (`joined_aliases`: the room answers
//!    to it before its first sync brings its published ones). Aliases are
//!    case-sensitive in Matrix, so they are compared exactly;
//! 3. a short alias (`#ops`, no server): the localpart of one of its
//!    aliases. A room whose aliases share the localpart counts once;
//! 4. its display name, exactly. A `#name` is tried here too, after the
//!    aliases: bridged IRC rooms are often called `#rust` and have no
//!    `#rust:...` alias.
//!
//! Each step must match exactly one eligible room, else the target is
//! ambiguous; a step that matches none falls through to the next.
//!
//! Pure code over the core's room summaries.

use std::collections::BTreeMap;

use mirx_core::RoomSummary;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum RoomTargetError {
    Unknown,
    Ambiguous,
}

/// The room among the `eligible` ones that `target` names.
/// `joined_aliases`: alias -> room ID, the aliases rooms were joined by
/// (`UiModel::joined_aliases`).
pub(super) fn resolve_room<'r>(
    rooms: &'r [RoomSummary],
    joined_aliases: &BTreeMap<String, String>,
    eligible: impl Fn(&RoomSummary) -> bool,
    target: &str,
) -> Result<&'r RoomSummary, RoomTargetError> {
    let short_alias = target
        .strip_prefix('#')
        .filter(|localpart| !localpart.contains(':'));
    let steps: [&dyn Fn(&RoomSummary) -> bool; 4] = [
        &|room| room.room_id == target,
        &|room| aliases(room, joined_aliases).any(|alias| alias == target),
        &|room| {
            short_alias.is_some_and(|localpart| {
                aliases(room, joined_aliases).any(|alias| alias_localpart(alias) == localpart)
            })
        },
        &|room| room.display_name == target,
    ];
    for names_room in steps {
        let mut matches = rooms
            .iter()
            .filter(|room| eligible(room) && names_room(room));
        let Some(room) = matches.next() else {
            continue;
        };
        if matches.next().is_some() {
            return Err(RoomTargetError::Ambiguous);
        }
        return Ok(room);
    }
    Err(RoomTargetError::Unknown)
}

/// `room`'s aliases: its published ones, then the ones it was joined by.
/// A joined alias of a room no longer listed, or left, stays in the map
/// and names nothing: only listed, eligible rooms are asked.
pub(super) fn aliases<'r>(
    room: &'r RoomSummary,
    joined_aliases: &'r BTreeMap<String, String>,
) -> impl Iterator<Item = &'r str> {
    room.canonical_alias
        .iter()
        .chain(&room.alt_aliases)
        .map(String::as_str)
        .chain(
            joined_aliases
                .iter()
                .filter(|(_, room_id)| **room_id == room.room_id)
                .map(|(alias, _)| alias.as_str()),
        )
}

/// `ops` for `#ops:example.org`.
fn alias_localpart(alias: &str) -> &str {
    alias
        .strip_prefix('#')
        .and_then(|rest| rest.split_once(':'))
        .map(|(localpart, _)| localpart)
        .unwrap_or_else(|| panic!("a Matrix room alias is #localpart:server, got {alias}"))
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use mirx_core::{RoomMembership, RoomNotificationMode, RoomSummary};

    use super::{RoomTargetError, resolve_room};

    fn room(room_id: &str, display_name: &str, aliases: &[&str]) -> RoomSummary {
        RoomSummary {
            room_id: room_id.to_owned(),
            display_name: display_name.to_owned(),
            topic: None,
            membership: RoomMembership::Join,
            encrypted: false,
            is_space: false,
            is_direct: false,
            direct_user_ids: Vec::new(),
            tags: Vec::new(),
            space_children: Vec::new(),
            notification_mode: Some(RoomNotificationMode::AllMessages),
            canonical_alias: aliases.first().map(|alias| (*alias).to_owned()),
            alt_aliases: aliases
                .iter()
                .skip(1)
                .map(|alias| (*alias).to_owned())
                .collect(),
        }
    }

    fn not_space(room: &RoomSummary) -> bool {
        !room.is_space
    }

    fn resolved<'r>(rooms: &'r [RoomSummary], target: &str) -> Result<&'r str, RoomTargetError> {
        resolve_room(rooms, &BTreeMap::new(), not_space, target).map(|room| room.room_id.as_str())
    }

    fn rooms() -> Vec<RoomSummary> {
        let mut space = room("!space:example", "Space", &["#space:example"]);
        space.is_space = true;
        vec![
            room(
                "!ops:example",
                "Ops",
                &["#ops:example", "#ops:other", "#war-room:example"],
            ),
            room(
                "!lobby:example",
                "Lobby",
                &["#lobby:example", "#hall:example"],
            ),
            room("!dev-a:example", "Dev A", &["#dev:example"]),
            room("!dev-b:example", "Dev B", &["#dev:other"]),
            room("!bridged:example", "#rust", &[]),
            room("!named:example", "#lobby", &[]),
            space,
        ]
    }

    #[test]
    fn names_a_room_by_id_alias_short_alias_and_display_name() {
        let rooms = rooms();
        for (target, room_id) in [
            ("!ops:example", "!ops:example"),
            ("#ops:example", "!ops:example"),
            ("#war-room:example", "!ops:example"),
            ("#hall:example", "!lobby:example"),
            ("#hall", "!lobby:example"),
            // Two aliases of one room share the localpart: one room.
            ("#ops", "!ops:example"),
            ("#dev:other", "!dev-b:example"),
            ("Lobby", "!lobby:example"),
            ("#rust", "!bridged:example"),
        ] {
            assert_eq!(resolved(&rooms, target), Ok(room_id), "{target}");
        }
    }

    #[test]
    fn an_alias_wins_over_a_display_name() {
        assert_eq!(resolved(&rooms(), "#lobby"), Ok("!lobby:example"));
    }

    #[test]
    fn aliases_are_case_sensitive() {
        let rooms = rooms();
        assert_eq!(
            resolved(&rooms, "#OPS:example"),
            Err(RoomTargetError::Unknown)
        );
        assert_eq!(resolved(&rooms, "#OPS"), Err(RoomTargetError::Unknown));
    }

    #[test]
    fn a_short_alias_shared_by_two_rooms_is_ambiguous() {
        assert_eq!(resolved(&rooms(), "#dev"), Err(RoomTargetError::Ambiguous));
    }

    #[test]
    fn unknown_targets_and_ineligible_rooms_name_nothing() {
        let rooms = rooms();
        for target in [
            "#nope",
            "#nope:example",
            "!nope:example",
            "Nope",
            "#space",
            "Space",
        ] {
            assert_eq!(
                resolved(&rooms, target),
                Err(RoomTargetError::Unknown),
                "{target}"
            );
        }
    }

    #[test]
    fn a_room_answers_to_the_alias_it_was_joined_by() {
        let mut rooms = rooms();
        // Joined, not synced yet: no published alias.
        rooms.push(room("!new:example", "!new:example", &[]));
        let joined_aliases = BTreeMap::from(
            [
                ("#new:example", "!new:example"),
                // Joined, then gone from the list (left, forgotten).
                ("#gone:example", "!gone:example"),
                // Equals another room's published alias.
                ("#hall:example", "!new:example"),
            ]
            .map(|(alias, room_id)| (alias.to_owned(), room_id.to_owned())),
        );
        let resolved = |target| {
            resolve_room(&rooms, &joined_aliases, not_space, target)
                .map(|room| room.room_id.as_str())
        };
        assert_eq!(resolved("#new:example"), Ok("!new:example"));
        assert_eq!(resolved("#new"), Ok("!new:example"));
        assert_eq!(resolved("#gone:example"), Err(RoomTargetError::Unknown));
        assert_eq!(resolved("#gone"), Err(RoomTargetError::Unknown));
        // A joined alias counts as a published one: both rooms match.
        assert_eq!(resolved("#hall:example"), Err(RoomTargetError::Ambiguous));
        assert_eq!(resolved("#hall"), Err(RoomTargetError::Ambiguous));
    }

    #[test]
    fn a_display_name_shared_by_two_rooms_is_ambiguous() {
        let rooms = vec![
            room("!one:example", "Shared", &[]),
            room("!two:example", "Shared", &[]),
        ];
        assert_eq!(resolved(&rooms, "Shared"), Err(RoomTargetError::Ambiguous));
    }
}
