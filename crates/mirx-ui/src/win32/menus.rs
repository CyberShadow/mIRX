//! The frame's menubar, laid out as mIRC's: File, View, Tools, Commands,
//! Window and Help. Favorites comes with the rooms it lists.
//!
//! Pure tables and the rules for which items apply; `create_frame_menu`
//! builds the Win32 menus from them. The Room, Message and Member menus are
//! right-click menus of the treebar, timeline and nick list, not bar menus.

use mirx_core::{
    RoomMemberSummary, RoomNotificationMode, VerificationState, VerificationStateEvent,
};

use super::commands::{RankChange, SlashCommand, VerificationAction, rank_change_role};
use super::ignore::{check_ignore, check_unignore};
use super::navigation::WindowCycle;
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
pub(super) const MENU_ID_MESSAGE_GO_TO_ORIGINAL: usize = 40020;
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
pub(super) const MENU_ID_ROOM_NOTIFY_ALL: usize = 40039;
pub(super) const MENU_ID_ROOM_NOTIFY_MENTIONS: usize = 40040;
pub(super) const MENU_ID_ROOM_NOTIFY_MUTE: usize = 40041;
pub(super) const MENU_ID_MEMBER_WHOIS: usize = 40042;
pub(super) const MENU_ID_MEMBER_IGNORE: usize = 40043;
pub(super) const MENU_ID_MEMBER_UNIGNORE: usize = 40044;
pub(super) const MENU_ID_MEMBER_OP: usize = 40045;
pub(super) const MENU_ID_MEMBER_DEOP: usize = 40046;
pub(super) const MENU_ID_MEMBER_VOICE: usize = 40047;
pub(super) const MENU_ID_MEMBER_DEVOICE: usize = 40048;
/// Colors..., which Alt+K sends, as in mIRC.
pub(super) const MENU_ID_TOOLS_COLORS: usize = 40049;
/// Verify This Session, which runs `/verify`.
pub(super) const MENU_ID_VERIFY_THIS_SESSION: usize = 40050;
/// Sessions, which runs `/devices`.
pub(super) const MENU_ID_TOOLS_SESSIONS: usize = 40051;
pub(super) const MENU_ID_MEMBER_DEVICES: usize = 40052;
pub(super) const MENU_ID_MEMBER_VERIFY: usize = 40053;
pub(super) const MENU_ID_RESET_E2EE: usize = 40054;
pub(super) const MENU_ID_MEMBER_ACCEPT_IDENTITY: usize = 40055;
/// Export Room Keys..., which runs `/e2ee export`.
pub(super) const MENU_ID_EXPORT_ROOM_KEYS: usize = 40056;
/// Import Room Keys..., which runs `/e2ee import`.
pub(super) const MENU_ID_IMPORT_ROOM_KEYS: usize = 40057;
/// View > Toolbar.
pub(super) const MENU_ID_VIEW_TOOLBAR: usize = 40058;
/// The toolbar's Send File: a command with no menu item, which runs
/// `/sendfile`.
pub(super) const TOOLBAR_ID_SEND_FILE: usize = 40059;
/// Aliases..., which Alt+A sends, as mIRC's Scripts Editor at its
/// Aliases tab.
pub(super) const MENU_ID_TOOLS_ALIASES: usize = 40060;
/// The window keys' command ids: accelerators with no menu item, in the
/// gap below the Commands menu's. Alt+n sends `ACCEL_ID_WINDOW_FIRST + n - 1`.
pub(super) const ACCEL_ID_WINDOW_FIRST: usize = 40080;
pub(super) const ACCEL_ID_WINDOW_LAST: usize = ACCEL_ID_WINDOW_FIRST + 8;
pub(super) const ACCEL_ID_WINDOW_NEXT: usize = 40089;
pub(super) const ACCEL_ID_WINDOW_PREVIOUS: usize = 40090;
pub(super) const ACCEL_ID_NEXT_CHANNEL: usize = 40091;
pub(super) const ACCEL_ID_NEXT_QUERY: usize = 40092;
pub(super) const ACCEL_ID_TOGGLE_MAXIMIZE: usize = 40093;
pub(super) const ACCEL_ID_TREEBAR_FOCUS: usize = 40094;
pub(super) const ACCEL_ID_LINE_MARKER: usize = 40095;
/// `COMMANDS_MENU[i]` has the id `MENU_ID_COMMANDS_FIRST + i`. The range
/// ends below the switchbar's (41001) and MDI's child ids (50000).
pub(super) const MENU_ID_COMMANDS_FIRST: usize = 40100;
/// List rooms, which Alt+L sends, as mIRC's List Channels.
pub(super) const MENU_ID_COMMANDS_LIST: usize = MENU_ID_COMMANDS_FIRST + 1;
/// Join room..., the menu's first item.
pub(super) const MENU_ID_COMMANDS_JOIN: usize = MENU_ID_COMMANDS_FIRST;

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
pub(super) const VIEW_MENU_BARS: [(Bar, usize); 3] = [
    (Bar::Toolbar, MENU_ID_VIEW_TOOLBAR),
    (Bar::Switchbar, MENU_ID_VIEW_SWITCHBAR),
    (Bar::Treebar, MENU_ID_VIEW_TREEBAR),
];

pub(super) const VIEW_MENU: &[MenuEntry] = &[
    // `&T` is the Treebar's.
    Item {
        id: MENU_ID_VIEW_TOOLBAR,
        label: "T&oolbar",
    },
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
pub(super) fn view_menu_checks(settings: &Settings) -> [(usize, bool); 3] {
    VIEW_MENU_BARS.map(|(bar, id)| (id, settings.bar_shown(bar)))
}

/// The bar a View item shows and hides.
pub(super) fn view_menu_bar(id: usize) -> Option<Bar> {
    VIEW_MENU_BARS
        .iter()
        .find(|(_, item)| *item == id)
        .map(|(bar, _)| *bar)
}

/// The room menu's Notifications submenu: one radio item per Matrix mode,
/// in this order, the room's own mode checked.
pub(super) const ROOM_NOTIFICATIONS_MENU: [(RoomNotificationMode, usize, &str); 3] = [
    (
        RoomNotificationMode::AllMessages,
        MENU_ID_ROOM_NOTIFY_ALL,
        "&All Messages",
    ),
    (
        RoomNotificationMode::MentionsAndKeywordsOnly,
        MENU_ID_ROOM_NOTIFY_MENTIONS,
        "&Mentions and Keywords",
    ),
    (
        RoomNotificationMode::Mute,
        MENU_ID_ROOM_NOTIFY_MUTE,
        "M&ute",
    ),
];

/// The mode a Notifications item sets.
pub(super) fn notification_menu_mode(id: usize) -> Option<RoomNotificationMode> {
    ROOM_NOTIFICATIONS_MENU
        .iter()
        .find(|(_, item, _)| *item == id)
        .map(|(mode, _, _)| *mode)
}

/// The Notifications item that shows a mode.
pub(super) fn notification_menu_id(mode: RoomNotificationMode) -> usize {
    ROOM_NOTIFICATIONS_MENU
        .iter()
        .find(|(item_mode, _, _)| *item_mode == mode)
        .map(|(_, id, _)| *id)
        .expect("every mode has a Notifications item")
}

/// A mode's name as its Notifications item reads, without the mnemonic.
pub(super) fn notification_mode_label(mode: RoomNotificationMode) -> String {
    ROOM_NOTIFICATIONS_MENU
        .iter()
        .find(|(item_mode, _, _)| *item_mode == mode)
        .map(|(_, _, label)| label.replace('&', ""))
        .expect("every mode has a Notifications item")
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
    Item {
        id: MENU_ID_RESET_E2EE,
        label: "Rese&t E2EE Identity...",
    },
    Item {
        id: MENU_ID_EXPORT_ROOM_KEYS,
        label: "E&xport Room Keys...",
    },
    Item {
        id: MENU_ID_IMPORT_ROOM_KEYS,
        label: "I&mport Room Keys...",
    },
    Item {
        id: MENU_ID_VERIFY_THIS_SESSION,
        label: "&Verify This Session",
    },
    Item {
        id: MENU_ID_TOOLS_SESSIONS,
        label: "Sess&ions",
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
        id: MENU_ID_TOOLS_COLORS,
        label: "&Colors...\tAlt+K",
    },
    Item {
        id: MENU_ID_TOOLS_OPTIONS,
        label: "&Options...\tAlt+O",
    },
    Item {
        id: MENU_ID_TOOLS_ALIASES,
        label: "A&liases...\tAlt+A",
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

/// mIRC's window keys.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum WindowKey {
    /// Alt+1..9: the nth switchbar button.
    Nth(usize),
    /// Ctrl+Tab and Ctrl+Shift+Tab, Ctrl+N and Ctrl+Q.
    Cycle { cycle: WindowCycle, forward: bool },
    /// Alt+X.
    ToggleMaximize,
    /// Ctrl+T: between the treebar and the active window.
    TreebarFocus,
    /// Ctrl+L: scrolls the active window back to its line marker.
    LineMarker,
}

pub(super) fn window_key(command_id: usize) -> Option<WindowKey> {
    let cycle = |cycle, forward| WindowKey::Cycle { cycle, forward };
    Some(match command_id {
        ACCEL_ID_WINDOW_FIRST..=ACCEL_ID_WINDOW_LAST => {
            WindowKey::Nth(command_id - ACCEL_ID_WINDOW_FIRST + 1)
        }
        ACCEL_ID_WINDOW_NEXT => cycle(WindowCycle::All, true),
        ACCEL_ID_WINDOW_PREVIOUS => cycle(WindowCycle::All, false),
        ACCEL_ID_NEXT_CHANNEL => cycle(WindowCycle::Channels, true),
        ACCEL_ID_NEXT_QUERY => cycle(WindowCycle::Queries, true),
        ACCEL_ID_TOGGLE_MAXIMIZE => WindowKey::ToggleMaximize,
        ACCEL_ID_TREEBAR_FOCUS => WindowKey::TreebarFocus,
        ACCEL_ID_LINE_MARKER => WindowKey::LineMarker,
        _ => return None,
    })
}

/// The menubar's menus, left to right. No mnemonic is `E`, `K`, `L`, `O`,
/// `X` or a digit: Alt+E is Select Server, Alt+K Colors, Alt+L List rooms,
/// Alt+O Options, Alt+X maximizes and Alt+1..9 switch windows, as in mIRC.
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
    // `&B` is Ban's.
    CommandsMenuItem {
        label: "Set &Away...",
        command: "away",
        prompt: Some("Away message (blank: back)"),
    },
    CommandsMenuItem {
        label: "Set Ba&ck",
        command: "back",
        prompt: None,
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

/// The nick list's right-click menu, as mIRC's: the items about the user,
/// then the Control submenu (`MEMBER_CONTROL_MENU`).
pub(super) const MEMBER_MENU: &[MenuEntry] = &[
    Item {
        id: MENU_ID_MEMBER_WHOIS,
        label: "Whois",
    },
    Item {
        id: MENU_ID_MEMBER_DEVICES,
        label: "Devices",
    },
    Item {
        id: MENU_ID_MEMBER_VERIFY,
        label: "Verify",
    },
    Item {
        id: MENU_ID_MEMBER_ACCEPT_IDENTITY,
        label: "Accept Identity",
    },
    Item {
        id: MENU_ID_MEMBER_QUERY,
        label: "Query User",
    },
    Item {
        id: MENU_ID_MEMBER_MENTION,
        label: "Mention/Insert Name",
    },
    Separator,
];

pub(super) const MEMBER_CONTROL_MENU: &[MenuEntry] = &[
    Item {
        id: MENU_ID_MEMBER_IGNORE,
        label: "Ignore",
    },
    Item {
        id: MENU_ID_MEMBER_UNIGNORE,
        label: "Unignore",
    },
    Separator,
    Item {
        id: MENU_ID_MEMBER_OP,
        label: "Op",
    },
    Item {
        id: MENU_ID_MEMBER_DEOP,
        label: "Deop",
    },
    Item {
        id: MENU_ID_MEMBER_VOICE,
        label: "Voice",
    },
    Item {
        id: MENU_ID_MEMBER_DEVOICE,
        label: "Devoice",
    },
    Separator,
    Item {
        id: MENU_ID_MEMBER_KICK,
        label: "Kick",
    },
    Item {
        id: MENU_ID_MEMBER_BAN,
        label: "Ban",
    },
];

/// The command a member menu item runs, as if typed with the member's
/// user ID, which names them exactly; `None` for Query and Mention, which
/// act on the window rather than through a command.
pub(super) fn member_menu_command(id: usize, user_id: &str) -> Option<SlashCommand> {
    let nick = user_id.to_owned();
    let rank = |change| {
        Some(SlashCommand::SetRank {
            nick: nick.clone(),
            change,
        })
    };
    match id {
        MENU_ID_MEMBER_QUERY | MENU_ID_MEMBER_MENTION => None,
        MENU_ID_MEMBER_WHOIS => Some(SlashCommand::Whois { nick: nick.clone() }),
        MENU_ID_MEMBER_DEVICES => Some(SlashCommand::Devices {
            target: Some(nick.clone()),
        }),
        MENU_ID_MEMBER_VERIFY => Some(SlashCommand::VerifyStart {
            target: Some(nick.clone()),
            device_id: None,
        }),
        MENU_ID_MEMBER_ACCEPT_IDENTITY => Some(SlashCommand::IdentityAccept {
            target: nick.clone(),
        }),
        MENU_ID_MEMBER_IGNORE => Some(SlashCommand::Ignore {
            target: Some(nick.clone()),
        }),
        MENU_ID_MEMBER_UNIGNORE => Some(SlashCommand::Unignore {
            target: nick.clone(),
        }),
        MENU_ID_MEMBER_OP => rank(RankChange::Op),
        MENU_ID_MEMBER_DEOP => rank(RankChange::Deop),
        MENU_ID_MEMBER_VOICE => rank(RankChange::Voice),
        MENU_ID_MEMBER_DEVOICE => rank(RankChange::Devoice),
        MENU_ID_MEMBER_KICK => Some(SlashCommand::Kick {
            nick: nick.clone(),
            reason: None,
        }),
        MENU_ID_MEMBER_BAN => Some(SlashCommand::Ban {
            nick: nick.clone(),
            reason: None,
        }),
        _ => panic!("{id} is not a member menu item"),
    }
}

/// Whether a member menu item applies to the selected member: the checks
/// its command makes before asking the server. The server decides the
/// rest; a refused op prints in the room window. `identity_warning` is
/// whether any room warns that the member's identity changed.
pub(super) fn member_menu_item_enabled(
    id: usize,
    member: &RoomMemberSummary,
    ignored_users: Option<&[String]>,
    own_user_id: Option<&str>,
    identity_warning: bool,
) -> bool {
    let no_nick = |_: &str| None;
    match id {
        MENU_ID_MEMBER_WHOIS
        | MENU_ID_MEMBER_DEVICES
        | MENU_ID_MEMBER_VERIFY
        | MENU_ID_MEMBER_QUERY
        | MENU_ID_MEMBER_MENTION => true,
        MENU_ID_MEMBER_ACCEPT_IDENTITY => identity_warning,
        MENU_ID_MEMBER_IGNORE => {
            check_ignore(ignored_users, own_user_id, &member.user_id, no_nick).is_ok()
        }
        MENU_ID_MEMBER_UNIGNORE => check_unignore(ignored_users, &member.user_id, no_nick).is_ok(),
        MENU_ID_MEMBER_OP => rank_change_role(RankChange::Op, member).is_ok(),
        MENU_ID_MEMBER_DEOP => rank_change_role(RankChange::Deop, member).is_ok(),
        MENU_ID_MEMBER_VOICE => rank_change_role(RankChange::Voice, member).is_ok(),
        MENU_ID_MEMBER_DEVOICE => rank_change_role(RankChange::Devoice, member).is_ok(),
        MENU_ID_MEMBER_KICK => member.can_kick,
        MENU_ID_MEMBER_BAN => member.can_ban,
        _ => panic!("{id} is not a member menu item"),
    }
}

/// Every member menu item, for enabling them.
pub(super) fn member_menu_item_ids() -> impl Iterator<Item = usize> {
    MEMBER_MENU
        .iter()
        .chain(MEMBER_CONTROL_MENU)
        .filter_map(|entry| match *entry {
            Item { id, .. } => Some(id),
            Separator => None,
        })
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
    let (signed_out, signed_in, waiting, syncing) = (
        connection == ConnectionState::SignedOut,
        connection == ConnectionState::SignedIn,
        connection == ConnectionState::WaitingToReconnect,
        connection == ConnectionState::Syncing,
    );
    FileMenuState {
        connect: signed_out || signed_in || waiting,
        disconnect: super::connection::disconnect_action(connection).is_ok(),
        select_server: signed_out,
        log_out: signed_in || waiting || syncing,
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
            VerificationState::Sent
                | VerificationState::Accepted
                | VerificationState::Ready
                | VerificationState::Confirmed,
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
    use mirx_core::RoomMemberRank;

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
                disconnect: true,
                select_server: false,
                log_out: false,
            }
        );
        assert_eq!(
            file_menu_state(ConnectionState::WaitingForBrowser),
            FileMenuState {
                connect: false,
                disconnect: true,
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
            file_menu_state(ConnectionState::WaitingToReconnect),
            FileMenuState {
                connect: true,
                disconnect: true,
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
        assert_eq!(
            file_menu_state(ConnectionState::LoggingOut),
            FileMenuState {
                connect: false,
                disconnect: true,
                select_server: false,
                log_out: false,
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
        assert_eq!(commands_menu_title(&COMMANDS_MENU[10]), "Change nick");
        let away = &COMMANDS_MENU[5];
        let back = &COMMANDS_MENU[6];
        assert_eq!(commands_menu_title(away), "Set Away");
        assert_eq!(
            commands_menu_line(away, " gone fishing "),
            "/away gone fishing"
        );
        // A blank message is back, as `/away` alone is.
        assert_eq!(commands_menu_line(away, "  "), "/away");
        assert_eq!(commands_menu_line(back, "ignored"), "/back");
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
    fn tools_ends_with_colors_options_and_aliases() {
        assert_eq!(
            TOOLS_MENU[TOOLS_MENU.len() - 4..],
            [
                Separator,
                Item {
                    id: MENU_ID_TOOLS_COLORS,
                    label: "&Colors...\tAlt+K",
                },
                Item {
                    id: MENU_ID_TOOLS_OPTIONS,
                    label: "&Options...\tAlt+O",
                },
                Item {
                    id: MENU_ID_TOOLS_ALIASES,
                    label: "A&liases...\tAlt+A",
                },
            ]
        );
    }

    #[test]
    fn view_items_check_the_shown_bars() {
        let mut settings = Settings::default();
        assert_eq!(
            view_menu_checks(&settings),
            [
                (MENU_ID_VIEW_TOOLBAR, true),
                (MENU_ID_VIEW_SWITCHBAR, true),
                (MENU_ID_VIEW_TREEBAR, true)
            ]
        );
        settings.treebar.show = false;
        settings.toolbar.show = false;
        assert_eq!(
            view_menu_checks(&settings),
            [
                (MENU_ID_VIEW_TOOLBAR, false),
                (MENU_ID_VIEW_SWITCHBAR, true),
                (MENU_ID_VIEW_TREEBAR, false)
            ]
        );
        assert_eq!(view_menu_bar(MENU_ID_VIEW_TREEBAR), Some(Bar::Treebar));
        assert_eq!(view_menu_bar(MENU_ID_VIEW_TOOLBAR), Some(Bar::Toolbar));
        // mIRC's order.
        assert_eq!(
            VIEW_MENU,
            [
                Item {
                    id: MENU_ID_VIEW_TOOLBAR,
                    label: "T&oolbar"
                },
                Item {
                    id: MENU_ID_VIEW_SWITCHBAR,
                    label: "&Switchbar"
                },
                Item {
                    id: MENU_ID_VIEW_TREEBAR,
                    label: "&Treebar"
                },
            ]
        );
        assert_eq!(view_menu_bar(MENU_ID_FIND), None);
        for entry in VIEW_MENU {
            let Item { id, .. } = entry else {
                panic!("View has no separator");
            };
            assert!(view_menu_bar(*id).is_some());
        }
    }

    #[test]
    fn notification_items_map_to_their_modes_and_back() {
        for (mode, id, label) in [
            (
                RoomNotificationMode::AllMessages,
                MENU_ID_ROOM_NOTIFY_ALL,
                "All Messages",
            ),
            (
                RoomNotificationMode::MentionsAndKeywordsOnly,
                MENU_ID_ROOM_NOTIFY_MENTIONS,
                "Mentions and Keywords",
            ),
            (RoomNotificationMode::Mute, MENU_ID_ROOM_NOTIFY_MUTE, "Mute"),
        ] {
            assert_eq!(notification_menu_mode(id), Some(mode));
            assert_eq!(notification_menu_id(mode), id);
            assert_eq!(notification_mode_label(mode), label);
        }
        assert_eq!(notification_menu_mode(MENU_ID_ROOM_PART), None);
    }

    #[test]
    fn menu_ids_are_unique() {
        let mut ids = [
            FILE_MENU,
            VIEW_MENU,
            TOOLS_MENU,
            WINDOW_MENU,
            HELP_MENU,
            MEMBER_MENU,
            MEMBER_CONTROL_MENU,
        ]
        .into_iter()
        .flatten()
        .filter_map(|entry| match entry {
            Item { id, .. } => Some(*id),
            Separator => None,
        })
        .chain((0..COMMANDS_MENU.len()).map(|index| MENU_ID_COMMANDS_FIRST + index))
        .chain(ROOM_NOTIFICATIONS_MENU.iter().map(|(_, id, _)| *id))
        .chain(ACCEL_ID_WINDOW_FIRST..=ACCEL_ID_LINE_MARKER)
        .chain([TOOLBAR_ID_SEND_FILE])
        .collect::<Vec<_>>();
        let count = ids.len();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), count);
        // Clear of the switchbar's and MDI's child ids too.
        assert!(
            ids.last()
                .is_some_and(|id| *id < super::super::SWITCHBAR_COMMAND_FIRST)
        );
    }

    #[test]
    fn window_keys_cover_their_id_range() {
        assert_eq!(window_key(ACCEL_ID_WINDOW_FIRST), Some(WindowKey::Nth(1)));
        assert_eq!(window_key(ACCEL_ID_WINDOW_LAST), Some(WindowKey::Nth(9)));
        assert_eq!(
            window_key(ACCEL_ID_WINDOW_PREVIOUS),
            Some(WindowKey::Cycle {
                cycle: WindowCycle::All,
                forward: false
            })
        );
        assert!((ACCEL_ID_WINDOW_FIRST..=ACCEL_ID_LINE_MARKER).all(|id| window_key(id).is_some()));
        assert_eq!(
            window_key(ACCEL_ID_LINE_MARKER),
            Some(WindowKey::LineMarker)
        );
        assert_eq!(window_key(ACCEL_ID_WINDOW_FIRST - 1), None);
        assert_eq!(window_key(ACCEL_ID_LINE_MARKER + 1), None);
        assert_eq!(window_key(MENU_ID_FIND), None);
    }

    fn member(user_id: &str, rank: RoomMemberRank) -> RoomMemberSummary {
        RoomMemberSummary {
            user_id: user_id.to_owned(),
            nick: user_id[1..user_id.find(':').expect("user ID")].to_owned(),
            nick_ambiguous: false,
            is_own_user: false,
            rank,
            can_kick: false,
            can_ban: false,
        }
    }

    #[test]
    fn member_items_run_the_typed_command_on_the_user_id() {
        let user_id = "@bob:example";
        let nick = || user_id.to_owned();
        for (id, command) in [
            (MENU_ID_MEMBER_QUERY, None),
            (MENU_ID_MEMBER_MENTION, None),
            (
                MENU_ID_MEMBER_WHOIS,
                Some(SlashCommand::Whois { nick: nick() }),
            ),
            (
                MENU_ID_MEMBER_DEVICES,
                Some(SlashCommand::Devices {
                    target: Some(nick()),
                }),
            ),
            (
                MENU_ID_MEMBER_VERIFY,
                Some(SlashCommand::VerifyStart {
                    target: Some(nick()),
                    device_id: None,
                }),
            ),
            (
                MENU_ID_MEMBER_ACCEPT_IDENTITY,
                Some(SlashCommand::IdentityAccept { target: nick() }),
            ),
            (
                MENU_ID_MEMBER_IGNORE,
                Some(SlashCommand::Ignore {
                    target: Some(nick()),
                }),
            ),
            (
                MENU_ID_MEMBER_UNIGNORE,
                Some(SlashCommand::Unignore { target: nick() }),
            ),
            (
                MENU_ID_MEMBER_OP,
                Some(SlashCommand::SetRank {
                    nick: nick(),
                    change: RankChange::Op,
                }),
            ),
            (
                MENU_ID_MEMBER_DEOP,
                Some(SlashCommand::SetRank {
                    nick: nick(),
                    change: RankChange::Deop,
                }),
            ),
            (
                MENU_ID_MEMBER_VOICE,
                Some(SlashCommand::SetRank {
                    nick: nick(),
                    change: RankChange::Voice,
                }),
            ),
            (
                MENU_ID_MEMBER_DEVOICE,
                Some(SlashCommand::SetRank {
                    nick: nick(),
                    change: RankChange::Devoice,
                }),
            ),
            (
                MENU_ID_MEMBER_KICK,
                Some(SlashCommand::Kick {
                    nick: nick(),
                    reason: None,
                }),
            ),
            (
                MENU_ID_MEMBER_BAN,
                Some(SlashCommand::Ban {
                    nick: nick(),
                    reason: None,
                }),
            ),
        ] {
            assert_eq!(member_menu_command(id, user_id), command, "{id}");
        }
        assert_eq!(member_menu_item_ids().count(), 14);
    }

    #[test]
    fn member_items_apply_when_their_command_would() {
        let enabled_items = |member: &RoomMemberSummary, ignored: &[&str]| {
            let ignored = ignored
                .iter()
                .map(|user_id| (*user_id).to_owned())
                .collect::<Vec<_>>();
            member_menu_item_ids()
                .filter(|id| {
                    member_menu_item_enabled(
                        *id,
                        member,
                        Some(&ignored),
                        Some("@me:example"),
                        false,
                    )
                })
                .collect::<Vec<_>>()
        };
        let always = [
            MENU_ID_MEMBER_WHOIS,
            MENU_ID_MEMBER_DEVICES,
            MENU_ID_MEMBER_VERIFY,
            MENU_ID_MEMBER_QUERY,
            MENU_ID_MEMBER_MENTION,
        ];

        let plain = member("@bob:example", RoomMemberRank::Member);
        assert_eq!(
            enabled_items(&plain, &[]),
            [
                &always[..],
                &[
                    MENU_ID_MEMBER_IGNORE,
                    MENU_ID_MEMBER_OP,
                    MENU_ID_MEMBER_VOICE
                ]
            ]
            .concat()
        );

        let mut voiced = member("@bob:example", RoomMemberRank::Voiced);
        voiced.can_kick = true;
        voiced.can_ban = true;
        assert_eq!(
            enabled_items(&voiced, &["@bob:example"]),
            [
                &always[..],
                &[
                    MENU_ID_MEMBER_UNIGNORE,
                    MENU_ID_MEMBER_OP,
                    MENU_ID_MEMBER_DEVOICE,
                    MENU_ID_MEMBER_KICK,
                    MENU_ID_MEMBER_BAN,
                ]
            ]
            .concat()
        );

        let operator = member("@op:example", RoomMemberRank::Moderator);
        assert_eq!(
            enabled_items(&operator, &[]),
            [&always[..], &[MENU_ID_MEMBER_IGNORE, MENU_ID_MEMBER_DEOP]].concat()
        );

        // You cannot ignore or deop yourself.
        let mut me = member("@me:example", RoomMemberRank::Moderator);
        me.is_own_user = true;
        assert_eq!(enabled_items(&me, &[]), always);

        // Neither Ignore nor Unignore applies before the list arrives.
        assert!(!member_menu_item_enabled(
            MENU_ID_MEMBER_IGNORE,
            &plain,
            None,
            Some("@me:example"),
            false
        ));
        assert!(!member_menu_item_enabled(
            MENU_ID_MEMBER_UNIGNORE,
            &plain,
            None,
            Some("@me:example"),
            false
        ));

        // Accept Identity applies exactly while a warning stands.
        for warning in [false, true] {
            assert_eq!(
                member_menu_item_enabled(
                    MENU_ID_MEMBER_ACCEPT_IDENTITY,
                    &plain,
                    Some(&[]),
                    Some("@me:example"),
                    warning
                ),
                warning
            );
        }
    }
}
