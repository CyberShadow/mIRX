//! Other users' presence, as the sync reports it: the nick list greys away
//! and offline members, as HexChat does, and a message to an away user's
//! query says they are away, as IRC's `RPL_AWAY` does.
//!
//! Pure code: the book of every user's latest presence the session heard.

use std::collections::{BTreeMap, BTreeSet};

use mirx_core::{PresenceChange, PresenceKind};

use super::commands::away_line;

#[derive(Debug, Clone, PartialEq, Eq)]
struct Presence {
    kind: PresenceKind,
    status_msg: Option<String>,
}

/// Each user's latest presence, from the core's `PresenceChanged` events.
/// A user the server shared no presence of has none.
#[derive(Debug, Default)]
pub(super) struct PresenceBook {
    users: BTreeMap<String, Presence>,
    /// Moves whenever a user's presence changes: the nick lists repaint
    /// when it differs from the one they were painted at.
    revision: u64,
    /// The `(query room, user)` pairs that printed the user's current away
    /// status: a change of the user's presence takes theirs out.
    announced: BTreeSet<(String, String)>,
}

impl PresenceBook {
    pub(super) fn apply(&mut self, changes: Vec<PresenceChange>) {
        for change in changes {
            let presence = Presence {
                kind: change.kind,
                status_msg: change.status_msg,
            };
            if self.users.get(&change.user_id) == Some(&presence) {
                continue;
            }
            self.revision += 1;
            self.announced
                .retain(|(_, user_id)| *user_id != change.user_id);
            self.users.insert(change.user_id, presence);
        }
    }

    pub(super) fn revision(&self) -> u64 {
        self.revision
    }

    pub(super) fn kind(&self, user_id: &str) -> Option<PresenceKind> {
        self.users.get(user_id).map(|presence| presence.kind)
    }

    /// The away line for a message sent to `room_id`, the query with
    /// `user_id`, who goes by `nick`; `None` unless they are away, and once
    /// per query for each away status. mIRC prints the server's `RPL_AWAY`
    /// at every message; Element marks users away after a few idle
    /// minutes, so that would fill a normal conversation.
    pub(super) fn away_notice(
        &mut self,
        room_id: &str,
        user_id: &str,
        nick: &str,
    ) -> Option<String> {
        let presence = self
            .users
            .get(user_id)
            .filter(|presence| presence.kind == PresenceKind::Unavailable)?;
        self.announced
            .insert((room_id.to_owned(), user_id.to_owned()))
            .then(|| away_line(nick, presence.status_msg.as_deref()))
    }

    /// At the session's end: its users' presence goes with it. The
    /// revision moves on, never back to one already painted.
    pub(super) fn clear(&mut self) {
        self.users.clear();
        self.announced.clear();
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

    #[test]
    fn an_away_user_is_announced_once_per_query_and_away_status() {
        const QUERY: &str = "!bob:example.org";
        const BOB: &str = "@bob:example.org";
        let mut book = PresenceBook::default();
        let notice = |book: &mut PresenceBook| book.away_notice(QUERY, BOB, "Bob");
        // Nothing known.
        assert_eq!(notice(&mut book), None);

        book.apply(vec![change(BOB, PresenceKind::Unavailable, Some("lunch"))]);
        assert_eq!(notice(&mut book).as_deref(), Some("* Bob is away: lunch"));
        assert_eq!(notice(&mut book), None);
        // Another query with him has its own.
        assert_eq!(
            book.away_notice("!other:example.org", BOB, "Bob")
                .as_deref(),
            Some("* Bob is away: lunch")
        );
        // The same status again is no news.
        book.apply(vec![change(BOB, PresenceKind::Unavailable, Some("lunch"))]);
        assert_eq!(notice(&mut book), None);
        // Another user's change keeps his.
        book.apply(vec![change(
            "@carol:example.org",
            PresenceKind::Offline,
            None,
        )]);
        assert_eq!(notice(&mut book), None);

        // A new status.
        book.apply(vec![change(
            BOB,
            PresenceKind::Unavailable,
            Some("meeting"),
        )]);
        assert_eq!(notice(&mut book).as_deref(), Some("* Bob is away: meeting"));
        // Back, then away again.
        book.apply(vec![change(BOB, PresenceKind::Online, None)]);
        assert_eq!(notice(&mut book), None);
        book.apply(vec![change(BOB, PresenceKind::Unavailable, None)]);
        assert_eq!(notice(&mut book).as_deref(), Some("* Bob is away"));

        // Offline is not away: Matrix delivers to offline users.
        book.apply(vec![change(BOB, PresenceKind::Offline, None)]);
        assert_eq!(notice(&mut book), None);

        book.apply(vec![change(BOB, PresenceKind::Unavailable, None)]);
        book.clear();
        assert_eq!(notice(&mut book), None);
    }
}
