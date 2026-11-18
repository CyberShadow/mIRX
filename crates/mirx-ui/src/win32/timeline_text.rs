//! mIRC-style text for timeline lines and nicks.
//!
//! Every user shown in the UI goes through [`nick_label`], so duplicate
//! display names are disambiguated the same way in the timeline, the nick
//! list, and the typing indicator. Ambiguity itself is decided by mirx-core.

use mirx_core::{
    RoomMemberChange, RoomMemberRank, RoomMessagePresentationKind, RoomPowerLevelUserChange,
    RoomPowerLevelValueChange, RoomUserProfile,
};

/// The label for a user: their display name, the display name with the user
/// ID when another member shares it, or the user ID when they have none.
pub(super) fn nick_label(user_id: &str, display_name: Option<&str>, ambiguous: bool) -> String {
    match display_name {
        // An empty display name cannot identify anyone; Matrix permits it.
        Some(name) if !name.is_empty() => {
            if ambiguous {
                format!("{name} ({user_id})")
            } else {
                name.to_owned()
            }
        }
        _ => user_id.to_owned(),
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
        profile.display_name.as_deref(),
        profile.display_name_ambiguous,
    )
}

/// The nick together with the user ID, as mIRC shows `nick (user@host)` on
/// joins and parts.
fn nick_with_user_id(profile: &RoomUserProfile) -> String {
    nick_label(&profile.user_id, profile.display_name.as_deref(), true)
}

/// Log lines start with a `[hh:mm] ` timestamp; wrapped continuation lines
/// are indented by this sample's width so they align past it.
pub(super) const TIMELINE_CONTINUATION_INDENT_SAMPLE: &str = "[00:00] ";

/// `[ts] <Nick> body`, `[ts] * Nick body`, or `[ts] -Nick- body`.
pub(super) fn message_line(
    timestamp: &str,
    presentation: RoomMessagePresentationKind,
    sender: &RoomUserProfile,
    body: &str,
) -> String {
    let nick = profile_nick(sender);
    match presentation {
        RoomMessagePresentationKind::Text => format!("{timestamp} <{nick}> {body}"),
        RoomMessagePresentationKind::Emote => format!("{timestamp} * {nick} {body}"),
        RoomMessagePresentationKind::Notice => format!("{timestamp} -{nick}- {body}"),
    }
}

/// An action-style line, as for media: `[ts] * Nick sent image: ...`.
pub(super) fn action_line(timestamp: &str, sender: &RoomUserProfile, body: &str) -> String {
    format!("{timestamp} * {} {body}", profile_nick(sender))
}

/// A line reporting an event rather than something a user said.
pub(super) fn event_line(timestamp: &str, body: &str) -> String {
    format!("{timestamp} * {body}")
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
        RoomMemberChange::DisplayNameChanged { old, new } => format!(
            "{} is now known as {}",
            nick_label(&target.user_id, old.as_deref(), false),
            nick_label(&target.user_id, new.as_deref(), false),
        ),
        RoomMemberChange::AvatarChanged => format!("{nick} changed avatar"),
    };
    match reason {
        // Clients send an empty reason when the user gave none.
        Some(reason) if !reason.is_empty() => format!("{text} ({reason})"),
        _ => text,
    }
}

/// mIRC reports privilege changes as `* Op sets mode: +o Nick`. Matrix power
/// levels are numbers rather than a few flags, so mode letters would lose
/// information; the line states the levels instead:
/// `Op sets power level: Bob 0 -> 50, Carol 50 -> 0; default 0 -> 10`, with
/// `; room permissions changed` when the levels needed for actions changed
/// too, or `Op changes room permissions` when only those did. mirx-core
/// reports at least one change.
pub(super) fn power_levels_change_text(
    sender: &RoomUserProfile,
    changes: &[RoomPowerLevelUserChange],
    users_default: Option<&RoomPowerLevelValueChange>,
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
    if levels.is_empty() {
        return format!("{op} changes room permissions");
    }
    let permissions = if other_permissions_changed {
        "; room permissions changed"
    } else {
        ""
    };
    format!("{op} sets power level: {}{permissions}", levels.join("; "))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn profile(user_id: &str, display_name: Option<&str>, ambiguous: bool) -> RoomUserProfile {
        RoomUserProfile {
            user_id: user_id.to_owned(),
            display_name: display_name.map(str::to_owned),
            display_name_ambiguous: ambiguous,
        }
    }

    #[test]
    fn nick_label_prefers_display_name_and_disambiguates_duplicates() {
        assert_eq!(nick_label("@alice:example", Some("Alice"), false), "Alice");
        assert_eq!(
            nick_label("@sam1:example", Some("Sam"), true),
            "Sam (@sam1:example)"
        );
        assert_eq!(nick_label("@bob:example", None, false), "@bob:example");
        assert_eq!(nick_label("@bob:example", None, true), "@bob:example");
        assert_eq!(nick_label("@eve:example", Some(""), false), "@eve:example");
    }

    #[test]
    fn message_lines_use_mirc_sender_prefixes() {
        let alice = profile("@alice:example", Some("Alice"), false);
        let line = |presentation| message_line("[12:34]", presentation, &alice, "hi");

        assert_eq!(
            line(RoomMessagePresentationKind::Text),
            "[12:34] <Alice> hi"
        );
        assert_eq!(
            line(RoomMessagePresentationKind::Emote),
            "[12:34] * Alice hi"
        );
        assert_eq!(
            line(RoomMessagePresentationKind::Notice),
            "[12:34] -Alice- hi"
        );
        assert_eq!(
            action_line("[12:34]", &alice, "sent image: a.png"),
            "[12:34] * Alice sent image: a.png"
        );
        assert_eq!(
            event_line("[12:34]", &topic_change_text(&alice, "release day")),
            "[12:34] * Alice changes topic to 'release day'"
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
        let anon = profile("@anon:example", None, false);
        let text = |change: RoomMemberChange, target: &RoomUserProfile, reason: Option<&str>| {
            member_change_text(&op, &change, target, reason)
        };

        assert_eq!(
            text(RoomMemberChange::Joined, &carol, None),
            "Joins: Carol (@carol:example)"
        );
        assert_eq!(
            text(RoomMemberChange::InvitationAccepted, &anon, None),
            "Joins: @anon:example"
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
                RoomMemberChange::DisplayNameChanged {
                    old: Some("Carol".to_owned()),
                    new: Some("Caz".to_owned()),
                },
                &carol,
                None,
            ),
            "Carol is now known as Caz"
        );
        assert_eq!(
            text(
                RoomMemberChange::DisplayNameChanged {
                    old: None,
                    new: Some("Carol".to_owned()),
                },
                &carol,
                None,
            ),
            "@carol:example is now known as Carol"
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

        assert_eq!(
            power_levels_change_text(&op, &[change(&bob, 0, 50)], None, false),
            "Op sets power level: Bob 0 -> 50"
        );
        assert_eq!(
            power_levels_change_text(
                &op,
                &[change(&anon, 0, 100), change(&sam, 50, 0)],
                None,
                false
            ),
            "Op sets power level: @anon:example 0 -> 100, Sam (@sam:example) 50 -> 0"
        );
        assert_eq!(
            power_levels_change_text(&op, &[], Some(&default), false),
            "Op sets power level: default 0 -> 10"
        );
        assert_eq!(
            power_levels_change_text(&op, &[change(&bob, 0, 50)], Some(&default), false),
            "Op sets power level: Bob 0 -> 50; default 0 -> 10"
        );
        assert_eq!(
            power_levels_change_text(&op, &[], None, true),
            "Op changes room permissions"
        );
        assert_eq!(
            power_levels_change_text(&op, &[change(&bob, 0, 50)], None, true),
            "Op sets power level: Bob 0 -> 50; room permissions changed"
        );
        assert_eq!(
            power_levels_change_text(&op, &[change(&bob, -1, 0)], Some(&default), true),
            "Op sets power level: Bob -1 -> 0; default 0 -> 10; room permissions changed"
        );
    }
}
