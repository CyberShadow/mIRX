//! The editbox's slash commands: parsing typed input into commands, and the
//! command table `/help` lists.
//!
//! Pure code, no Win32 calls. Errors carry the typed command name so the
//! window can print them the way mIRC does: `* /kick: insufficient
//! parameters`.

use std::fmt;

use mirx_core::{
    AwayChanged, MemberPowerLevelChange, MemberPowerRole, PresenceKind, RoomMemberRank,
    RoomMemberSummary, RoomMessagePresentationKind, UserDevices, UserIdentityState, UserProfile,
};

use super::address_book::AddressBookTab;
use super::aliases::Aliases;
use super::colors::PaletteIndex;
use super::nick_colors::{ModeCondition, NickColor};
use super::settings::{Bar, FontSetting, parse_palette_index, parse_points};
use super::timeline_text::{context_line, rank_prefix, topic_is_text};
use super::timestamp::{LocalTimeFn, TimestampFormat};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum ComposeSubmission<'a> {
    Message {
        presentation: RoomMessagePresentationKind,
        body: &'a str,
    },
    LocalCommand(Result<SlashCommand, CommandError>),
    /// `/name args` naming an alias (`aliases.rs`), run by `script.rs`.
    Alias {
        name: &'a str,
        args: &'a str,
    },
}

/// `/notify`'s forms.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum NotifyCommand {
    /// `/notify` or `/notify -l`.
    List,
    /// `/notify [+]<nick> [note]`: adds the user, or replaces their note
    /// and whois; `+` performs a `/whois` when they come on.
    Add {
        target: String,
        note: Option<String>,
        whois: bool,
    },
    /// `/notify -r <nick>`.
    Remove { target: String },
}

/// `/cnick`'s forms.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum NickColorCommand {
    /// `/cnick`: the list.
    List,
    /// `/cnick on|off`.
    Active(bool),
    /// `/cnick [-fany] <nick|@user:server|pattern> <color|*> [modes]`:
    /// updates the first entry with the pattern, or appends one; `force`
    /// (`-f`) always appends.
    Add {
        target: String,
        color: NickColor,
        modes: Option<ModeCondition>,
        notify: bool,
        force: bool,
    },
    /// `/cnick -r <nick|@user:server|pattern|N>`.
    Remove(NickColorTarget),
}

/// The entry `/cnick -r` removes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum NickColorTarget {
    /// The first with this nick's user ID, this ID or this pattern.
    Entry(String),
    /// The Nth, from 1, as `/cnick` numbers them.
    Number(usize),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum SlashCommand {
    Status,
    Find {
        query: String,
    },
    Window {
        target: String,
    },
    Join {
        target: String,
    },
    /// `/create <name>` or `/create -public <#alias> [name]`.
    Create(CreateRoomArgs),
    /// `/list [-s <server>] [match text]`: the Rooms List.
    List {
        server: Option<String>,
        filter: Option<String>,
    },
    /// `/query <nick> [message]`.
    Query {
        target: String,
        message: Option<String>,
    },
    /// `/msg` (Text), `/notice` (Notice) and `/describe` (Emote).
    Msg {
        target: String,
        body: String,
        presentation: RoomMessagePresentationKind,
    },
    Names {
        target: Option<String>,
    },
    Whois {
        nick: String,
    },
    /// `/away <message>`, and `/away` or `/back` (`None`): mark yourself
    /// away or back.
    Away {
        message: Option<String>,
    },
    /// `/devices [nick]`: without a nick, your own sessions.
    Devices {
        target: Option<String>,
    },
    /// `/devices delete <session>`: sign out another of your sessions.
    DevicesDelete {
        device_id: String,
    },
    /// `/devices rename <session> <name>`: rename one of your sessions.
    DevicesRename {
        device_id: String,
        display_name: String,
    },
    /// `/identity accept <nick>`: accept a user's changed identity.
    IdentityAccept {
        target: String,
    },
    /// `/ignore [nick]`: without a nick, lists the ignore list.
    Ignore {
        target: Option<String>,
    },
    Unignore {
        target: String,
    },
    /// `/notify`: mIRC's notify list.
    Notify(NotifyCommand),
    /// `/cnick`: mIRC's nick colour list.
    NickColor(NickColorCommand),
    /// `/channel [room]`: Channel Central for this room or the named one.
    Channel {
        target: Option<String>,
    },
    /// `/abook [-n|-c|-l]`: the Address Book at its Notify, Control or
    /// Colors tab.
    AddressBook {
        tab: AddressBookTab,
    },
    Close,
    Exit,
    /// `/server [server]`: connect, as File > Connect does.
    Server {
        server: Option<String>,
    },
    /// Disconnect without logging out, as File > Disconnect does.
    Quit,
    Kick {
        nick: String,
        reason: Option<String>,
    },
    Ban {
        nick: String,
        reason: Option<String>,
    },
    /// A banned user is no member, so the target is a user ID or a nick
    /// known from another room.
    Unban {
        target: String,
        reason: Option<String>,
    },
    Invite {
        nick: String,
        room: Option<String>,
    },
    /// `/op`, `/deop`, `/voice` and `/devoice`, of one or more nicks.
    SetRank {
        nicks: Vec<String>,
        change: RankChange,
    },
    Nick {
        display_name: String,
    },
    Accept {
        target: Option<String>,
    },
    Reject {
        target: Option<String>,
    },
    Part {
        target: Option<String>,
    },
    React {
        target_event_id: String,
        key: String,
    },
    /// Without a body, arms the editbox to reply.
    Reply {
        target_event_id: String,
        body: Option<String>,
    },
    /// Without a path, opens the Send File dialog.
    SendFile {
        path: Option<String>,
    },
    /// Saves the active window's buffer; without a file, opens the Save
    /// dialog.
    SaveBuf {
        path: Option<String>,
    },
    /// Without a body, arms the editbox with the message to edit.
    Edit {
        target_event_id: String,
        body: Option<String>,
    },
    Redact {
        event_id: String,
    },
    Unreact {
        reaction_event_id: String,
    },
    /// `/topic [#room] [text]`: without text, shows the topic.
    Topic {
        room: Option<String>,
        topic: Option<String>,
    },
    /// `/amsg` (Text) and `/ame` (Emote): to every channel window.
    AllChannels {
        presentation: RoomMessagePresentationKind,
        body: String,
    },
    Clear,
    /// `/clearall [-snq]`.
    ClearAll(ClearAll),
    E2ee(E2eeSlashCommand),
    /// `/verify accept|cancel|confirm|reject`: answers the current flow.
    Verify {
        action: VerificationAction,
    },
    /// `/verify [nick [device]]`: starts a verification. No nick verifies
    /// this session with your other sessions; a nick alone, that user.
    VerifyStart {
        target: Option<String>,
        device_id: Option<String>,
    },
    Help {
        topic: Option<String>,
    },
    /// `/alias [name [command]]`.
    Alias(AliasCommand),
    /// `/echo [colour] [-ast] [#room] <text>`.
    Echo(EchoCommand),
    Timestamp(TimestampCommand),
    Log(LogCommand),
    Font(FontCommand),
    /// `/switchbar` and `/treebar`.
    Bar {
        bar: Bar,
        command: BarCommand,
    },
}

/// Which windows `/clearall` clears, as mIRC's switches choose: `-s`
/// Status, `-n` the channels, `-q` the queries. Without switches, every
/// window with a buffer, invites too.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct ClearAll {
    pub(super) status: bool,
    pub(super) channels: bool,
    pub(super) queries: bool,
    pub(super) invites: bool,
}

/// `/echo`'s line, which only this client shows: mIRC codes in `text`
/// render, `color` (a palette index) colours the rest of the line, and
/// `timestamp` (`-t`) stamps it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct EchoCommand {
    pub(super) color: Option<PaletteIndex>,
    pub(super) place: EchoPlace,
    pub(super) timestamp: bool,
    pub(super) text: String,
}

/// Where `/echo` prints: the active window (`-a`, the default), Status
/// (`-s`), or the window of a room named `#alias` or `!id`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum EchoPlace {
    Active,
    Status,
    Window(String),
}

/// `/alias` lists the aliases, `/alias name` removes one, `/alias name
/// command` sets one. The name is without its `/`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum AliasCommand {
    List,
    Remove(String),
    Set { name: String, command: String },
}

/// `/switchbar [on|off]`, `/treebar [on|off]`: show the setting, or show or
/// hide the bar.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum BarCommand {
    Show,
    On,
    Off,
}

/// `/font [-b] <size> <font name>`, mIRC's `/font -z`: one font for every
/// window. No parameters opens the font dialog.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum FontCommand {
    Dialog,
    Set(FontSetting),
}

/// `/log [on|off] [#room]`: show whether a window logs, or turn its log on
/// or off. No room means the active window.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum LogCommand {
    Show,
    On { target: Option<String> },
    Off { target: Option<String> },
}

/// `/timestamp [on|off|-f format]`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum TimestampCommand {
    Show,
    On,
    Off,
    Format(TimestampFormat),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum E2eeSlashCommand {
    ShowStatus,
    Refresh,
    RecoverDialog,
    /// `/e2ee reset`: the Reset E2EE Identity dialog.
    ResetDialog,
    /// `/e2ee reset cancel`: stop a reset that waits for approval.
    ResetCancel,
    Setup {
        passphrase: Option<String>,
        wait_for_backups_to_upload: bool,
    },
    /// `/e2ee export|import [file]`: the room key file's passphrase dialog;
    /// without a file, the Save or Open dialog first. The passphrase is
    /// never an argument.
    RoomKeys {
        transfer: RoomKeyTransfer,
        path: Option<String>,
    },
}

/// Which way room keys go through a file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum RoomKeyTransfer {
    Export,
    Import,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum RankChange {
    Op,
    Deop,
    Voice,
    Devoice,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum VerificationAction {
    Accept,
    Cancel,
    Confirm,
    Reject,
}

impl SlashCommand {
    /// The command's name as `/help` and error lines spell it.
    pub(super) fn name(&self) -> &'static str {
        match self {
            Self::Status => "status",
            Self::Find { .. } => "find",
            Self::Window { .. } => "window",
            Self::Join { .. } => "join",
            Self::Create(_) => "create",
            Self::List { .. } => "list",
            Self::Query { .. } => "query",
            Self::Msg { presentation, .. } => match presentation {
                RoomMessagePresentationKind::Text => "msg",
                RoomMessagePresentationKind::Notice => "notice",
                RoomMessagePresentationKind::Emote => "describe",
            },
            Self::AllChannels { presentation, .. } => match presentation {
                RoomMessagePresentationKind::Text => "amsg",
                RoomMessagePresentationKind::Emote => "ame",
                RoomMessagePresentationKind::Notice => {
                    unreachable!("there is no notice to every channel")
                }
            },
            Self::Names { .. } => "names",
            Self::Channel { .. } => "channel",
            Self::Whois { .. } => "whois",
            Self::Away { .. } => "away",
            Self::Devices { .. } | Self::DevicesDelete { .. } | Self::DevicesRename { .. } => {
                "devices"
            }
            Self::IdentityAccept { .. } => "identity",
            Self::Ignore { .. } => "ignore",
            Self::Unignore { .. } => "unignore",
            Self::Notify(_) => "notify",
            Self::NickColor(_) => "cnick",
            Self::AddressBook { .. } => "abook",
            Self::Close => "close",
            Self::Exit => "exit",
            Self::Server { .. } => "server",
            Self::Quit => "quit",
            Self::Kick { .. } => "kick",
            Self::Ban { .. } => "ban",
            Self::Unban { .. } => "unban",
            Self::Invite { .. } => "invite",
            Self::SetRank { change, .. } => match change {
                RankChange::Op => "op",
                RankChange::Deop => "deop",
                RankChange::Voice => "voice",
                RankChange::Devoice => "devoice",
            },
            Self::Nick { .. } => "nick",
            Self::Accept { .. } => "accept",
            Self::Reject { .. } => "reject",
            Self::Part { .. } => "part",
            Self::React { .. } => "react",
            Self::Reply { .. } => "reply",
            Self::SendFile { .. } => "sendfile",
            Self::SaveBuf { .. } => "savebuf",
            Self::Edit { .. } => "edit",
            Self::Redact { .. } => "redact",
            Self::Unreact { .. } => "unreact",
            Self::Topic { .. } => "topic",
            Self::Clear => "clear",
            Self::ClearAll(_) => "clearall",
            Self::E2ee(_) => "e2ee",
            Self::Verify { .. } | Self::VerifyStart { .. } => "verify",
            Self::Help { .. } => "help",
            Self::Alias(_) => "alias",
            Self::Echo(_) => "echo",
            Self::Timestamp(_) => "timestamp",
            Self::Log(_) => "log",
            Self::Font(_) => "font",
            Self::Bar { bar, .. } => bar.name(),
        }
    }
}

/// A command that could not run, printed as `* /{command}: {reason}`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct CommandError {
    /// The typed name, lowercased, without the `/`.
    pub(super) command: String,
    pub(super) reason: String,
}

impl CommandError {
    pub(super) fn new(command: impl Into<String>, reason: impl Into<String>) -> Self {
        Self {
            command: command.into(),
            reason: reason.into(),
        }
    }

    pub(super) fn insufficient_parameters(command: impl Into<String>) -> Self {
        Self::new(command, "insufficient parameters")
    }

    fn unknown(command: impl Into<String>) -> Self {
        Self::new(command, "unknown command")
    }
}

impl fmt::Display for CommandError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "* /{}: {}", self.command, self.reason)
    }
}

pub(super) struct CommandSpec {
    pub(super) name: &'static str,
    /// The parameters after the name, empty for none.
    pub(super) usage: &'static str,
    pub(super) summary: &'static str,
}

/// Every command the editbox knows, sorted by name.
pub(super) const COMMANDS: &[CommandSpec] = &[
    CommandSpec {
        name: "accept",
        usage: "[room]",
        summary: "accept a room invite",
    },
    CommandSpec {
        name: "alias",
        usage: "[name [command]]",
        summary: "add, remove or list aliases",
    },
    CommandSpec {
        name: "ame",
        usage: "<action>",
        summary: "send an action to every channel",
    },
    CommandSpec {
        name: "amsg",
        usage: "<text>",
        summary: "send a message to every channel",
    },
    CommandSpec {
        name: "away",
        usage: "[message]",
        summary: "mark yourself away, or back without a message",
    },
    CommandSpec {
        name: "back",
        usage: "",
        summary: "mark yourself back",
    },
    CommandSpec {
        name: "ban",
        usage: "<nick> [reason]",
        summary: "ban a member from this room",
    },
    CommandSpec {
        name: "channel",
        usage: "[#room]",
        summary: "Channel Central: this room's name, topic, bans and settings, or the named room's",
    },
    CommandSpec {
        name: "clear",
        usage: "",
        summary: "clear this window",
    },
    CommandSpec {
        name: "clearall",
        usage: "[-snq]",
        summary: "clear every window, or Status (-s), the channels (-n), the queries (-q)",
    },
    CommandSpec {
        name: "close",
        usage: "",
        summary: "close this window",
    },
    CommandSpec {
        name: "cnick",
        usage: "[-rfany] [on|off|nick color|* [modes]]",
        summary: "colour nicks in the nick list and messages (-r: remove; no nick: list)",
    },
    CommandSpec {
        name: "create",
        usage: "[-public #alias] <name>",
        summary: "create a room: private and encrypted, or public and listed",
    },
    CommandSpec {
        name: "deop",
        usage: "<nick> [nick ...]",
        summary: "take members' operator status",
    },
    CommandSpec {
        name: "describe",
        usage: "<nick|#room> <action>",
        summary: "send an action to a user or room",
    },
    CommandSpec {
        name: "devices",
        usage: "[nick] | delete <session> | rename <session> <name>",
        summary: "list a user's sessions, or sign out or rename yours",
    },
    CommandSpec {
        name: "devoice",
        usage: "<nick> [nick ...]",
        summary: "take members' voice",
    },
    CommandSpec {
        name: "e2ee",
        usage: "[status|refresh|recover|reset [cancel]|setup [wait] [passphrase <passphrase>]|export [file]|import [file]]",
        summary: "show or manage end-to-end encryption",
    },
    CommandSpec {
        name: "echo",
        usage: "[colour] [-ast] [#room] <text>",
        summary: "print a line only you see",
    },
    CommandSpec {
        name: "edit",
        usage: "<event-id> [text]",
        summary: "replace the text of your message",
    },
    CommandSpec {
        name: "exit",
        usage: "",
        summary: "close mirx",
    },
    CommandSpec {
        name: "find",
        usage: "<text>",
        summary: "find text in this window",
    },
    CommandSpec {
        name: "font",
        usage: "[[-b] <size> <font name>]",
        summary: "set the text font, or choose it in a dialog",
    },
    CommandSpec {
        name: "help",
        usage: "[command]",
        summary: "list the commands, or show one's usage",
    },
    CommandSpec {
        name: "identity",
        usage: "accept <nick>",
        summary: "accept a user's changed identity",
    },
    CommandSpec {
        name: "ignore",
        usage: "[nick]",
        summary: "ignore a user's messages (no nick: list ignored users)",
    },
    CommandSpec {
        name: "invite",
        usage: "<nick> [#room]",
        summary: "invite a user to this room, or the named one",
    },
    CommandSpec {
        name: "join",
        usage: "<room-id-or-alias>",
        summary: "join a room",
    },
    CommandSpec {
        name: "kick",
        usage: "<nick> [reason]",
        summary: "kick a member from this room",
    },
    CommandSpec {
        name: "list",
        usage: "[-s server] [match text]",
        summary: "list the rooms in a server's room directory",
    },
    CommandSpec {
        name: "log",
        usage: "[on|off] [#room]",
        summary: "logs a window's lines to a file",
    },
    CommandSpec {
        name: "me",
        usage: "<action>",
        summary: "send an action",
    },
    CommandSpec {
        name: "msg",
        usage: "<nick|#room> <message>",
        summary: "send a message without opening a window",
    },
    CommandSpec {
        name: "names",
        usage: "[#room]",
        summary: "list this room's members, or the named room's",
    },
    CommandSpec {
        name: "nick",
        usage: "<name>",
        summary: "change your display name",
    },
    CommandSpec {
        name: "notice",
        usage: "<nick|#room> <message>",
        summary: "send a notice",
    },
    CommandSpec {
        name: "notify",
        usage: "[-r] [nick [note]]",
        summary: "watch for a user on Matrix (-r: stop; no nick: list)",
    },
    CommandSpec {
        name: "op",
        usage: "<nick> [nick ...]",
        summary: "make members operators (moderators)",
    },
    CommandSpec {
        name: "part",
        usage: "[#room]",
        summary: "leave this room, or the named one",
    },
    CommandSpec {
        name: "query",
        usage: "<nick> [message]",
        summary: "open a query with a user, and send a message there",
    },
    CommandSpec {
        name: "quit",
        usage: "",
        summary: "disconnect (stop syncing) without logging out",
    },
    CommandSpec {
        name: "react",
        usage: "<event-id> <key>",
        summary: "react to a message",
    },
    CommandSpec {
        name: "redact",
        usage: "<event-id>",
        summary: "delete a message",
    },
    CommandSpec {
        name: "reject",
        usage: "[room]",
        summary: "reject a room invite",
    },
    CommandSpec {
        name: "reply",
        usage: "<event-id> [message]",
        summary: "reply to a message",
    },
    CommandSpec {
        name: "savebuf",
        usage: "[file]",
        summary: "save this window's lines to a file (no file: choose one)",
    },
    CommandSpec {
        name: "say",
        usage: "<text>",
        summary: "send text to this window",
    },
    CommandSpec {
        name: "sendfile",
        usage: "[path]",
        summary: "send a file to this room (no path: choose one)",
    },
    CommandSpec {
        name: "server",
        usage: "[server]",
        summary: "connect to the homeserver",
    },
    CommandSpec {
        name: "status",
        usage: "",
        summary: "activate the Status window",
    },
    CommandSpec {
        name: "switchbar",
        usage: "[on|off]",
        summary: "show or hide the switchbar",
    },
    CommandSpec {
        name: "timestamp",
        usage: "[on|off|-f format]",
        summary: "show, turn on or off, or format line timestamps",
    },
    CommandSpec {
        name: "toolbar",
        usage: "[on|off]",
        summary: "show or hide the toolbar",
    },
    CommandSpec {
        name: "topic",
        usage: "[#room] [text]",
        summary: "show or set a room's topic (#alias or !id names the room)",
    },
    CommandSpec {
        name: "treebar",
        usage: "[on|off]",
        summary: "show or hide the treebar",
    },
    CommandSpec {
        name: "unban",
        usage: "<nick> [reason]",
        summary: "lift a user's ban from this room",
    },
    CommandSpec {
        name: "unignore",
        usage: "<nick>",
        summary: "stop ignoring a user",
    },
    CommandSpec {
        name: "unreact",
        usage: "<reaction-event-id>",
        summary: "remove your reaction",
    },
    CommandSpec {
        name: "verify",
        usage: "[nick [device]] | accept|cancel|confirm|reject",
        summary: "verify a session or user, or answer a verification",
    },
    CommandSpec {
        name: "voice",
        usage: "<nick> [nick ...]",
        summary: "give members voice",
    },
    CommandSpec {
        name: "whois",
        usage: "<nick>",
        summary: "show who a user is, from their server",
    },
    CommandSpec {
        name: "window",
        usage: "<room>",
        summary: "activate a room's window",
    },
];

/// What a line typed in the editbox is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum ComposeInput<'a> {
    /// Text, or a command line (`classify_command`).
    Line(ComposeSubmission<'a>),
    /// `//line`: mIRC's evaluated command line, `line` with its
    /// identifiers evaluated and then run as a command (`script.rs`).
    Evaluate(&'a str),
}

/// Classifies editbox input: text, `//line` to evaluate, or a command
/// line (`classify_command`). Text that starts with `/` is sent with
/// `/say`.
pub(super) fn classify_input<'a>(input: &'a str, aliases: &Aliases) -> ComposeInput<'a> {
    let Some(line) = input.strip_prefix('/') else {
        return ComposeInput::Line(ComposeSubmission::Message {
            presentation: RoomMessagePresentationKind::Text,
            body: input,
        });
    };
    if let Some(line) = line.strip_prefix('/') {
        return ComposeInput::Evaluate(line);
    }
    ComposeInput::Line(classify_command(input, |name| aliases.get(name).is_some()))
}

/// Classifies a command line, `/name arguments`: `/!name` is always the
/// built-in `name`; otherwise a name `is_alias` takes is an alias call.
/// The rest are built-ins: `/me` and `/say` are messages, anything else a
/// command.
pub(super) fn classify_command<'a>(
    line: &'a str,
    is_alias: impl Fn(&str) -> bool,
) -> ComposeSubmission<'a> {
    let (name, argument) = split_command(line);
    if let Some(name) = name.strip_prefix('!').filter(|name| !name.is_empty()) {
        return classify_builtin(name, argument);
    }
    if is_alias(name) {
        return ComposeSubmission::Alias {
            name,
            args: argument,
        };
    }
    classify_builtin(name, argument)
}

fn classify_builtin<'a>(name: &str, argument: &'a str) -> ComposeSubmission<'a> {
    let presentation = if name.eq_ignore_ascii_case("me") {
        RoomMessagePresentationKind::Emote
    } else if name.eq_ignore_ascii_case("say") {
        RoomMessagePresentationKind::Text
    } else {
        return ComposeSubmission::LocalCommand(parse_named(name, argument));
    };
    if argument.is_empty() {
        return ComposeSubmission::LocalCommand(Err(CommandError::insufficient_parameters(
            name.to_ascii_lowercase(),
        )));
    }
    ComposeSubmission::Message {
        presentation,
        body: argument,
    }
}

/// The command name after the `/`, and the trimmed argument text.
fn split_command(input: &str) -> (&str, &str) {
    let command_end = input
        .bytes()
        .position(|byte| byte.is_ascii_whitespace())
        .unwrap_or(input.len());
    let argument = input[command_end..].trim_matches(|ch: char| ch.is_ascii_whitespace());
    (&input[1..command_end], argument)
}

/// Splits off the first whitespace-separated word.
fn split_word(text: &str) -> (&str, &str) {
    let word_end = text
        .bytes()
        .position(|byte| byte.is_ascii_whitespace())
        .unwrap_or(text.len());
    (
        &text[..word_end],
        text[word_end..].trim_matches(|ch: char| ch.is_ascii_whitespace()),
    )
}

/// Parses `/name arguments`. The input starts with a single `/`.
pub(super) fn parse(input: &str) -> Result<SlashCommand, CommandError> {
    let (name, argument) = split_command(input);
    parse_named(name, argument)
}

/// Parses the built-in command `name` with its trimmed `argument` text.
fn parse_named(name: &str, argument: &str) -> Result<SlashCommand, CommandError> {
    let name = name.to_ascii_lowercase();
    let required = |argument: &str| {
        if argument.is_empty() {
            Err(CommandError::insufficient_parameters(name.as_str()))
        } else {
            Ok(argument.to_owned())
        }
    };
    let optional = |argument: &str| (!argument.is_empty()).then(|| argument.to_owned());
    let no_parameters = |command: SlashCommand| {
        if argument.is_empty() {
            Ok(command)
        } else {
            Err(CommandError::new(name.as_str(), "takes no parameters"))
        }
    };

    match name.as_str() {
        "status" => no_parameters(SlashCommand::Status),
        "find" => Ok(SlashCommand::Find {
            query: required(argument)?,
        }),
        "window" => Ok(SlashCommand::Window {
            target: required(argument)?,
        }),
        "join" => Ok(SlashCommand::Join {
            target: required(argument)?,
        }),
        "create" => parse_create(argument).map(SlashCommand::Create),
        "list" => parse_list(argument),
        "query" => {
            required(argument)?;
            let (target, message) = split_word(argument);
            Ok(SlashCommand::Query {
                target: target.to_owned(),
                message: optional(message),
            })
        }
        "msg" | "notice" | "describe" => {
            let (target, body) = split_word(argument);
            if body.is_empty() {
                return Err(CommandError::insufficient_parameters(name.as_str()));
            }
            let presentation = match name.as_str() {
                "msg" => RoomMessagePresentationKind::Text,
                "notice" => RoomMessagePresentationKind::Notice,
                _ => RoomMessagePresentationKind::Emote,
            };
            Ok(SlashCommand::Msg {
                target: target.to_owned(),
                body: body.to_owned(),
                presentation,
            })
        }
        "names" => Ok(SlashCommand::Names {
            target: optional(argument),
        }),
        "channel" => Ok(SlashCommand::Channel {
            target: optional(argument),
        }),
        "whois" => Ok(SlashCommand::Whois {
            nick: single_word(&name, argument)?,
        }),
        "away" => Ok(SlashCommand::Away {
            message: optional(argument),
        }),
        "back" => no_parameters(SlashCommand::Away { message: None }),
        "devices" => parse_devices(argument),
        "identity" => parse_identity(argument),
        "ignore" if argument.is_empty() => Ok(SlashCommand::Ignore { target: None }),
        "ignore" => Ok(SlashCommand::Ignore {
            target: Some(single_word(&name, argument)?),
        }),
        "unignore" => Ok(SlashCommand::Unignore {
            target: single_word(&name, argument)?,
        }),
        "notify" => parse_notify(argument).map(SlashCommand::Notify),
        "cnick" => parse_cnick(argument).map(SlashCommand::NickColor),
        "abook" => parse_abook(argument),
        "close" => no_parameters(SlashCommand::Close),
        "exit" => no_parameters(SlashCommand::Exit),
        "server" => Ok(SlashCommand::Server {
            server: optional(argument),
        }),
        // IRC's /quit takes a message; Matrix has none to send.
        "quit" if argument.is_empty() => Ok(SlashCommand::Quit),
        "quit" => Err(CommandError::new("quit", "Matrix has no quit message")),
        "kick" | "ban" | "unban" | "invite" => {
            required(argument)?;
            let (nick, rest) = split_word(argument);
            let (nick, rest) = (nick.to_owned(), optional(rest));
            Ok(match name.as_str() {
                "kick" => SlashCommand::Kick { nick, reason: rest },
                "ban" => SlashCommand::Ban { nick, reason: rest },
                "unban" => SlashCommand::Unban {
                    target: nick,
                    reason: rest,
                },
                _ => SlashCommand::Invite { nick, room: rest },
            })
        }
        "op" | "deop" | "voice" | "devoice" => {
            let change = match name.as_str() {
                "op" => RankChange::Op,
                "deop" => RankChange::Deop,
                "voice" => RankChange::Voice,
                _ => RankChange::Devoice,
            };
            required(argument)?;
            Ok(SlashCommand::SetRank {
                nicks: argument.split_whitespace().map(str::to_owned).collect(),
                change,
            })
        }
        "nick" => Ok(SlashCommand::Nick {
            display_name: required(argument)?,
        }),
        "accept" => Ok(SlashCommand::Accept {
            target: optional(argument),
        }),
        "reject" => Ok(SlashCommand::Reject {
            target: optional(argument),
        }),
        "part" => Ok(SlashCommand::Part {
            target: optional(argument),
        }),
        "react" => {
            let (target_event_id, key) = event_and_text(&name, argument)?;
            Ok(SlashCommand::React {
                target_event_id,
                key,
            })
        }
        "reply" => {
            let (target_event_id, body) = event_and_optional_text(&name, argument)?;
            Ok(SlashCommand::Reply {
                target_event_id,
                body,
            })
        }
        "sendfile" => Ok(SlashCommand::SendFile {
            path: optional(argument),
        }),
        "savebuf" => Ok(SlashCommand::SaveBuf {
            path: optional(argument),
        }),
        "edit" => {
            let (target_event_id, body) = event_and_optional_text(&name, argument)?;
            Ok(SlashCommand::Edit {
                target_event_id,
                body,
            })
        }
        "redact" => Ok(SlashCommand::Redact {
            event_id: single_word(&name, argument)?,
        }),
        "unreact" => Ok(SlashCommand::Unreact {
            reaction_event_id: single_word(&name, argument)?,
        }),
        "topic" => {
            // As in mIRC, a first word that names a room is the room: a
            // topic starting with one needs the room named first.
            let (first, rest) = split_word(argument);
            let (room, topic) = if first.starts_with(['#', '!']) {
                (Some(first.to_owned()), rest)
            } else {
                (None, argument)
            };
            Ok(SlashCommand::Topic {
                room,
                topic: optional(topic),
            })
        }
        "amsg" => Ok(SlashCommand::AllChannels {
            presentation: RoomMessagePresentationKind::Text,
            body: required(argument)?,
        }),
        "ame" => Ok(SlashCommand::AllChannels {
            presentation: RoomMessagePresentationKind::Emote,
            body: required(argument)?,
        }),
        "clear" => no_parameters(SlashCommand::Clear),
        "clearall" => clearall_switches(argument).map(SlashCommand::ClearAll),
        "e2ee" => parse_e2ee(argument).map(SlashCommand::E2ee),
        "verify" => parse_verify(argument),
        "help" => Ok(SlashCommand::Help {
            topic: optional(argument),
        }),
        "echo" => parse_echo(argument).map(SlashCommand::Echo),
        "alias" => {
            let (name, command) = split_word(argument);
            let name = name.strip_prefix('/').unwrap_or(name).to_owned();
            Ok(SlashCommand::Alias(
                match (argument.is_empty(), command.is_empty()) {
                    (true, _) => AliasCommand::List,
                    (false, true) => AliasCommand::Remove(name),
                    (false, false) => AliasCommand::Set {
                        name,
                        command: command.to_owned(),
                    },
                },
            ))
        }
        "timestamp" => parse_timestamp(argument).map(SlashCommand::Timestamp),
        "log" => parse_log(argument).map(SlashCommand::Log),
        "font" => parse_font(argument).map(SlashCommand::Font),
        "switchbar" => parse_bar(Bar::Switchbar, argument),
        "toolbar" => parse_bar(Bar::Toolbar, argument),
        "treebar" => parse_bar(Bar::Treebar, argument),
        _ => Err(CommandError::unknown(name)),
    }
}

/// `[colour] [-switches] [#room|!id] <text>`, mIRC's `/echo` order. With
/// `-a` or `-s` the window is chosen, so a `#` word is text.
fn parse_echo(argument: &str) -> Result<EchoCommand, CommandError> {
    let error = |reason: &str| CommandError::new("echo", reason);
    let (first, rest) = split_word(argument);
    let (color, mut rest) = if !first.is_empty() && first.bytes().all(|byte| byte.is_ascii_digit())
    {
        let color = parse_palette_index(first).map_err(|_| error("colour must be 0-15"))?;
        (Some(color), rest)
    } else {
        (None, argument)
    };
    let (mut active, mut status, mut timestamp) = (false, false, false);
    loop {
        let (word, after) = split_word(rest);
        let Some(letters) = word.strip_prefix('-').filter(|letters| !letters.is_empty()) else {
            break;
        };
        for letter in letters.chars() {
            match letter.to_ascii_lowercase() {
                'a' => active = true,
                's' => status = true,
                't' => timestamp = true,
                _ => return Err(error(&format!("unknown switch -{letter}"))),
            }
        }
        rest = after;
    }
    let place = match (active, status) {
        (true, true) => return Err(error("-a and -s cannot both be given")),
        (true, false) => EchoPlace::Active,
        (false, true) => EchoPlace::Status,
        (false, false) => match split_word(rest) {
            (window, text) if window.starts_with(['#', '!']) => {
                rest = text;
                EchoPlace::Window(window.to_owned())
            }
            _ => EchoPlace::Active,
        },
    };
    if rest.is_empty() {
        return Err(CommandError::insufficient_parameters("echo"));
    }
    Ok(EchoCommand {
        color,
        place,
        timestamp,
        text: rest.to_owned(),
    })
}

/// `/clearall`'s switches, combined (`-sn`) or apart (`-s -n`). mIRC's
/// others name windows mirx does not have.
fn clearall_switches(argument: &str) -> Result<ClearAll, CommandError> {
    if argument.is_empty() {
        return Ok(ClearAll {
            status: true,
            channels: true,
            queries: true,
            invites: true,
        });
    }
    let mut switches = ClearAll {
        status: false,
        channels: false,
        queries: false,
        invites: false,
    };
    for word in argument.split_ascii_whitespace() {
        let letters = word
            .strip_prefix('-')
            .filter(|letters| !letters.is_empty())
            .ok_or_else(|| CommandError::new("clearall", format!("unknown parameter {word}")))?;
        for letter in letters.chars() {
            match letter.to_ascii_lowercase() {
                's' => switches.status = true,
                'n' => switches.channels = true,
                'q' => switches.queries = true,
                _ => {
                    return Err(CommandError::new(
                        "clearall",
                        format!("unknown switch -{letter}"),
                    ));
                }
            }
        }
    }
    Ok(switches)
}

/// What `/create` asks for. The alias's server is checked against the
/// user's own when the command runs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum CreateRoomArgs {
    Private {
        name: String,
    },
    Public {
        alias_localpart: String,
        /// The server the alias named, if it named one.
        alias_server: Option<String>,
        /// `None` names the room after the alias.
        name: Option<String>,
    },
}

/// `<name>` or `-public <#alias> [name]`; the name is the rest of the line.
fn parse_create(argument: &str) -> Result<CreateRoomArgs, CommandError> {
    let (switch, rest) = split_word(argument);
    match switch {
        "" => Err(CommandError::insufficient_parameters("create")),
        "-public" => {
            let (alias, name) = split_word(rest);
            if alias.is_empty() {
                return Err(CommandError::insufficient_parameters("create"));
            }
            let invalid = || CommandError::new("create", format!("invalid alias {alias}"));
            let body = alias.strip_prefix('#').ok_or_else(invalid)?;
            let (localpart, server) = match body.split_once(':') {
                Some((localpart, server)) => (localpart, Some(server)),
                None => (body, None),
            };
            if localpart.is_empty() || server == Some("") {
                return Err(invalid());
            }
            Ok(CreateRoomArgs::Public {
                alias_localpart: localpart.to_owned(),
                alias_server: server.map(str::to_owned),
                name: (!name.is_empty()).then(|| name.to_owned()),
            })
        }
        switch if switch.starts_with('-') => Err(CommandError::new(
            "create",
            format!("unknown switch {switch}"),
        )),
        _ => Ok(CreateRoomArgs::Private {
            name: argument.to_owned(),
        }),
    }
}

/// `[-s <server>] [match text]`. The match text is the rest of the line, as
/// the server matches it; empty lists every room.
fn parse_list(argument: &str) -> Result<SlashCommand, CommandError> {
    let (switch, rest) = split_word(argument);
    let (server, filter) = match switch {
        "-s" => {
            let (server, filter) = split_word(rest);
            if server.is_empty() {
                return Err(CommandError::insufficient_parameters("list"));
            }
            (Some(server.to_owned()), filter)
        }
        switch if switch.starts_with('-') => {
            return Err(CommandError::new(
                "list",
                format!("unknown switch {switch}"),
            ));
        }
        _ => (None, argument),
    };
    Ok(SlashCommand::List {
        server,
        filter: (!filter.is_empty()).then(|| filter.to_owned()),
    })
}

/// `<event-id> <text>`: both parts are required.
fn event_and_text(name: &str, argument: &str) -> Result<(String, String), CommandError> {
    let (event_id, text) = split_word(argument);
    if text.is_empty() {
        return Err(CommandError::insufficient_parameters(name));
    }
    Ok((event_id.to_owned(), text.to_owned()))
}

/// `<event-id> [text]`.
fn event_and_optional_text(
    name: &str,
    argument: &str,
) -> Result<(String, Option<String>), CommandError> {
    let (event_id, text) = split_word(argument);
    if event_id.is_empty() {
        return Err(CommandError::insufficient_parameters(name));
    }
    Ok((
        event_id.to_owned(),
        (!text.is_empty()).then(|| text.to_owned()),
    ))
}

/// Exactly one word.
fn single_word(name: &str, argument: &str) -> Result<String, CommandError> {
    let (word, rest) = split_word(argument);
    if word.is_empty() {
        return Err(CommandError::insufficient_parameters(name));
    }
    if !rest.is_empty() {
        return Err(CommandError::new(name, "too many parameters"));
    }
    Ok(word.to_owned())
}

/// `[nick]`, `delete <session>` or `rename <session> <name>`. A nick
/// named `delete` or `rename` is given by its full ID, as for `/verify`.
fn parse_devices(argument: &str) -> Result<SlashCommand, CommandError> {
    let (action, rest) = split_word(argument);
    let (device_id, more) = split_word(rest);
    match action.to_ascii_lowercase().as_str() {
        "" => Ok(SlashCommand::Devices { target: None }),
        "delete" if device_id.is_empty() || !more.is_empty() => Err(CommandError::new(
            "devices",
            "usage: /devices delete <session>",
        )),
        "delete" => Ok(SlashCommand::DevicesDelete {
            device_id: device_id.to_owned(),
        }),
        // `more` is the name, spaces and all.
        "rename" if more.is_empty() => Err(CommandError::new(
            "devices",
            "usage: /devices rename <session> <name>",
        )),
        "rename" => Ok(SlashCommand::DevicesRename {
            device_id: device_id.to_owned(),
            display_name: more.to_owned(),
        }),
        _ => Ok(SlashCommand::Devices {
            target: Some(single_word("devices", argument)?),
        }),
    }
}

/// `[-l]`, `-r <nick>` or `[+]<nick> [note]`. mIRC's `-s`, `-h`, `-n` and
/// `on`/`off` are not mirx's: a user named `on` is given by their ID.
fn parse_notify(argument: &str) -> Result<NotifyCommand, CommandError> {
    let (first, rest) = split_word(argument);
    match first {
        "" => Ok(NotifyCommand::List),
        "-l" if rest.is_empty() => Ok(NotifyCommand::List),
        "-l" => Err(CommandError::new("notify", "too many parameters")),
        "-r" => Ok(NotifyCommand::Remove {
            target: single_word("notify", rest)?,
        }),
        switch if switch.starts_with('-') => Err(CommandError::new(
            "notify",
            format!("unknown switch {switch}"),
        )),
        target => {
            let (whois, target) = target
                .strip_prefix('+')
                .map_or((false, target), |target| (true, target));
            if target.is_empty() {
                return Err(CommandError::insufficient_parameters("notify"));
            }
            Ok(NotifyCommand::Add {
                target: target.to_owned(),
                note: (!rest.is_empty()).then(|| rest.to_owned()),
                whois,
            })
        }
    }
}

/// mIRC's `/cnick -rfanylNmNsN [on|off|nick] [color] [modes]`, with the
/// switches that mean something on Matrix: `-r` removes (by nick, ID,
/// pattern or number), `-f` adds a new entry rather than updating, `-a`
/// is Any mode, `-n` No mode, `-y` the notify list. mIRC's `-iopv` lists,
/// idle time, method and position are not mirx's. `*` as the nick is any
/// user; a user named `on` or `off` is given by their ID.
fn parse_cnick(argument: &str) -> Result<NickColorCommand, CommandError> {
    let error = |reason: String| CommandError::new("cnick", reason);
    let (first, rest) = split_word(argument);
    let (switches, rest) = match first.strip_prefix('-') {
        Some(switches) => (switches, rest),
        None => ("", argument),
    };
    let (mut remove, mut force, mut any, mut none, mut notify) =
        (false, false, false, false, false);
    for switch in switches.chars() {
        let flag = match switch.to_ascii_lowercase() {
            'r' => &mut remove,
            'f' => &mut force,
            'a' => &mut any,
            'n' => &mut none,
            'y' => &mut notify,
            _ => return Err(error(format!("unknown switch -{switch}"))),
        };
        *flag = true;
    }
    if first == "-" {
        return Err(error("unknown switch -".to_owned()));
    }
    if remove && (force || any || none || notify) {
        return Err(error("-r takes no other switches".to_owned()));
    }
    if remove {
        let target = single_word("cnick", rest)?;
        return Ok(NickColorCommand::Remove(match target.parse::<usize>() {
            Ok(number) => NickColorTarget::Number(number),
            Err(_) => NickColorTarget::Entry(target),
        }));
    }
    let adding = !switches.is_empty();
    match rest.split_ascii_whitespace().collect::<Vec<_>>().as_slice() {
        [] if !adding => Ok(NickColorCommand::List),
        [state] if !adding && state.eq_ignore_ascii_case("on") => {
            Ok(NickColorCommand::Active(true))
        }
        [state] if !adding && state.eq_ignore_ascii_case("off") => {
            Ok(NickColorCommand::Active(false))
        }
        [] | [_] => Err(CommandError::insufficient_parameters("cnick")),
        [target, color, modes @ ..] => {
            let modes = match modes {
                [] => "",
                [modes] => modes,
                _ => return Err(error("too many parameters".to_owned())),
            };
            let modes = ModeCondition::from_parts(modes, any, none).map_err(error)?;
            Ok(NickColorCommand::Add {
                target: (*target).to_owned(),
                color: NickColor::parse(color).map_err(error)?,
                modes,
                notify,
                force,
            })
        }
    }
}

/// `[-n|-c|-l]`: mIRC's switches for the tabs mirx has (`-wh` open tabs it
/// has not); no switch opens Notify. mIRC's nickname, which opens the
/// user's entry, is not mirx's yet.
fn parse_abook(argument: &str) -> Result<SlashCommand, CommandError> {
    let (first, rest) = split_word(argument);
    let tab = match first {
        "" | "-n" => AddressBookTab::Notify,
        "-c" => AddressBookTab::Control,
        "-l" => AddressBookTab::Colors,
        switch if switch.starts_with('-') => {
            return Err(CommandError::new(
                "abook",
                format!("unknown switch {switch}"),
            ));
        }
        _ => return Err(CommandError::new("abook", "too many parameters")),
    };
    if !rest.is_empty() {
        return Err(CommandError::new("abook", "too many parameters"));
    }
    Ok(SlashCommand::AddressBook { tab })
}

/// `accept <nick>`, the one action yet.
fn parse_identity(argument: &str) -> Result<SlashCommand, CommandError> {
    let (action, rest) = split_word(argument);
    match action.to_ascii_lowercase().as_str() {
        "" => Err(CommandError::insufficient_parameters("identity")),
        "accept" => Ok(SlashCommand::IdentityAccept {
            target: single_word("identity", rest)?,
        }),
        _ => Err(CommandError::new("identity", "invalid parameters")),
    }
}

/// The reasons never repeat the argument: it may hold a passphrase.
fn parse_e2ee(argument: &str) -> Result<E2eeSlashCommand, CommandError> {
    let invalid = || CommandError::new("e2ee", "invalid parameters");
    let (action, rest) = split_word(argument);
    match action.to_ascii_lowercase().as_str() {
        "" | "status" if rest.is_empty() => Ok(E2eeSlashCommand::ShowStatus),
        "refresh" if rest.is_empty() => Ok(E2eeSlashCommand::Refresh),
        "recover" if rest.is_empty() => Ok(E2eeSlashCommand::RecoverDialog),
        "reset" if rest.is_empty() => Ok(E2eeSlashCommand::ResetDialog),
        "reset" if rest.eq_ignore_ascii_case("cancel") => Ok(E2eeSlashCommand::ResetCancel),
        "setup" => parse_e2ee_setup(rest).ok_or_else(invalid),
        "export" => parse_e2ee_room_keys(RoomKeyTransfer::Export, rest),
        "import" => parse_e2ee_room_keys(RoomKeyTransfer::Import, rest),
        _ => Err(invalid()),
    }
}

/// `[file]`, the rest of the line, spaces and all. A GUI process has no
/// current directory worth meaning, and a key file must not land, or be
/// looked for, anywhere surprising: a file is a full path.
fn parse_e2ee_room_keys(
    transfer: RoomKeyTransfer,
    argument: &str,
) -> Result<E2eeSlashCommand, CommandError> {
    let path = match argument {
        "" => None,
        path if std::path::Path::new(path).is_absolute() => Some(path.to_owned()),
        _ => {
            return Err(CommandError::new(
                "e2ee",
                "give the file's full path, or none to choose it",
            ));
        }
    };
    Ok(E2eeSlashCommand::RoomKeys { transfer, path })
}

/// `[wait] [passphrase <passphrase>]`.
fn parse_e2ee_setup(argument: &str) -> Option<E2eeSlashCommand> {
    let (keyword, rest) = split_word(argument);
    let (wait_for_backups_to_upload, passphrase_part) = if keyword.eq_ignore_ascii_case("wait") {
        (true, rest)
    } else {
        (false, argument)
    };
    if passphrase_part.is_empty() {
        return Some(E2eeSlashCommand::Setup {
            passphrase: None,
            wait_for_backups_to_upload,
        });
    }

    let (keyword, passphrase) = split_word(passphrase_part);
    if !keyword.eq_ignore_ascii_case("passphrase") || passphrase.is_empty() {
        return None;
    }
    Some(E2eeSlashCommand::Setup {
        passphrase: Some(passphrase.to_owned()),
        wait_for_backups_to_upload,
    })
}

fn parse_font(argument: &str) -> Result<FontCommand, CommandError> {
    if argument.is_empty() {
        return Ok(FontCommand::Dialog);
    }
    let error = |reason: String| CommandError::new("font", reason);
    let (word, rest) = split_word(argument);
    let (bold, argument) = match word {
        "-b" | "-B" => (true, rest),
        _ if word.starts_with('-') => return Err(error(format!("unknown option {word}"))),
        _ => (false, argument),
    };
    let (points, face) = split_word(argument);
    if face.is_empty() {
        return Err(CommandError::insufficient_parameters("font"));
    }
    FontSetting::new(face, parse_points(points).map_err(error)?, bold)
        .map(FontCommand::Set)
        .map_err(error)
}

fn parse_timestamp(argument: &str) -> Result<TimestampCommand, CommandError> {
    let (word, rest) = split_word(argument);
    match word.to_ascii_lowercase().as_str() {
        "" => Ok(TimestampCommand::Show),
        "on" if rest.is_empty() => Ok(TimestampCommand::On),
        "off" if rest.is_empty() => Ok(TimestampCommand::Off),
        "-f" if rest.is_empty() => Err(CommandError::insufficient_parameters("timestamp")),
        "-f" => TimestampFormat::parse(rest)
            .map(TimestampCommand::Format)
            .map_err(|reason| CommandError::new("timestamp", reason)),
        _ => Err(CommandError::new(
            "timestamp",
            "usage: /timestamp [on|off|-f format]",
        )),
    }
}

fn parse_log(argument: &str) -> Result<LogCommand, CommandError> {
    let (word, rest) = split_word(argument);
    let (target, extra) = split_word(rest);
    let target = (!target.is_empty()).then(|| target.to_owned());
    match word.to_ascii_lowercase().as_str() {
        "" => Ok(LogCommand::Show),
        "on" if extra.is_empty() => Ok(LogCommand::On { target }),
        "off" if extra.is_empty() => Ok(LogCommand::Off { target }),
        _ => Err(CommandError::new("log", "usage: /log [on|off] [#room]")),
    }
}

fn parse_bar(bar: Bar, argument: &str) -> Result<SlashCommand, CommandError> {
    let command = match argument.to_ascii_lowercase().as_str() {
        "" => BarCommand::Show,
        "on" => BarCommand::On,
        "off" => BarCommand::Off,
        _ => {
            return Err(CommandError::new(
                bar.name(),
                format!("usage: /{} [on|off]", bar.name()),
            ));
        }
    };
    Ok(SlashCommand::Bar { bar, command })
}

/// `[nick [device]]`, or one of the actions alone. A user named like an
/// action is named by user ID.
fn parse_verify(argument: &str) -> Result<SlashCommand, CommandError> {
    let (first, rest) = split_word(argument);
    let action = match first.to_ascii_lowercase().as_str() {
        "accept" => Some(VerificationAction::Accept),
        "cancel" => Some(VerificationAction::Cancel),
        "confirm" => Some(VerificationAction::Confirm),
        "reject" => Some(VerificationAction::Reject),
        _ => None,
    };
    if let Some(action) = action {
        if !rest.is_empty() {
            return Err(CommandError::new("verify", "invalid parameters"));
        }
        return Ok(SlashCommand::Verify { action });
    }
    let (device, extra) = split_word(rest);
    if !extra.is_empty() {
        return Err(CommandError::new("verify", "too many parameters"));
    }
    let word = |word: &str| (!word.is_empty()).then(|| word.to_owned());
    Ok(SlashCommand::VerifyStart {
        target: word(first),
        device_id: word(device),
    })
}

/// `/help`: every command, or one command's usage.
pub(super) fn help_lines(topic: Option<&str>) -> Result<Vec<String>, String> {
    let Some(topic) = topic else {
        let names = COMMANDS
            .iter()
            .map(|spec| format!("/{}", spec.name))
            .collect::<Vec<_>>()
            .join(" ");
        return Ok(vec![
            format!("* Commands: {names}"),
            "* /help <command> for usage".to_owned(),
        ]);
    };
    let name = topic
        .strip_prefix('/')
        .unwrap_or(topic)
        .to_ascii_lowercase();
    let spec = COMMANDS
        .iter()
        .find(|spec| spec.name == name)
        .ok_or_else(|| format!("no such command: {topic}"))?;
    let usage = if spec.usage.is_empty() {
        String::new()
    } else {
        format!(" {}", spec.usage)
    };
    Ok(vec![format!("* /{}{usage} - {}", spec.name, spec.summary)])
}

/// The member a nick names, resolved as IRC users expect: an exact user ID,
/// else a nick, the name the nick list shows (case-insensitive, as IRC
/// nicks are), else a user ID localpart (`alice` for `@alice:example.org`
/// named Alice). Each step must match exactly one user; a member listed in
/// several rooms is one user. A nameless member's nick is their localpart,
/// so `ghost` names both `@ghost` and a member named `Ghost`.
pub(super) fn resolve_nick<'m>(
    members: impl IntoIterator<Item = &'m RoomMemberSummary>,
    nick: &str,
) -> Result<&'m RoomMemberSummary, String> {
    let members = members.into_iter().collect::<Vec<_>>();
    let folded = nick.to_lowercase();
    let steps: [&dyn Fn(&RoomMemberSummary) -> bool; 3] = [
        &|member| member.user_id == nick,
        &|member| member.nick.to_lowercase() == folded,
        &|member| user_id_localpart(&member.user_id).to_lowercase() == folded,
    ];
    for matches_nick in steps {
        let mut matched = Vec::<&RoomMemberSummary>::new();
        for member in members
            .iter()
            .copied()
            .filter(|member| matches_nick(member))
        {
            if !matched.iter().any(|known| known.user_id == member.user_id) {
                matched.push(member);
            }
        }
        match matched.as_slice() {
            [] => continue,
            [member] => return Ok(member),
            _ => {
                let user_ids = matched
                    .iter()
                    .map(|member| member.user_id.as_str())
                    .collect::<Vec<_>>()
                    .join(", ");
                return Err(format!("{nick} is ambiguous ({user_ids})"));
            }
        }
    }
    Err(format!("no such nick: {nick}"))
}

/// A user a command names: a user ID, which may name someone outside the
/// known rooms, or a known member's nick.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct UserTarget<'m> {
    pub(super) user_id: String,
    pub(super) member: Option<&'m RoomMemberSummary>,
}

pub(super) fn resolve_user<'m>(
    members: impl IntoIterator<Item = &'m RoomMemberSummary>,
    nick: &str,
) -> Result<UserTarget<'m>, String> {
    let members = members.into_iter().collect::<Vec<_>>();
    if is_user_id(nick) {
        return Ok(UserTarget {
            user_id: nick.to_owned(),
            member: members.into_iter().find(|member| member.user_id == nick),
        });
    }
    let member = resolve_nick(members, nick)?;
    Ok(UserTarget {
        user_id: member.user_id.clone(),
        member: Some(member),
    })
}

/// `@localpart:server`, the shape of a Matrix user ID.
pub(super) fn is_user_id(text: &str) -> bool {
    text.strip_prefix('@')
        .and_then(|rest| rest.split_once(':'))
        .is_some_and(|(localpart, server)| !localpart.is_empty() && !server.is_empty())
}

fn user_id_localpart(user_id: &str) -> &str {
    user_id
        .strip_prefix('@')
        .and_then(|rest| rest.split_once(':'))
        .map_or(user_id, |(localpart, _)| localpart)
}

/// `/names`: the members, in nick list order, with their rank prefixes.
pub(super) fn names_line(room_name: &str, members: &[RoomMemberSummary]) -> String {
    let names = members
        .iter()
        .map(|member| format!("{}{}", rank_prefix(member.rank), member.nick))
        .collect::<Vec<_>>()
        .join(" ");
    format!("* Members of {room_name}: {names}")
}

/// `/whois`: the user's ID and profile display name, the rooms shared
/// with them (each with their rank prefix there), their homeserver, then
/// whether they are away and how long idle. The nick is the one a room
/// knows them by, else the display name, else the ID. Offline prints no
/// presence: a server without presence says it of everyone.
pub(super) fn whois_lines(
    profile: &UserProfile,
    room_nick: Option<&str>,
    shared_rooms: &[(RoomMemberRank, &str)],
) -> Vec<String> {
    let nick = room_nick
        .or(profile.display_name.as_deref())
        .unwrap_or(&profile.user_id);
    let mut lines = vec![match &profile.display_name {
        Some(display_name) => format!("* {nick} is {} ({display_name})", profile.user_id),
        None => format!("* {nick} is {}", profile.user_id),
    }];
    if !shared_rooms.is_empty() {
        let rooms = shared_rooms
            .iter()
            .map(|(rank, room_name)| format!("{}{room_name}", rank_prefix(*rank)))
            .collect::<Vec<_>>()
            .join(" ");
        lines.push(format!("* {nick} shares rooms with you: {rooms}"));
    }
    let (_, server) = profile.user_id.split_once(':').unwrap_or_else(|| {
        panic!(
            "a Matrix user ID is @localpart:server, got {}",
            profile.user_id
        )
    });
    lines.push(format!("* {nick}'s homeserver is {server}"));
    if let Some(presence) = &profile.presence
        && presence.kind != PresenceKind::Offline
    {
        if presence.kind == PresenceKind::Unavailable {
            lines.push(away_line(nick, presence.status_msg.as_deref()));
        }
        if let Some(idle_ms) = presence.last_active_ago_ms
            && presence.currently_active != Some(true)
        {
            lines.push(format!(
                "* {nick} has been idle {}",
                duration_text(idle_ms / 1000)
            ));
        }
    }
    lines
}

/// An away user, as mIRC prints it: in `/whois`, and when you message
/// them.
pub(super) fn away_line(nick: &str, status_msg: Option<&str>) -> String {
    match status_msg {
        Some(message) => format!("* {nick} is away: {message}"),
        None => format!("* {nick} is away"),
    }
}

/// mIRC's `$duration`: `1wk 2days 3hrs 4mins 5secs`, without the units
/// that are zero, and `0secs` for none.
pub(super) fn duration_text(secs: u64) -> String {
    const UNITS: [(u64, &str); 5] = [
        (7 * 24 * 3600, "wk"),
        (24 * 3600, "day"),
        (3600, "hr"),
        (60, "min"),
        (1, "sec"),
    ];
    if secs == 0 {
        return "0secs".to_owned();
    }
    let mut rest = secs;
    let mut parts = Vec::new();
    for (unit_secs, unit) in UNITS {
        let count = rest / unit_secs;
        rest %= unit_secs;
        match count {
            0 => {}
            1 => parts.push(format!("1{unit}")),
            count => parts.push(format!("{count}{unit}s")),
        }
    }
    parts.join(" ")
}

/// The answer to `/away` and `/back`, in mIRC's words. An away the
/// server does not show others says why.
pub(super) fn away_lines(changed: &AwayChanged) -> Vec<String> {
    match (&changed.message, changed.seen_as) {
        (None, _) => vec!["* You are no longer marked as being away".to_owned()],
        (Some(_), seen_as) => {
            let mut lines = vec!["* You have been marked as being away".to_owned()];
            match seen_as {
                PresenceKind::Unavailable => {}
                PresenceKind::Online => lines.push(
                    "* Others still see you as online: another of your sessions is active"
                        .to_owned(),
                ),
                PresenceKind::Offline => lines.push(
                    "* Others see you as offline: your server does not share presence".to_owned(),
                ),
            }
            lines
        }
    }
}

/// The status bar after `/away` or `/back`.
pub(super) fn away_status(changed: &AwayChanged) -> String {
    match &changed.message {
        Some(message) => format!("away: {message}"),
        None => "back".to_owned(),
    }
}

/// The answers to `/devices delete` and `/devices rename`.
pub(super) fn device_signed_out_line(device_id: &str) -> String {
    format!("* Session {device_id} signed out")
}

pub(super) fn device_sign_out_approval_line(device_id: &str) -> String {
    format!(
        "* To sign out {device_id}, confirm on the page your browser opened; /devices lists \
         your sessions afterwards"
    )
}

pub(super) fn device_renamed_line(device_id: &str, display_name: &str) -> String {
    format!("* Session {device_id} renamed \"{display_name}\"")
}

/// `/devices`: the user's sessions, `/whois`-style. Another user's list
/// opens with how far you trust their identity and how to verify them;
/// your own list leaves that to the Status window's E2EE lines, which
/// say whether this session is verified. Each session line gives its ID,
/// name, trust and, for yours, when the server last saw it.
/// `identity_changed`: a room's identity stream reports the user's
/// identity changed since it was pinned, which only the stream knows.
pub(super) fn devices_lines(
    devices: &UserDevices,
    nick: &str,
    own_user: bool,
    identity_changed: bool,
    local_time: LocalTimeFn,
) -> Vec<String> {
    let user_id = &devices.user_id;
    let mut lines = vec![if own_user {
        format!("* {nick} sessions:")
    } else {
        match devices.identity {
            UserIdentityState::Verified => format!("* {nick} is verified"),
            UserIdentityState::Unverified if identity_changed => format!(
                "* {nick} changed identity; /identity accept {user_id}, or /verify {user_id}"
            ),
            UserIdentityState::Unverified => {
                format!("* {nick} is not verified; /verify {user_id}")
            }
            UserIdentityState::NoCrossSigning => format!(
                "* {nick} has no cross-signing; verify a device: /verify {user_id} <device>"
            ),
            UserIdentityState::VerificationViolation => format!(
                "* {nick} changed identity since you verified them; /verify {user_id}, or \
                 /identity accept {user_id}"
            ),
        }
    }];
    lines.extend(devices.devices.iter().map(|device| {
        let mut line = format!("* {nick} {}", device.device_id);
        if let Some(display_name) = &device.display_name {
            line.push_str(&format!(" \"{display_name}\""));
        }
        line.push_str(if device.verified {
            " verified"
        } else {
            " unverified"
        });
        if let Some(last_seen_ms) = device.last_seen_ms {
            line.push_str(&format!(
                ", last seen {}",
                local_time(last_seen_ms).asctime()
            ));
        }
        if device.this_session {
            line.push_str(" (this session)");
        }
        line
    }));
    lines
}

/// The power role a rank change gives the member, or why it would change
/// nothing. Demoting yourself is refused: unlike an IRC op, a Matrix user
/// cannot be given their level back by services.
pub(super) fn rank_change_role(
    change: RankChange,
    member: &RoomMemberSummary,
) -> Result<MemberPowerRole, String> {
    let nick = &member.nick;
    let is_operator = member.rank <= RoomMemberRank::Moderator;
    match change {
        RankChange::Op if is_operator => Err(format!("{nick} is already an operator")),
        RankChange::Op => Ok(MemberPowerRole::Moderator),
        RankChange::Deop if member.rank == RoomMemberRank::Creator => {
            Err(format!("{nick} is the room creator"))
        }
        RankChange::Deop if !is_operator => Err(format!("{nick} is not an operator")),
        RankChange::Deop if member.is_own_user => Err("you cannot deop yourself".to_owned()),
        RankChange::Deop => Ok(MemberPowerRole::Default),
        RankChange::Voice if member.rank == RoomMemberRank::Member => Ok(MemberPowerRole::Voiced),
        RankChange::Voice => Err(format!("{nick} already has voice")),
        RankChange::Devoice if member.rank == RoomMemberRank::Voiced => {
            Ok(MemberPowerRole::Default)
        }
        RankChange::Devoice => Err(format!("{nick} is not voiced")),
    }
}

/// The members' new roles for rank changes of nicks in a room's
/// `members`: all or none, the first nick that does not resolve, is named
/// twice or would change nothing refuses the lot.
pub(super) fn member_role_changes<'n>(
    members: &[RoomMemberSummary],
    ranks: impl IntoIterator<Item = (&'n str, RankChange)>,
) -> Result<Vec<MemberPowerLevelChange>, String> {
    let mut changes = Vec::<MemberPowerLevelChange>::new();
    for (nick, change) in ranks {
        let member = resolve_nick(members, nick)?;
        if changes.iter().any(|known| known.user_id == member.user_id) {
            return Err(format!("{} is named twice", member.nick));
        }
        changes.push(MemberPowerLevelChange {
            user_id: member.user_id.clone(),
            role: rank_change_role(change, member)?,
        });
    }
    Ok(changes)
}

/// `/topic` without text: `* Topic is '...'`, or `* No topic is set`.
pub(super) fn topic_line(topic: Option<&str>) -> String {
    match topic {
        Some(topic) => context_line(&topic_is_text(topic)),
        None => context_line("No topic is set"),
    }
}

/// The echo of `/msg` (`-> *nick* text`), `/notice` (`-> -nick- text`)
/// and `/describe` (`-> nick * action`).
pub(super) fn msg_echo(
    presentation: RoomMessagePresentationKind,
    target: &str,
    body: &str,
) -> String {
    match presentation {
        RoomMessagePresentationKind::Text => format!("-> *{target}* {body}"),
        RoomMessagePresentationKind::Notice => format!("-> -{target}- {body}"),
        RoomMessagePresentationKind::Emote => format!("-> {target} * {body}"),
    }
}

/// What Up recalls for a submitted command. Commands that may carry an
/// e2ee passphrase, valid or not, are never kept.
pub(super) fn local_command_history_entry(
    input: &str,
    command: &Result<SlashCommand, CommandError>,
) -> Option<String> {
    match command {
        Ok(SlashCommand::E2ee(E2eeSlashCommand::Setup {
            passphrase: Some(_),
            ..
        })) => None,
        Err(error) if error.command == "e2ee" => None,
        _ => Some(input.to_owned()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mirx_core::UserPresence;

    fn error(command: &str, reason: &str) -> Result<SlashCommand, CommandError> {
        Err(CommandError::new(command, reason))
    }

    fn insufficient(command: &str) -> Result<SlashCommand, CommandError> {
        error(command, "insufficient parameters")
    }

    /// The line `input` is, which must not be `//line`.
    fn line<'a>(input: &'a str, aliases: &Aliases) -> ComposeSubmission<'a> {
        match classify_input(input, aliases) {
            ComposeInput::Line(submission) => submission,
            ComposeInput::Evaluate(line) => panic!("{input:?} evaluates {line:?}"),
        }
    }

    fn classify(input: &str) -> ComposeSubmission<'_> {
        line(input, &Aliases::defaults())
    }

    fn text(body: &str) -> ComposeSubmission<'_> {
        ComposeSubmission::Message {
            presentation: RoomMessagePresentationKind::Text,
            body,
        }
    }

    #[test]
    fn server_and_quit_parse_like_mirc() {
        assert_eq!(parse("/server"), Ok(SlashCommand::Server { server: None }));
        assert_eq!(
            parse("/server matrix.org"),
            Ok(SlashCommand::Server {
                server: Some("matrix.org".to_owned())
            })
        );
        assert_eq!(parse("/quit"), Ok(SlashCommand::Quit));
        assert_eq!(
            parse("/quit bye").unwrap_err().to_string(),
            "* /quit: Matrix has no quit message"
        );
        assert_eq!(
            help_lines(Some("server")),
            Ok(vec![
                "* /server [server] - connect to the homeserver".to_owned()
            ])
        );
        assert_eq!(
            help_lines(Some("quit")),
            Ok(vec![
                "* /quit - disconnect (stop syncing) without logging out".to_owned()
            ])
        );
    }

    #[test]
    fn command_error_displays_like_mirc() {
        assert_eq!(
            CommandError::insufficient_parameters("kick").to_string(),
            "* /kick: insufficient parameters"
        );
        assert_eq!(
            CommandError::unknown("foo").to_string(),
            "* /foo: unknown command"
        );
    }

    #[test]
    fn classify_input_distinguishes_text_emote_evaluated_lines_and_commands() {
        assert_eq!(
            classify("hello"),
            ComposeSubmission::Message {
                presentation: RoomMessagePresentationKind::Text,
                body: "hello",
            }
        );
        for input in ["/me waves", "/ME    waves"] {
            assert_eq!(
                classify(input),
                ComposeSubmission::Message {
                    presentation: RoomMessagePresentationKind::Emote,
                    body: "waves",
                }
            );
        }
        for input in ["/me", "/me    "] {
            assert_eq!(
                classify(input),
                ComposeSubmission::LocalCommand(insufficient("me"))
            );
        }
        for (input, line) in [
            ("//me waves", "me waves"),
            ("//echo -a $me", "echo -a $me"),
            ("//", ""),
            ("///status", "/status"),
        ] {
            assert_eq!(
                classify_input(input, &Aliases::defaults()),
                ComposeInput::Evaluate(line),
                "{input}"
            );
        }
        assert_eq!(
            classify("/status"),
            ComposeSubmission::LocalCommand(Ok(SlashCommand::Status))
        );
        assert_eq!(
            classify("/"),
            ComposeSubmission::LocalCommand(error("", "unknown command"))
        );
    }

    #[test]
    fn aliases_come_before_built_ins_except_after_a_bang() {
        let aliases =
            Aliases::from_source(vec!["/j /join #$$1".to_owned(), "/me say I am".to_owned()])
                .unwrap();
        let classify = |input| line(input, &aliases);
        assert_eq!(
            classify("/j #ops"),
            ComposeSubmission::Alias {
                name: "j",
                args: "#ops"
            }
        );
        assert_eq!(
            classify("/J   #ops  x "),
            ComposeSubmission::Alias {
                name: "J",
                args: "#ops  x"
            }
        );
        assert_eq!(
            classify("/!j x"),
            ComposeSubmission::LocalCommand(error("j", "unknown command"))
        );
        assert_eq!(
            classify("/!JOIN x"),
            ComposeSubmission::LocalCommand(Ok(SlashCommand::Join {
                target: "x".to_owned()
            }))
        );
        assert_eq!(
            classify("/me hi"),
            ComposeSubmission::Alias {
                name: "me",
                args: "hi"
            }
        );
        assert_eq!(
            classify("/!me waves"),
            ComposeSubmission::Message {
                presentation: RoomMessagePresentationKind::Emote,
                body: "waves",
            }
        );
        for input in ["/!", "/! j"] {
            assert_eq!(
                classify(input),
                ComposeSubmission::LocalCommand(error("!", "unknown command")),
                "{input}"
            );
        }
        for input in ["/say hi there", "/SAY   hi there"] {
            assert_eq!(classify(input), text("hi there"), "{input}");
        }
        assert_eq!(classify("/say /not a command"), text("/not a command"));
        assert_eq!(
            classify("/say"),
            ComposeSubmission::LocalCommand(insufficient("say"))
        );
        assert_eq!(classify("plain"), text("plain"));
        // An alias is called only with is_alias's leave.
        assert_eq!(
            classify_command("/j x", |_| false),
            ComposeSubmission::LocalCommand(error("j", "unknown command"))
        );
    }

    #[test]
    fn echo_takes_mircs_colour_switches_and_window() {
        let echo = |color: Option<u8>, place: EchoPlace, timestamp, text: &str| {
            Ok(SlashCommand::Echo(EchoCommand {
                color: color.map(|index| PaletteIndex::new(index).unwrap()),
                place,
                timestamp,
                text: text.to_owned(),
            }))
        };
        let window = |name: &str| EchoPlace::Window(name.to_owned());
        assert_eq!(
            parse("/echo hi  there"),
            echo(None, EchoPlace::Active, false, "hi  there")
        );
        assert_eq!(
            parse("/echo 4 hi"),
            echo(Some(4), EchoPlace::Active, false, "hi")
        );
        assert_eq!(
            parse("/echo 007 hi"),
            echo(Some(7), EchoPlace::Active, false, "hi")
        );
        assert_eq!(
            parse("/echo -a hi"),
            echo(None, EchoPlace::Active, false, "hi")
        );
        for input in ["/echo -s hi", "/echo -S hi"] {
            assert_eq!(
                parse(input),
                echo(None, EchoPlace::Status, false, "hi"),
                "{input}"
            );
        }
        for input in ["/echo -st hi", "/echo -s -t hi", "/echo -ts hi"] {
            assert_eq!(
                parse(input),
                echo(None, EchoPlace::Status, true, "hi"),
                "{input}"
            );
        }
        assert_eq!(
            parse("/echo #ops hi"),
            echo(None, window("#ops"), false, "hi")
        );
        assert_eq!(
            parse("/echo 4 -t !id:x hi"),
            echo(Some(4), window("!id:x"), true, "hi")
        );
        // A chosen window: the word is text.
        assert_eq!(
            parse("/echo -s #ops hi"),
            echo(None, EchoPlace::Status, false, "#ops hi")
        );
        assert_eq!(parse("/echo -t"), insufficient("echo"));
        assert_eq!(parse("/echo"), insufficient("echo"));
        assert_eq!(parse("/echo #ops"), insufficient("echo"));
        assert_eq!(parse("/echo 16 hi"), error("echo", "colour must be 0-15"));
        assert_eq!(
            parse("/echo -as hi"),
            error("echo", "-a and -s cannot both be given")
        );
        assert_eq!(parse("/echo -e hi"), error("echo", "unknown switch -e"));
        assert_eq!(parse("/echo 4"), insufficient("echo"));
    }

    #[test]
    fn alias_lists_removes_or_sets() {
        let alias = |command| Ok(SlashCommand::Alias(command));
        assert_eq!(parse("/alias"), alias(AliasCommand::List));
        assert_eq!(
            parse("/ALIAS j"),
            alias(AliasCommand::Remove("j".to_owned()))
        );
        assert_eq!(
            parse("/alias /j /join #$$1"),
            alias(AliasCommand::Set {
                name: "j".to_owned(),
                command: "/join #$$1".to_owned()
            })
        );
        assert_eq!(
            parse("/alias x { say a | say b }"),
            alias(AliasCommand::Set {
                name: "x".to_owned(),
                command: "{ say a | say b }".to_owned()
            })
        );
    }

    #[test]
    fn parse_accepts_status_find_e2ee_and_verify_forms() {
        assert_eq!(parse("/StAtUs"), Ok(SlashCommand::Status));
        assert_eq!(
            parse("/FIND Mixed Case"),
            Ok(SlashCommand::Find {
                query: "Mixed Case".to_owned(),
            })
        );
        for input in ["/e2ee", "/e2ee status"] {
            assert_eq!(
                parse(input),
                Ok(SlashCommand::E2ee(E2eeSlashCommand::ShowStatus))
            );
        }
        assert_eq!(
            parse("/e2ee refresh"),
            Ok(SlashCommand::E2ee(E2eeSlashCommand::Refresh))
        );
        assert_eq!(
            parse("/e2ee recover"),
            Ok(SlashCommand::E2ee(E2eeSlashCommand::RecoverDialog))
        );
        assert_eq!(
            parse("/e2ee reset"),
            Ok(SlashCommand::E2ee(E2eeSlashCommand::ResetDialog))
        );
        assert_eq!(
            parse("/E2EE RESET Cancel"),
            Ok(SlashCommand::E2ee(E2eeSlashCommand::ResetCancel))
        );
        for (input, passphrase, wait) in [
            ("/e2ee setup", None, false),
            ("/e2ee setup wait", None, true),
            (
                "/e2ee setup passphrase secret phrase",
                Some("secret phrase"),
                false,
            ),
            (
                "/e2ee setup wait passphrase secret phrase",
                Some("secret phrase"),
                true,
            ),
        ] {
            assert_eq!(
                parse(input),
                Ok(SlashCommand::E2ee(E2eeSlashCommand::Setup {
                    passphrase: passphrase.map(str::to_owned),
                    wait_for_backups_to_upload: wait,
                })),
                "{input}"
            );
        }
        for (input, transfer, path) in [
            ("/e2ee export", RoomKeyTransfer::Export, None),
            (
                "/e2ee export C:\\a b\\k.txt",
                RoomKeyTransfer::Export,
                Some("C:\\a b\\k.txt"),
            ),
            ("/E2EE IMPORT", RoomKeyTransfer::Import, None),
            (
                "/e2ee import  D:\\k.txt ",
                RoomKeyTransfer::Import,
                Some("D:\\k.txt"),
            ),
        ] {
            assert_eq!(
                parse(input),
                Ok(SlashCommand::E2ee(E2eeSlashCommand::RoomKeys {
                    transfer,
                    path: path.map(str::to_owned),
                })),
                "{input}"
            );
        }
        for (input, action) in [
            ("/verify accept", VerificationAction::Accept),
            ("/verify cancel", VerificationAction::Cancel),
            ("/verify confirm", VerificationAction::Confirm),
            ("/verify reject", VerificationAction::Reject),
        ] {
            assert_eq!(parse(input), Ok(SlashCommand::Verify { action }));
        }
        let start = |target: Option<&str>, device_id: Option<&str>| {
            Ok(SlashCommand::VerifyStart {
                target: target.map(str::to_owned),
                device_id: device_id.map(str::to_owned),
            })
        };
        assert_eq!(parse("/verify"), start(None, None));
        assert_eq!(parse("/verify  Bob "), start(Some("Bob"), None));
        assert_eq!(parse("/verify maybe"), start(Some("maybe"), None));
        assert_eq!(
            parse("/verify @bob:example DEV1"),
            start(Some("@bob:example"), Some("DEV1"))
        );
        assert_eq!(
            parse("/verify @accept:example"),
            start(Some("@accept:example"), None)
        );
    }

    #[test]
    fn parse_rejects_unknown_commands_and_malformed_parameters() {
        assert_eq!(parse("/wat now"), error("wat", "unknown command"));
        assert_eq!(parse("/WAT"), error("wat", "unknown command"));
        assert_eq!(parse("/status now"), error("status", "takes no parameters"));
        assert_eq!(parse("/clear now"), error("clear", "takes no parameters"));
        assert_eq!(parse("/find"), insufficient("find"));
        assert_eq!(parse("/window"), insufficient("window"));
        assert_eq!(
            parse("/verify accept extra"),
            error("verify", "invalid parameters")
        );
        assert_eq!(
            parse("/verify bob DEV1 more"),
            error("verify", "too many parameters")
        );
        for input in [
            "/e2ee recover secret",
            "/e2ee reset secret",
            "/e2ee reset cancel secret",
            "/e2ee bogus",
            "/e2ee setup passphrase",
            "/e2ee setup wait passphrases secret phrase",
            "/e2ee setup secret phrase",
        ] {
            let rejected = parse(input);
            assert_eq!(rejected, error("e2ee", "invalid parameters"), "{input}");
            assert!(!rejected.unwrap_err().to_string().contains("secret"));
        }
        // A room key file is a full path; the reason does not repeat the
        // word, which may be a passphrase typed in the wrong place.
        for input in [
            "/e2ee export secret",
            "/e2ee import secret\\keys.txt",
            "/e2ee export \\secret.txt",
            "/e2ee import C:secret.txt",
        ] {
            let rejected = parse(input);
            assert_eq!(
                rejected,
                error("e2ee", "give the file's full path, or none to choose it"),
                "{input}"
            );
            assert!(!rejected.unwrap_err().to_string().contains("secret"));
        }
    }

    #[test]
    fn parse_create_takes_a_name_or_a_public_alias_and_name() {
        let create = |args| Ok(SlashCommand::Create(args));
        let public = |localpart: &str, server: Option<&str>, name: Option<&str>| {
            create(CreateRoomArgs::Public {
                alias_localpart: localpart.to_owned(),
                alias_server: server.map(str::to_owned),
                name: name.map(str::to_owned),
            })
        };
        assert_eq!(
            parse("/create Team room"),
            create(CreateRoomArgs::Private {
                name: "Team room".to_owned()
            })
        );
        assert_eq!(parse("/create -public #foo"), public("foo", None, None));
        assert_eq!(
            parse("/create -public #foo:ex Foo Bar"),
            public("foo", Some("ex"), Some("Foo Bar"))
        );
        assert_eq!(parse("/create"), insufficient("create"));
        assert_eq!(parse("/create -public"), insufficient("create"));
        for alias in ["foo", "#", "#:ex", "#foo:"] {
            assert_eq!(
                parse(&format!("/create -public {alias} Foo")),
                error("create", &format!("invalid alias {alias}")),
                "{alias}"
            );
        }
        assert_eq!(
            parse("/create -x Foo"),
            error("create", "unknown switch -x")
        );
    }

    #[test]
    fn parse_list_takes_a_server_switch_and_match_text() {
        let list = |server: Option<&str>, filter: Option<&str>| {
            Ok(SlashCommand::List {
                server: server.map(str::to_owned),
                filter: filter.map(str::to_owned),
            })
        };
        assert_eq!(parse("/list"), list(None, None));
        assert_eq!(parse("/list   "), list(None, None));
        assert_eq!(parse("/list rust lang"), list(None, Some("rust lang")));
        assert_eq!(parse("/list -s matrix.org"), list(Some("matrix.org"), None));
        assert_eq!(
            parse("/list -s matrix.org  rust lang "),
            list(Some("matrix.org"), Some("rust lang"))
        );
        assert_eq!(parse("/list -s"), insufficient("list"));
        assert_eq!(parse("/list -s   "), insufficient("list"));
        assert_eq!(parse("/list -x rust"), error("list", "unknown switch -x"));
    }

    #[test]
    fn parse_accepts_room_and_message_command_forms() {
        assert_eq!(
            parse("/join #ops:example.org"),
            Ok(SlashCommand::Join {
                target: "#ops:example.org".to_owned(),
            })
        );
        assert_eq!(
            parse("/join !room:example.org"),
            Ok(SlashCommand::Join {
                target: "!room:example.org".to_owned(),
            })
        );
        for input in [
            "/query @alice:example.org",
            "/query   @alice:example.org   ",
        ] {
            assert_eq!(
                parse(input),
                Ok(SlashCommand::Query {
                    target: "@alice:example.org".to_owned(),
                    message: None,
                })
            );
        }
        assert_eq!(
            parse("/find   two   words   here   "),
            Ok(SlashCommand::Find {
                query: "two   words   here".to_owned(),
            })
        );
        assert_eq!(parse("/part"), Ok(SlashCommand::Part { target: None }));
        assert_eq!(
            parse("/part !room:example.org"),
            Ok(SlashCommand::Part {
                target: Some("!room:example.org".to_owned()),
            })
        );
        for (input, key) in [
            ("/react $react-normal +1", "+1"),
            ("/react $react-normal eyes", "eyes"),
            ("/react $react-normal two words", "two words"),
        ] {
            assert_eq!(
                parse(input),
                Ok(SlashCommand::React {
                    target_event_id: "$react-normal".to_owned(),
                    key: key.to_owned(),
                })
            );
        }
        for input in [
            "/edit $event-to-edit replacement text",
            "/EDIT   $event-to-edit   replacement text   ",
        ] {
            assert_eq!(
                parse(input),
                Ok(SlashCommand::Edit {
                    target_event_id: "$event-to-edit".to_owned(),
                    body: Some("replacement text".to_owned()),
                })
            );
        }
        for input in [
            "/reply $event-to-reply hello there",
            "/REPLY   $event-to-reply   hello there   ",
        ] {
            assert_eq!(
                parse(input),
                Ok(SlashCommand::Reply {
                    target_event_id: "$event-to-reply".to_owned(),
                    body: Some("hello there".to_owned()),
                })
            );
        }
        for input in ["/reply $event-to-reply", "/reply   $event-to-reply   "] {
            assert_eq!(
                parse(input),
                Ok(SlashCommand::Reply {
                    target_event_id: "$event-to-reply".to_owned(),
                    body: None,
                })
            );
        }
        for input in ["/edit $event-to-edit", "/edit   $event-to-edit   "] {
            assert_eq!(
                parse(input),
                Ok(SlashCommand::Edit {
                    target_event_id: "$event-to-edit".to_owned(),
                    body: None,
                })
            );
        }
        assert_eq!(
            parse("/unreact $reaction-alice-plus-one"),
            Ok(SlashCommand::Unreact {
                reaction_event_id: "$reaction-alice-plus-one".to_owned(),
            })
        );
        assert_eq!(
            parse("/redact $redaction-target"),
            Ok(SlashCommand::Redact {
                event_id: "$redaction-target".to_owned(),
            })
        );
        let topic = |room: Option<&str>, topic: Option<&str>| {
            Ok(SlashCommand::Topic {
                room: room.map(str::to_owned),
                topic: topic.map(str::to_owned),
            })
        };
        for (input, expected) in [
            (
                "/topic incident review",
                topic(None, Some("incident review")),
            ),
            (
                "/topic   incident review   ",
                topic(None, Some("incident review")),
            ),
            ("/topic", topic(None, None)),
            ("/topic   ", topic(None, None)),
            ("/topic #a", topic(Some("#a"), None)),
            (
                "/topic !r:x text here",
                topic(Some("!r:x"), Some("text here")),
            ),
            ("/topic hello #world", topic(None, Some("hello #world"))),
            (
                "/topic #ops #1 priority",
                topic(Some("#ops"), Some("#1 priority")),
            ),
        ] {
            assert_eq!(parse(input), expected, "{input}");
        }
        assert_eq!(
            parse("/sendfile C:\\some file.txt"),
            Ok(SlashCommand::SendFile {
                path: Some("C:\\some file.txt".to_owned()),
            })
        );
        for input in ["/sendfile", "/sendfile   "] {
            assert_eq!(parse(input), Ok(SlashCommand::SendFile { path: None }));
        }
        assert_eq!(
            parse("/savebuf saved ops.txt"),
            Ok(SlashCommand::SaveBuf {
                path: Some("saved ops.txt".to_owned()),
            })
        );
        assert_eq!(parse("/savebuf"), Ok(SlashCommand::SaveBuf { path: None }));
    }

    #[test]
    fn parse_accepts_accept_and_reject_forms() {
        for input in ["/accept", "/accept    "] {
            assert_eq!(parse(input), Ok(SlashCommand::Accept { target: None }));
        }
        assert_eq!(
            parse("/accept !room:example.org"),
            Ok(SlashCommand::Accept {
                target: Some("!room:example.org".to_owned()),
            })
        );
        assert_eq!(
            parse("/accept Invite Name"),
            Ok(SlashCommand::Accept {
                target: Some("Invite Name".to_owned()),
            })
        );
        assert_eq!(parse("/reject"), Ok(SlashCommand::Reject { target: None }));
        assert_eq!(
            parse("/reject Invite Name"),
            Ok(SlashCommand::Reject {
                target: Some("Invite Name".to_owned()),
            })
        );
    }

    #[test]
    fn parse_rejects_missing_or_extra_parameters() {
        for (input, command) in [
            ("/join", "join"),
            ("/query", "query"),
            ("/query   ", "query"),
            ("/find   ", "find"),
            ("/react", "react"),
            ("/react $react-normal", "react"),
            ("/react $react-normal   ", "react"),
            ("/edit", "edit"),
            ("/edit   ", "edit"),
            ("/reply", "reply"),
            ("/reply   ", "reply"),
            ("/unreact", "unreact"),
            ("/redact", "redact"),
            ("/redact   ", "redact"),
        ] {
            assert_eq!(parse(input), insufficient(command), "{input}");
        }
        assert_eq!(
            parse("/unreact $one $two"),
            error("unreact", "too many parameters")
        );
        assert_eq!(
            parse("/redact $event extra"),
            error("redact", "too many parameters")
        );
    }

    #[test]
    fn e2ee_passphrase_inputs_are_never_history_entries() {
        for input in [
            "/e2ee recover secret phrase",
            "/e2ee setup wait passphrases secret phrase",
            "/e2ee setup wait passphrase secret phrase",
            "/e2ee setup passphrase secret phrase",
        ] {
            let ComposeSubmission::LocalCommand(command) = classify(input) else {
                panic!("expected a command: {input}");
            };
            assert_eq!(
                local_command_history_entry(input, &command),
                None,
                "{input}"
            );
        }
        for input in ["/e2ee setup wait", "/e2ee", "/kick", "/find needle"] {
            let ComposeSubmission::LocalCommand(command) = classify(input) else {
                panic!("expected a command: {input}");
            };
            assert_eq!(
                local_command_history_entry(input, &command),
                Some(input.to_owned()),
                "{input}"
            );
        }
    }

    #[test]
    fn commands_table_is_sorted_and_every_entry_parses() {
        assert!(
            COMMANDS.windows(2).all(|pair| pair[0].name < pair[1].name),
            "COMMANDS must be sorted by name without duplicates"
        );
        for spec in COMMANDS {
            let input = format!("/{}", spec.name);
            if let ComposeSubmission::LocalCommand(Err(error)) = classify(&input) {
                assert_ne!(error.reason, "unknown command", "{input}");
            }
        }
    }

    #[test]
    fn every_command_variant_is_in_the_commands_table() {
        // One sample per variant; `SlashCommand::name` is exhaustive, so a
        // new variant needs a name, and a sample here.
        let samples = [
            "/status",
            "/find x",
            "/window x",
            "/join x",
            "/query x",
            "/accept",
            "/reject",
            "/part",
            "/react $e k",
            "/reply $e t",
            "/sendfile p",
            "/savebuf f",
            "/edit $e t",
            "/redact $e",
            "/unreact $e",
            "/topic t",
            "/clear",
            "/e2ee",
            "/verify accept",
            "/help",
            "/msg x t",
            "/notice x t",
            "/names",
            "/channel",
            "/whois x",
            "/away x",
            "/back",
            "/amsg x",
            "/ame x",
            "/describe x y",
            "/clearall",
            "/close",
            "/exit",
            "/kick x",
            "/ban x",
            "/unban x",
            "/invite x",
            "/op x",
            "/deop x",
            "/voice x",
            "/devoice x",
            "/nick x",
            "/alias",
            "/echo x",
        ];
        for input in samples {
            let name = parse(input).expect(input).name();
            assert!(
                COMMANDS.iter().any(|spec| spec.name == name),
                "{name} is missing from COMMANDS"
            );
        }
    }

    #[test]
    fn font_opens_the_dialog_or_sets_the_font() {
        let font = |face, points, bold| {
            Ok(SlashCommand::Font(FontCommand::Set(
                FontSetting::new(face, points, bold).unwrap(),
            )))
        };
        assert_eq!(parse("/font"), Ok(SlashCommand::Font(FontCommand::Dialog)));
        assert_eq!(parse("/font 9 Fixedsys"), font("Fixedsys", 9, false));
        assert_eq!(
            parse("/font -b 12 Courier New"),
            font("Courier New", 12, true)
        );
        assert_eq!(parse("/FONT -B 72  Courier"), font("Courier", 72, true));
        for (input, reason) in [
            ("/font -x 9 A", "unknown option -x"),
            ("/font 0 A", "size must be 1-72"),
            ("/font 99 A", "size must be 1-72"),
            ("/font abc Foo", "size must be 1-72"),
            ("/font 9", "insufficient parameters"),
            ("/font -b", "insufficient parameters"),
            ("/font -b 9", "insufficient parameters"),
            ("/font 9 A,B", "font name cannot contain a comma"),
        ] {
            assert_eq!(parse(input), error("font", reason), "{input}");
        }
        assert_eq!(
            help_lines(Some("font")),
            Ok(vec![
                "* /font [[-b] <size> <font name>] - set the text font, or choose it in a dialog"
                    .to_owned()
            ])
        );
    }

    #[test]
    fn bars_show_or_toggle() {
        for (name, bar) in [
            ("toolbar", Bar::Toolbar),
            ("switchbar", Bar::Switchbar),
            ("treebar", Bar::Treebar),
        ] {
            let command = |command| Ok(SlashCommand::Bar { bar, command });
            assert_eq!(parse(&format!("/{name}")), command(BarCommand::Show));
            assert_eq!(parse(&format!("/{name} on")), command(BarCommand::On));
            assert_eq!(
                parse(&format!("/{} OFF", name.to_ascii_uppercase())),
                command(BarCommand::Off)
            );
            for argument in ["maybe", "on now"] {
                assert_eq!(
                    parse(&format!("/{name} {argument}")),
                    error(name, &format!("usage: /{name} [on|off]")),
                    "{argument}"
                );
            }
            assert_eq!(
                help_lines(Some(name)),
                Ok(vec![format!(
                    "* /{name} [on|off] - show or hide the {name}"
                )])
            );
        }
    }

    #[test]
    fn log_shows_or_turns_a_windows_log_on_or_off() {
        let log = |command| Ok(SlashCommand::Log(command));
        assert_eq!(parse("/log"), log(LogCommand::Show));
        assert_eq!(parse("/log on"), log(LogCommand::On { target: None }));
        assert_eq!(parse("/LOG OFF"), log(LogCommand::Off { target: None }));
        assert_eq!(
            parse("/log on #r"),
            log(LogCommand::On {
                target: Some("#r".to_owned())
            })
        );
        assert_eq!(
            parse("/log off !x:y"),
            log(LogCommand::Off {
                target: Some("!x:y".to_owned())
            })
        );
        for input in ["/log maybe", "/log on a b", "/log #r", "/log off #r now"] {
            assert_eq!(
                parse(input),
                error("log", "usage: /log [on|off] [#room]"),
                "{input}"
            );
        }
        assert_eq!(
            help_lines(Some("log")),
            Ok(vec![
                "* /log [on|off] [#room] - logs a window's lines to a file".to_owned()
            ])
        );
    }

    #[test]
    fn timestamp_shows_toggles_or_formats() {
        let timestamp = |command| Ok(SlashCommand::Timestamp(command));
        assert_eq!(parse("/timestamp"), timestamp(TimestampCommand::Show));
        assert_eq!(parse("/timestamp on"), timestamp(TimestampCommand::On));
        assert_eq!(parse("/TIMESTAMP OFF"), timestamp(TimestampCommand::Off));
        assert_eq!(
            parse("/timestamp -f [H:nn] <x>"),
            timestamp(TimestampCommand::Format(
                TimestampFormat::parse("[H:nn] <x>").unwrap()
            ))
        );
        assert_eq!(parse("/timestamp -f"), insufficient("timestamp"));
        for input in ["/timestamp maybe", "/timestamp on now", "/timestamp -x"] {
            assert_eq!(
                parse(input),
                error("timestamp", "usage: /timestamp [on|off|-f format]"),
                "{input}"
            );
        }
        assert_eq!(
            help_lines(Some("timestamp")),
            Ok(vec![
                "* /timestamp [on|off|-f format] - show, turn on or off, or format line timestamps"
                    .to_owned()
            ])
        );
    }

    #[test]
    fn help_lists_every_command_or_shows_one() {
        let lines = help_lines(None).unwrap();
        assert_eq!(lines.len(), 2);
        assert!(lines[0].starts_with(
            "* Commands: /accept /alias /ame /amsg /away /back /ban /channel /clear /clearall /close /cnick /create /deop /describe /devices /devoice /e2ee"
        ));
        assert!(lines[0].contains(" /help /identity /ignore "));
        assert!(lines[0].contains(" /reply /savebuf /say /sendfile "));
        assert!(lines[0].contains(" /timestamp /toolbar /topic /treebar "));
        assert_eq!(lines[1], "* /help <command> for usage");
        assert_eq!(
            help_lines(Some("join")),
            Ok(vec!["* /join <room-id-or-alias> - join a room".to_owned()])
        );
        assert_eq!(
            help_lines(Some("create")),
            Ok(vec![
                "* /create [-public #alias] <name> - create a room: private and encrypted, or public and listed"
                    .to_owned()
            ])
        );
        assert_eq!(
            help_lines(Some("topic")),
            Ok(vec![
                "* /topic [#room] [text] - show or set a room's topic (#alias or !id names the room)"
                    .to_owned()
            ])
        );
        assert_eq!(
            topic_line(Some("incident review")),
            "* Topic is 'incident review'"
        );
        assert_eq!(topic_line(None), "* No topic is set");
        assert_eq!(
            help_lines(Some("/CLEAR")),
            Ok(vec!["* /clear - clear this window".to_owned()])
        );
        assert_eq!(
            help_lines(Some("nope")),
            Err("no such command: nope".to_owned())
        );
        assert_eq!(
            parse("/help kick"),
            Ok(SlashCommand::Help {
                topic: Some("kick".to_owned()),
            })
        );
    }

    fn member(user_id: &str, nick: &str, rank: RoomMemberRank) -> RoomMemberSummary {
        RoomMemberSummary {
            user_id: user_id.to_owned(),
            nick: nick.to_owned(),
            nick_ambiguous: false,
            is_own_user: false,
            rank,
            can_kick: false,
            can_ban: false,
        }
    }

    #[test]
    fn parse_accepts_message_and_member_info_commands() {
        assert_eq!(
            parse("/query Alice  hello there "),
            Ok(SlashCommand::Query {
                target: "Alice".to_owned(),
                message: Some("hello there".to_owned()),
            })
        );
        assert_eq!(
            parse("/MSG Alice hi there"),
            Ok(SlashCommand::Msg {
                target: "Alice".to_owned(),
                body: "hi there".to_owned(),
                presentation: RoomMessagePresentationKind::Text,
            })
        );
        assert_eq!(
            parse("/notice !ops:example hi"),
            Ok(SlashCommand::Msg {
                target: "!ops:example".to_owned(),
                body: "hi".to_owned(),
                presentation: RoomMessagePresentationKind::Notice,
            })
        );
        assert_eq!(parse("/names"), Ok(SlashCommand::Names { target: None }));
        assert_eq!(
            parse("/channel"),
            Ok(SlashCommand::Channel { target: None })
        );
        assert_eq!(
            parse("/channel #ops"),
            Ok(SlashCommand::Channel {
                target: Some("#ops".to_owned()),
            })
        );
        assert_eq!(
            help_lines(Some("channel")),
            Ok(vec![
                "* /channel [#room] - Channel Central: this room's name, topic, bans and settings, or the named room's"
                    .to_owned()
            ])
        );
        assert_eq!(
            parse("/names Ops"),
            Ok(SlashCommand::Names {
                target: Some("Ops".to_owned()),
            })
        );
        assert_eq!(
            parse("/whois  @alice:remote "),
            Ok(SlashCommand::Whois {
                nick: "@alice:remote".to_owned(),
            })
        );
        assert_eq!(
            parse("/whois Alice Smith"),
            error("whois", "too many parameters")
        );
        assert_eq!(
            parse("/devices"),
            Ok(SlashCommand::Devices { target: None })
        );
        assert_eq!(
            parse("/DEVICES  Bob "),
            Ok(SlashCommand::Devices {
                target: Some("Bob".to_owned()),
            })
        );
        assert_eq!(
            parse("/devices Bob DEV1"),
            error("devices", "too many parameters")
        );
        assert_eq!(
            parse("/devices @delete:example"),
            Ok(SlashCommand::Devices {
                target: Some("@delete:example".to_owned()),
            })
        );
        assert_eq!(
            parse("/devices DELETE  PH0NE "),
            Ok(SlashCommand::DevicesDelete {
                device_id: "PH0NE".to_owned(),
            })
        );
        for input in ["/devices delete", "/devices delete PH0NE L4PT0P"] {
            assert_eq!(
                parse(input),
                error("devices", "usage: /devices delete <session>"),
                "{input}"
            );
        }
        assert_eq!(
            parse("/devices rename PH0NE  Work  phone "),
            Ok(SlashCommand::DevicesRename {
                device_id: "PH0NE".to_owned(),
                display_name: "Work  phone".to_owned(),
            })
        );
        for input in [
            "/devices rename",
            "/devices rename PH0NE",
            "/devices rename PH0NE  ",
        ] {
            assert_eq!(
                parse(input),
                error("devices", "usage: /devices rename <session> <name>"),
                "{input}"
            );
        }
        assert_eq!(
            parse("/IDENTITY Accept  Bob "),
            Ok(SlashCommand::IdentityAccept {
                target: "Bob".to_owned(),
            })
        );
        assert_eq!(parse("/identity"), insufficient("identity"));
        assert_eq!(parse("/identity accept"), insufficient("identity"));
        assert_eq!(
            parse("/identity accept Bob Carol"),
            error("identity", "too many parameters")
        );
        assert_eq!(
            parse("/identity reject Bob"),
            error("identity", "invalid parameters")
        );
        assert_eq!(parse("/ignore"), Ok(SlashCommand::Ignore { target: None }));
        assert_eq!(
            parse("/IGNORE bob "),
            Ok(SlashCommand::Ignore {
                target: Some("bob".to_owned()),
            })
        );
        assert_eq!(
            parse("/ignore bob spam"),
            error("ignore", "too many parameters")
        );
        assert_eq!(
            parse("/unignore @bob:example"),
            Ok(SlashCommand::Unignore {
                target: "@bob:example".to_owned(),
            })
        );
        assert_eq!(parse("/unignore"), insufficient("unignore"));
        assert_eq!(
            parse("/unignore a b"),
            error("unignore", "too many parameters")
        );
        assert_eq!(
            help_lines(Some("ignore")),
            Ok(vec![
                "* /ignore [nick] - ignore a user's messages (no nick: list ignored users)"
                    .to_owned()
            ])
        );
        assert_eq!(parse("/close"), Ok(SlashCommand::Close));
        assert_eq!(parse("/exit"), Ok(SlashCommand::Exit));
        for (input, command) in [
            ("/msg", "msg"),
            ("/msg Alice", "msg"),
            ("/notice Alice  ", "notice"),
            ("/whois", "whois"),
        ] {
            assert_eq!(parse(input), insufficient(command), "{input}");
        }
        assert_eq!(parse("/close now"), error("close", "takes no parameters"));
        assert_eq!(parse("/exit now"), error("exit", "takes no parameters"));
    }

    #[test]
    fn notify_lists_adds_with_a_note_and_removes() {
        let notify = |command| Ok(SlashCommand::Notify(command));
        assert_eq!(parse("/notify"), notify(NotifyCommand::List));
        assert_eq!(parse("/NOTIFY -l "), notify(NotifyCommand::List));
        assert_eq!(
            parse("/notify bob"),
            notify(NotifyCommand::Add {
                target: "bob".to_owned(),
                note: None,
                whois: false,
            })
        );
        assert_eq!(
            parse("/notify @bob:example  lunch  buddy "),
            notify(NotifyCommand::Add {
                target: "@bob:example".to_owned(),
                note: Some("lunch  buddy".to_owned()),
                whois: false,
            })
        );
        assert_eq!(
            parse("/notify +bob lunch"),
            notify(NotifyCommand::Add {
                target: "bob".to_owned(),
                note: Some("lunch".to_owned()),
                whois: true,
            })
        );
        assert_eq!(parse("/notify +"), insufficient("notify"));
        let abook = |tab| Ok(SlashCommand::AddressBook { tab });
        assert_eq!(parse("/abook"), abook(AddressBookTab::Notify));
        assert_eq!(parse("/ABOOK -n"), abook(AddressBookTab::Notify));
        assert_eq!(parse("/abook -c "), abook(AddressBookTab::Control));
        assert_eq!(parse("/abook -l"), abook(AddressBookTab::Colors));
        assert_eq!(parse("/abook -w"), error("abook", "unknown switch -w"));
        assert_eq!(parse("/abook bob"), error("abook", "too many parameters"));
        assert_eq!(
            parse("/abook -c bob"),
            error("abook", "too many parameters")
        );
        assert_eq!(parse("/notify + bob"), insufficient("notify"));
        assert_eq!(
            parse("/notify -r bob"),
            notify(NotifyCommand::Remove {
                target: "bob".to_owned(),
            })
        );
        assert_eq!(parse("/notify -r"), insufficient("notify"));
        assert_eq!(
            parse("/notify -r bob carol"),
            error("notify", "too many parameters")
        );
        assert_eq!(
            parse("/notify -l bob"),
            error("notify", "too many parameters")
        );
        assert_eq!(parse("/notify -s"), error("notify", "unknown switch -s"));
        assert_eq!(
            help_lines(Some("notify")),
            Ok(vec![
                "* /notify [-r] [nick [note]] - watch for a user on Matrix (-r: stop; no nick: list)"
                    .to_owned()
            ])
        );
    }

    #[test]
    fn cnick_lists_switches_adds_and_removes() {
        let cnick = |command| Ok(SlashCommand::NickColor(command));
        let add = |target: &str, color, modes, notify, force| {
            cnick(NickColorCommand::Add {
                target: target.to_owned(),
                color,
                modes,
                notify,
                force,
            })
        };
        let red = NickColor::Palette(PaletteIndex::new(4).unwrap());
        assert_eq!(parse("/cnick"), cnick(NickColorCommand::List));
        assert_eq!(parse("/CNICK on"), cnick(NickColorCommand::Active(true)));
        assert_eq!(parse("/cnick OFF "), cnick(NickColorCommand::Active(false)));
        assert_eq!(parse("/cnick bob 4"), add("bob", red, None, false, false));
        assert_eq!(
            parse("/cnick -fy *:example *"),
            add("*:example", NickColor::Auto, None, true, true)
        );
        assert_eq!(
            parse("/cnick @bob:example 4 @+"),
            add(
                "@bob:example",
                red,
                Some(ModeCondition::Ranks(
                    [RoomMemberRank::Moderator, RoomMemberRank::Voiced].into()
                )),
                false,
                false
            )
        );
        assert_eq!(
            parse("/cnick -a * 4"),
            add("*", red, Some(ModeCondition::AnyMode), false, false)
        );
        assert_eq!(
            parse("/cnick -n * 4"),
            add("*", red, Some(ModeCondition::NoMode), false, false)
        );
        // A user named on is given by ID once a switch is in the way.
        assert_eq!(parse("/cnick -y on"), insufficient("cnick"));
        assert_eq!(parse("/cnick bob"), insufficient("cnick"));
        assert_eq!(parse("/cnick -f"), insufficient("cnick"));
        assert_eq!(
            parse("/cnick bob 16"),
            error("cnick", "color must be 0-15 or *, not 16")
        );
        assert_eq!(
            parse("/cnick bob 4 @x"),
            error("cnick", "modes must be some of ~&@+, not @x")
        );
        assert_eq!(
            parse("/cnick -an bob 4"),
            error("cnick", "any mode and no mode cannot be combined")
        );
        assert_eq!(
            parse("/cnick -a bob 4 @"),
            error("cnick", "modes cannot be given with any mode or no mode")
        );
        assert_eq!(
            parse("/cnick bob 4 @ +"),
            error("cnick", "too many parameters")
        );
        assert_eq!(
            parse("/cnick -i bob 4"),
            error("cnick", "unknown switch -i")
        );
        assert_eq!(parse("/cnick - bob 4"), error("cnick", "unknown switch -"));
        assert_eq!(
            parse("/cnick -r bob"),
            cnick(NickColorCommand::Remove(NickColorTarget::Entry(
                "bob".to_owned()
            )))
        );
        assert_eq!(
            parse("/cnick -r 2"),
            cnick(NickColorCommand::Remove(NickColorTarget::Number(2)))
        );
        assert_eq!(parse("/cnick -r"), insufficient("cnick"));
        assert_eq!(
            parse("/cnick -r bob carol"),
            error("cnick", "too many parameters")
        );
        assert_eq!(
            parse("/cnick -rf bob"),
            error("cnick", "-r takes no other switches")
        );
        assert_eq!(
            help_lines(Some("cnick")),
            Ok(vec![
                "* /cnick [-rfany] [on|off|nick color|* [modes]] - colour nicks in the nick list and messages (-r: remove; no nick: list)"
                    .to_owned()
            ])
        );
    }

    #[test]
    fn resolve_nick_prefers_user_id_then_display_name_then_localpart() {
        let members = [
            member("@alice:example", "Alice", RoomMemberRank::Member),
            member("@bob:example", "alice2", RoomMemberRank::Member),
            member("@sam-one:example", "Sam", RoomMemberRank::Member),
            member("@sam-two:example", "sam", RoomMemberRank::Member),
            member("@carol:example", "carol", RoomMemberRank::Member),
            // The same user listed by a second room is still one user.
            member("@alice:example", "Alice", RoomMemberRank::Moderator),
        ];
        let resolve = |nick| resolve_nick(&members, nick).map(|member| member.user_id.as_str());

        assert_eq!(resolve("@bob:example"), Ok("@bob:example"));
        assert_eq!(resolve("ALICE"), Ok("@alice:example"));
        assert_eq!(resolve("Alice2"), Ok("@bob:example"));
        assert_eq!(resolve("carol"), Ok("@carol:example"));
        assert_eq!(resolve("bob"), Ok("@bob:example"));
        assert_eq!(
            resolve("Sam"),
            Err("Sam is ambiguous (@sam-one:example, @sam-two:example)".to_owned())
        );
        assert_eq!(resolve("Nobody"), Err("no such nick: Nobody".to_owned()));
        assert_eq!(
            resolve("@nobody:example"),
            Err("no such nick: @nobody:example".to_owned())
        );
    }

    /// A nick is the name shown, so `ghost` is the nameless `@ghost` as
    /// much as Casper, named `Ghost`, and names neither alone.
    #[test]
    fn resolve_nick_matches_the_nick_shown_before_any_localpart() {
        let members = [
            member("@casper:example", "Ghost", RoomMemberRank::Member),
            member("@ghost:example", "ghost", RoomMemberRank::Member),
        ];
        let resolve = |nick| resolve_nick(&members, nick).map(|member| member.user_id.as_str());

        assert_eq!(
            resolve("ghost"),
            Err("ghost is ambiguous (@casper:example, @ghost:example)".to_owned())
        );
        assert_eq!(resolve("casper"), Ok("@casper:example"));
        assert_eq!(resolve("@ghost:example"), Ok("@ghost:example"));
    }

    #[test]
    fn resolve_user_accepts_any_user_id() {
        let members = [member("@alice:example", "Alice", RoomMemberRank::Member)];
        assert_eq!(
            resolve_user(&members, "@stranger:example"),
            Ok(UserTarget {
                user_id: "@stranger:example".to_owned(),
                member: None,
            })
        );
        assert_eq!(
            resolve_user(&members, "@alice:example"),
            Ok(UserTarget {
                user_id: "@alice:example".to_owned(),
                member: Some(&members[0]),
            })
        );
        assert_eq!(
            resolve_user(&members, "alice"),
            Ok(UserTarget {
                user_id: "@alice:example".to_owned(),
                member: Some(&members[0]),
            })
        );
        assert_eq!(
            resolve_user(&members, "@"),
            Err("no such nick: @".to_owned())
        );
    }

    #[test]
    fn devices_lines_read_like_whois() {
        use mirx_core::DeviceSummary;

        let device =
            |device_id: &str, display_name: Option<&str>, verified, this_session, last_seen_ms| {
                DeviceSummary {
                    device_id: device_id.to_owned(),
                    display_name: display_name.map(str::to_owned),
                    verified,
                    this_session,
                    last_seen_ms,
                }
            };
        let own = UserDevices {
            user_id: "@me:example".to_owned(),
            identity: UserIdentityState::Verified,
            devices: vec![
                device(
                    "L4PT0P",
                    Some("Element Desktop"),
                    true,
                    true,
                    Some(1_795_183_331_000),
                ),
                device("PH0NE", None, false, false, Some(0)),
                device("K3YL3SS", Some("Old \"web\""), false, false, None),
            ],
        };
        assert_eq!(
            devices_lines(
                &own,
                "me",
                true,
                false,
                super::super::timestamp::utc_local_time
            ),
            vec![
                "* me sessions:",
                "* me L4PT0P \"Element Desktop\" verified, last seen Fri Nov 20 14:02:11 2026 (this session)",
                "* me PH0NE unverified, last seen Thu Jan 01 00:00:00 1970",
                "* me K3YL3SS \"Old \"web\"\" unverified",
            ]
        );

        let other = |identity| UserDevices {
            user_id: "@bob:example".to_owned(),
            identity,
            devices: vec![device("B0B", None, false, false, None)],
        };
        for (identity, line) in [
            (UserIdentityState::Verified, "* Bob is verified"),
            (
                UserIdentityState::Unverified,
                "* Bob is not verified; /verify @bob:example",
            ),
            (
                UserIdentityState::NoCrossSigning,
                "* Bob has no cross-signing; verify a device: /verify @bob:example <device>",
            ),
            (
                UserIdentityState::VerificationViolation,
                "* Bob changed identity since you verified them; /verify @bob:example, or \
                 /identity accept @bob:example",
            ),
        ] {
            assert_eq!(
                devices_lines(
                    &other(identity),
                    "Bob",
                    false,
                    false,
                    super::super::timestamp::utc_local_time
                ),
                vec![line, "* Bob B0B unverified"],
                "{identity:?}"
            );
        }
        let none = UserDevices {
            devices: Vec::new(),
            ..other(UserIdentityState::Unverified)
        };
        assert_eq!(
            devices_lines(
                &none,
                "Bob",
                false,
                false,
                super::super::timestamp::utc_local_time
            ),
            vec!["* Bob is not verified; /verify @bob:example"]
        );
        // Only a room's identity stream knows a pinned identity changed.
        assert_eq!(
            devices_lines(
                &none,
                "Bob",
                false,
                true,
                super::super::timestamp::utc_local_time
            ),
            vec!["* Bob changed identity; /identity accept @bob:example, or /verify @bob:example"]
        );
    }

    #[test]
    fn names_whois_and_msg_lines_read_like_mirc() {
        let members = [
            member("@creator:example", "Creator", RoomMemberRank::Creator),
            member("@admin:example", "Admin", RoomMemberRank::Administrator),
            member("@mod:example", "Mod", RoomMemberRank::Moderator),
            member("@voice:example", "Voice", RoomMemberRank::Voiced),
            member("@ghost:example", "ghost", RoomMemberRank::Member),
        ];
        assert_eq!(
            names_line("Ops", &members),
            "* Members of Ops: ~Creator &Admin @Mod +Voice ghost"
        );
        let profile = |user_id: &str, display_name: Option<&str>| UserProfile {
            user_id: user_id.to_owned(),
            display_name: display_name.map(str::to_owned),
            presence: None,
        };
        assert_eq!(
            whois_lines(
                &profile("@mod:example", Some("Moderator Mo")),
                Some("Mod"),
                &[
                    (RoomMemberRank::Moderator, "Ops"),
                    (RoomMemberRank::Member, "Lobby")
                ]
            ),
            vec![
                "* Mod is @mod:example (Moderator Mo)".to_owned(),
                "* Mod shares rooms with you: @Ops Lobby".to_owned(),
                "* Mod's homeserver is example".to_owned(),
            ]
        );
        // A stranger: no shared rooms, and the display name is the nick.
        assert_eq!(
            whois_lines(&profile("@far:remote.org:8448", Some("Far")), None, &[]),
            vec![
                "* Far is @far:remote.org:8448 (Far)".to_owned(),
                "* Far's homeserver is remote.org:8448".to_owned(),
            ]
        );
        assert_eq!(
            // A member without a display name goes by their localpart there;
            // the full ID is where mIRC prints user@host.
            whois_lines(
                &profile("@ghost:example", None),
                Some("ghost"),
                &[(RoomMemberRank::Member, "Ops")]
            ),
            vec![
                "* ghost is @ghost:example".to_owned(),
                "* ghost shares rooms with you: Ops".to_owned(),
                "* ghost's homeserver is example".to_owned(),
            ]
        );
        assert_eq!(
            msg_echo(RoomMessagePresentationKind::Text, "Alice", "hi"),
            "-> *Alice* hi"
        );
        assert_eq!(
            msg_echo(RoomMessagePresentationKind::Notice, "Alice", "hi"),
            "-> -Alice- hi"
        );
    }

    #[test]
    fn duration_text_reads_like_mircs_duration() {
        for (secs, text) in [
            (0, "0secs"),
            (1, "1sec"),
            (59, "59secs"),
            (60, "1min"),
            (61, "1min 1sec"),
            (3600, "1hr"),
            (3725, "1hr 2mins 5secs"),
            (90061, "1day 1hr 1min 1sec"),
            (694861, "1wk 1day 1hr 1min 1sec"),
            (2 * 694861, "2wks 2days 2hrs 2mins 2secs"),
        ] {
            assert_eq!(duration_text(secs), text, "{secs}");
        }
    }

    #[test]
    fn whois_says_who_is_away_and_idle_last() {
        let profile = |kind, status_msg: Option<&str>, idle_ms, currently_active| UserProfile {
            user_id: "@bob:example".to_owned(),
            display_name: Some("Bob".to_owned()),
            presence: Some(UserPresence {
                kind,
                status_msg: status_msg.map(str::to_owned),
                last_active_ago_ms: idle_ms,
                currently_active,
            }),
        };
        let presence_lines = |profile: &UserProfile| {
            let lines = whois_lines(profile, None, &[(RoomMemberRank::Member, "Ops")]);
            // Who they are, the rooms you share and their server first.
            assert_eq!(
                lines[..3],
                [
                    "* Bob is @bob:example (Bob)",
                    "* Bob shares rooms with you: Ops",
                    "* Bob's homeserver is example"
                ]
            );
            lines[3..].to_vec()
        };
        assert_eq!(
            presence_lines(&profile(
                PresenceKind::Unavailable,
                Some("lunch"),
                Some(303_999),
                Some(false)
            )),
            ["* Bob is away: lunch", "* Bob has been idle 5mins 3secs"]
        );
        assert_eq!(
            presence_lines(&profile(PresenceKind::Unavailable, None, None, None)),
            ["* Bob is away"]
        );
        assert_eq!(
            presence_lines(&profile(PresenceKind::Online, None, Some(500), None)),
            ["* Bob has been idle 0secs"]
        );
        // Active now, or offline (what a server without presence says of
        // everyone): nothing to say.
        assert!(
            presence_lines(&profile(
                PresenceKind::Online,
                None,
                Some(9_000),
                Some(true)
            ))
            .is_empty()
        );
        assert!(
            presence_lines(&profile(
                PresenceKind::Offline,
                Some("gone"),
                Some(9_000),
                None
            ))
            .is_empty()
        );
    }

    #[test]
    fn parse_accepts_moderation_commands() {
        assert_eq!(
            parse("/kick Bob  spamming the channel "),
            Ok(SlashCommand::Kick {
                nick: "Bob".to_owned(),
                reason: Some("spamming the channel".to_owned()),
            })
        );
        assert_eq!(
            parse("/ban Bob"),
            Ok(SlashCommand::Ban {
                nick: "Bob".to_owned(),
                reason: None,
            })
        );
        assert_eq!(
            parse("/unban @bob:example sorry"),
            Ok(SlashCommand::Unban {
                target: "@bob:example".to_owned(),
                reason: Some("sorry".to_owned()),
            })
        );
        assert_eq!(
            parse("/invite Bob Lobby Room"),
            Ok(SlashCommand::Invite {
                nick: "Bob".to_owned(),
                room: Some("Lobby Room".to_owned()),
            })
        );
        for (input, nicks, change) in [
            ("/op Bob", &["Bob"][..], RankChange::Op),
            ("/DEOP Bob", &["Bob"][..], RankChange::Deop),
            ("/voice Bob", &["Bob"][..], RankChange::Voice),
            ("/devoice Bob", &["Bob"][..], RankChange::Devoice),
            ("/op Bob Carol", &["Bob", "Carol"][..], RankChange::Op),
            (
                "/devoice a  b   c ",
                &["a", "b", "c"][..],
                RankChange::Devoice,
            ),
        ] {
            let command = parse(input).unwrap();
            assert_eq!(
                command,
                SlashCommand::SetRank {
                    nicks: nicks.iter().map(|nick| (*nick).to_owned()).collect(),
                    change,
                },
                "{input}"
            );
            assert_eq!(
                command.name(),
                input[1..input.find(' ').unwrap()].to_ascii_lowercase()
            );
        }
        assert_eq!(
            parse("/nick New Name"),
            Ok(SlashCommand::Nick {
                display_name: "New Name".to_owned(),
            })
        );
        for name in [
            "kick", "ban", "unban", "invite", "op", "deop", "voice", "devoice", "nick",
        ] {
            assert_eq!(parse(&format!("/{name}")), insufficient(name), "{name}");
        }
        assert_eq!(parse("/op   "), insufficient("op"));
    }

    #[test]
    fn mode_is_not_a_command() {
        for input in ["/mode +m", "/mode #ops +i", "/MODE"] {
            assert_eq!(parse(input), Err(CommandError::unknown("mode")), "{input}");
        }
        assert_eq!(
            help_lines(Some("mode")),
            Err("no such command: mode".to_owned())
        );
    }

    #[test]
    fn parse_accepts_amsg_ame_describe_and_clearall() {
        assert_eq!(
            parse("/amsg  hi all "),
            Ok(SlashCommand::AllChannels {
                presentation: RoomMessagePresentationKind::Text,
                body: "hi all".to_owned(),
            })
        );
        assert_eq!(
            parse("/AME waves"),
            Ok(SlashCommand::AllChannels {
                presentation: RoomMessagePresentationKind::Emote,
                body: "waves".to_owned(),
            })
        );
        assert_eq!(parse("/amsg"), insufficient("amsg"));
        assert_eq!(parse("/ame"), insufficient("ame"));
        let describe = parse("/describe #ops waves hello").unwrap();
        assert_eq!(
            describe,
            SlashCommand::Msg {
                target: "#ops".to_owned(),
                body: "waves hello".to_owned(),
                presentation: RoomMessagePresentationKind::Emote,
            }
        );
        assert_eq!(describe.name(), "describe");
        assert_eq!(parse("/describe #ops"), insufficient("describe"));
        // Only /describe sends an emote to one target.
        for line in ["/msg x y", "/notice x y"] {
            assert!(
                matches!(
                    parse(line),
                    Ok(SlashCommand::Msg { presentation, .. })
                        if presentation != RoomMessagePresentationKind::Emote
                ),
                "{line}"
            );
        }
        let switches = |status, channels, queries, invites| {
            Ok(SlashCommand::ClearAll(ClearAll {
                status,
                channels,
                queries,
                invites,
            }))
        };
        assert_eq!(parse("/clearall"), switches(true, true, true, true));
        assert_eq!(parse("/clearall -sn"), switches(true, true, false, false));
        assert_eq!(parse("/clearall -s -q"), switches(true, false, true, false));
        assert_eq!(parse("/clearall -Q"), switches(false, false, true, false));
        assert_eq!(
            parse("/clearall -x"),
            error("clearall", "unknown switch -x")
        );
        assert_eq!(
            parse("/clearall -sm"),
            error("clearall", "unknown switch -m")
        );
        assert_eq!(
            parse("/clearall -"),
            error("clearall", "unknown parameter -")
        );
        assert_eq!(
            parse("/clearall foo"),
            error("clearall", "unknown parameter foo")
        );
        assert_eq!(
            msg_echo(RoomMessagePresentationKind::Emote, "#ops", "waves"),
            "-> #ops * waves"
        );
    }

    #[test]
    fn parse_accepts_away_and_back() {
        assert_eq!(parse("/away"), Ok(SlashCommand::Away { message: None }));
        assert_eq!(
            parse("/away  gone fishing "),
            Ok(SlashCommand::Away {
                message: Some("gone fishing".to_owned()),
            })
        );
        assert_eq!(parse("/BACK"), Ok(SlashCommand::Away { message: None }));
        assert_eq!(parse("/back x"), error("back", "takes no parameters"));
    }

    #[test]
    fn away_answers_like_irc_and_says_when_others_do_not_see_it() {
        let changed = |message: Option<&str>, seen_as| AwayChanged {
            message: message.map(str::to_owned),
            seen_as,
        };
        let away = "* You have been marked as being away";
        assert_eq!(
            away_lines(&changed(Some("lunch"), PresenceKind::Unavailable)),
            [away]
        );
        assert_eq!(
            away_lines(&changed(Some("lunch"), PresenceKind::Online)),
            [
                away,
                "* Others still see you as online: another of your sessions is active"
            ]
        );
        assert_eq!(
            away_lines(&changed(Some("lunch"), PresenceKind::Offline)),
            [
                away,
                "* Others see you as offline: your server does not share presence"
            ]
        );
        // Being present needs no warning, whatever the server says.
        for seen_as in [
            PresenceKind::Online,
            PresenceKind::Unavailable,
            PresenceKind::Offline,
        ] {
            assert_eq!(
                away_lines(&changed(None, seen_as)),
                ["* You are no longer marked as being away"]
            );
        }
        assert_eq!(
            away_status(&changed(Some("gone fishing"), PresenceKind::Unavailable)),
            "away: gone fishing"
        );
        assert_eq!(away_status(&changed(None, PresenceKind::Online)), "back");
    }

    #[test]
    fn rank_changes_follow_the_member_rank() {
        let mut own = member("@me:example", "Me", RoomMemberRank::Moderator);
        own.is_own_user = true;
        let cases = [
            (
                RankChange::Op,
                RoomMemberRank::Member,
                Ok(MemberPowerRole::Moderator),
            ),
            (
                RankChange::Op,
                RoomMemberRank::Voiced,
                Ok(MemberPowerRole::Moderator),
            ),
            (
                RankChange::Op,
                RoomMemberRank::Moderator,
                Err("Bob is already an operator"),
            ),
            (
                RankChange::Op,
                RoomMemberRank::Creator,
                Err("Bob is already an operator"),
            ),
            (
                RankChange::Deop,
                RoomMemberRank::Moderator,
                Ok(MemberPowerRole::Default),
            ),
            (
                RankChange::Deop,
                RoomMemberRank::Administrator,
                Ok(MemberPowerRole::Default),
            ),
            (
                RankChange::Deop,
                RoomMemberRank::Creator,
                Err("Bob is the room creator"),
            ),
            (
                RankChange::Deop,
                RoomMemberRank::Voiced,
                Err("Bob is not an operator"),
            ),
            (
                RankChange::Voice,
                RoomMemberRank::Member,
                Ok(MemberPowerRole::Voiced),
            ),
            (
                RankChange::Voice,
                RoomMemberRank::Voiced,
                Err("Bob already has voice"),
            ),
            (
                RankChange::Voice,
                RoomMemberRank::Moderator,
                Err("Bob already has voice"),
            ),
            (
                RankChange::Devoice,
                RoomMemberRank::Voiced,
                Ok(MemberPowerRole::Default),
            ),
            (
                RankChange::Devoice,
                RoomMemberRank::Member,
                Err("Bob is not voiced"),
            ),
        ];
        for (change, rank, expected) in cases {
            assert_eq!(
                rank_change_role(change, &member("@bob:example", "Bob", rank)),
                expected.map_err(str::to_owned),
                "{change:?} {rank:?}"
            );
        }
        assert_eq!(
            rank_change_role(RankChange::Deop, &own),
            Err("you cannot deop yourself".to_owned())
        );
    }
}
