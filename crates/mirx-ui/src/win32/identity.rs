//! Warnings when another user's cryptographic identity changes, from the
//! core's room identity streams. As mIRC prints a nick change or a quit in
//! every channel shared with the user, the warning prints, in red, in every
//! encrypted room window shared with them; it stays in the room's model
//! until the stream reports the user pinned or verified again.

use std::collections::BTreeMap;

use mirx_core::{IdentityChange, IdentityTrust, RoomIdentityChanges};

use super::{StatusLineKind, UiModel, ViewActivityLevel, ViewId, WindowState, raise_view_activity};

/// A room member whose identity changed since this session accepted or
/// verified it. Ordered by gravity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(super) enum IdentityWarning {
    /// Changed since it was pinned (first seen, or accepted).
    Changed,
    /// Changed since you verified it.
    VerifiedChanged,
}

/// The warning a trust state calls for; none for a pinned or verified one.
fn identity_warning(trust: IdentityTrust) -> Option<IdentityWarning> {
    match trust {
        IdentityTrust::Verified | IdentityTrust::Pinned => None,
        IdentityTrust::PinViolation => Some(IdentityWarning::Changed),
        IdentityTrust::VerificationViolation => Some(IdentityWarning::VerifiedChanged),
    }
}

/// The commands name the user by ID: a nick may have spaces.
pub(super) fn warning_line(warning: IdentityWarning, nick: &str, user_id: &str) -> String {
    match warning {
        IdentityWarning::Changed => format!(
            "* Warning: {nick}'s identity was reset; /identity accept {user_id} to accept it, \
             or /verify {user_id}"
        ),
        IdentityWarning::VerifiedChanged => format!(
            "* Warning: you verified {nick}, and their identity was reset; /verify {user_id} \
             to verify it again, or /identity accept {user_id} to withdraw your verification"
        ),
    }
}

pub(super) fn accepted_line(nick: &str) -> String {
    format!("* {nick}'s new identity accepted")
}

/// Applies one room's changes to its warnings: a new or graver warning
/// prints its line, a pinned or verified user loses theirs silently.
/// Returns the room's new warnings and the lines to print.
fn changed_warnings(
    warnings: &BTreeMap<String, IdentityWarning>,
    changes: &[IdentityChange],
    nick_of: impl Fn(&str) -> String,
) -> (BTreeMap<String, IdentityWarning>, Vec<String>) {
    let mut warnings = warnings.clone();
    let mut lines = Vec::new();
    for change in changes {
        match identity_warning(change.trust) {
            Some(warning) => {
                if warnings.insert(change.user_id.clone(), warning) != Some(warning) {
                    lines.push(warning_line(
                        warning,
                        &nick_of(&change.user_id),
                        &change.user_id,
                    ));
                }
            }
            None => {
                warnings.remove(&change.user_id);
            }
        }
    }
    (warnings, lines)
}

/// `Event::RoomIdentityChanges`. The room is joined, but its window may be
/// a closed query: its model keeps the lines for when it opens again. A
/// warning in a background window colours its switchbar button, as a state
/// notice does.
pub(super) fn apply_room_identity_changes(state: &mut WindowState, changes: RoomIdentityChanges) {
    let RoomIdentityChanges { room_id, changes } = changes;
    let room_view = state.model.room_views.get(&room_id);
    let empty = BTreeMap::new();
    let current = room_view.map_or(&empty, |view| &view.identity_warnings);
    let (warnings, lines) = changed_warnings(current, &changes, |user_id| {
        room_view.map_or_else(|| user_id.to_owned(), |view| view.member_nick(user_id))
    });
    // Mutable access rebuilds the window: take it only for a change.
    if warnings == *current {
        return;
    }
    let now = state.clock.now_ms();
    let room_view = state.model.room_views.entry_or_default(room_id.clone());
    room_view.identity_warnings = warnings;
    for line in &lines {
        room_view.push_echo(now, None, StatusLineKind::Error, line.clone());
    }
    let view_id = ViewId::Room(room_id);
    if !lines.is_empty() && state.active_view != view_id {
        raise_view_activity(state, view_id, ViewActivityLevel::Event);
    }
}

/// The gravest warning any room holds for the user.
pub(super) fn user_identity_warning(model: &UiModel, user_id: &str) -> Option<IdentityWarning> {
    model
        .room_views
        .values()
        .filter_map(|view| view.identity_warnings.get(user_id).copied())
        .max()
}

#[cfg(test)]
mod tests {
    use super::super::test_fixtures::*;
    use super::*;

    fn changes(room_id: &str, user_id: &str, trust: IdentityTrust) -> RoomIdentityChanges {
        RoomIdentityChanges {
            room_id: room_id.to_owned(),
            changes: vec![IdentityChange {
                user_id: user_id.to_owned(),
                trust,
            }],
        }
    }

    fn echo_lines(state: &WindowState, room_id: &str) -> Vec<(StatusLineKind, String)> {
        state
            .model
            .room_views
            .get(room_id)
            .map(|view| {
                view.echo
                    .lines_for(None)
                    .map(|line| (line.kind, line.text.clone()))
                    .collect()
            })
            .unwrap_or_default()
    }

    fn state_with_bob() -> WindowState {
        let mut state = test_state();
        state.model.rooms = vec![
            room("!ops:example", "Ops", true),
            room("!eng:example", "Eng", true),
        ];
        let ops = state
            .model
            .room_views
            .entry_or_default("!ops:example".to_owned());
        ops.members = vec![member_summary("@bob:example", Some("Bob"), false)];
        state
    }

    fn warnings(state: &WindowState, room_id: &str) -> Vec<(String, IdentityWarning)> {
        state
            .model
            .room_views
            .get(room_id)
            .map(|view| {
                view.identity_warnings
                    .iter()
                    .map(|(user_id, warning)| (user_id.clone(), *warning))
                    .collect()
            })
            .unwrap_or_default()
    }

    #[test]
    fn a_changed_identity_warns_once_in_red_and_clears_silently() {
        let mut state = state_with_bob();
        let changed = changes("!ops:example", "@bob:example", IdentityTrust::PinViolation);
        apply_room_identity_changes(&mut state, changed.clone());
        let line = (
            StatusLineKind::Error,
            "* Warning: Bob's identity was reset; /identity accept @bob:example to accept \
             it, or /verify @bob:example"
                .to_owned(),
        );
        assert_eq!(echo_lines(&state, "!ops:example"), vec![line.clone()]);
        assert_eq!(
            warnings(&state, "!ops:example"),
            vec![("@bob:example".to_owned(), IdentityWarning::Changed)]
        );

        // The same state again (the next launch's first item) is no news.
        let revision = state.model.room_views.revision("!ops:example");
        apply_room_identity_changes(&mut state, changed);
        assert_eq!(echo_lines(&state, "!ops:example"), vec![line.clone()]);
        assert_eq!(state.model.room_views.revision("!ops:example"), revision);

        apply_room_identity_changes(
            &mut state,
            changes("!ops:example", "@bob:example", IdentityTrust::Pinned),
        );
        assert_eq!(warnings(&state, "!ops:example"), Vec::new());
        assert_eq!(echo_lines(&state, "!ops:example"), vec![line]);
    }

    #[test]
    fn a_verified_identity_that_changed_names_the_verification() {
        let mut state = state_with_bob();
        apply_room_identity_changes(
            &mut state,
            changes("!ops:example", "@bob:example", IdentityTrust::PinViolation),
        );
        apply_room_identity_changes(
            &mut state,
            changes(
                "!ops:example",
                "@bob:example",
                IdentityTrust::VerificationViolation,
            ),
        );
        assert_eq!(
            echo_lines(&state, "!ops:example")[1],
            (
                StatusLineKind::Error,
                "* Warning: you verified Bob, and their identity was reset; /verify \
                 @bob:example to verify it again, or /identity accept @bob:example to \
                 withdraw your verification"
                    .to_owned()
            )
        );
        assert_eq!(
            warnings(&state, "!ops:example"),
            vec![("@bob:example".to_owned(), IdentityWarning::VerifiedChanged)]
        );
        // Verified again: the warning goes, and nothing prints.
        apply_room_identity_changes(
            &mut state,
            changes("!ops:example", "@bob:example", IdentityTrust::Verified),
        );
        assert_eq!(warnings(&state, "!ops:example"), Vec::new());
        assert_eq!(echo_lines(&state, "!ops:example").len(), 2);
    }

    #[test]
    fn a_warning_raises_a_background_window_to_event_activity() {
        let mut state = state_with_bob();
        state.active_view = ViewId::Room("!ops:example".to_owned());
        for room_id in ["!ops:example", "!eng:example"] {
            apply_room_identity_changes(
                &mut state,
                changes(room_id, "@bob:example", IdentityTrust::PinViolation),
            );
        }
        assert_eq!(
            state
                .room_activity
                .get(&ViewId::Room("!eng:example".to_owned())),
            Some(&ViewActivityLevel::Event)
        );
        assert_eq!(
            state
                .room_activity
                .get(&ViewId::Room("!ops:example".to_owned())),
            None,
            "the active window shows its line"
        );
        // A room without members known names the user by ID.
        assert_eq!(
            echo_lines(&state, "!eng:example")[0].1,
            "* Warning: @bob:example's identity was reset; /identity accept @bob:example to \
             accept it, or /verify @bob:example"
        );
    }

    #[test]
    fn a_pinned_user_without_a_warning_changes_nothing() {
        let mut state = state_with_bob();
        let revision = state.model.room_views.revision("!ops:example");
        apply_room_identity_changes(
            &mut state,
            changes("!ops:example", "@bob:example", IdentityTrust::Pinned),
        );
        assert_eq!(state.model.room_views.revision("!ops:example"), revision);
        assert_eq!(state.model.room_views.revision("!eng:example"), None);
    }

    #[test]
    fn the_gravest_warning_of_any_room_counts() {
        let mut state = state_with_bob();
        apply_room_identity_changes(
            &mut state,
            changes("!ops:example", "@bob:example", IdentityTrust::PinViolation),
        );
        assert_eq!(
            user_identity_warning(&state.model, "@bob:example"),
            Some(IdentityWarning::Changed)
        );
        apply_room_identity_changes(
            &mut state,
            changes(
                "!eng:example",
                "@bob:example",
                IdentityTrust::VerificationViolation,
            ),
        );
        assert_eq!(
            user_identity_warning(&state.model, "@bob:example"),
            Some(IdentityWarning::VerifiedChanged)
        );
        assert_eq!(user_identity_warning(&state.model, "@carol:example"), None);
    }
}
