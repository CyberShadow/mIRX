//! mIRC's Address Book (Tools > Address Book..., Alt+B, `/abook`), at the
//! two tabs Matrix has a counterpart for: Notify, the notify list and its
//! display option, and Control, the account's ignore list (Matrix rooms
//! have no auto-op, auto-voice or protect lists).
//!
//! Pure code: the form the dialog edits, its rows, and the checks its Add
//! and Edit entries pass. `address_book_dialog` shows it.

use mirx_core::RoomMemberSummary;

use super::commands::resolve_user;
use super::ignore::{check_ignore, user_label, user_name};
use super::notify::{NotifyEntry, NotifySetting};

/// The dialog's tabs, in order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum AddressBookTab {
    Notify,
    Control,
}

impl AddressBookTab {
    pub(super) const ALL: [Self; 2] = [Self::Notify, Self::Control];

    pub(super) fn label(self) -> &'static str {
        match self {
            Self::Notify => "Notify",
            Self::Control => "Control",
        }
    }

    pub(super) fn index(self) -> usize {
        Self::ALL
            .iter()
            .position(|tab| *tab == self)
            .expect("every tab is in ALL")
    }
}

/// Who the dialog can name, as the known rooms had them when it opened: a
/// nick of a member, or any user ID, as `/notify` and `/ignore` take them.
#[derive(Debug, Clone, Default)]
pub(super) struct People {
    members: Vec<RoomMemberSummary>,
    own_user_id: Option<String>,
}

impl People {
    pub(super) fn new(members: Vec<RoomMemberSummary>, own_user_id: Option<String>) -> Self {
        Self {
            members,
            own_user_id,
        }
    }

    /// The user `text` names.
    pub(super) fn resolve(&self, text: &str) -> Result<String, String> {
        Ok(resolve_user(&self.members, text.trim())?.user_id)
    }

    pub(super) fn nick_of(&self, user_id: &str) -> Option<String> {
        self.members
            .iter()
            .find(|member| member.user_id == user_id)
            .map(|member| member.nick.clone())
    }
}

/// The Control tab's ignore list: as the server listed it when the dialog
/// opened, and as edited. OK sends the difference.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct IgnoreEdit {
    opened: Vec<String>,
    edited: Vec<String>,
}

/// What OK asks the server to change, adds first.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(super) struct IgnoreChanges {
    pub(super) ignore: Vec<String>,
    pub(super) unignore: Vec<String>,
}

impl IgnoreEdit {
    pub(super) fn new(opened: Vec<String>) -> Self {
        Self {
            edited: opened.clone(),
            opened,
        }
    }

    pub(super) fn users(&self) -> &[String] {
        &self.edited
    }

    /// The user an Add entry names, refused as `/ignore` refuses them
    /// against the list as edited.
    pub(super) fn check_add(&self, people: &People, text: &str) -> Result<String, String> {
        let user_id = people.resolve(text)?;
        check_ignore(
            Some(&self.edited),
            people.own_user_id.as_deref(),
            &user_id,
            |user_id| people.nick_of(user_id),
        )?;
        Ok(user_id)
    }

    /// Adds a user `check_add` accepted.
    pub(super) fn add(&mut self, user_id: String) {
        assert!(
            !self.edited.contains(&user_id),
            "{user_id} is already on the edited ignore list"
        );
        self.edited.push(user_id);
    }

    pub(super) fn remove(&mut self, index: usize) {
        self.edited.remove(index);
    }

    pub(super) fn changes(&self) -> IgnoreChanges {
        let missing_from = |list: &[String], other: &[String]| -> Vec<String> {
            list.iter()
                .filter(|user_id| !other.contains(user_id))
                .cloned()
                .collect()
        };
        IgnoreChanges {
            ignore: missing_from(&self.edited, &self.opened),
            unignore: missing_from(&self.opened, &self.edited),
        }
    }
}

/// What the dialog edits; nothing applies until OK.
#[derive(Debug, Clone)]
pub(super) struct AddressBookForm {
    pub(super) notify: NotifySetting,
    /// `None` while the server's list is not known: signed out, or before
    /// the first sync. The Control tab is greyed then.
    pub(super) ignore: Option<IgnoreEdit>,
    pub(super) people: People,
}

/// An entry dialog's fields, as typed.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(super) struct EntryFields {
    pub(super) nickname: String,
    pub(super) note: String,
    pub(super) whois: bool,
}

impl AddressBookForm {
    /// The Notify tab's rows: the user, `+` first when the entry performs
    /// `/whois`, and the note.
    pub(super) fn notify_rows(&self) -> Vec<[String; 2]> {
        self.notify
            .list
            .entries()
            .iter()
            .map(|entry| {
                [
                    entry.label(|user_id| self.people.nick_of(user_id)),
                    entry.note.clone().unwrap_or_default(),
                ]
            })
            .collect()
    }

    /// The Control tab's rows; none while the list is not known.
    pub(super) fn ignore_rows(&self) -> Vec<String> {
        self.ignore
            .iter()
            .flat_map(IgnoreEdit::users)
            .map(|user_id| user_label(user_id, &|user_id: &str| self.people.nick_of(user_id)))
            .collect()
    }

    /// The entry Add Notify's fields make, refused for a user who cannot
    /// be named or is listed already.
    pub(super) fn check_new_notify(&self, fields: &EntryFields) -> Result<NotifyEntry, String> {
        let user_id = self.people.resolve(&fields.nickname)?;
        if self.notify.list.contains(&user_id) {
            return Err(format!(
                "{} is already on the notify list",
                user_name(&user_id, &|user_id: &str| self.people.nick_of(user_id))
            ));
        }
        Ok(notify_entry(user_id, fields))
    }

    /// Adds an entry, or replaces a listed user's note and whois (Edit).
    pub(super) fn put_notify(&mut self, entry: NotifyEntry) {
        self.notify.list.add(entry.user_id, entry.note, entry.whois);
    }

    pub(super) fn remove_notify(&mut self, index: usize) {
        let user_id = self.notify.list.entries()[index].user_id.clone();
        assert!(self.notify.list.remove(&user_id));
    }

    /// The fields Edit Notify opens with for the entry at `index`: the
    /// user ID, which it does not change.
    pub(super) fn notify_fields(&self, index: usize) -> EntryFields {
        let entry = &self.notify.list.entries()[index];
        EntryFields {
            nickname: entry.user_id.clone(),
            note: entry.note.clone().unwrap_or_default(),
            whois: entry.whois,
        }
    }
}

/// The entry for `user_id` with the fields' note and whois.
pub(super) fn notify_entry(user_id: String, fields: &EntryFields) -> NotifyEntry {
    let note = fields.note.trim();
    NotifyEntry {
        user_id,
        note: (!note.is_empty()).then(|| note.to_owned()),
        whois: fields.whois,
    }
}

#[cfg(test)]
mod tests {
    use mirx_core::RoomMemberRank;

    use super::super::notify::NotifyList;
    use super::*;

    fn listed(list: &NotifyList) -> Vec<(&str, Option<&str>, bool)> {
        list.entries()
            .iter()
            .map(|entry| (entry.user_id.as_str(), entry.note.as_deref(), entry.whois))
            .collect()
    }

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

    fn people() -> People {
        People::new(
            vec![
                member("@alice:example", "Alice"),
                member("@bob:example", "Bob"),
                member("@bob:other", "Bob"),
                member("@carol:example", "Carol"),
            ],
            Some("@alice:example".to_owned()),
        )
    }

    fn form(ignore: Option<&[&str]>) -> AddressBookForm {
        let mut notify = NotifySetting::default();
        notify
            .list
            .add("@carol:example".to_owned(), Some("lunch".to_owned()), true);
        notify.list.add("@dave:remote".to_owned(), None, false);
        AddressBookForm {
            notify,
            ignore: ignore
                .map(|users| IgnoreEdit::new(users.iter().map(|u| (*u).to_owned()).collect())),
            people: people(),
        }
    }

    fn fields(nickname: &str, note: &str, whois: bool) -> EntryFields {
        EntryFields {
            nickname: nickname.to_owned(),
            note: note.to_owned(),
            whois,
        }
    }

    #[test]
    fn tabs_are_mircs_two_in_order() {
        assert_eq!(
            AddressBookTab::ALL.map(AddressBookTab::label),
            ["Notify", "Control"]
        );
        assert_eq!(AddressBookTab::Control.index(), 1);
    }

    #[test]
    fn rows_name_each_user_with_whois_and_note() {
        let with_list = form(Some(&["@carol:example", "@spam:remote"]));
        assert_eq!(
            with_list.notify_rows(),
            [
                ["+Carol (@carol:example)".to_owned(), "lunch".to_owned()],
                ["@dave:remote".to_owned(), String::new()],
            ]
        );
        assert_eq!(
            with_list.ignore_rows(),
            ["Carol (@carol:example)", "@spam:remote"]
        );
        assert_eq!(form(None).ignore_rows(), Vec::<String>::new());
    }

    #[test]
    fn notify_entries_are_checked_added_edited_and_removed() {
        let mut form = form(None);
        assert_eq!(
            form.check_new_notify(&fields("erin", "", false)),
            Err("no such nick: erin".to_owned())
        );
        assert_eq!(
            form.check_new_notify(&fields("bob", "", false)),
            Err("bob is ambiguous (@bob:example, @bob:other)".to_owned())
        );
        assert_eq!(
            form.check_new_notify(&fields(" carol ", "", false)),
            Err("Carol is already on the notify list".to_owned())
        );
        // A stranger's ID; the note is trimmed.
        let entry = form
            .check_new_notify(&fields("@erin:remote", "  work ", true))
            .expect("an ID");
        form.put_notify(entry);
        assert_eq!(
            form.notify_fields(0),
            fields("@carol:example", "lunch", true)
        );
        let edited = notify_entry("@carol:example".to_owned(), &fields("", "", false));
        form.put_notify(edited);
        form.remove_notify(1);
        assert_eq!(
            listed(&form.notify.list),
            [
                ("@carol:example", None, false),
                ("@erin:remote", Some("work"), true),
            ]
        );
    }

    #[test]
    fn ignore_adds_are_refused_as_ignore_refuses_them() {
        let mut edit = IgnoreEdit::new(vec!["@spam:remote".to_owned()]);
        let people = people();
        assert_eq!(
            edit.check_add(&people, "alice"),
            Err("you cannot ignore yourself".to_owned())
        );
        assert_eq!(
            edit.check_add(&people, "@spam:remote"),
            Err("@spam:remote is already ignored".to_owned())
        );
        assert_eq!(
            edit.check_add(&people, "dave"),
            Err("no such nick: dave".to_owned())
        );
        let carol = edit.check_add(&people, "Carol").expect("Carol");
        edit.add(carol);
        // Against the list as edited.
        assert_eq!(
            edit.check_add(&people, "carol"),
            Err("Carol is already ignored".to_owned())
        );
        assert_eq!(edit.users(), ["@spam:remote", "@carol:example"]);
    }

    #[test]
    fn ignore_changes_are_the_difference_adds_first() {
        let mut edit = IgnoreEdit::new(vec!["@spam:remote".to_owned(), "@bob:example".to_owned()]);
        assert_eq!(edit.changes(), IgnoreChanges::default());
        edit.add("@carol:example".to_owned());
        edit.remove(0);
        assert_eq!(
            edit.changes(),
            IgnoreChanges {
                ignore: vec!["@carol:example".to_owned()],
                unignore: vec!["@spam:remote".to_owned()],
            }
        );
        // Added then deleted, deleted then added: nothing.
        edit.remove(1);
        edit.add("@spam:remote".to_owned());
        assert_eq!(
            edit.changes(),
            IgnoreChanges {
                ignore: Vec::new(),
                unignore: Vec::new(),
            }
        );
    }
}
