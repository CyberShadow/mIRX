//! Where a failed request's error prints, as mIRC prints a refused kick or
//! topic change (`#chan You're not channel operator`) in the channel window.
//!
//! Pure code over the core's error events.

use mirx_core::{CoreErrorEvent, Operation};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ErrorDestination<'e> {
    /// The window of the room the user acted in or on, if it has one.
    Room(&'e str),
    /// The window the user typed the command in, as `* /{command}: ...`.
    ActiveWindow { command: &'static str },
    /// The Status window.
    Status,
}

/// Where mIRC would print the error. Actions the user took in or on a room
/// print in its window; the answer to a command about a user prints where
/// it was typed. Everything else prints in Status: account-wide
/// operations, the automatic ones (read markers), and the room loads that
/// already show their failure in the window (history, images, quotes).
pub(super) fn error_destination(error: &CoreErrorEvent) -> ErrorDestination<'_> {
    match error.operation {
        Operation::AcceptRoomInvite
        | Operation::RejectRoomInvite
        | Operation::LeaveRoom
        | Operation::KickRoomMember
        | Operation::BanRoomMember
        | Operation::UnbanRoomMember
        | Operation::InviteRoomMember
        | Operation::SetRoomMemberPowerLevel
        | Operation::SetRoomTopic
        | Operation::SetRoomNotificationMode
        | Operation::SendMessage
        | Operation::SendFile
        | Operation::EditMessage
        | Operation::SendReaction
        | Operation::RedactEvent => ErrorDestination::Room(
            error
                .room_id
                .as_deref()
                .unwrap_or_else(|| panic!("a {:?} error must name its room", error.operation)),
        ),
        Operation::IgnoreUser => ErrorDestination::ActiveWindow { command: "ignore" },
        Operation::UnignoreUser => ErrorDestination::ActiveWindow {
            command: "unignore",
        },
        Operation::FetchUserProfile => ErrorDestination::ActiveWindow { command: "whois" },
        Operation::ListUserDevices => ErrorDestination::ActiveWindow { command: "devices" },
        Operation::RequestVerification => ErrorDestination::ActiveWindow { command: "verify" },
        Operation::Login
        | Operation::RestoreSession
        | Operation::SessionUpdate
        | Operation::StartSync
        | Operation::StopSync
        | Operation::JoinRoom
        | Operation::CreateDirectRoom
        | Operation::CreateRoom
        | Operation::SetDisplayName
        | Operation::HighlightWords
        | Operation::AddHighlightWord
        | Operation::RemoveHighlightWord
        | Operation::IgnoredUsers
        | Operation::MarkRoomRead
        | Operation::BackfillRoomHistory
        | Operation::FetchRoomMedia
        | Operation::FetchRoomEvent
        | Operation::ListPublicRooms
        | Operation::RoomList
        | Operation::RoomReadState
        | Operation::SendQueue
        | Operation::E2eeStatus
        | Operation::SetupE2eeRecovery
        | Operation::RecoverE2ee
        | Operation::Verification
        | Operation::Logout
        | Operation::Shutdown => ErrorDestination::Status,
    }
}

#[cfg(test)]
mod tests {
    use mirx_core::{CoreErrorEvent, Operation};

    use super::{ErrorDestination, error_destination};

    fn error(operation: Operation, room_id: Option<&str>) -> CoreErrorEvent {
        CoreErrorEvent {
            operation,
            room_id: room_id.map(str::to_owned),
            message: "failed".to_owned(),
        }
    }

    const ROOM_OPERATIONS: [Operation; 15] = [
        Operation::AcceptRoomInvite,
        Operation::RejectRoomInvite,
        Operation::LeaveRoom,
        Operation::KickRoomMember,
        Operation::BanRoomMember,
        Operation::UnbanRoomMember,
        Operation::InviteRoomMember,
        Operation::SetRoomMemberPowerLevel,
        Operation::SetRoomTopic,
        Operation::SetRoomNotificationMode,
        Operation::SendMessage,
        Operation::SendFile,
        Operation::EditMessage,
        Operation::SendReaction,
        Operation::RedactEvent,
    ];

    #[test]
    fn actions_on_a_room_print_in_its_window() {
        for operation in ROOM_OPERATIONS {
            let error = error(operation, Some("!ops:example"));
            assert_eq!(
                error_destination(&error),
                ErrorDestination::Room("!ops:example"),
                "{operation:?}"
            );
        }
    }

    #[test]
    fn everything_else_prints_in_status_with_or_without_a_room() {
        for operation in [
            Operation::Login,
            Operation::RestoreSession,
            Operation::SessionUpdate,
            Operation::StartSync,
            Operation::StopSync,
            Operation::JoinRoom,
            Operation::CreateDirectRoom,
            Operation::CreateRoom,
            Operation::SetDisplayName,
            Operation::HighlightWords,
            Operation::AddHighlightWord,
            Operation::RemoveHighlightWord,
            Operation::IgnoredUsers,
            Operation::ListPublicRooms,
            Operation::RoomList,
            Operation::RoomReadState,
            Operation::SendQueue,
            Operation::E2eeStatus,
            Operation::SetupE2eeRecovery,
            Operation::RecoverE2ee,
            Operation::Verification,
            Operation::Logout,
            Operation::Shutdown,
        ] {
            assert_eq!(
                error_destination(&error(operation, None)),
                ErrorDestination::Status,
                "{operation:?}"
            );
        }
        for operation in [
            Operation::MarkRoomRead,
            Operation::BackfillRoomHistory,
            Operation::FetchRoomMedia,
            Operation::FetchRoomEvent,
            Operation::CreateRoom,
        ] {
            assert_eq!(
                error_destination(&error(operation, Some("!ops:example"))),
                ErrorDestination::Status,
                "{operation:?}"
            );
        }
    }

    #[test]
    fn user_commands_answer_in_the_active_window() {
        for (operation, command) in [
            (Operation::IgnoreUser, "ignore"),
            (Operation::UnignoreUser, "unignore"),
            (Operation::FetchUserProfile, "whois"),
            (Operation::ListUserDevices, "devices"),
            (Operation::RequestVerification, "verify"),
        ] {
            assert_eq!(
                error_destination(&error(operation, None)),
                ErrorDestination::ActiveWindow { command },
                "{operation:?}"
            );
        }
    }

    #[test]
    #[should_panic(expected = "a KickRoomMember error must name its room")]
    fn a_room_action_error_without_its_room_is_a_core_bug() {
        error_destination(&error(Operation::KickRoomMember, None));
    }
}
