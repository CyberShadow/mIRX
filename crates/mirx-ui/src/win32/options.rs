//! mIRC's Options dialog (Alt+O), without Win32: its category tree and the
//! form its pages edit, checked and applied to the settings at once.
//!
//! The pages hold mirx's settings under the categories mIRC files them
//! under; `options_dialog` shows them.

use mirx_core::{HighlightWord, validate_highlight_word};

use super::settings::{
    ActivityColors, LoggingSetting, Settings, TipsSetting, TraySetting, parse_button_width,
};
use super::timestamp::TimestampFormat;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum OptionsPage {
    ConnectOptions,
    IrcMessages,
    IrcHighlight,
    IrcLogging,
    Display,
    DisplayTray,
    DisplayTips,
}

use OptionsPage::{
    ConnectOptions, Display, DisplayTips, DisplayTray, IrcHighlight, IrcLogging, IrcMessages,
};

/// The tree, in order: (label, depth, page). A category without a page of
/// its own shows its first child's.
pub(super) const OPTIONS_TREE: &[(&str, u8, OptionsPage)] = &[
    ("Connect", 0, ConnectOptions),
    ("Options", 1, ConnectOptions),
    ("IRC", 0, IrcMessages),
    ("Messages", 1, IrcMessages),
    ("Highlight", 1, IrcHighlight),
    ("Logging", 1, IrcLogging),
    ("Display", 0, Display),
    ("Tray", 1, DisplayTray),
    ("Tips", 1, DisplayTips),
];

/// The page the dialog opens on.
pub(super) const FIRST_PAGE: OptionsPage = ConnectOptions;

/// The tree item that is the page itself, which selecting its category
/// selects.
pub(super) fn page_tree_index(page: OptionsPage) -> usize {
    OPTIONS_TREE
        .iter()
        .rposition(|(_, _, item_page)| *item_page == page)
        .expect("every page is in the tree")
}

/// Each tree item's path: `Connect/Options`.
pub(super) fn tree_paths() -> Vec<String> {
    let mut category = "";
    OPTIONS_TREE
        .iter()
        .map(|(label, depth, _)| match depth {
            0 => {
                category = label;
                (*label).to_owned()
            }
            1 => format!("{category}/{label}"),
            _ => unreachable!("the tree is two levels deep"),
        })
        .collect()
}

/// The tree item whose path (`IRC/Messages`) is `path`.
pub(super) fn tree_index(path: &str) -> Option<usize> {
    tree_paths().iter().position(|item_path| item_path == path)
}

/// The fields of the form, for pointing at the one that is wrong.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum OptionsField {
    TimestampFormat,
    SwitchbarButtons,
}

impl OptionsField {
    pub(super) fn page(self) -> OptionsPage {
        match self {
            Self::TimestampFormat => IrcMessages,
            Self::SwitchbarButtons => Display,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct OptionsError {
    pub(super) field: OptionsField,
    pub(super) message: String,
}

/// What the pages show, text fields as typed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct OptionsForm {
    pub(super) connect_on_startup: bool,
    pub(super) timestamp_show: bool,
    pub(super) timestamp_format: String,
    pub(super) switchbar_buttons: String,
    /// Picked from the palette, so never wrong.
    pub(super) activity_colors: ActivityColors,
    /// Checkboxes, so never wrong.
    pub(super) logging: LoggingSetting,
    /// Checkboxes, so never wrong.
    pub(super) tray: TraySetting,
    /// Checkboxes, so never wrong.
    pub(super) tips: TipsSetting,
}

impl OptionsForm {
    pub(super) fn from_settings(settings: &Settings) -> Self {
        Self {
            connect_on_startup: settings.connect.on_startup,
            timestamp_show: settings.timestamp.show,
            timestamp_format: settings.timestamp.format.to_string(),
            switchbar_buttons: settings.switchbar.buttons.to_string(),
            activity_colors: settings.switchbar.activity,
            logging: settings.logging,
            tray: settings.tray,
            tips: settings.tips,
        }
    }

    /// `current` with the form applied, or the first wrong field in tree
    /// order. Settings the form does not show are kept.
    pub(super) fn apply(&self, current: &Settings) -> Result<Settings, OptionsError> {
        let error = |field, reason: String| OptionsError {
            field,
            message: reason,
        };
        let format = TimestampFormat::parse(&self.timestamp_format).map_err(|reason| {
            error(OptionsField::TimestampFormat, format!("Timestamp {reason}"))
        })?;
        let buttons = parse_button_width(&self.switchbar_buttons).map_err(|reason| {
            error(
                OptionsField::SwitchbarButtons,
                format!("Switchbar {reason}"),
            )
        })?;
        let mut settings = current.clone();
        settings.connect.on_startup = self.connect_on_startup;
        settings.timestamp.show = self.timestamp_show;
        settings.timestamp.format = format;
        settings.switchbar.buttons = buttons;
        settings.switchbar.activity = self.activity_colors;
        settings.logging = self.logging;
        settings.tray = self.tray;
        settings.tips = self.tips;
        Ok(settings)
    }
}

/// The words typed into the Highlight page's box, which takes several at
/// once as mIRC's does: `deploy, build, *help*`. Each must be a valid
/// highlight word, as the core checks.
pub(super) fn parse_highlight_input(input: &str) -> Result<Vec<String>, String> {
    input
        .split(',')
        .map(str::trim)
        .filter(|word| !word.is_empty())
        .map(|word| {
            validate_highlight_word(word)
                .map(|()| word.to_owned())
                .map_err(|reason| format!("Highlight \"{word}\": {reason}"))
        })
        .collect()
}

/// Adds words to the list as highlighting ones: a word already listed
/// highlights from now on, a new one goes at the end.
pub(super) fn add_highlight_words(list: &mut Vec<HighlightWord>, words: Vec<String>) {
    for word in words {
        match list.iter_mut().find(|listed| listed.word == word) {
            Some(listed) => listed.highlights = true,
            None => list.push(HighlightWord {
                word,
                highlights: true,
            }),
        }
    }
}

/// What OK sends to the server for the Highlight page.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(super) struct HighlightChanges {
    pub(super) add: Vec<String>,
    pub(super) remove: Vec<String>,
}

/// The requests that turn the server's list into the edited one, in list
/// order: every edited highlighting word the server does not highlight
/// yet, and every server word the edit deleted.
pub(super) fn highlight_word_changes(
    current: &[HighlightWord],
    edited: &[HighlightWord],
) -> HighlightChanges {
    HighlightChanges {
        add: edited
            .iter()
            .filter(|word| word.highlights && !current.contains(word))
            .map(|word| word.word.clone())
            .collect(),
        remove: current
            .iter()
            .filter(|word| !edited.iter().any(|edited| edited.word == word.word))
            .map(|word| word.word.clone())
            .collect(),
    }
}

/// A list item's text: the word, marked when it only notifies, as words
/// other clients add do; mirx colours only the highlighting ones.
pub(super) fn highlight_item_text(word: &HighlightWord) -> String {
    if word.highlights {
        word.word.clone()
    } else {
        format!("{} (notify only)", word.word)
    }
}

/// The Highlight page's list as edited, next to the server's list it
/// started from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct HighlightList {
    current: Vec<HighlightWord>,
    edited: Vec<HighlightWord>,
}

impl HighlightList {
    pub(super) fn new(current: Vec<HighlightWord>) -> Self {
        Self {
            edited: current.clone(),
            current,
        }
    }

    pub(super) fn items(&self) -> Vec<String> {
        self.edited.iter().map(highlight_item_text).collect()
    }

    /// Adds what was typed, or says what is wrong with it and changes
    /// nothing.
    pub(super) fn add(&mut self, input: &str) -> Result<(), String> {
        add_highlight_words(&mut self.edited, parse_highlight_input(input)?);
        Ok(())
    }

    pub(super) fn delete(&mut self, index: usize) {
        self.edited.remove(index);
    }

    pub(super) fn changes(&self) -> HighlightChanges {
        highlight_word_changes(&self.current, &self.edited)
    }
}

#[cfg(test)]
mod tests {
    use super::super::colors::PaletteIndex;
    use super::super::settings::{ButtonWidth, FontSetting};
    use super::*;

    fn custom_settings() -> Settings {
        let mut settings = Settings {
            text_font: FontSetting::new("Courier", 12, true).unwrap(),
            ..Settings::default()
        };
        settings.switchbar.show = false;
        settings.treebar.show = false;
        settings
    }

    #[test]
    fn the_tree_is_mircs_and_categories_show_their_first_page() {
        assert_eq!(
            tree_paths(),
            [
                "Connect",
                "Connect/Options",
                "IRC",
                "IRC/Messages",
                "IRC/Highlight",
                "IRC/Logging",
                "Display",
                "Display/Tray",
                "Display/Tips"
            ]
        );
        assert_eq!(page_tree_index(ConnectOptions), 1);
        assert_eq!(page_tree_index(IrcMessages), 3);
        assert_eq!(page_tree_index(IrcHighlight), 4);
        assert_eq!(page_tree_index(IrcLogging), 5);
        // Display has a page of its own, above its children's.
        assert_eq!(page_tree_index(Display), 6);
        assert_eq!(page_tree_index(DisplayTray), 7);
        assert_eq!(page_tree_index(DisplayTips), 8);
        assert_eq!(tree_index("Connect"), Some(0));
        assert_eq!(tree_index("IRC/Messages"), Some(3));
        assert_eq!(tree_index("IRC/Logging"), Some(5));
        assert_eq!(tree_index("Display"), Some(6));
        assert_eq!(tree_index("Display/Tray"), Some(7));
        assert_eq!(tree_index("Display/Tips"), Some(8));
        for path in ["", "IRC/", "Messages", "irc/messages", "IRC > Messages"] {
            assert_eq!(tree_index(path), None, "{path:?}");
        }
        assert_eq!(FIRST_PAGE, ConnectOptions);
        for field in [
            OptionsField::TimestampFormat,
            OptionsField::SwitchbarButtons,
        ] {
            assert!(
                OPTIONS_TREE
                    .iter()
                    .any(|(_, _, page)| *page == field.page())
            );
        }
    }

    #[test]
    fn an_unchanged_form_changes_nothing() {
        let settings = custom_settings();
        assert_eq!(
            OptionsForm::from_settings(&settings).apply(&settings),
            Ok(settings)
        );
    }

    #[test]
    fn each_field_changes_its_own_setting_and_keeps_the_others() {
        let current = custom_settings();
        let form = OptionsForm::from_settings(&current);
        let applied = |change: fn(&mut OptionsForm)| {
            let mut form = form.clone();
            change(&mut form);
            form.apply(&current).unwrap()
        };

        let mut expected = current.clone();
        expected.connect.on_startup = false;
        assert_eq!(applied(|form| form.connect_on_startup = false), expected);

        let mut expected = current.clone();
        expected.timestamp.show = false;
        assert_eq!(applied(|form| form.timestamp_show = false), expected);

        let mut expected = current.clone();
        expected.timestamp.format = TimestampFormat::parse("[HH:nn:ss]").unwrap();
        assert_eq!(
            applied(|form| form.timestamp_format = "[HH:nn:ss]".to_owned()),
            expected
        );

        let mut expected = current.clone();
        expected.switchbar.buttons = ButtonWidth::new(90).unwrap();
        assert_eq!(
            applied(|form| form.switchbar_buttons = "90".to_owned()),
            expected
        );

        let mut expected = current.clone();
        expected.switchbar.activity.message = PaletteIndex::new(9).unwrap();
        assert_eq!(
            applied(|form| form.activity_colors.message = PaletteIndex::new(9).unwrap()),
            expected
        );

        for change in [
            |form: &mut OptionsForm| form.logging.channels = true,
            |form: &mut OptionsForm| form.logging.queries = true,
            |form: &mut OptionsForm| form.logging.strip_codes = true,
            |form: &mut OptionsForm| form.logging.timestamp = false,
            |form: &mut OptionsForm| form.tray.always = true,
            |form: &mut OptionsForm| form.tray.startup = true,
            |form: &mut OptionsForm| form.tray.minimize = true,
            |form: &mut OptionsForm| form.tray.single_click = true,
            |form: &mut OptionsForm| form.tips.highlights = false,
            |form: &mut OptionsForm| form.tips.queries = false,
            |form: &mut OptionsForm| form.tips.channels = true,
        ] {
            let mut changed = form.clone();
            change(&mut changed);
            let mut expected = current.clone();
            expected.logging = changed.logging;
            expected.tray = changed.tray;
            expected.tips = changed.tips;
            assert_ne!(expected, current);
            assert_eq!(applied(change), expected);
        }
    }

    #[test]
    fn a_wrong_field_is_reported_with_its_page() {
        let current = Settings::default();
        let mut form = OptionsForm::from_settings(&current);
        form.timestamp_format.clear();
        assert_eq!(
            form.apply(&current),
            Err(OptionsError {
                field: OptionsField::TimestampFormat,
                message: "Timestamp format cannot be empty".to_owned(),
            })
        );

        let mut form = OptionsForm::from_settings(&current);
        for width in ["39", "401", "abc", ""] {
            form.switchbar_buttons = width.to_owned();
            assert_eq!(
                form.apply(&current),
                Err(OptionsError {
                    field: OptionsField::SwitchbarButtons,
                    message: "Switchbar buttons must be 40-400".to_owned(),
                }),
                "{width:?}"
            );
        }

        // The first wrong field in tree order is the one reported.
        form.timestamp_format.clear();
        assert_eq!(
            form.apply(&current).unwrap_err().field,
            OptionsField::TimestampFormat
        );
    }

    fn word(word: &str, highlights: bool) -> HighlightWord {
        HighlightWord {
            word: word.to_owned(),
            highlights,
        }
    }

    #[test]
    fn highlight_input_takes_comma_separated_words() {
        assert_eq!(
            parse_highlight_input(" deploy, build ,, *help*, release day,"),
            Ok(vec![
                "deploy".to_owned(),
                "build".to_owned(),
                "*help*".to_owned(),
                "release day".to_owned(),
            ])
        );
        assert_eq!(parse_highlight_input("  "), Ok(Vec::new()));
        assert_eq!(
            parse_highlight_input("deploy, a/b"),
            Err(r#"Highlight "a/b": a highlight word cannot contain / or \"#.to_owned())
        );
        assert_eq!(
            parse_highlight_input(".m.rule"),
            Err(r#"Highlight ".m.rule": a highlight word cannot start with ."#.to_owned())
        );
    }

    #[test]
    fn adding_a_listed_word_makes_it_highlight_and_a_new_one_goes_last() {
        let mut list = vec![word("deploy", true), word("build", false)];
        add_highlight_words(&mut list, vec!["release".to_owned(), "build".to_owned()]);
        assert_eq!(
            list,
            [
                word("deploy", true),
                word("build", true),
                word("release", true)
            ]
        );
    }

    #[test]
    fn highlight_changes_add_new_and_upgraded_words_and_remove_deleted_ones() {
        let current = [
            word("deploy", true),
            word("build", false),
            word("ci", false),
        ];
        assert_eq!(
            highlight_word_changes(&current, &current),
            HighlightChanges::default()
        );
        assert_eq!(
            highlight_word_changes(
                &current,
                &[
                    word("build", true),
                    word("ci", false),
                    word("release", true)
                ]
            ),
            HighlightChanges {
                add: vec!["build".to_owned(), "release".to_owned()],
                remove: vec!["deploy".to_owned()],
            }
        );
    }

    #[test]
    fn the_highlight_list_marks_notify_only_words_and_tracks_edits() {
        let mut list = HighlightList::new(vec![word("deploy", true), word("build", false)]);
        assert_eq!(list.items(), ["deploy", "build (notify only)"]);

        assert_eq!(
            list.add("a/b"),
            Err(r#"Highlight "a/b": a highlight word cannot contain / or \"#.to_owned())
        );
        assert_eq!(list.items(), ["deploy", "build (notify only)"]);

        list.add("release, build").unwrap();
        assert_eq!(list.items(), ["deploy", "build", "release"]);
        list.delete(0);
        assert_eq!(
            list.changes(),
            HighlightChanges {
                add: vec!["build".to_owned(), "release".to_owned()],
                remove: vec!["deploy".to_owned()],
            }
        );
    }
}
