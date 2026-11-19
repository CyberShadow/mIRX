//! The frame's menubar, laid out as mIRC's: File, View, Tools, Commands,
//! Window and Help. Favorites comes with the rooms it lists.
//!
//! Pure tables and the rules for which items apply; `create_frame_menu`
//! builds the Win32 menus from them. The Room, Message and Member menus are
//! right-click menus of the treebar, timeline and nick list, not bar menus.

use mirx_core::{VerificationState, VerificationStateEvent};

use super::commands::VerificationAction;
use super::settings::{Bar, Settings};
use crate::automation::ConnectionState;

pub(super) const MENU_ID_LOGOUT: usize = 40004;
pub(super) const MENU_ID_RECOVER_E2EE: usize = 40005;
/// Find has no menubar item; Ctrl+F and the window menu's Find Text send it.
pub(super) const MENU_ID_FIND: usize = 40006;
pub(super) const MENU_ID_VERIFY_ACCEPT: usize = 40007;
pub(super) const MENU_ID_VERIFY_CANCEL: usize = 40008;
pub(super) const MENU_ID_VERIFY_CONFIRM_SAS: usize = 40009;
pub(super) const MENU_ID_VERIFY_REJECT_SAS: usize = 40010;
pub(super) const MENU_ID_ROOM_ACCEPT_INVITE: usize = 40011;
pub(super) const MENU_ID_ROOM_REJECT_INVITE: usize = 40012;
pub(super) const MENU_ID_ROOM_PART: usize = 40013;
pub(super) const MENU_ID_MESSAGE_REPLY: usize = 40014;
pub(super) const MENU_ID_MESSAGE_REDACT: usize = 40015;
pub(super) const MENU_ID_MESSAGE_SAVE_ATTACHMENT: usize = 40016;
pub(super) const MENU_ID_SETUP_E2EE_RECOVERY: usize = 40017;
pub(super) const MENU_ID_MESSAGE_EDIT: usize = 40018;
pub(super) const MENU_ID_MESSAGE_REACT: usize = 40019;
pub(super) const MENU_ID_MEMBER_QUERY: usize = 40022;
pub(super) const MENU_ID_MEMBER_MENTION: usize = 40023;
pub(super) const MENU_ID_MEMBER_KICK: usize = 40024;
pub(super) const MENU_ID_MEMBER_BAN: usize = 40025;
pub(super) const MENU_ID_WINDOW_CASCADE: usize = 40026;
pub(super) const MENU_ID_WINDOW_TILE_HORIZONTAL: usize = 40027;
pub(super) const MENU_ID_WINDOW_TILE_VERTICAL: usize = 40028;
pub(super) const MENU_ID_WINDOW_ARRANGE_ICONS: usize = 40029;
pub(super) const MENU_ID_FILE_CONNECT: usize = 40030;
pub(super) const MENU_ID_FILE_DISCONNECT: usize = 40031;
pub(super) const MENU_ID_FILE_SELECT_SERVER: usize = 40032;
pub(super) const MENU_ID_FILE_EXIT: usize = 40033;
pub(super) const MENU_ID_HELP_TOPICS: usize = 40034;
pub(super) const MENU_ID_HELP_ABOUT: usize = 40035;
pub(super) const MENU_ID_VIEW_SWITCHBAR: usize = 40036;
pub(super) const MENU_ID_VIEW_TREEBAR: usize = 40037;
/// Options..., which Alt+O sends, as in mIRC.
pub(super) const MENU_ID_TOOLS_OPTIONS: usize = 40038;
/// `COMMANDS_MENU[i]` has the id `MENU_ID_COMMANDS_FIRST + i`. The range
/// ends below the switchbar's (41001) and MDI's child ids (50000).
pub(super) const MENU_ID_COMMANDS_FIRST: usize = 40100;
/// List rooms, which Alt+L sends, as mIRC's List Channels.
pub(super) const MENU_ID_COMMANDS_LIST: usize = MENU_ID_COMMANDS_FIRST + 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum MenuEntry {
    Item { id: usize, label: &'static str },
    Separator,
}

use MenuEntry::{Item, Separator};

pub(super) const FILE_MENU: &[MenuEntry] = &[
    Item {
        id: MENU_ID_FILE_CONNECT,
        label: "&Connect",
    },
    Item {
        id: MENU_ID_FILE_DISCONNECT,
        label: "&Disconnect",
    },
    Item {
        id: MENU_ID_FILE_SELECT_SERVER,
        label: "Select &Server...\tAlt+E",
    },
    Separator,
    Item {
        id: MENU_ID_LOGOUT,
        label: "&Log Out",
    },
    Separator,
    Item {
        id: MENU_ID_FILE_EXIT,
        label: "E&xit",
    },
];

/// The bars View shows and hides, with their items.
pub(super) const VIEW_MENU_BARS: [(Bar, usize); 2] = [
    (Bar::Switchbar, MENU_ID_VIEW_SWITCHBAR),
    (Bar::Treebar, MENU_ID_VIEW_TREEBAR),
];

pub(super) const VIEW_MENU: &[MenuEntry] = &[
    Item {
        id: MENU_ID_VIEW_SWITCHBAR,
        label: "&Switchbar",
    },
    Item {
        id: MENU_ID_VIEW_TREEBAR,
        label: "&Treebar",
    },
];

/// The View items' check marks: each is checked while its bar is shown.
pub(super) fn view_menu_checks(settings: &Settings) -> [(usize, bool); 2] {
    VIEW_MENU_BARS.map(|(bar, id)| (id, settings.bar_shown(bar)))
}

/// The bar a View item shows and hides.
pub(super) fn view_menu_bar(id: usize) -> Option<Bar> {
    VIEW_MENU_BARS
        .iter()
        .find(|(_, item)| *item == id)
        .map(|(bar, _)| *bar)
}

pub(super) const TOOLS_MENU: &[MenuEntry] = &[
    Item {
        id: MENU_ID_SETUP_E2EE_RECOVERY,
        label: "Set Up E2EE &Recovery...",
    },
    Item {
        id: MENU_ID_RECOVER_E2EE,
        label: "Recover &E2EE...",
    },
    Separator,
    Item {
        id: MENU_ID_VERIFY_ACCEPT,
        label: "&Accept Verification",
    },
    Item {
        id: MENU_ID_VERIFY_CONFIRM_SAS,
        label: "Confirm &SAS",
    },
    Item {
        id: MENU_ID_VERIFY_REJECT_SAS,
        label: "Re&ject SAS",
    },
    Item {
        id: MENU_ID_VERIFY_CANCEL,
        label: "&Cancel Verification",
    },
    Separator,
    Item {
        id: MENU_ID_TOOLS_OPTIONS,
        label: "&Options...\tAlt+O",
    },
];

pub(super) const WINDOW_MENU: &[MenuEntry] = &[
    Item {
        id: MENU_ID_WINDOW_CASCADE,
        label: "Cascade",
    },
    Item {
        id: MENU_ID_WINDOW_TILE_HORIZONTAL,
        label: "Tile Horizontal",
    },
    Item {
        id: MENU_ID_WINDOW_TILE_VERTICAL,
        label: "Tile Vertical",
    },
    Item {
        id: MENU_ID_WINDOW_ARRANGE_ICONS,
        label: "Arrange Icons",
    },
];

pub(super) const HELP_MENU: &[MenuEntry] = &[
    Item {
        id: MENU_ID_HELP_TOPICS,
        label: "&Help Topics",
    },
    Separator,
    Item {
        id: MENU_ID_HELP_ABOUT,
        label: "&About mirx",
    },
];

/// The menubar's menus, left to right. No mnemonic is `E`, `L` or `O`:
/// Alt+E is Select Server, Alt+L List rooms and Alt+O Options, as in mIRC.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum BarMenu {
    File,
    View,
    Tools,
    Commands,
    Window,
    Help,
}

pub(super) const MENU_BAR: [(BarMenu, &str); 6] = [
    (BarMenu::File, "&File"),
    (BarMenu::View, "&View"),
    (BarMenu::Tools, "&Tools"),
    (BarMenu::Commands, "&Commands"),
    (BarMenu::Window, "&Window"),
    (BarMenu::Help, "&Help"),
];

/// A Commands menu item: mIRC's default Commands menu, run as the slash
/// command typed into the active window.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct CommandsMenuItem {
    pub(super) label: &'static str,
    /// The slash command's name, without the `/`.
    pub(super) command: &'static str,
    /// The single-field dialog's label for the command's parameters, or
    /// `None` for a command that runs as is.
    pub(super) prompt: Option<&'static str>,
}

pub(super) const COMMANDS_MENU: &[CommandsMenuItem] = &[
    CommandsMenuItem {
        label: "&Join room...",
        command: "join",
        prompt: Some("Room ID or alias"),
    },
    CommandsMenuItem {
        label: "&List rooms...\tAlt+L",
        command: "list",
        prompt: Some("Match text (blank lists all)"),
    },
    CommandsMenuItem {
        label: "&Part room",
        command: "part",
        prompt: None,
    },
    CommandsMenuItem {
        label: "&Query user...",
        command: "query",
        prompt: Some("Nick or user ID"),
    },
    CommandsMenuItem {
        label: "&Whois user...",
        command: "whois",
        prompt: Some("Nick or user ID"),
    },
    CommandsMenuItem {
        label: "&Invite user...",
        command: "invite",
        prompt: Some("Nick or user ID"),
    },
    CommandsMenuItem {
        label: "&Kick user...",
        command: "kick",
        prompt: Some("Nick [reason]"),
    },
    CommandsMenuItem {
        label: "&Ban user...",
        command: "ban",
        prompt: Some("Nick [reason]"),
    },
    CommandsMenuItem {
        label: "Change &nick...",
        command: "nick",
        prompt: Some("New nick"),
    },
];

pub(super) fn commands_menu_item(id: usize) -> Option<&'static CommandsMenuItem> {
    COMMANDS_MENU.get(id.checked_sub(MENU_ID_COMMANDS_FIRST)?)
}

/// The prompt dialog's title: the label without its mnemonic, its
/// accelerator and `...`.
pub(super) fn commands_menu_title(item: &CommandsMenuItem) -> String {
    let label = item.label.split('\t').next().unwrap_or_default();
    label.replace('&', "").trim_end_matches("...").to_owned()
}

/// The line the item runs, as if typed: `/join #ops`, or `/part`. Empty
/// input gives `/join`, which the parser reports as insufficient parameters.
pub(super) fn commands_menu_line(item: &CommandsMenuItem, input: &str) -> String {
    match item.prompt {
        Some(_) => format!("/{} {}", item.command, input.trim())
            .trim_end()
            .to_owned(),
        None => format!("/{}", item.command),
    }
}

/// Which File items apply; mIRC greys Disconnect while offline.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct FileMenuState {
    pub(super) connect: bool,
    pub(super) disconnect: bool,
    pub(super) select_server: bool,
    pub(super) log_out: bool,
}

pub(super) fn file_menu_state(connection: ConnectionState) -> FileMenuState {
    let (signed_out, signed_in, syncing) = (
        connection == ConnectionState::SignedOut,
        connection == ConnectionState::SignedIn,
        connection == ConnectionState::Syncing,
    );
    FileMenuState {
        connect: signed_out || signed_in,
        disconnect: syncing,
        select_server: signed_out,
        log_out: signed_in || syncing,
    }
}

pub(super) fn verification_menu_action_enabled(
    last_verification: Option<&VerificationStateEvent>,
    action: VerificationAction,
) -> bool {
    let Some(event) = last_verification else {
        return false;
    };

    matches!(
        (&event.state, action),
        (
            VerificationState::Requested,
            VerificationAction::Accept | VerificationAction::Cancel
        ) | (
            VerificationState::Accepted | VerificationState::Ready | VerificationState::Confirmed,
            VerificationAction::Cancel
        ) | (
            VerificationState::SasReady { .. },
            VerificationAction::Cancel | VerificationAction::Confirm | VerificationAction::Reject
        )
    )
}

/// The Status window's "Actions:" hint for a verification: the Tools items
/// that apply now, and their commands.
pub(super) fn verification_actions_text(
    last_verification: Option<&VerificationStateEvent>,
) -> Option<&'static str> {
    let actions = (
        verification_menu_action_enabled(last_verification, VerificationAction::Accept),
        verification_menu_action_enabled(last_verification, VerificationAction::Cancel),
        verification_menu_action_enabled(last_verification, VerificationAction::Confirm),
        verification_menu_action_enabled(last_verification, VerificationAction::Reject),
    );

    match actions {
        (true, true, false, false) => Some(
            "Tools > Accept Verification, Tools > Cancel Verification, /verify accept, /verify cancel",
        ),
        (false, true, false, false) => Some("Tools > Cancel Verification, /verify cancel"),
        (false, true, true, true) => Some(
            "Tools > Confirm SAS, Tools > Reject SAS, Tools > Cancel Verification, /verify confirm, /verify reject, /verify cancel",
        ),
        (false, false, false, false) => None,
        _ => unreachable!("verification action text must match the menu action predicate"),
    }
}

#[cfg(test)]
mod tests {
    use super::super::commands::{COMMANDS, parse};
    use super::*;

    #[test]
    fn file_menu_greys_what_does_not_apply() {
        assert_eq!(
            file_menu_state(ConnectionState::SignedOut),
            FileMenuState {
                connect: true,
                disconnect: false,
                select_server: true,
                log_out: false,
            }
        );
        assert_eq!(
            file_menu_state(ConnectionState::SigningIn),
            FileMenuState {
                connect: false,
                disconnect: false,
                select_server: false,
                log_out: false,
            }
        );
        assert_eq!(
            file_menu_state(ConnectionState::SignedIn),
            FileMenuState {
                connect: true,
                disconnect: false,
                select_server: false,
                log_out: true,
            }
        );
        assert_eq!(
            file_menu_state(ConnectionState::Syncing),
            FileMenuState {
                connect: false,
                disconnect: true,
                select_server: false,
                log_out: true,
            }
        );
    }

    #[test]
    fn commands_menu_items_are_slash_commands() {
        for item in COMMANDS_MENU {
            assert!(
                COMMANDS.iter().any(|spec| spec.name == item.command),
                "{item:?}"
            );
            if item.prompt.is_none() {
                assert!(parse(&commands_menu_line(item, "")).is_ok(), "{item:?}");
            }
        }
        assert_eq!(
            commands_menu_item(MENU_ID_COMMANDS_FIRST),
            Some(&COMMANDS_MENU[0])
        );
        assert_eq!(
            commands_menu_item(MENU_ID_COMMANDS_FIRST + COMMANDS_MENU.len()),
            None
        );
        assert_eq!(commands_menu_item(MENU_ID_COMMANDS_FIRST - 1), None);
    }

    #[test]
    fn commands_menu_lines_read_as_typed() {
        let join = &COMMANDS_MENU[0];
        let list = &COMMANDS_MENU[1];
        let part = &COMMANDS_MENU[2];
        assert_eq!(
            commands_menu_line(join, "  #ops:example.org "),
            "/join #ops:example.org"
        );
        assert_eq!(commands_menu_line(join, "  "), "/join");
        assert_eq!(
            parse(&commands_menu_line(join, ""))
                .unwrap_err()
                .to_string(),
            "* /join: insufficient parameters"
        );
        assert_eq!(commands_menu_line(part, "ignored"), "/part");
        assert_eq!(commands_menu_title(join), "Join room");
        assert_eq!(commands_menu_title(list), "List rooms");
        assert_eq!(commands_menu_title(&COMMANDS_MENU[8]), "Change nick");
        // A blank match lists every room.
        assert_eq!(commands_menu_line(list, "  "), "/list");
        assert_eq!(commands_menu_line(list, " rust "), "/list rust");
        assert_eq!(commands_menu_item(MENU_ID_COMMANDS_LIST), Some(list));
    }

    #[test]
    fn bar_mnemonics_are_unique_and_leave_the_accelerators_free() {
        let mnemonics = MENU_BAR
            .iter()
            .map(|(_, label)| {
                let (_, rest) = label
                    .split_once('&')
                    .expect("every bar menu has a mnemonic");
                rest.chars().next().unwrap().to_ascii_uppercase()
            })
            .collect::<Vec<_>>();
        assert_eq!(mnemonics, ['F', 'V', 'T', 'C', 'W', 'H']);
    }

    #[test]
    fn tools_ends_with_options() {
        assert_eq!(
            TOOLS_MENU[TOOLS_MENU.len() - 2..],
            [
                Separator,
                Item {
                    id: MENU_ID_TOOLS_OPTIONS,
                    label: "&Options...\tAlt+O",
                },
            ]
        );
    }

    #[test]
    fn view_items_check_the_shown_bars() {
        let mut settings = Settings::default();
        assert_eq!(
            view_menu_checks(&settings),
            [(MENU_ID_VIEW_SWITCHBAR, true), (MENU_ID_VIEW_TREEBAR, true)]
        );
        settings.treebar.show = false;
        assert_eq!(
            view_menu_checks(&settings),
            [
                (MENU_ID_VIEW_SWITCHBAR, true),
                (MENU_ID_VIEW_TREEBAR, false)
            ]
        );
        assert_eq!(view_menu_bar(MENU_ID_VIEW_TREEBAR), Some(Bar::Treebar));
        assert_eq!(view_menu_bar(MENU_ID_FIND), None);
        for entry in VIEW_MENU {
            let Item { id, .. } = entry else {
                panic!("View has no separator");
            };
            assert!(view_menu_bar(*id).is_some());
        }
    }

    #[test]
    fn menu_ids_are_unique() {
        let mut ids = [FILE_MENU, VIEW_MENU, TOOLS_MENU, WINDOW_MENU, HELP_MENU]
            .into_iter()
            .flatten()
            .filter_map(|entry| match entry {
                Item { id, .. } => Some(*id),
                Separator => None,
            })
            .chain((0..COMMANDS_MENU.len()).map(|index| MENU_ID_COMMANDS_FIRST + index))
            .collect::<Vec<_>>();
        let count = ids.len();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), count);
    }
}
