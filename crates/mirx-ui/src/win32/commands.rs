//! The editbox's slash commands: parsing typed input into commands, and the
//! command table `/help` lists.
//!
//! Pure code, no Win32 calls. Errors carry the typed command name so the
//! window can print them the way mIRC does: `* /kick: insufficient
//! parameters`.

use std::borrow::Cow;
use std::fmt;

use mirx_core::{RoomMemberRank, RoomMemberSummary, RoomMessagePresentationKind};

use super::nick_completion::member_mention_text;
use super::timeline_text::rank_prefix;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum ComposeSubmission<'a> {
    Message {
        presentation: RoomMessagePresentationKind,
        body: Cow<'a, str>,
    },
    LocalCommand(Result<SlashCommand, CommandError>),
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
    /// `/query <nick> [message]`.
    Query {
        target: String,
        message: Option<String>,
    },
    /// `/msg` (Text) and `/notice` (Notice).
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
    Close,
    Exit,
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
    Reply {
        target_event_id: String,
        body: String,
    },
    SendFile {
        path: String,
    },
    Edit {
        target_event_id: String,
        body: String,
    },
    Redact {
        event_id: String,
    },
    Unreact {
        reaction_event_id: String,
    },
    Topic {
        topic: String,
    },
    Clear,
    E2ee(E2eeSlashCommand),
    Verify {
        action: VerificationAction,
    },
    Help {
        topic: Option<String>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum E2eeSlashCommand {
    ShowStatus,
    Refresh,
    RecoverDialog,
    Setup {
        passphrase: Option<String>,
        wait_for_backups_to_upload: bool,
    },
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
            Self::Query { .. } => "query",
            Self::Msg {
                presentation: RoomMessagePresentationKind::Notice,
                ..
            } => "notice",
            Self::Msg { .. } => "msg",
            Self::Names { .. } => "names",
            Self::Whois { .. } => "whois",
            Self::Close => "close",
            Self::Exit => "exit",
            Self::Accept { .. } => "accept",
            Self::Reject { .. } => "reject",
            Self::Part { .. } => "part",
            Self::React { .. } => "react",
            Self::Reply { .. } => "reply",
            Self::SendFile { .. } => "sendfile",
            Self::Edit { .. } => "edit",
            Self::Redact { .. } => "redact",
            Self::Unreact { .. } => "unreact",
            Self::Topic { .. } => "topic",
            Self::Clear => "clear",
            Self::E2ee(_) => "e2ee",
            Self::Verify { .. } => "verify",
            Self::Help { .. } => "help",
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
        name: "clear",
        usage: "",
        summary: "clear this window",
    },
    CommandSpec {
        name: "close",
        usage: "",
        summary: "close this window",
    },
    CommandSpec {
        name: "e2ee",
        usage: "[status|refresh|recover|setup [wait] [passphrase <passphrase>]]",
        summary: "show or manage end-to-end encryption",
    },
    CommandSpec {
        name: "edit",
        usage: "<event-id> <text>",
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
        name: "help",
        usage: "[command]",
        summary: "list the commands, or show one's usage",
    },
    CommandSpec {
        name: "join",
        usage: "<room-id-or-alias>",
        summary: "join a room",
    },
    CommandSpec {
        name: "me",
        usage: "<action>",
        summary: "send an action",
    },
    CommandSpec {
        name: "msg",
        usage: "<nick|room> <message>",
        summary: "send a message without opening a window",
    },
    CommandSpec {
        name: "names",
        usage: "[room]",
        summary: "list this room's members, or the named room's",
    },
    CommandSpec {
        name: "notice",
        usage: "<nick|room> <message>",
        summary: "send a notice",
    },
    CommandSpec {
        name: "part",
        usage: "[room]",
        summary: "leave this room, or the named one",
    },
    CommandSpec {
        name: "query",
        usage: "<nick> [message]",
        summary: "open a query with a user, and send a message there",
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
        usage: "<event-id> <message>",
        summary: "reply to a message",
    },
    CommandSpec {
        name: "sendfile",
        usage: "<path>",
        summary: "send a file to this room",
    },
    CommandSpec {
        name: "status",
        usage: "",
        summary: "activate the Status window",
    },
    CommandSpec {
        name: "topic",
        usage: "<text>",
        summary: "set this room's topic",
    },
    CommandSpec {
        name: "unreact",
        usage: "<reaction-event-id>",
        summary: "remove your reaction",
    },
    CommandSpec {
        name: "verify",
        usage: "<accept|cancel|confirm|reject>",
        summary: "answer a device verification",
    },
    CommandSpec {
        name: "whois",
        usage: "<nick>",
        summary: "show who a user is and the rooms you share",
    },
    CommandSpec {
        name: "window",
        usage: "<room>",
        summary: "activate a room's window",
    },
];

/// Classifies editbox input: text, `//text` (literal slash), `/me`, or a
/// command.
pub(super) fn parse_compose_submission(input: &str) -> ComposeSubmission<'_> {
    if !input.starts_with('/') {
        return ComposeSubmission::Message {
            presentation: RoomMessagePresentationKind::Text,
            body: Cow::Borrowed(input),
        };
    }

    if input.starts_with("//") {
        return ComposeSubmission::Message {
            presentation: RoomMessagePresentationKind::Text,
            body: Cow::Owned(input[1..].to_owned()),
        };
    }

    let (name, argument) = split_command(input);
    if name.eq_ignore_ascii_case("me") {
        if argument.is_empty() {
            return ComposeSubmission::LocalCommand(Err(CommandError::insufficient_parameters(
                "me",
            )));
        }
        return ComposeSubmission::Message {
            presentation: RoomMessagePresentationKind::Emote,
            body: Cow::Borrowed(argument),
        };
    }

    ComposeSubmission::LocalCommand(parse(input))
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
        "query" => {
            required(argument)?;
            let (target, message) = split_word(argument);
            Ok(SlashCommand::Query {
                target: target.to_owned(),
                message: optional(message),
            })
        }
        "msg" | "notice" => {
            let (target, body) = split_word(argument);
            if body.is_empty() {
                return Err(CommandError::insufficient_parameters(name.as_str()));
            }
            let presentation = if name == "msg" {
                RoomMessagePresentationKind::Text
            } else {
                RoomMessagePresentationKind::Notice
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
        "whois" => Ok(SlashCommand::Whois {
            nick: required(argument)?,
        }),
        "close" => no_parameters(SlashCommand::Close),
        "exit" => no_parameters(SlashCommand::Exit),
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
            let (target_event_id, body) = event_and_text(&name, argument)?;
            Ok(SlashCommand::Reply {
                target_event_id,
                body,
            })
        }
        "sendfile" => Ok(SlashCommand::SendFile {
            path: required(argument)?,
        }),
        "edit" => {
            let (target_event_id, body) = event_and_text(&name, argument)?;
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
        "topic" => Ok(SlashCommand::Topic {
            topic: required(argument)?,
        }),
        "clear" => no_parameters(SlashCommand::Clear),
        "e2ee" => parse_e2ee(argument).map(SlashCommand::E2ee),
        "verify" => parse_verify(argument),
        "help" => Ok(SlashCommand::Help {
            topic: optional(argument),
        }),
        _ => Err(CommandError::unknown(name)),
    }
}

/// `<event-id> <text>`: both parts are required.
fn event_and_text(name: &str, argument: &str) -> Result<(String, String), CommandError> {
    let (event_id, text) = split_word(argument);
    if text.is_empty() {
        return Err(CommandError::insufficient_parameters(name));
    }
    Ok((event_id.to_owned(), text.to_owned()))
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

/// The reasons never repeat the argument: it may hold a passphrase.
fn parse_e2ee(argument: &str) -> Result<E2eeSlashCommand, CommandError> {
    let invalid = || CommandError::new("e2ee", "invalid parameters");
    let (action, rest) = split_word(argument);
    match action.to_ascii_lowercase().as_str() {
        "" | "status" if rest.is_empty() => Ok(E2eeSlashCommand::ShowStatus),
        "refresh" if rest.is_empty() => Ok(E2eeSlashCommand::Refresh),
        "recover" if rest.is_empty() => Ok(E2eeSlashCommand::RecoverDialog),
        "setup" => parse_e2ee_setup(rest).ok_or_else(invalid),
        _ => Err(invalid()),
    }
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

fn parse_verify(argument: &str) -> Result<SlashCommand, CommandError> {
    let action = match argument.to_ascii_lowercase().as_str() {
        "" => return Err(CommandError::insufficient_parameters("verify")),
        "accept" => VerificationAction::Accept,
        "cancel" => VerificationAction::Cancel,
        "confirm" => VerificationAction::Confirm,
        "reject" => VerificationAction::Reject,
        _ => {
            return Err(CommandError::new(
                "verify",
                format!("unknown action: {argument} (accept, cancel, confirm, reject)"),
            ));
        }
    };
    Ok(SlashCommand::Verify { action })
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
    Ok(vec![format!("* /{}{usage} — {}", spec.name, spec.summary)])
}

/// The member a nick names, resolved as IRC users expect: an exact user ID,
/// else a display name (case-insensitive, as IRC nicks are), else a user ID
/// localpart (`alice` for `@alice:example.org`). Each step must match exactly
/// one user; a member listed in several rooms is one user.
pub(super) fn resolve_nick<'m>(
    members: impl IntoIterator<Item = &'m RoomMemberSummary>,
    nick: &str,
) -> Result<&'m RoomMemberSummary, String> {
    let members = members.into_iter().collect::<Vec<_>>();
    let folded = nick.to_lowercase();
    let steps: [&dyn Fn(&RoomMemberSummary) -> bool; 3] = [
        &|member| member.user_id == nick,
        &|member| {
            member
                .display_name
                .as_deref()
                .is_some_and(|name| name.to_lowercase() == folded)
        },
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
fn is_user_id(text: &str) -> bool {
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

/// The nick a member goes by in command output.
fn member_nick(member: &RoomMemberSummary) -> &str {
    member_mention_text(&member.user_id, member.display_name.as_deref())
}

/// `/names`: the members, in nick list order, with their rank prefixes.
pub(super) fn names_lines(room_name: &str, members: &[RoomMemberSummary]) -> Vec<String> {
    let names = members
        .iter()
        .map(|member| format!("{}{}", rank_prefix(member.rank), member_nick(member)))
        .collect::<Vec<_>>()
        .join(" ");
    vec![
        format!("* Names {room_name}: {names}"),
        "* End of /NAMES list".to_owned(),
    ]
}

/// `/whois`: who the member is and the rooms shared with them, each with
/// the member's rank prefix there.
pub(super) fn whois_lines(
    member: &RoomMemberSummary,
    shared_rooms: &[(RoomMemberRank, &str)],
) -> Vec<String> {
    let nick = member_nick(member);
    let mut lines = vec![format!("* {nick} is {}", member.user_id)];
    if !shared_rooms.is_empty() {
        let rooms = shared_rooms
            .iter()
            .map(|(rank, room_name)| format!("{}{room_name}", rank_prefix(*rank)))
            .collect::<Vec<_>>()
            .join(" ");
        lines.push(format!("* {nick} on {rooms}"));
    }
    lines.push(format!("* {nick} End of /WHOIS list"));
    lines
}

/// The echo of `/msg` (`-> *nick* text`) and `/notice` (`-> -nick- text`).
pub(super) fn msg_echo(
    presentation: RoomMessagePresentationKind,
    target: &str,
    body: &str,
) -> String {
    match presentation {
        RoomMessagePresentationKind::Text => format!("-> *{target}* {body}"),
        RoomMessagePresentationKind::Notice => format!("-> -{target}- {body}"),
        RoomMessagePresentationKind::Emote => unreachable!("/msg and /notice never send emotes"),
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

    fn error(command: &str, reason: &str) -> Result<SlashCommand, CommandError> {
        Err(CommandError::new(command, reason))
    }

    fn insufficient(command: &str) -> Result<SlashCommand, CommandError> {
        error(command, "insufficient parameters")
    }

    fn text(body: &str) -> ComposeSubmission<'static> {
        ComposeSubmission::Message {
            presentation: RoomMessagePresentationKind::Text,
            body: Cow::Owned(body.to_owned()),
        }
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
    fn parse_compose_submission_distinguishes_text_emote_literal_slash_and_commands() {
        assert_eq!(
            parse_compose_submission("hello"),
            ComposeSubmission::Message {
                presentation: RoomMessagePresentationKind::Text,
                body: Cow::Borrowed("hello"),
            }
        );
        for input in ["/me waves", "/ME    waves"] {
            assert_eq!(
                parse_compose_submission(input),
                ComposeSubmission::Message {
                    presentation: RoomMessagePresentationKind::Emote,
                    body: Cow::Borrowed("waves"),
                }
            );
        }
        for input in ["/me", "/me    "] {
            assert_eq!(
                parse_compose_submission(input),
                ComposeSubmission::LocalCommand(insufficient("me"))
            );
        }
        for (input, body) in [
            ("//me waves", "/me waves"),
            ("//status", "/status"),
            ("//find needle", "/find needle"),
            ("//react $react-normal +1", "/react $react-normal +1"),
            (
                "//edit $event replacement text",
                "/edit $event replacement text",
            ),
            ("//reply $event thanks", "/reply $event thanks"),
            ("//accept", "/accept"),
            ("//reject", "/reject"),
            ("///status", "//status"),
        ] {
            assert_eq!(parse_compose_submission(input), text(body), "{input}");
        }
        assert_eq!(
            parse_compose_submission("/status"),
            ComposeSubmission::LocalCommand(Ok(SlashCommand::Status))
        );
        assert_eq!(
            parse_compose_submission("/"),
            ComposeSubmission::LocalCommand(error("", "unknown command"))
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
        for (input, action) in [
            ("/verify accept", VerificationAction::Accept),
            ("/verify cancel", VerificationAction::Cancel),
            ("/verify confirm", VerificationAction::Confirm),
            ("/verify reject", VerificationAction::Reject),
        ] {
            assert_eq!(parse(input), Ok(SlashCommand::Verify { action }));
        }
    }

    #[test]
    fn parse_rejects_unknown_commands_and_malformed_parameters() {
        assert_eq!(parse("/wat now"), error("wat", "unknown command"));
        assert_eq!(parse("/WAT"), error("wat", "unknown command"));
        assert_eq!(parse("/status now"), error("status", "takes no parameters"));
        assert_eq!(parse("/clear now"), error("clear", "takes no parameters"));
        assert_eq!(parse("/find"), insufficient("find"));
        assert_eq!(parse("/window"), insufficient("window"));
        assert_eq!(parse("/verify"), insufficient("verify"));
        assert_eq!(
            parse("/verify maybe"),
            error(
                "verify",
                "unknown action: maybe (accept, cancel, confirm, reject)"
            )
        );
        for input in [
            "/e2ee recover secret",
            "/e2ee bogus",
            "/e2ee setup passphrase",
            "/e2ee setup wait passphrases secret phrase",
            "/e2ee setup secret phrase",
        ] {
            let rejected = parse(input);
            assert_eq!(rejected, error("e2ee", "invalid parameters"), "{input}");
            assert!(!rejected.unwrap_err().to_string().contains("secret"));
        }
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
                    body: "replacement text".to_owned(),
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
                    body: "hello there".to_owned(),
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
        for input in ["/topic incident review", "/topic   incident review   "] {
            assert_eq!(
                parse(input),
                Ok(SlashCommand::Topic {
                    topic: "incident review".to_owned(),
                })
            );
        }
        assert_eq!(
            parse("/sendfile C:\\some file.txt"),
            Ok(SlashCommand::SendFile {
                path: "C:\\some file.txt".to_owned(),
            })
        );
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
            ("/topic", "topic"),
            ("/topic   ", "topic"),
            ("/sendfile", "sendfile"),
            ("/react", "react"),
            ("/react $react-normal", "react"),
            ("/react $react-normal   ", "react"),
            ("/edit", "edit"),
            ("/edit $event-to-edit", "edit"),
            ("/edit $event-to-edit   ", "edit"),
            ("/reply", "reply"),
            ("/reply $event-to-reply", "reply"),
            ("/reply $event-to-reply   ", "reply"),
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
            let ComposeSubmission::LocalCommand(command) = parse_compose_submission(input) else {
                panic!("expected a command: {input}");
            };
            assert_eq!(
                local_command_history_entry(input, &command),
                None,
                "{input}"
            );
        }
        for input in ["/e2ee setup wait", "/e2ee", "/kick", "/find needle"] {
            let ComposeSubmission::LocalCommand(command) = parse_compose_submission(input) else {
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
            if let ComposeSubmission::LocalCommand(Err(error)) = parse_compose_submission(&input) {
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
            "/whois x",
            "/close",
            "/exit",
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
    fn help_lists_every_command_or_shows_one() {
        let lines = help_lines(None).unwrap();
        assert_eq!(lines.len(), 2);
        assert!(lines[0].starts_with("* Commands: /accept /clear /close /e2ee"));
        assert!(lines[0].contains(" /help "));
        assert_eq!(lines[1], "* /help <command> for usage");
        assert_eq!(
            help_lines(Some("join")),
            Ok(vec!["* /join <room-id-or-alias> — join a room".to_owned()])
        );
        assert_eq!(
            help_lines(Some("/CLEAR")),
            Ok(vec!["* /clear — clear this window".to_owned()])
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

    fn member(
        user_id: &str,
        display_name: Option<&str>,
        rank: RoomMemberRank,
    ) -> RoomMemberSummary {
        RoomMemberSummary {
            user_id: user_id.to_owned(),
            display_name: display_name.map(str::to_owned),
            display_name_ambiguous: false,
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
            parse("/names Ops"),
            Ok(SlashCommand::Names {
                target: Some("Ops".to_owned()),
            })
        );
        assert_eq!(
            parse("/whois Alice Smith"),
            Ok(SlashCommand::Whois {
                nick: "Alice Smith".to_owned(),
            })
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
    fn resolve_nick_prefers_user_id_then_display_name_then_localpart() {
        let members = [
            member("@alice:example", Some("Alice"), RoomMemberRank::Member),
            member("@bob:example", Some("alice2"), RoomMemberRank::Member),
            member("@sam-one:example", Some("Sam"), RoomMemberRank::Member),
            member("@sam-two:example", Some("sam"), RoomMemberRank::Member),
            member("@carol:example", None, RoomMemberRank::Member),
            // The same user listed by a second room is still one user.
            member("@alice:example", Some("Alice"), RoomMemberRank::Moderator),
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

    #[test]
    fn resolve_user_accepts_any_user_id() {
        let members = [member(
            "@alice:example",
            Some("Alice"),
            RoomMemberRank::Member,
        )];
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
    fn names_whois_and_msg_lines_read_like_mirc() {
        let members = [
            member("@creator:example", Some("Creator"), RoomMemberRank::Creator),
            member(
                "@admin:example",
                Some("Admin"),
                RoomMemberRank::Administrator,
            ),
            member("@mod:example", Some("Mod"), RoomMemberRank::Moderator),
            member("@voice:example", Some("Voice"), RoomMemberRank::Voiced),
            member("@ghost:example", None, RoomMemberRank::Member),
        ];
        assert_eq!(
            names_lines("Ops", &members),
            vec![
                "* Names Ops: ~Creator &Admin @Mod +Voice @ghost:example".to_owned(),
                "* End of /NAMES list".to_owned(),
            ]
        );
        assert_eq!(
            whois_lines(
                &members[2],
                &[
                    (RoomMemberRank::Moderator, "Ops"),
                    (RoomMemberRank::Member, "Lobby")
                ]
            ),
            vec![
                "* Mod is @mod:example".to_owned(),
                "* Mod on @Ops Lobby".to_owned(),
                "* Mod End of /WHOIS list".to_owned(),
            ]
        );
        assert_eq!(
            whois_lines(&members[4], &[]),
            vec![
                "* @ghost:example is @ghost:example".to_owned(),
                "* @ghost:example End of /WHOIS list".to_owned(),
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
}
