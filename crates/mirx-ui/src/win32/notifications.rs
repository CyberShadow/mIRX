//! How a room's Matrix notification mode shapes mIRC's activity colours and
//! frame flashing. mIRC has no per-window notification setting; Matrix
//! rooms have three (All Messages, Mentions and Keywords, Mute), kept in
//! the user's push rules.
//!
//! Pure code: the colour a room's activity may reach, and whether it wants
//! the user's attention.

use mirx_core::RoomNotificationMode;

use super::settings::TipsSetting;
use crate::shared::ViewActivityLevel;

/// The colour a room's activity may reach: a muted room only ever shows that
/// something happened, whatever the line or the server's counts say.
pub(super) fn room_activity_level(
    level: ViewActivityLevel,
    mode: RoomNotificationMode,
) -> ViewActivityLevel {
    match mode {
        RoomNotificationMode::Mute => level.min(ViewActivityLevel::Event),
        RoomNotificationMode::AllMessages | RoomNotificationMode::MentionsAndKeywordsOnly => level,
    }
}

/// mIRC flashes the frame for highlights and query messages; the room's
/// mode decides whether a query message is worth it, and a muted room never
/// flashes. `level` is the uncapped one.
pub(super) fn activity_wants_attention(
    level: ViewActivityLevel,
    room_is_direct: bool,
    mode: RoomNotificationMode,
) -> bool {
    match (mode, level) {
        (RoomNotificationMode::Mute, _) => false,
        (_, ViewActivityLevel::Highlight) => true,
        (RoomNotificationMode::AllMessages, ViewActivityLevel::Message) => room_is_direct,
        (RoomNotificationMode::MentionsAndKeywordsOnly, ViewActivityLevel::Message) => false,
        (_, ViewActivityLevel::Event) => false,
    }
}

/// Whether a line shows a tip (mIRC's Options > Display > Tips) while mirx
/// is not the active application: what the options choose, capped by the
/// room's mode as flashing is. `level` is the uncapped one.
pub(super) fn activity_wants_tip(
    level: ViewActivityLevel,
    room_is_direct: bool,
    mode: RoomNotificationMode,
    tips: &TipsSetting,
) -> bool {
    match (mode, level) {
        (RoomNotificationMode::Mute, _) => false,
        (_, ViewActivityLevel::Highlight) => tips.highlights,
        (RoomNotificationMode::AllMessages, ViewActivityLevel::Message) => {
            if room_is_direct {
                tips.direct
            } else {
                tips.rooms
            }
        }
        (RoomNotificationMode::MentionsAndKeywordsOnly, ViewActivityLevel::Message) => false,
        (_, ViewActivityLevel::Event) => false,
    }
}

#[cfg(test)]
mod tests {
    use mirx_core::RoomNotificationMode::{AllMessages, MentionsAndKeywordsOnly, Mute};

    use super::super::settings::TipsSetting;
    use super::{activity_wants_attention, activity_wants_tip, room_activity_level};
    use crate::shared::ViewActivityLevel::{Event, Highlight, Message};

    #[test]
    fn mute_caps_activity_at_event_and_other_modes_keep_it() {
        for (level, mode, expected) in [
            (Event, AllMessages, Event),
            (Message, AllMessages, Message),
            (Highlight, AllMessages, Highlight),
            (Event, MentionsAndKeywordsOnly, Event),
            (Message, MentionsAndKeywordsOnly, Message),
            (Highlight, MentionsAndKeywordsOnly, Highlight),
            (Event, Mute, Event),
            (Message, Mute, Event),
            (Highlight, Mute, Event),
        ] {
            assert_eq!(
                room_activity_level(level, mode),
                expected,
                "{level:?} {mode:?}"
            );
        }
    }

    #[test]
    fn highlights_and_query_messages_want_attention_unless_the_mode_says_otherwise() {
        for (level, room_is_direct, mode, expected) in [
            (Event, false, AllMessages, false),
            (Event, true, AllMessages, false),
            (Message, false, AllMessages, false),
            (Message, true, AllMessages, true),
            (Highlight, false, AllMessages, true),
            (Highlight, true, AllMessages, true),
            (Event, true, MentionsAndKeywordsOnly, false),
            (Message, false, MentionsAndKeywordsOnly, false),
            (Message, true, MentionsAndKeywordsOnly, false),
            (Highlight, false, MentionsAndKeywordsOnly, true),
            (Highlight, true, MentionsAndKeywordsOnly, true),
            (Event, true, Mute, false),
            (Message, true, Mute, false),
            (Highlight, false, Mute, false),
            (Highlight, true, Mute, false),
        ] {
            assert_eq!(
                activity_wants_attention(level, room_is_direct, mode),
                expected,
                "{level:?} direct={room_is_direct} {mode:?}"
            );
        }
    }

    #[test]
    fn tips_follow_the_options_capped_by_the_rooms_mode() {
        let all = TipsSetting {
            highlights: true,
            direct: true,
            rooms: true,
        };
        let none = TipsSetting {
            highlights: false,
            direct: false,
            rooms: false,
        };
        for (level, room_is_direct, mode, tips, expected) in [
            (Highlight, false, AllMessages, all, true),
            (Highlight, true, MentionsAndKeywordsOnly, all, true),
            (Highlight, false, AllMessages, none, false),
            (Message, true, AllMessages, all, true),
            (Message, true, AllMessages, none, false),
            (Message, false, AllMessages, all, true),
            (Message, false, AllMessages, TipsSetting::default(), false),
            (Message, true, AllMessages, TipsSetting::default(), true),
            (Message, true, MentionsAndKeywordsOnly, all, false),
            (Message, false, MentionsAndKeywordsOnly, all, false),
            (Event, true, AllMessages, all, false),
            (Event, false, AllMessages, all, false),
            (Highlight, true, Mute, all, false),
            (Message, true, Mute, all, false),
            (Event, true, Mute, all, false),
        ] {
            assert_eq!(
                activity_wants_tip(level, room_is_direct, mode, &tips),
                expected,
                "{level:?} direct={room_is_direct} {mode:?} {tips:?}"
            );
        }
    }
}
