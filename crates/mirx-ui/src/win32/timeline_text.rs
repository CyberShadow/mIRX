//! mIRC-style text for timeline lines and nicks.
//!
//! mirx-core names users: a member's nick (their display name, else their
//! localpart) and whether another member goes by the same one. Every user
//! shown in the UI goes through [`nick_label`], which only adds the user ID
//! to an ambiguous nick, the same way in the timeline, the nick list, and
//! the typing indicator.

use std::ops::Range;

use mirx_core::{
    RoomJoinRule, RoomMemberChange, RoomMemberRank, RoomMessagePresentationKind,
    RoomPowerLevelUserChange, RoomPowerLevelValueChange, RoomUserProfile,
};

/// The label for a user: their nick, the nick with the user ID when another
/// member goes by it too, or the user ID when they are not a member the
/// core knows (no nick).
pub(super) fn nick_label(user_id: &str, nick: Option<&str>, ambiguous: bool) -> String {
    match nick {
        Some(nick) if ambiguous => format!("{nick} ({user_id})"),
        Some(nick) => nick.to_owned(),
        None => user_id.to_owned(),
    }
}

/// The nick list prefix for a rank, after the IRCd convention `~` owner,
/// `&` admin, `@` op, `+` voice. Matrix's default kick and ban level is 50,
/// which is what IRC ops can do, so moderators are `@`; IRC bridges such as
/// matrix-appservice-irc map `+o` to power level 50 likewise, and
/// weechat-matrix shows the same `& @ +` prefixes.
pub(super) fn rank_prefix(rank: RoomMemberRank) -> &'static str {
    match rank {
        RoomMemberRank::Creator => "~",
        RoomMemberRank::Administrator => "&",
        RoomMemberRank::Moderator => "@",
        RoomMemberRank::Voiced => "+",
        RoomMemberRank::Member => "",
    }
}

pub(super) fn profile_nick(profile: &RoomUserProfile) -> String {
    nick_label(
        &profile.user_id,
        profile.nick.as_deref(),
        profile.nick_ambiguous,
    )
}

/// The nick together with the user ID, as mIRC shows `nick (user@host)` on
/// joins and parts.
fn nick_with_user_id(profile: &RoomUserProfile) -> String {
    nick_label(&profile.user_id, profile.nick.as_deref(), true)
}

/// A line a user's message or action makes: its text, where the sender's
/// nick is in it, and where the body starts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct MessageLine {
    pub(super) text: String,
    pub(super) nick: Range<usize>,
    pub(super) body_offset: usize,
}

/// `prefix`, then `before`, the sender's nick and `after`, then `body`.
fn line_with_nick(
    prefix: &str,
    before: &str,
    sender: &RoomUserProfile,
    after: &str,
    body: &str,
) -> MessageLine {
    let nick_start = prefix.len() + before.len();
    let mut text = format!("{prefix}{before}{}", profile_nick(sender));
    let nick = nick_start..text.len();
    text.push_str(after);
    let body_offset = text.len();
    text.push_str(body);
    MessageLine {
        text,
        nick,
        body_offset,
    }
}

/// `[ts] <Nick> body`, `[ts] * Nick body`, or `[ts] -Nick- body`. The
/// timestamp `prefix` is `"[ts] "`, or empty when timestamps are off.
pub(super) fn message_line(
    prefix: &str,
    presentation: RoomMessagePresentationKind,
    sender: &RoomUserProfile,
    body: &str,
) -> MessageLine {
    match presentation {
        RoomMessagePresentationKind::Text => line_with_nick(prefix, "<", sender, "> ", body),
        RoomMessagePresentationKind::Emote => action_line(prefix, sender, body),
        RoomMessagePresentationKind::Notice => line_with_nick(prefix, "-", sender, "- ", body),
    }
}

/// An action-style line, as for an emote or media: `[ts] * Nick sent
/// image: ...`.
pub(super) fn action_line(prefix: &str, sender: &RoomUserProfile, body: &str) -> MessageLine {
    line_with_nick(prefix, "* ", sender, " ", body)
}

/// A line reporting an event rather than something a user said.
pub(super) fn event_line(prefix: &str, body: &str) -> String {
    format!("{prefix}* {body}")
}

/// A line about the window itself rather than a timed event, such as the
/// lines mIRC prints when a channel window opens: `* body`, no timestamp.
pub(super) fn context_line(body: &str) -> String {
    format!("* {body}")
}

/// mIRC's first line in a channel window, with the room ID standing in for
/// the channel name a Matrix display name cannot uniquely give.
pub(super) fn now_talking_text(display_name: &str, room_id: &str) -> String {
    format!("Now talking in {display_name} ({room_id})")
}

pub(super) fn topic_is_text(topic: &str) -> String {
    format!("Topic is '{topic}'")
}

pub(super) const ENCRYPTED_ROOM_TEXT: &str = "Messages in this room are end-to-end encrypted";

pub(super) fn invited_text(display_name: &str) -> String {
    format!("You are invited to join {display_name}")
}

pub(super) const INVITE_RESPONSE_TEXT: &str = "Use /accept or /reject to respond.";

pub(super) fn topic_change_text(sender: &RoomUserProfile, topic: &str) -> String {
    format!("{} changes topic to '{topic}'", profile_nick(sender))
}

pub(super) fn name_change_text(sender: &RoomUserProfile, name: Option<&str>) -> String {
    let op = profile_nick(sender);
    match name {
        Some(name) => format!("{op} changes room name to '{name}'"),
        None => format!("{op} removes room name"),
    }
}

pub(super) fn join_rule_change_text(sender: &RoomUserProfile, rule: &RoomJoinRule) -> String {
    let op = profile_nick(sender);
    let made = match rule {
        RoomJoinRule::Public => "public",
        RoomJoinRule::Invite => "invite only",
        RoomJoinRule::Knock => "ask to join",
        RoomJoinRule::Restricted => "open to members of its spaces",
        RoomJoinRule::KnockRestricted => "ask to join, or open to members of its spaces",
        RoomJoinRule::Private => "private",
        RoomJoinRule::Other(rule) => return format!("{op} sets the join rule to {rule}"),
    };
    format!("{op} makes the room {made}")
}

pub(super) fn encryption_change_text(sender: &RoomUserProfile) -> String {
    format!("{} turns on end-to-end encryption", profile_nick(sender))
}

pub(super) fn member_change_text(
    sender: &RoomUserProfile,
    change: &RoomMemberChange,
    target: &RoomUserProfile,
    reason: Option<&str>,
) -> String {
    let op = profile_nick(sender);
    let nick = profile_nick(target);
    let text = match change {
        RoomMemberChange::Joined | RoomMemberChange::InvitationAccepted => {
            format!("Joins: {}", nick_with_user_id(target))
        }
        RoomMemberChange::Left => format!("Parts: {}", nick_with_user_id(target)),
        RoomMemberChange::Kicked => format!("{nick} was kicked by {op}"),
        RoomMemberChange::Banned | RoomMemberChange::KickedAndBanned => {
            format!("{nick} was banned by {op}")
        }
        RoomMemberChange::Unbanned => format!("{op} removed ban on {nick}"),
        RoomMemberChange::Invited => format!("{op} invites {nick}"),
        RoomMemberChange::InvitationRejected => format!("{nick} declined the invitation"),
        RoomMemberChange::InvitationRevoked => format!("{op} revoked the invitation for {nick}"),
        RoomMemberChange::Knocked => format!("{nick} knocks"),
        // Accepting a knock invites the knocker; they join separately.
        RoomMemberChange::KnockAccepted => format!("{op} accepted knock from {nick}"),
        RoomMemberChange::KnockRetracted => format!("{nick} withdrew knock"),
        RoomMemberChange::KnockDenied => format!("{op} denied knock from {nick}"),
        RoomMemberChange::NickChanged { old, new } => format!("{old} is now known as {new}"),
        RoomMemberChange::AvatarChanged => format!("{nick} changed avatar"),
    };
    match reason {
        // Clients send an empty reason when the user gave none.
        Some(reason) if !reason.is_empty() => format!("{text} ({reason})"),
        _ => text,
    }
}

/// mIRC reports privilege changes as `* Op sets mode: +o Nick`. Matrix
/// power levels are numbers rather than a few flags: the two room modes
/// that are flags take mIRC's letters (`+m` moderated, `+t` only operators
/// set the topic; pluses first, `m` before `t`), and the levels are numbers:
/// `Op sets mode: +t-m; power level: Bob 0 -> 50, Carol 50 -> 0; default 0
/// -> 10`, with `; room permissions changed` when the levels needed for
/// other actions changed too. Without a mode it is `Op sets power level:
/// ...`, or `Op changes room permissions` when only those changed.
/// mirx-core reports at least one change.
pub(super) fn power_levels_change_text(
    sender: &RoomUserProfile,
    changes: &[RoomPowerLevelUserChange],
    users_default: Option<&RoomPowerLevelValueChange>,
    topic_by_operators: Option<bool>,
    moderated: Option<bool>,
    other_permissions_changed: bool,
) -> String {
    let op = profile_nick(sender);
    let users = (!changes.is_empty()).then(|| {
        changes
            .iter()
            .map(|change| {
                format!(
                    "{} {} -> {}",
                    profile_nick(&change.target),
                    change.old,
                    change.new
                )
            })
            .collect::<Vec<_>>()
            .join(", ")
    });
    let default = users_default.map(|change| format!("default {} -> {}", change.old, change.new));
    let levels = users.into_iter().chain(default).collect::<Vec<_>>();
    let modes = [('m', moderated), ('t', topic_by_operators)];
    let signed = |sign: char, on: bool| {
        let letters = modes
            .iter()
            .filter(|(_, mode)| *mode == Some(on))
            .map(|(letter, _)| *letter)
            .collect::<String>();
        if letters.is_empty() {
            letters
        } else {
            format!("{sign}{letters}")
        }
    };
    let mode = format!("{}{}", signed('+', true), signed('-', false));
    let levels = levels.join("; ");
    let head = match (mode.is_empty(), levels.is_empty()) {
        (true, true) => return format!("{op} changes room permissions"),
        (true, false) => format!("{op} sets power level: {levels}"),
        (false, true) => format!("{op} sets mode: {mode}"),
        (false, false) => format!("{op} sets mode: {mode}; power level: {levels}"),
    };
    let permissions = if other_permissions_changed {
        "; room permissions changed"
    } else {
        ""
    };
    format!("{head}{permissions}")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn profile(user_id: &str, nick: Option<&str>, ambiguous: bool) -> RoomUserProfile {
        RoomUserProfile {
            user_id: user_id.to_owned(),
            nick: nick.map(str::to_owned),
            nick_ambiguous: ambiguous,
        }
    }

    #[test]
    fn nick_label_shows_the_nick_and_the_id_when_ambiguous() {
        assert_eq!(nick_label("@alice:example", Some("Alice"), false), "Alice");
        assert_eq!(nick_label("@ghost:example", Some("ghost"), false), "ghost");
        assert_eq!(
            nick_label("@ghost:example", Some("ghost"), true),
            "ghost (@ghost:example)"
        );
        // No nick: not a member the core knows.
        assert_eq!(nick_label("@bob:example", None, false), "@bob:example");
        assert_eq!(nick_label("@bob:example", None, true), "@bob:example");
    }

    fn message_line_parts(line: MessageLine) -> (String, String, usize) {
        let nick = line.text[line.nick.clone()].to_owned();
        (line.text, nick, line.body_offset)
    }

    #[test]
    fn message_lines_use_mirc_sender_prefixes() {
        let alice = profile("@alice:example", Some("Alice"), false);
        let line =
            |presentation| message_line_parts(message_line("[12:34] ", presentation, &alice, "hi"));

        // The body offset is where the body starts: message styles shift
        // by it. The nick range is the nick alone, a hotlink.
        assert_eq!(
            line(RoomMessagePresentationKind::Text),
            ("[12:34] <Alice> hi".to_owned(), "Alice".to_owned(), 16)
        );
        assert_eq!(
            line(RoomMessagePresentationKind::Emote),
            ("[12:34] * Alice hi".to_owned(), "Alice".to_owned(), 16)
        );
        assert_eq!(
            line(RoomMessagePresentationKind::Notice),
            ("[12:34] -Alice- hi".to_owned(), "Alice".to_owned(), 16)
        );
        assert_eq!(
            message_line("[12:34] ", RoomMessagePresentationKind::Text, &alice, "hi").nick,
            9..14
        );
        assert_eq!(
            message_line_parts(action_line("[12:34] ", &alice, "sent image: a.png")),
            (
                "[12:34] * Alice sent image: a.png".to_owned(),
                "Alice".to_owned(),
                16
            )
        );
        assert_eq!(
            event_line("[12:34] ", &topic_change_text(&alice, "release day")),
            "[12:34] * Alice changes topic to 'release day'"
        );
        assert_eq!(
            name_change_text(&alice, Some("Ops")),
            "Alice changes room name to 'Ops'"
        );
        assert_eq!(name_change_text(&alice, None), "Alice removes room name");
        for (rule, text) in [
            (RoomJoinRule::Public, "Alice makes the room public"),
            (RoomJoinRule::Invite, "Alice makes the room invite only"),
            (RoomJoinRule::Knock, "Alice makes the room ask to join"),
            (
                RoomJoinRule::Restricted,
                "Alice makes the room open to members of its spaces",
            ),
            (
                RoomJoinRule::KnockRestricted,
                "Alice makes the room ask to join, or open to members of its spaces",
            ),
            (RoomJoinRule::Private, "Alice makes the room private"),
            (
                RoomJoinRule::Other("org.example.vip".to_owned()),
                "Alice sets the join rule to org.example.vip",
            ),
        ] {
            assert_eq!(join_rule_change_text(&alice, &rule), text);
        }
        assert_eq!(
            encryption_change_text(&alice),
            "Alice turns on end-to-end encryption"
        );
        assert_eq!(
            message_line_parts(message_line(
                "",
                RoomMessagePresentationKind::Text,
                &alice,
                "hi"
            )),
            ("<Alice> hi".to_owned(), "Alice".to_owned(), 8)
        );
        assert_eq!(
            message_line("", RoomMessagePresentationKind::Notice, &alice, "hi").nick,
            1..6
        );
        assert_eq!(
            message_line("", RoomMessagePresentationKind::Emote, &alice, "hi").nick,
            2..7
        );
        assert_eq!(event_line("", "message redacted"), "* message redacted");
    }

    #[test]
    fn an_ambiguous_nick_spans_its_whole_label() {
        let sam = profile("@sam1:example", Some("Sam"), true);
        assert_eq!(
            message_line_parts(message_line(
                "[12:34] ",
                RoomMessagePresentationKind::Text,
                &sam,
                "yo"
            )),
            (
                "[12:34] <Sam (@sam1:example)> yo".to_owned(),
                "Sam (@sam1:example)".to_owned(),
                30
            )
        );
    }

    #[test]
    fn window_opening_lines_follow_mirc() {
        assert_eq!(
            context_line(&now_talking_text("Ops", "!ops:example")),
            "* Now talking in Ops (!ops:example)"
        );
        assert_eq!(
            context_line(&topic_is_text("it's release day")),
            "* Topic is 'it's release day'"
        );
        assert_eq!(
            context_line(&invited_text("Lobby")),
            "* You are invited to join Lobby"
        );
    }

    #[test]
    fn member_change_text_covers_every_change() {
        let op = profile("@op:example", Some("Op"), false);
        let carol = profile("@carol:example", Some("Carol"), false);
        let sam = profile("@sam:example", Some("Sam"), true);
        // Without a display name, the core names a member by localpart.
        let anon = profile("@anon:example", Some("anon"), false);
        let text = |change: RoomMemberChange, target: &RoomUserProfile, reason: Option<&str>| {
            member_change_text(&op, &change, target, reason)
        };

        assert_eq!(
            text(RoomMemberChange::Joined, &carol, None),
            "Joins: Carol (@carol:example)"
        );
        assert_eq!(
            text(RoomMemberChange::InvitationAccepted, &anon, None),
            "Joins: anon (@anon:example)"
        );
        assert_eq!(
            text(RoomMemberChange::Left, &carol, Some("bye")),
            "Parts: Carol (@carol:example) (bye)"
        );
        assert_eq!(
            text(RoomMemberChange::Kicked, &sam, Some("flood")),
            "Sam (@sam:example) was kicked by Op (flood)"
        );
        assert_eq!(
            text(RoomMemberChange::Banned, &carol, None),
            "Carol was banned by Op"
        );
        assert_eq!(
            text(RoomMemberChange::KickedAndBanned, &carol, Some("")),
            "Carol was banned by Op"
        );
        assert_eq!(
            text(RoomMemberChange::Unbanned, &carol, None),
            "Op removed ban on Carol"
        );
        assert_eq!(
            text(RoomMemberChange::Invited, &carol, None),
            "Op invites Carol"
        );
        assert_eq!(
            text(RoomMemberChange::InvitationRejected, &carol, None),
            "Carol declined the invitation"
        );
        assert_eq!(
            text(RoomMemberChange::InvitationRevoked, &carol, None),
            "Op revoked the invitation for Carol"
        );
        assert_eq!(
            text(RoomMemberChange::Knocked, &carol, Some("let me in")),
            "Carol knocks (let me in)"
        );
        assert_eq!(
            text(RoomMemberChange::KnockAccepted, &carol, None),
            "Op accepted knock from Carol"
        );
        assert_eq!(
            text(RoomMemberChange::KnockRetracted, &carol, None),
            "Carol withdrew knock"
        );
        assert_eq!(
            text(RoomMemberChange::KnockDenied, &carol, None),
            "Op denied knock from Carol"
        );
        assert_eq!(
            text(
                RoomMemberChange::NickChanged {
                    old: "Carol".to_owned(),
                    new: "Caz".to_owned(),
                },
                &carol,
                None,
            ),
            "Carol is now known as Caz"
        );
        assert_eq!(
            text(
                RoomMemberChange::NickChanged {
                    old: "carol".to_owned(),
                    new: "Carol".to_owned(),
                },
                &carol,
                None,
            ),
            "carol is now known as Carol"
        );
        assert_eq!(
            text(RoomMemberChange::AvatarChanged, &carol, None),
            "Carol changed avatar"
        );
    }

    #[test]
    fn rank_prefix_follows_ircd_convention() {
        assert_eq!(rank_prefix(RoomMemberRank::Creator), "~");
        assert_eq!(rank_prefix(RoomMemberRank::Administrator), "&");
        assert_eq!(rank_prefix(RoomMemberRank::Moderator), "@");
        assert_eq!(rank_prefix(RoomMemberRank::Voiced), "+");
        assert_eq!(rank_prefix(RoomMemberRank::Member), "");
    }

    #[test]
    fn power_levels_change_text_covers_every_shape() {
        let op = profile("@op:example", Some("Op"), false);
        let bob = profile("@bob:example", Some("Bob"), false);
        let sam = profile("@sam:example", Some("Sam"), true);
        let anon = profile("@anon:example", None, false);
        let change = |target: &RoomUserProfile, old, new| RoomPowerLevelUserChange {
            target: target.clone(),
            old,
            new,
        };
        let default = RoomPowerLevelValueChange { old: 0, new: 10 };
        // Without modes.
        let text = |changes: &[RoomPowerLevelUserChange],
                    users_default: Option<&RoomPowerLevelValueChange>,
                    other| {
            power_levels_change_text(&op, changes, users_default, None, None, other)
        };

        assert_eq!(
            text(&[change(&bob, 0, 50)], None, false),
            "Op sets power level: Bob 0 -> 50"
        );
        assert_eq!(
            text(&[change(&anon, 0, 100), change(&sam, 50, 0)], None, false),
            "Op sets power level: @anon:example 0 -> 100, Sam (@sam:example) 50 -> 0"
        );
        assert_eq!(
            text(&[], Some(&default), false),
            "Op sets power level: default 0 -> 10"
        );
        assert_eq!(
            text(&[change(&bob, 0, 50)], Some(&default), false),
            "Op sets power level: Bob 0 -> 50; default 0 -> 10"
        );
        assert_eq!(text(&[], None, true), "Op changes room permissions");
        assert_eq!(
            text(&[change(&bob, 0, 50)], None, true),
            "Op sets power level: Bob 0 -> 50; room permissions changed"
        );
        assert_eq!(
            text(&[change(&bob, -1, 0)], Some(&default), true),
            "Op sets power level: Bob -1 -> 0; default 0 -> 10; room permissions changed"
        );

        // With modes: pluses first, m before t.
        for (topic_by_operators, moderated, mode) in [
            (None, Some(true), "+m"),
            (Some(false), None, "-t"),
            (Some(true), Some(true), "+mt"),
            (Some(true), Some(false), "+t-m"),
            (Some(false), Some(true), "+m-t"),
            (Some(false), Some(false), "-mt"),
        ] {
            assert_eq!(
                power_levels_change_text(&op, &[], None, topic_by_operators, moderated, false),
                format!("Op sets mode: {mode}")
            );
        }
        assert_eq!(
            power_levels_change_text(&op, &[change(&bob, 0, 1)], None, None, Some(true), false),
            "Op sets mode: +m; power level: Bob 0 -> 1"
        );
        assert_eq!(
            power_levels_change_text(
                &op,
                &[change(&bob, 1, 0)],
                Some(&default),
                Some(true),
                Some(false),
                true
            ),
            "Op sets mode: +t-m; power level: Bob 1 -> 0; default 0 -> 10; \
             room permissions changed"
        );
        assert_eq!(
            power_levels_change_text(&op, &[], None, Some(true), None, true),
            "Op sets mode: +t; room permissions changed"
        );
    }
}
