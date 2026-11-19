//! mIRC's Options dialog (Alt+O), without Win32: its category tree and the
//! form its pages edit, checked and applied to the settings at once.
//!
//! The pages hold mirx's settings under the categories mIRC files them
//! under; `options_dialog` shows them.

use super::settings::{ConnectSetting, Settings, parse_button_width};
use super::timestamp::TimestampFormat;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum OptionsPage {
    ConnectOptions,
    IrcMessages,
    Display,
}

use OptionsPage::{ConnectOptions, Display, IrcMessages};

/// The tree, in order: (label, depth, page). A category without a page of
/// its own shows its first child's.
pub(super) const OPTIONS_TREE: &[(&str, u8, OptionsPage)] = &[
    ("Connect", 0, ConnectOptions),
    ("Options", 1, ConnectOptions),
    ("IRC", 0, IrcMessages),
    ("Messages", 1, IrcMessages),
    ("Display", 0, Display),
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

/// The page's heading: its tree path, `Connect > Options`.
pub(super) fn page_heading(page: OptionsPage) -> String {
    tree_paths()[page_tree_index(page)].replace('/', " > ")
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
}

impl OptionsForm {
    pub(super) fn from_settings(settings: &Settings) -> Self {
        Self {
            connect_on_startup: settings.connect.on_startup,
            timestamp_show: settings.timestamp.show,
            timestamp_format: settings.timestamp.format.to_string(),
            switchbar_buttons: settings.switchbar.buttons.to_string(),
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
        settings.connect = ConnectSetting {
            on_startup: self.connect_on_startup,
        };
        settings.timestamp.show = self.timestamp_show;
        settings.timestamp.format = format;
        settings.switchbar.buttons = buttons;
        Ok(settings)
    }
}

#[cfg(test)]
mod tests {
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
                "Display"
            ]
        );
        assert_eq!(page_tree_index(ConnectOptions), 1);
        assert_eq!(page_tree_index(IrcMessages), 3);
        assert_eq!(page_tree_index(Display), 4);
        assert_eq!(page_heading(ConnectOptions), "Connect > Options");
        assert_eq!(page_heading(IrcMessages), "IRC > Messages");
        assert_eq!(page_heading(Display), "Display");
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
}
