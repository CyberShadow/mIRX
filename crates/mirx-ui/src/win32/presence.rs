//! Other users' presence, as the sync reports it: the nick list greys away
//! and offline members, as HexChat does.
//!
//! Pure code: the book of every user's latest presence the session heard.

use std::collections::BTreeMap;

use mirx_core::{PresenceChange, PresenceKind};

/// Each user's latest presence, from the core's `PresenceChanged` events.
/// A user the server shared no presence of has none.
#[derive(Debug, Default)]
pub(super) struct PresenceBook {
    users: BTreeMap<String, PresenceKind>,
    /// Moves whenever a user's presence changes: the nick lists repaint
    /// when it differs from the one they were painted at.
    revision: u64,
}

impl PresenceBook {
    pub(super) fn apply(&mut self, changes: Vec<PresenceChange>) {
        for change in changes {
            if self.users.insert(change.user_id, change.kind) != Some(change.kind) {
                self.revision += 1;
            }
        }
    }

    pub(super) fn revision(&self) -> u64 {
        self.revision
    }

    pub(super) fn kind(&self, user_id: &str) -> Option<PresenceKind> {
        self.users.get(user_id).copied()
    }

    /// At the session's end: its users' presence goes with it. The
    /// revision moves on, never back to one already painted.
    pub(super) fn clear(&mut self) {
        self.users.clear();
        self.revision += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn change(user_id: &str, kind: PresenceKind, status_msg: Option<&str>) -> PresenceChange {
        PresenceChange {
            user_id: user_id.to_owned(),
            kind,
            status_msg: status_msg.map(str::to_owned),
        }
    }

    #[test]
    fn the_latest_presence_of_each_user_counts() {
        let mut book = PresenceBook::default();
        assert_eq!(book.kind("@bob:example.org"), None);
        book.apply(vec![
            change("@bob:example.org", PresenceKind::Unavailable, Some("lunch")),
            change("@carol:example.org", PresenceKind::Offline, None),
        ]);
        book.apply(vec![change("@bob:example.org", PresenceKind::Online, None)]);
        assert_eq!(book.kind("@bob:example.org"), Some(PresenceKind::Online));
        assert_eq!(book.kind("@carol:example.org"), Some(PresenceKind::Offline));
        assert_eq!(book.kind("@dave:example.org"), None);

        book.clear();
        assert_eq!(book.kind("@carol:example.org"), None);
    }

    #[test]
    fn the_revision_moves_only_on_a_change() {
        let mut book = PresenceBook::default();
        let bob = || change("@bob:example.org", PresenceKind::Unavailable, Some("lunch"));
        book.apply(vec![bob()]);
        let revision = book.revision();
        assert_ne!(revision, PresenceBook::default().revision());
        // The same again (a sync repeating it, or the snapshot's copy of
        // the first sync's) is no change.
        book.apply(vec![bob()]);
        assert_eq!(book.revision(), revision);
        book.apply(vec![change("@bob:example.org", PresenceKind::Online, None)]);
        assert_ne!(book.revision(), revision);

        let revision = book.revision();
        book.clear();
        assert!(book.revision() > revision);
    }
}
