//! mIRC's notify list, its buddy list, over Matrix presence: the users to
//! watch for, kept in `mirx.ini` as mIRC keeps them in `mirc.ini`.
//!
//! Once connected, Status says who on the list the server says is on
//! (`* Bob is on Matrix`), then who comes and goes (`* Bob has left
//! Matrix`). A server shares a user's presence only with users who share
//! a room with them, and some servers share none: such users are never
//! announced.
//!
//! Pure code: the list, what was announced, and the lines. `nick_of` gives
//! a user's nick where a known room has them as a member.

use std::collections::BTreeSet;

use super::ignore::user_label;

/// One listed user: mIRC's notify entry, by Matrix user ID.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct NotifyEntry {
    pub(super) user_id: String,
    pub(super) note: Option<String>,
}

/// The notify list, in the order added. A user is listed once.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(super) struct NotifyList(Vec<NotifyEntry>);

/// What `add` did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Added {
    New,
    /// The user was listed: their note is replaced.
    Updated,
}

impl NotifyList {
    /// Lists `user_id` with `note`, or replaces their note.
    pub(super) fn add(&mut self, user_id: String, note: Option<String>) -> Added {
        match self.0.iter_mut().find(|entry| entry.user_id == user_id) {
            Some(entry) => {
                entry.note = note;
                Added::Updated
            }
            None => {
                self.0.push(NotifyEntry { user_id, note });
                Added::New
            }
        }
    }

    /// Whether `user_id` was listed.
    pub(super) fn remove(&mut self, user_id: &str) -> bool {
        let before = self.0.len();
        self.0.retain(|entry| entry.user_id != user_id);
        self.0.len() != before
    }

    pub(super) fn contains(&self, user_id: &str) -> bool {
        self.0.iter().any(|entry| entry.user_id == user_id)
    }

    pub(super) fn entries(&self) -> &[NotifyEntry] {
        &self.0
    }
}

/// Which listed users Status said are on, since the connection came up.
/// Not connected, it says nothing: the presence the store kept from the
/// last run, or heard before a disconnect, is not news.
#[derive(Debug, Default)]
pub(super) struct NotifyWatch {
    on: Option<BTreeSet<String>>,
}

impl NotifyWatch {
    /// The connection is up: the next `update` is the roll call.
    pub(super) fn connected(&mut self) {
        self.on = Some(BTreeSet::new());
    }

    pub(super) fn disconnected(&mut self) {
        self.on = None;
    }

    pub(super) fn is_connected(&self) -> bool {
        self.on.is_some()
    }

    /// The listed users who came (`true`) or went (`false`) since the last
    /// update, in list order, by `is_on`; records them. It compares states,
    /// not events, so presence repeated by the store and the sync, or
    /// heard in any order with list changes, never says a thing twice.
    /// Users no longer listed are forgotten without a word.
    pub(super) fn update(
        &mut self,
        list: &NotifyList,
        is_on: impl Fn(&str) -> bool,
    ) -> Vec<(String, bool)> {
        let Some(announced) = &mut self.on else {
            return Vec::new();
        };
        announced.retain(|user_id| list.contains(user_id));
        list.entries()
            .iter()
            .filter_map(|entry| {
                let on = is_on(&entry.user_id);
                let changed = if on {
                    announced.insert(entry.user_id.clone())
                } else {
                    announced.remove(&entry.user_id)
                };
                changed.then(|| (entry.user_id.clone(), on))
            })
            .collect()
    }
}

/// The Status line's text for a user who came or went, as mIRC's with
/// Matrix for IRC.
pub(super) fn change_text(name: &str, on: bool) -> String {
    if on {
        format!("{name} is on Matrix")
    } else {
        format!("{name} has left Matrix")
    }
}

/// `/notify` without a nick: the list, each user with whether they are on
/// where that is known (`is_on`: `None` while not connected, and for a user
/// whose presence the server does not share), and their note.
pub(super) fn list_lines(
    list: &NotifyList,
    is_on: impl Fn(&str) -> Option<bool>,
    nick_of: impl Fn(&str) -> Option<String>,
) -> Vec<String> {
    if list.entries().is_empty() {
        return vec!["* Notify list is empty".to_owned()];
    }
    std::iter::once("* Notify list:".to_owned())
        .chain(list.entries().iter().map(|entry| {
            let note = entry
                .note
                .as_ref()
                .map_or(String::new(), |note| format!(" - {note}"));
            let presence = match is_on(&entry.user_id) {
                Some(true) => " is on Matrix",
                Some(false) => " is not on Matrix",
                None => "",
            };
            format!(
                "*   {}{presence}{note}",
                user_label(&entry.user_id, &nick_of)
            )
        }))
        .collect()
}

/// What `/notify <nick>` says it did.
pub(super) fn added_line(
    added: Added,
    user_id: &str,
    nick_of: impl Fn(&str) -> Option<String>,
) -> String {
    let label = user_label(user_id, &nick_of);
    match added {
        Added::New => format!("* Added {label} to notify list"),
        Added::Updated => format!("* Updated {label} in notify list"),
    }
}

pub(super) fn removed_line(user_id: &str, nick_of: impl Fn(&str) -> Option<String>) -> String {
    format!(
        "* Removed {} from notify list",
        user_label(user_id, &nick_of)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn nick_of(user_id: &str) -> Option<String> {
        (user_id == "@bob:example").then(|| "Bob".to_owned())
    }

    #[test]
    fn a_user_is_listed_once_in_the_order_added() {
        let mut list = NotifyList::default();
        assert_eq!(
            list.add("@bob:example".to_owned(), Some("lunch".to_owned())),
            Added::New
        );
        assert_eq!(list.add("@carol:example".to_owned(), None), Added::New);
        assert_eq!(list.add("@bob:example".to_owned(), None), Added::Updated);
        assert_eq!(
            list.entries(),
            [
                NotifyEntry {
                    user_id: "@bob:example".to_owned(),
                    note: None,
                },
                NotifyEntry {
                    user_id: "@carol:example".to_owned(),
                    note: None,
                },
            ]
        );
        assert!(list.contains("@carol:example"));
        assert!(list.remove("@bob:example"));
        assert!(!list.remove("@bob:example"));
        assert!(!list.contains("@bob:example"));
        assert_eq!(list.entries().len(), 1);
    }

    #[test]
    fn the_listing_names_each_user_with_their_note() {
        let mut list = NotifyList::default();
        let unknown = |_: &str| None;
        assert_eq!(
            list_lines(&list, unknown, nick_of),
            ["* Notify list is empty"]
        );
        list.add("@bob:example".to_owned(), Some("lunch buddy".to_owned()));
        list.add("@carol:example".to_owned(), None);
        list.add("@dave:example".to_owned(), Some("work".to_owned()));
        assert_eq!(
            list_lines(&list, unknown, nick_of),
            [
                "* Notify list:",
                "*   Bob (@bob:example) - lunch buddy",
                "*   @carol:example",
                "*   @dave:example - work",
            ]
        );
        let is_on = |user_id: &str| match user_id {
            "@bob:example" => Some(true),
            "@carol:example" => Some(false),
            _ => None,
        };
        assert_eq!(
            list_lines(&list, is_on, nick_of),
            [
                "* Notify list:",
                "*   Bob (@bob:example) is on Matrix - lunch buddy",
                "*   @carol:example is not on Matrix",
                "*   @dave:example - work",
            ]
        );
    }

    fn listed(user_ids: &[&str]) -> NotifyList {
        let mut list = NotifyList::default();
        for user_id in user_ids {
            list.add((*user_id).to_owned(), None);
        }
        list
    }

    fn on_set<'a>(on: &'a [&'a str]) -> impl Fn(&str) -> bool + 'a {
        |user_id| on.contains(&user_id)
    }

    fn change(user_id: &str, on: bool) -> (String, bool) {
        (user_id.to_owned(), on)
    }

    #[test]
    fn the_watch_says_nothing_until_connected() {
        let list = listed(&["@bob:example"]);
        let mut watch = NotifyWatch::default();
        assert!(!watch.is_connected());
        assert_eq!(watch.update(&list, on_set(&["@bob:example"])), []);
        watch.connected();
        assert!(watch.is_connected());
        assert_eq!(
            watch.update(&list, on_set(&["@bob:example"])),
            [change("@bob:example", true)]
        );
        watch.disconnected();
        assert_eq!(watch.update(&list, on_set(&[])), []);
    }

    #[test]
    fn the_roll_call_names_who_is_on_in_list_order() {
        let list = listed(&[
            "@carol:example",
            "@bob:example",
            "@dave:example",
            "@erin:example",
        ]);
        let mut watch = NotifyWatch::default();
        watch.connected();
        // Dave is offline, Erin unknown.
        assert_eq!(
            watch.update(&list, on_set(&["@bob:example", "@carol:example"])),
            [change("@carol:example", true), change("@bob:example", true)]
        );
        // Repeats say nothing.
        assert_eq!(
            watch.update(&list, on_set(&["@bob:example", "@carol:example"])),
            []
        );
        // A reconnect calls the roll again.
        watch.disconnected();
        watch.connected();
        assert_eq!(
            watch.update(&list, on_set(&["@bob:example"])),
            [change("@bob:example", true)]
        );
    }

    #[test]
    fn the_watch_says_who_comes_and_goes() {
        let mut list = listed(&["@bob:example", "@carol:example"]);
        let mut watch = NotifyWatch::default();
        watch.connected();
        assert_eq!(
            watch.update(&list, on_set(&["@bob:example"])),
            [change("@bob:example", true)]
        );
        assert_eq!(
            watch.update(&list, on_set(&["@carol:example"])),
            [
                change("@bob:example", false),
                change("@carol:example", true)
            ]
        );
        // Carol is removed while on, and goes: no line; listed again,
        // she is news again.
        list.remove("@carol:example");
        assert_eq!(watch.update(&list, on_set(&[])), []);
        list.add("@carol:example".to_owned(), None);
        assert_eq!(
            watch.update(&list, on_set(&["@carol:example"])),
            [change("@carol:example", true)]
        );
    }

    #[test]
    fn change_texts_are_mircs_with_matrix() {
        assert_eq!(change_text("Bob", true), "Bob is on Matrix");
        assert_eq!(change_text("Bob", false), "Bob has left Matrix");
    }

    #[test]
    fn changes_say_what_they_did() {
        assert_eq!(
            added_line(Added::New, "@bob:example", nick_of),
            "* Added Bob (@bob:example) to notify list"
        );
        assert_eq!(
            added_line(Added::Updated, "@carol:example", nick_of),
            "* Updated @carol:example in notify list"
        );
        assert_eq!(
            removed_line("@bob:example", nick_of),
            "* Removed Bob (@bob:example) from notify list"
        );
    }
}
