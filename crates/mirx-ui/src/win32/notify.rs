//! mIRC's notify list, its buddy list, over Matrix presence: the users to
//! watch for, kept in `mirx.ini` as mIRC keeps them in `mirc.ini`.
//!
//! Pure code: the list, and the lines `/notify` prints. `nick_of` gives a
//! user's nick where a known room has them as a member.

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

/// `/notify` without a nick: the list, each user with their note.
pub(super) fn list_lines(
    list: &NotifyList,
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
            format!("*   {}{note}", user_label(&entry.user_id, &nick_of))
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
        assert_eq!(list_lines(&list, nick_of), ["* Notify list is empty"]);
        list.add("@bob:example".to_owned(), Some("lunch buddy".to_owned()));
        list.add("@carol:example".to_owned(), None);
        assert_eq!(
            list_lines(&list, nick_of),
            [
                "* Notify list:",
                "*   Bob (@bob:example) - lunch buddy",
                "*   @carol:example",
            ]
        );
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
