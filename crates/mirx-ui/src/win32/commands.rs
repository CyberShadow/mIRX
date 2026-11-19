//! The editbox's slash commands: parsing typed input into commands, and the
//! command table `/help` lists.
//!
//! Pure code, no Win32 calls. Errors carry the typed command name so the
//! window can print them the way mIRC does: `* /kick: insufficient
//! parameters`.

use std::borrow::Cow;
use std::fmt;

use mirx_core::RoomMessagePresentationKind;

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
    Query {
        user_id: String,
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
        "query" => Ok(SlashCommand::Query {
            user_id: required(argument)?,
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
                    user_id: "@alice:example.org".to_owned(),
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
}
