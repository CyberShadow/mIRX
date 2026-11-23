//! mIRC's Address Book (Tools > Address Book..., Alt+B, `/abook`), at the
//! three tabs Matrix has a counterpart for: Notify, the notify list and its
//! display option; Control, the account's ignore list (Matrix rooms have
//! no auto-op, auto-voice or protect lists); and Colors, the nick colour
//! list `/cnick` edits.
//!
//! Pure code: the form the dialog edits, its rows, and the checks its Add
//! and Edit entries pass. `address_book_dialog` shows it.

use mirx_core::RoomMemberSummary;

use super::commands::resolve_user;
use super::ignore::{check_ignore, user_label, user_name};
use super::nick_colors::{
    ModeCondition, NickColor, NickColorEntry, NickColorSetting, UserPattern, glyphs,
};
use super::notify::{NotifyEntry, NotifySetting};

/// The dialog's tabs, in order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum AddressBookTab {
    Notify,
    Control,
    Colors,
}

impl AddressBookTab {
    pub(super) const ALL: [Self; 3] = [Self::Notify, Self::Control, Self::Colors];

    pub(super) fn label(self) -> &'static str {
        match self {
            Self::Notify => "Notify",
            Self::Control => "Control",
            Self::Colors => "Colors",
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

    /// Whom a nick colour entry names, as `/cnick` takes it: a pattern
    /// with `*` or `?` as typed, else a user.
    pub(super) fn resolve_pattern(&self, text: &str) -> Result<UserPattern, String> {
        let text = text.trim();
        if text.contains(['*', '?']) {
            UserPattern::new(text)
        } else {
            UserPattern::new(&self.resolve(text)?)
        }
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
    pub(super) nick_colors: NickColorSetting,
    pub(super) people: People,
}

/// A nick colour entry dialog's fields, as typed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ColorEntryFields {
    pub(super) pattern: String,
    pub(super) color: NickColor,
    pub(super) modes: String,
    pub(super) any_mode: bool,
    pub(super) no_mode: bool,
    pub(super) notify: bool,
}

impl Default for ColorEntryFields {
    /// Add's: any user in an auto colour.
    fn default() -> Self {
        Self {
            pattern: String::new(),
            color: NickColor::Auto,
            modes: String::new(),
            any_mode: false,
            no_mode: false,
            notify: false,
        }
    }
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

impl AddressBookForm {
    /// The Colors tab's rows: whom the entry names, its colour, and its
    /// other Apply items.
    pub(super) fn color_rows(&self) -> Vec<[String; 3]> {
        self.nick_colors
            .list
            .entries()
            .iter()
            .map(|entry| {
                [
                    entry.label(&|user_id: &str| self.people.nick_of(user_id)),
                    entry.color.text(),
                    entry.apply_text(),
                ]
            })
            .collect()
    }

    /// The entry a colour entry dialog's fields make, refused as `/cnick`
    /// refuses them.
    pub(super) fn check_color_entry(
        &self,
        fields: &ColorEntryFields,
    ) -> Result<NickColorEntry, String> {
        Ok(NickColorEntry {
            pattern: self.people.resolve_pattern(&fields.pattern)?,
            color: fields.color,
            modes: ModeCondition::from_parts(fields.modes.trim(), fields.any_mode, fields.no_mode)?,
            notify: fields.notify,
        })
    }

    /// Edit's fields for the entry at `index`: its pattern or user ID as
    /// it is kept.
    pub(super) fn color_fields(&self, index: usize) -> ColorEntryFields {
        let entry = &self.nick_colors.list.entries()[index];
        let (modes, any_mode, no_mode) = match &entry.modes {
            None => (String::new(), false, false),
            Some(ModeCondition::AnyMode) => (String::new(), true, false),
            Some(ModeCondition::NoMode) => (String::new(), false, true),
            Some(ModeCondition::Ranks(ranks)) => (glyphs(ranks), false, false),
        };
        ColorEntryFields {
            pattern: entry.pattern.as_str().to_owned(),
            color: entry.color,
            modes,
            any_mode,
            no_mode,
            notify: entry.notify,
        }
    }

    /// Add appends (`index` `None`); Edit replaces the entry at `index`.
    pub(super) fn put_color(&mut self, index: Option<usize>, entry: NickColorEntry) {
        match index {
            Some(index) => self.nick_colors.list.set(index, entry),
            None => self.nick_colors.list.push(entry),
        }
    }

    pub(super) fn remove_color(&mut self, index: usize) {
        self.nick_colors.list.remove(index);
    }

    /// Up and Down: the entry at `index` swaps with its neighbour, and
    /// keeps its selection at its new place, which this returns.
    pub(super) fn move_color(&mut self, index: usize, up: bool) -> usize {
        if up {
            self.nick_colors.list.move_up(index);
            index - 1
        } else {
            self.nick_colors.list.move_up(index + 1);
            index + 1
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
            nick_colors: NickColorSetting::default(),
            people: people(),
        }
    }

    fn color_fields(pattern: &str, modes: &str, any_mode: bool, no_mode: bool) -> ColorEntryFields {
        ColorEntryFields {
            pattern: pattern.to_owned(),
            modes: modes.to_owned(),
            any_mode,
            no_mode,
            ..ColorEntryFields::default()
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
    fn tabs_are_mircs_three_in_order() {
        assert_eq!(
            AddressBookTab::ALL.map(AddressBookTab::label),
            ["Notify", "Control", "Colors"]
        );
        assert_eq!(AddressBookTab::Colors.index(), 2);
    }

    #[test]
    fn color_entries_are_checked_as_cnick_checks_them() {
        let form = form(None);
        let check = |fields: ColorEntryFields| form.check_color_entry(&fields);
        assert_eq!(
            check(color_fields("bob", "", false, false)),
            Err("bob is ambiguous (@bob:example, @bob:other)".to_owned())
        );
        assert_eq!(
            check(color_fields("erin", "", false, false)),
            Err("no such nick: erin".to_owned())
        );
        assert_eq!(
            check(color_fields("* bot", "", false, false)),
            Err("* bot is not a user ID or a pattern with * or ?".to_owned())
        );
        assert_eq!(
            check(color_fields("carol", "@x", false, false)),
            Err("modes must be some of ~&@+, not @x".to_owned())
        );
        assert_eq!(
            check(color_fields("carol", "", true, true)),
            Err("any mode and no mode cannot be combined".to_owned())
        );
        assert_eq!(
            check(color_fields("carol", "@", true, false)),
            Err("modes cannot be given with any mode or no mode".to_owned())
        );
        // A nick is its user's ID; a pattern stays as typed, trimmed.
        assert_eq!(
            check(color_fields(" Carol ", " @+ ", false, false)),
            Ok(NickColorEntry::parse_ini_value("@carol:example auto modes=@+").unwrap())
        );
        assert_eq!(
            check(color_fields(" *bot* ", "", false, true)),
            Ok(NickColorEntry::parse_ini_value("*bot* auto none").unwrap())
        );
    }

    #[test]
    fn color_entries_list_edit_move_and_remove_in_order() {
        let mut form = form(None);
        for value in [
            "@carol:example 4 modes=~@",
            "*bot* auto any notify",
            "@dave:remote 12 none",
        ] {
            form.put_color(None, NickColorEntry::parse_ini_value(value).unwrap());
        }
        assert_eq!(
            form.color_rows(),
            [
                ["Carol (@carol:example)", "4", "~@"],
                ["*bot*", "auto", "any mode, notify"],
                ["@dave:remote", "12", "no mode"],
            ]
            .map(|row| row.map(str::to_owned))
        );
        assert_eq!(
            form.color_fields(0),
            ColorEntryFields {
                color: NickColor::parse("4").unwrap(),
                ..color_fields("@carol:example", "~@", false, false)
            }
        );
        assert_eq!(
            form.color_fields(1),
            ColorEntryFields {
                notify: true,
                ..color_fields("*bot*", "", true, false)
            }
        );
        assert!(form.color_fields(2).no_mode);
        assert_eq!(form.move_color(2, true), 1);
        assert_eq!(form.move_color(0, false), 1);
        let edited = NickColorEntry::parse_ini_value("@erin:remote 3").unwrap();
        form.put_color(Some(2), edited);
        form.remove_color(0);
        assert_eq!(
            form.color_rows()
                .into_iter()
                .map(|[label, ..]| label)
                .collect::<Vec<_>>(),
            ["Carol (@carol:example)", "@erin:remote"]
        );
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
