use std::collections::HashMap;
use std::rc::Rc;

use matrix_sdk::Client;
use matrix_sdk::authentication::AuthApi;
use matrix_sdk::encryption::recovery::RecoveryError;
use matrix_sdk::encryption::verification::VerificationRequest as MatrixVerificationRequest;
use tokio::task::JoinHandle as TokioJoinHandle;

use super::core::{PendingIdentityReset, WorkerState};
use super::{
    EventSender, emit_error, emit_event, emit_invalid_state, emit_matrix_error,
    emit_room_invalid_state, emit_room_matrix_error,
};
use crate::api::{
    AcceptRoomInviteRequest, BackfillRoomHistoryRequest, BanRoomMemberRequest,
    CreateDirectRoomRequest, CreateRoomRequest, E2eeIdentityResetOutcome, E2eeRecoveryOperation,
    E2eeRecoverySetupSucceeded, E2eeRecoverySucceeded, EditMessageRequest, Event,
    InviteRoomMemberRequest, JoinRoomRequest, KickRoomMemberRequest, LeaveRoomRequest,
    MarkRoomReadRequest, Operation, RecoverE2eeRequest, RedactEventRequest,
    RejectRoomInviteRequest, RenameDeviceRequest, RequestVerificationRequest,
    ResetE2eeIdentityRequest, RoomJoined, SendReactionRequest, SetDisplayNameRequest,
    SetRoomMemberPowerLevelRequest, SetRoomNotificationModeRequest, SetRoomTopicRequest,
    SetupE2eeRecoveryRequest, SignOutDeviceRequest, UnbanRoomMemberRequest, VerificationFlowId,
};
use crate::matrix::{self, BackupServerStatusMode, MatrixCoreError};

pub(super) async fn handle_join_room(
    event_tx: &EventSender,
    state: &mut WorkerState,
    request: JoinRoomRequest,
) -> bool {
    let WorkerState::Ready { client, .. } = state else {
        emit_invalid_state(
            event_tx,
            Operation::JoinRoom,
            "join room requires an authenticated session",
        );
        return false;
    };

    match matrix::join_room(client, &request).await {
        Ok(room_id) => {
            emit_snapshot_then_room_joined(client, event_tx, Operation::JoinRoom, room_id.into())
                .await
        }
        Err(err) => emit_matrix_error(event_tx, Operation::JoinRoom, err),
    }

    false
}

pub(super) async fn handle_accept_room_invite(
    event_tx: &EventSender,
    state: &mut WorkerState,
    request: AcceptRoomInviteRequest,
) -> bool {
    let WorkerState::Ready { client, .. } = state else {
        emit_room_invalid_state(
            event_tx,
            Operation::AcceptRoomInvite,
            &request.room_id,
            "accept room invitation requires an authenticated session",
        );
        return false;
    };

    match matrix::accept_room_invite(client, &request.room_id).await {
        Ok(()) => {
            emit_snapshot_then_room_joined(
                client,
                event_tx,
                Operation::AcceptRoomInvite,
                request.room_id,
            )
            .await
        }
        Err(err) => {
            emit_room_matrix_error(event_tx, Operation::AcceptRoomInvite, &request.room_id, err)
        }
    }

    false
}

pub(super) async fn handle_create_direct_room(
    event_tx: &EventSender,
    state: &mut WorkerState,
    request: CreateDirectRoomRequest,
) -> bool {
    let WorkerState::Ready { client, .. } = state else {
        emit_invalid_state(
            event_tx,
            Operation::CreateDirectRoom,
            "create direct room requires an authenticated session",
        );
        return false;
    };

    match matrix::create_direct_room(client, &request.user_id).await {
        Ok(room_id) => {
            emit_snapshot_then_room_joined(
                client,
                event_tx,
                Operation::CreateDirectRoom,
                room_id.into(),
            )
            .await
        }
        Err(err) => emit_matrix_error(event_tx, Operation::CreateDirectRoom, err),
    }

    false
}

pub(super) async fn handle_create_room(
    event_tx: &EventSender,
    state: &mut WorkerState,
    request: CreateRoomRequest,
) -> bool {
    let WorkerState::Ready { client, .. } = state else {
        emit_invalid_state(
            event_tx,
            Operation::CreateRoom,
            "create room requires an authenticated session",
        );
        return false;
    };

    match matrix::create_room(client, &request).await {
        Ok(room_id) => {
            emit_snapshot_then_room_joined(client, event_tx, Operation::CreateRoom, room_id.into())
                .await
        }
        Err(err) => emit_matrix_error(event_tx, Operation::CreateRoom, err),
    }

    false
}

/// Emits the room snapshot that lists a room just joined, then
/// `RoomJoined`, so the UI knows the room by the time it opens its window.
/// A failed snapshot is the operation's error about the room, and no
/// `RoomJoined` follows.
pub(super) async fn emit_snapshot_then_room_joined(
    client: &Client,
    event_tx: &EventSender,
    operation: Operation,
    room_id: String,
) {
    match matrix::emit_room_snapshot(client, event_tx).await {
        Ok(()) => emit_event(event_tx, Event::RoomJoined(RoomJoined { room_id })),
        Err(err) => emit_room_matrix_error(event_tx, operation, &room_id, err),
    }
}

pub(super) async fn handle_leave_room(
    event_tx: &EventSender,
    state: &mut WorkerState,
    request: LeaveRoomRequest,
) -> bool {
    let WorkerState::Ready { client, .. } = state else {
        emit_room_invalid_state(
            event_tx,
            Operation::LeaveRoom,
            &request.room_id,
            "leave room requires an authenticated session",
        );
        return false;
    };

    if let Err(err) = matrix::leave_joined_room(client, &request.room_id).await {
        emit_room_matrix_error(event_tx, Operation::LeaveRoom, &request.room_id, err);
        return false;
    }

    if let Err(err) = matrix::emit_room_snapshot(client, event_tx).await {
        emit_room_matrix_error(event_tx, Operation::LeaveRoom, &request.room_id, err);
    }

    false
}

pub(super) async fn handle_kick_room_member(
    event_tx: &EventSender,
    state: &mut WorkerState,
    request: KickRoomMemberRequest,
) -> bool {
    let WorkerState::Ready { client, .. } = state else {
        emit_room_invalid_state(
            event_tx,
            Operation::KickRoomMember,
            &request.room_id,
            "kick room member requires an authenticated session",
        );
        return false;
    };

    if let Err(err) = matrix::kick_room_member(client, &request).await {
        emit_room_matrix_error(event_tx, Operation::KickRoomMember, &request.room_id, err);
        return false;
    }

    if let Err(err) =
        matrix::emit_joined_room_members_snapshot(client, event_tx, &request.room_id).await
    {
        emit_room_matrix_error(event_tx, Operation::KickRoomMember, &request.room_id, err);
    }

    false
}

pub(super) async fn handle_ban_room_member(
    event_tx: &EventSender,
    state: &mut WorkerState,
    request: BanRoomMemberRequest,
) -> bool {
    let WorkerState::Ready { client, .. } = state else {
        emit_room_invalid_state(
            event_tx,
            Operation::BanRoomMember,
            &request.room_id,
            "ban room member requires an authenticated session",
        );
        return false;
    };

    if let Err(err) = matrix::ban_room_member(client, &request).await {
        emit_room_matrix_error(event_tx, Operation::BanRoomMember, &request.room_id, err);
        return false;
    }

    if let Err(err) =
        matrix::emit_joined_room_members_snapshot(client, event_tx, &request.room_id).await
    {
        emit_room_matrix_error(event_tx, Operation::BanRoomMember, &request.room_id, err);
    }

    false
}

pub(super) async fn handle_unban_room_member(
    event_tx: &EventSender,
    state: &mut WorkerState,
    request: UnbanRoomMemberRequest,
) -> bool {
    let WorkerState::Ready { client, .. } = state else {
        emit_room_invalid_state(
            event_tx,
            Operation::UnbanRoomMember,
            &request.room_id,
            "unban room member requires an authenticated session",
        );
        return false;
    };

    if let Err(err) = matrix::unban_room_member(client, &request).await {
        emit_room_matrix_error(event_tx, Operation::UnbanRoomMember, &request.room_id, err);
        return false;
    }

    if let Err(err) =
        matrix::emit_joined_room_members_snapshot(client, event_tx, &request.room_id).await
    {
        emit_room_matrix_error(event_tx, Operation::UnbanRoomMember, &request.room_id, err);
    }

    false
}

pub(super) async fn handle_invite_room_member(
    event_tx: &EventSender,
    state: &mut WorkerState,
    request: InviteRoomMemberRequest,
) -> bool {
    let WorkerState::Ready { client, .. } = state else {
        emit_room_invalid_state(
            event_tx,
            Operation::InviteRoomMember,
            &request.room_id,
            "invite room member requires an authenticated session",
        );
        return false;
    };

    if let Err(err) = matrix::invite_room_member(client, &request).await {
        emit_room_matrix_error(event_tx, Operation::InviteRoomMember, &request.room_id, err);
        return false;
    }

    if let Err(err) =
        matrix::emit_joined_room_members_snapshot(client, event_tx, &request.room_id).await
    {
        emit_room_matrix_error(event_tx, Operation::InviteRoomMember, &request.room_id, err);
    }

    false
}

pub(super) async fn handle_set_room_member_power_level(
    event_tx: &EventSender,
    state: &mut WorkerState,
    request: SetRoomMemberPowerLevelRequest,
) -> bool {
    let WorkerState::Ready { client, .. } = state else {
        emit_room_invalid_state(
            event_tx,
            Operation::SetRoomMemberPowerLevel,
            &request.room_id,
            "set room member power level requires an authenticated session",
        );
        return false;
    };

    if let Err(err) = matrix::set_room_member_power_level(client, &request).await {
        emit_room_matrix_error(
            event_tx,
            Operation::SetRoomMemberPowerLevel,
            &request.room_id,
            err,
        );
        return false;
    }

    if let Err(err) =
        matrix::emit_joined_room_members_snapshot(client, event_tx, &request.room_id).await
    {
        emit_room_matrix_error(
            event_tx,
            Operation::SetRoomMemberPowerLevel,
            &request.room_id,
            err,
        );
    }

    false
}

pub(super) async fn handle_set_display_name(
    event_tx: &EventSender,
    state: &mut WorkerState,
    request: SetDisplayNameRequest,
) -> bool {
    let WorkerState::Ready { client, .. } = state else {
        emit_invalid_state(
            event_tx,
            Operation::SetDisplayName,
            "set display name requires an authenticated session",
        );
        return false;
    };

    if let Err(err) = matrix::set_display_name(client, &request).await {
        emit_matrix_error(event_tx, Operation::SetDisplayName, err);
    }

    false
}

pub(super) async fn handle_reject_room_invite(
    event_tx: &EventSender,
    state: &mut WorkerState,
    request: RejectRoomInviteRequest,
) -> bool {
    let WorkerState::Ready { client, .. } = state else {
        emit_room_invalid_state(
            event_tx,
            Operation::RejectRoomInvite,
            &request.room_id,
            "reject room invitation requires an authenticated session",
        );
        return false;
    };

    if let Err(err) = matrix::reject_room_invite(client, &request.room_id).await {
        emit_room_matrix_error(event_tx, Operation::RejectRoomInvite, &request.room_id, err);
        return false;
    }

    if let Err(err) = matrix::emit_room_snapshot(client, event_tx).await {
        emit_room_matrix_error(event_tx, Operation::RejectRoomInvite, &request.room_id, err);
    }

    false
}

pub(super) async fn handle_send_message(
    event_tx: &EventSender,
    state: &mut WorkerState,
    request: crate::api::SendMessageRequest,
) -> bool {
    let WorkerState::Ready { client, .. } = state else {
        emit_room_invalid_state(
            event_tx,
            Operation::SendMessage,
            &request.room_id,
            "send message requires an authenticated session",
        );
        return false;
    };

    let room_id = request.room_id.clone();
    if let Err(err) = matrix::send_message(client, request).await {
        emit_room_matrix_error(event_tx, Operation::SendMessage, &room_id, err);
    }

    false
}

pub(super) async fn handle_send_file(
    event_tx: &EventSender,
    state: &mut WorkerState,
    request: crate::api::SendFileRequest,
) -> bool {
    let WorkerState::Ready { client, .. } = state else {
        emit_room_invalid_state(
            event_tx,
            Operation::SendFile,
            &request.room_id,
            "send file requires an authenticated session",
        );
        return false;
    };

    let room_id = request.room_id.clone();
    if let Err(err) = matrix::send_file(client, request).await {
        emit_room_matrix_error(event_tx, Operation::SendFile, &room_id, err);
    }

    false
}

pub(super) async fn handle_send_reaction(
    event_tx: &EventSender,
    state: &mut WorkerState,
    request: SendReactionRequest,
) -> bool {
    let WorkerState::Ready { client, .. } = state else {
        emit_room_invalid_state(
            event_tx,
            Operation::SendReaction,
            &request.room_id,
            "send reaction requires an authenticated session",
        );
        return false;
    };

    let room_id = request.room_id.clone();
    if let Err(err) = matrix::send_reaction(client, request).await {
        emit_room_matrix_error(event_tx, Operation::SendReaction, &room_id, err);
    }

    false
}

pub(super) async fn handle_edit_message(
    event_tx: &EventSender,
    state: &mut WorkerState,
    request: EditMessageRequest,
) -> bool {
    let WorkerState::Ready { client, .. } = state else {
        emit_room_invalid_state(
            event_tx,
            Operation::EditMessage,
            &request.room_id,
            "edit message requires an authenticated session",
        );
        return false;
    };

    let room_id = request.room_id.clone();
    if let Err(err) = matrix::edit_message(client, request).await {
        emit_room_matrix_error(event_tx, Operation::EditMessage, &room_id, err);
    }

    false
}

pub(super) async fn handle_redact_event(
    event_tx: &EventSender,
    state: &mut WorkerState,
    request: RedactEventRequest,
) -> bool {
    let WorkerState::Ready { client, .. } = state else {
        emit_room_invalid_state(
            event_tx,
            Operation::RedactEvent,
            &request.room_id,
            "redact event requires an authenticated session",
        );
        return false;
    };

    let room_id = request.room_id.clone();
    if let Err(err) = matrix::redact_event(client, request).await {
        emit_room_matrix_error(event_tx, Operation::RedactEvent, &room_id, err);
    }

    false
}

pub(super) async fn handle_set_room_topic(
    event_tx: &EventSender,
    state: &mut WorkerState,
    request: SetRoomTopicRequest,
) -> bool {
    let WorkerState::Ready { client, .. } = state else {
        emit_room_invalid_state(
            event_tx,
            Operation::SetRoomTopic,
            &request.room_id,
            "set room topic requires an authenticated session",
        );
        return false;
    };

    let room_id = request.room_id.clone();
    if let Err(err) = matrix::set_room_topic(client, request).await {
        emit_room_matrix_error(event_tx, Operation::SetRoomTopic, &room_id, err);
    }

    false
}

pub(super) async fn handle_set_room_notification_mode(
    event_tx: &EventSender,
    state: &mut WorkerState,
    request: SetRoomNotificationModeRequest,
) -> bool {
    let WorkerState::Ready { client, .. } = state else {
        emit_room_invalid_state(
            event_tx,
            Operation::SetRoomNotificationMode,
            &request.room_id,
            "set room notification mode requires an authenticated session",
        );
        return false;
    };

    let room_id = request.room_id.clone();
    if let Err(err) = matrix::set_room_notification_mode(client, request).await {
        emit_room_matrix_error(event_tx, Operation::SetRoomNotificationMode, &room_id, err);
    }

    false
}

pub(super) async fn handle_add_highlight_word(
    event_tx: &EventSender,
    state: &mut WorkerState,
    word: String,
) -> bool {
    let WorkerState::Ready { client, .. } = state else {
        emit_invalid_state(
            event_tx,
            Operation::AddHighlightWord,
            "add highlight word requires an authenticated session",
        );
        return false;
    };

    if let Err(err) = matrix::add_highlight_word(client, &word).await {
        emit_matrix_error(event_tx, Operation::AddHighlightWord, err);
    }

    false
}

pub(super) async fn handle_remove_highlight_word(
    event_tx: &EventSender,
    state: &mut WorkerState,
    word: String,
) -> bool {
    let WorkerState::Ready { client, .. } = state else {
        emit_invalid_state(
            event_tx,
            Operation::RemoveHighlightWord,
            "remove highlight word requires an authenticated session",
        );
        return false;
    };

    if let Err(err) = matrix::remove_highlight_word(client, &word).await {
        emit_matrix_error(event_tx, Operation::RemoveHighlightWord, err);
    }

    false
}

pub(super) async fn handle_ignore_user(
    event_tx: &EventSender,
    state: &mut WorkerState,
    user_id: String,
) -> bool {
    let WorkerState::Ready { client, .. } = state else {
        emit_invalid_state(
            event_tx,
            Operation::IgnoreUser,
            "ignore user requires an authenticated session",
        );
        return false;
    };

    if let Err(err) = matrix::ignore_user(client, &user_id).await {
        emit_matrix_error(event_tx, Operation::IgnoreUser, err);
    }

    false
}

pub(super) async fn handle_unignore_user(
    event_tx: &EventSender,
    state: &mut WorkerState,
    user_id: String,
) -> bool {
    let WorkerState::Ready { client, .. } = state else {
        emit_invalid_state(
            event_tx,
            Operation::UnignoreUser,
            "unignore user requires an authenticated session",
        );
        return false;
    };

    if let Err(err) = matrix::unignore_user(client, &user_id).await {
        emit_matrix_error(event_tx, Operation::UnignoreUser, err);
    }

    false
}

pub(super) async fn handle_fetch_user_profile(
    event_tx: &EventSender,
    state: &mut WorkerState,
    user_id: String,
) -> bool {
    let WorkerState::Ready { client, .. } = state else {
        emit_invalid_state(
            event_tx,
            Operation::FetchUserProfile,
            "fetch user profile requires an authenticated session",
        );
        return false;
    };

    match matrix::fetch_user_profile(client, &user_id).await {
        Ok(profile) => emit_event(event_tx, Event::UserProfile(profile)),
        Err(err) => emit_matrix_error(event_tx, Operation::FetchUserProfile, err),
    }

    false
}

pub(super) async fn handle_mark_room_read(
    event_tx: &EventSender,
    state: &mut WorkerState,
    request: MarkRoomReadRequest,
) -> bool {
    let WorkerState::Ready { client, .. } = state else {
        emit_room_invalid_state(
            event_tx,
            Operation::MarkRoomRead,
            &request.room_id,
            "mark room read requires an authenticated session",
        );
        return false;
    };

    if let Err(err) = matrix::mark_room_read(client, &request).await {
        emit_room_matrix_error(event_tx, Operation::MarkRoomRead, &request.room_id, err);
    }

    false
}

pub(super) async fn handle_backfill_room_history(
    event_tx: &EventSender,
    state: &mut WorkerState,
    request: BackfillRoomHistoryRequest,
) -> bool {
    let WorkerState::Ready { client, .. } = state else {
        emit_room_invalid_state(
            event_tx,
            Operation::BackfillRoomHistory,
            &request.room_id,
            "backfill room history requires an authenticated session",
        );
        return false;
    };

    let room_id = request.room_id.clone();
    match matrix::backfill_room_history(client, request).await {
        Ok(backfill) => emit_event(event_tx, Event::RoomHistoryBackfill(backfill)),
        Err(err) => emit_room_matrix_error(event_tx, Operation::BackfillRoomHistory, &room_id, err),
    }

    false
}

pub(super) async fn handle_fetch_room_media(
    event_tx: &EventSender,
    state: &mut WorkerState,
    request: crate::api::FetchRoomMediaRequest,
) -> bool {
    let WorkerState::Ready { client, .. } = state else {
        emit_room_invalid_state(
            event_tx,
            Operation::FetchRoomMedia,
            &request.room_id,
            "fetch room media requires an authenticated session",
        );
        return false;
    };

    let media = matrix::fetch_room_media(client, request).await;
    emit_event(event_tx, Event::RoomMedia(media));

    false
}

pub(super) async fn handle_fetch_room_event(
    event_tx: &EventSender,
    state: &mut WorkerState,
    request: crate::api::FetchRoomEventRequest,
) -> bool {
    let WorkerState::Ready { client, .. } = state else {
        emit_room_invalid_state(
            event_tx,
            Operation::FetchRoomEvent,
            &request.room_id,
            "fetch room event requires an authenticated session",
        );
        return false;
    };

    let fetch = matrix::fetch_room_event(client, request).await;
    emit_event(event_tx, Event::RoomEvent(fetch));

    false
}

pub(super) async fn handle_list_public_rooms(
    event_tx: &EventSender,
    state: &mut WorkerState,
    request: crate::api::ListPublicRoomsRequest,
) -> bool {
    let WorkerState::Ready { client, .. } = state else {
        emit_invalid_state(
            event_tx,
            Operation::ListPublicRooms,
            "list public rooms requires an authenticated session",
        );
        return false;
    };

    let page = matrix::list_public_rooms(client, request).await;
    emit_event(event_tx, Event::PublicRooms(page));

    false
}

pub(super) async fn handle_refresh_e2ee_status(
    event_tx: &EventSender,
    state: &mut WorkerState,
) -> bool {
    let WorkerState::Ready { client, .. } = state else {
        emit_invalid_state(
            event_tx,
            Operation::E2eeStatus,
            "E2EE status requires an authenticated session",
        );
        return false;
    };

    match matrix::summarize_e2ee_status(client, BackupServerStatusMode::Fetch).await {
        Ok(status) => emit_event(event_tx, Event::E2eeStatus(status)),
        Err(err) => emit_matrix_error(event_tx, Operation::E2eeStatus, err),
    }

    false
}

pub(super) async fn handle_recover_e2ee(
    event_tx: &EventSender,
    state: &mut WorkerState,
    request: &RecoverE2eeRequest,
) -> bool {
    let WorkerState::Ready { client, .. } = state else {
        emit_invalid_state(
            event_tx,
            Operation::RecoverE2ee,
            "E2EE recovery requires an authenticated session",
        );
        return false;
    };

    match matrix::recover_e2ee(client, request).await {
        Ok(status) => {
            emit_event(
                event_tx,
                Event::E2eeRecoverySucceeded(E2eeRecoverySucceeded {
                    operation: E2eeRecoveryOperation::Recover,
                }),
            );
            emit_event(event_tx, Event::E2eeStatus(status));
        }
        Err(err) => emit_matrix_error(event_tx, Operation::RecoverE2ee, err),
    }

    false
}

pub(super) async fn handle_setup_e2ee_recovery(
    event_tx: &EventSender,
    state: &mut WorkerState,
    request: &SetupE2eeRecoveryRequest,
) -> bool {
    let WorkerState::Ready { client, .. } = state else {
        emit_invalid_state(
            event_tx,
            Operation::SetupE2eeRecovery,
            "E2EE recovery setup requires an authenticated session",
        );
        return false;
    };

    match matrix::setup_e2ee_recovery(client, request).await {
        Ok((recovery_key, status)) => {
            emit_event(
                event_tx,
                Event::E2eeRecoverySetupSucceeded(E2eeRecoverySetupSucceeded { recovery_key }),
            );
            emit_event(event_tx, Event::E2eeStatus(status));
        }
        Err(err) => emit_matrix_error(event_tx, Operation::SetupE2eeRecovery, err),
    }

    false
}

pub(super) async fn handle_list_user_devices(
    event_tx: &EventSender,
    state: &mut WorkerState,
    user_id: String,
) -> bool {
    let WorkerState::Ready { client, .. } = state else {
        emit_invalid_state(
            event_tx,
            Operation::ListUserDevices,
            "listing devices requires an authenticated session",
        );
        return false;
    };

    match matrix::list_user_devices(client, &user_id).await {
        Ok(devices) => emit_event(event_tx, Event::UserDevices(devices)),
        Err(err) => emit_matrix_error(event_tx, Operation::ListUserDevices, err),
    }

    false
}

pub(super) async fn handle_sign_out_device(
    event_tx: &EventSender,
    state: &mut WorkerState,
    request: SignOutDeviceRequest,
) -> bool {
    let WorkerState::Ready { client, .. } = state else {
        emit_invalid_state(
            event_tx,
            Operation::SignOutDevice,
            "signing out a session requires an authenticated session",
        );
        return false;
    };

    match matrix::sign_out_device(client, &request.device_id, request.password).await {
        Ok(matrix::SignOutOutcome::SignedOut) => {
            emit_event(event_tx, Event::DeviceSignedOut(request.device_id))
        }
        Ok(matrix::SignOutOutcome::Approval { url }) => emit_event(
            event_tx,
            Event::DeviceSignOutApproval {
                device_id: request.device_id,
                url,
            },
        ),
        Err(err) => emit_matrix_error(event_tx, Operation::SignOutDevice, err),
    }

    false
}

pub(super) async fn handle_rename_device(
    event_tx: &EventSender,
    state: &mut WorkerState,
    request: RenameDeviceRequest,
) -> bool {
    let WorkerState::Ready { client, .. } = state else {
        emit_invalid_state(
            event_tx,
            Operation::RenameDevice,
            "renaming a session requires an authenticated session",
        );
        return false;
    };

    match matrix::rename_device(client, &request.device_id, &request.display_name).await {
        Ok(()) => emit_event(
            event_tx,
            Event::DeviceRenamed {
                device_id: request.device_id,
                display_name: request.display_name,
            },
        ),
        Err(err) => emit_matrix_error(event_tx, Operation::RenameDevice, err),
    }

    false
}

pub(super) async fn handle_accept_user_identity(
    event_tx: &EventSender,
    state: &mut WorkerState,
    user_id: String,
) -> bool {
    let WorkerState::Ready { client, .. } = state else {
        emit_invalid_state(
            event_tx,
            Operation::AcceptUserIdentity,
            "accepting an identity requires an authenticated session",
        );
        return false;
    };

    match matrix::accept_user_identity(client, &user_id).await {
        Ok(()) => emit_event(event_tx, Event::UserIdentityAccepted(user_id)),
        Err(err) => emit_matrix_error(event_tx, Operation::AcceptUserIdentity, err),
    }

    false
}

pub(super) async fn handle_reset_e2ee_identity(
    event_tx: &EventSender,
    state: &mut WorkerState,
    request: ResetE2eeIdentityRequest,
) -> bool {
    let WorkerState::Ready {
        client,
        identity_reset,
        ..
    } = state
    else {
        emit_invalid_state(
            event_tx,
            Operation::ResetE2eeIdentity,
            "resetting your E2EE identity requires an authenticated session",
        );
        return false;
    };
    if identity_reset.is_some() {
        emit_invalid_state(
            event_tx,
            Operation::ResetE2eeIdentity,
            "an identity reset is already under way; /e2ee reset cancel stops it",
        );
        return false;
    }

    // The reset deletes the backup before the server says how to confirm
    // it: refuse what cannot work while nothing is lost.
    let refusal = match (
        client.auth_api().expect("a ready session is signed in"),
        &request.password,
    ) {
        (AuthApi::Matrix(_), None) => {
            Some("resetting your E2EE identity needs your account password")
        }
        (AuthApi::OAuth(_), Some(_)) => {
            Some("your server confirms an identity reset in the browser, not with a password")
        }
        (AuthApi::Matrix(_), Some(_)) | (AuthApi::OAuth(_), None) => None,
        // `AuthApi` is non-exhaustive; mirx signs in with these two only.
        (api, _) => panic!("a session signed in with {api:?}"),
    };
    if let Some(refusal) = refusal {
        emit_invalid_state(event_tx, Operation::ResetE2eeIdentity, refusal);
        return false;
    }

    let handle = match matrix::start_identity_reset(client).await {
        Ok(None) => return end_identity_reset(event_tx, client, IdentityResetEnd::Uploaded).await,
        Ok(Some(handle)) => handle,
        Err(err) => {
            return end_identity_reset(event_tx, client, IdentityResetEnd::Failed(err)).await;
        }
    };
    let auth = match (
        matrix::identity_reset_auth(handle.auth_type()),
        request.password,
    ) {
        (Ok(matrix::IdentityResetAuth::Browser { url }), _) => {
            emit_event(event_tx, Event::E2eeIdentityResetApproval { url });
            None
        }
        (Ok(matrix::IdentityResetAuth::Password { session }), Some(password)) => {
            Some(matrix::password_auth(client, session, password))
        }
        (Ok(matrix::IdentityResetAuth::Password { .. }), None) => {
            let err = matrix::identity_reset_error(&"your server asks for your account password");
            return end_identity_reset(event_tx, client, IdentityResetEnd::Refused(err)).await;
        }
        (Err(err), _) => {
            return end_identity_reset(event_tx, client, IdentityResetEnd::Refused(err)).await;
        }
    };
    let handle = Rc::new(handle);
    let task = tokio::task::spawn_local({
        let handle = handle.clone();
        async move { handle.reset(auth).await }
    });
    *identity_reset = Some(PendingIdentityReset {
        handle,
        task,
        cancel_requested: false,
    });

    false
}

pub(super) async fn handle_cancel_e2ee_identity_reset(
    event_tx: &EventSender,
    state: &mut WorkerState,
) -> bool {
    let WorkerState::Ready {
        identity_reset: Some(pending),
        ..
    } = state
    else {
        emit_invalid_state(
            event_tx,
            Operation::ResetE2eeIdentity,
            "no identity reset is under way",
        );
        return false;
    };

    // The SDK sees it after its next refused attempt, within half a
    // second; the task's end answers.
    pending.handle.cancel().await;
    pending.cancel_requested = true;

    false
}

pub(super) async fn finish_pending_identity_reset(
    event_tx: &EventSender,
    state: &mut WorkerState,
    result: Result<(), RecoveryError>,
) {
    let WorkerState::Ready {
        client,
        identity_reset,
        ..
    } = state
    else {
        unreachable!("only a ready session waits for an identity reset");
    };
    let pending = identity_reset
        .take()
        .expect("the worker waited for this reset");
    let end = match result {
        Ok(()) if pending.cancel_requested => IdentityResetEnd::Cancelled,
        Ok(()) => IdentityResetEnd::Uploaded,
        Err(err) => IdentityResetEnd::Refused(matrix::identity_reset_failure(&err)),
    };
    end_identity_reset(event_tx, client, end).await;
}

/// How the attempt to upload a reset's new identity ended.
pub(super) enum IdentityResetEnd {
    /// The server took the new identity.
    Uploaded,
    /// Stopped by `CancelE2eeIdentityReset`: no upload, unless the last
    /// one got through.
    Cancelled,
    /// Before the SDK asked the server for the upload.
    Failed(MatrixCoreError),
    /// The server's authentication was not passed: the reset had already
    /// turned the key backup and recovery off.
    Refused(MatrixCoreError),
}

/// Answers a reset, then reports the E2EE status, which is the truth
/// about this session: the backup and recovery are off even when the
/// identity did not change.
pub(super) async fn end_identity_reset(
    event_tx: &EventSender,
    client: &Client,
    end: IdentityResetEnd,
) -> bool {
    match end {
        IdentityResetEnd::Uploaded => emit_identity_reset(event_tx, E2eeIdentityResetOutcome::Done),
        IdentityResetEnd::Cancelled => match matrix::settle_identity_reset(client).await {
            Ok(true) => emit_identity_reset(event_tx, E2eeIdentityResetOutcome::Done),
            Ok(false) => emit_identity_reset(event_tx, E2eeIdentityResetOutcome::Cancelled),
            Err(err) => emit_matrix_error(event_tx, Operation::ResetE2eeIdentity, err),
        },
        IdentityResetEnd::Failed(err) => {
            emit_matrix_error(event_tx, Operation::ResetE2eeIdentity, err);
            if let Err(err) = matrix::settle_identity_reset(client).await {
                emit_matrix_error(event_tx, Operation::ResetE2eeIdentity, err);
            }
        }
        IdentityResetEnd::Refused(err) => {
            emit_error(
                event_tx,
                Operation::ResetE2eeIdentity,
                None,
                format!(
                    "{err}; your key backup and recovery were turned off before the server was asked"
                ),
            );
            if let Err(err) = matrix::settle_identity_reset(client).await {
                emit_matrix_error(event_tx, Operation::ResetE2eeIdentity, err);
            }
        }
    }
    match matrix::summarize_e2ee_status(client, BackupServerStatusMode::Fetch).await {
        Ok(status) => emit_event(event_tx, Event::E2eeStatus(status)),
        Err(err) => emit_matrix_error(event_tx, Operation::E2eeStatus, err),
    }
    false
}

pub(super) fn emit_identity_reset(event_tx: &EventSender, outcome: E2eeIdentityResetOutcome) {
    emit_event(event_tx, Event::E2eeIdentityReset(outcome));
}

pub(super) async fn handle_request_verification(
    event_tx: &EventSender,
    state: &mut WorkerState,
    request: RequestVerificationRequest,
) -> bool {
    let WorkerState::Ready {
        client,
        verification_tasks,
        ..
    } = state
    else {
        emit_invalid_state(
            event_tx,
            Operation::RequestVerification,
            "verification requires an authenticated session",
        );
        return false;
    };

    match matrix::request_verification(client, &request).await {
        Ok((announcement, flow_request)) => {
            let flow = VerificationFlowId {
                flow_id: announcement.flow_id.clone(),
                user_id: announcement.user_id.clone(),
            };
            emit_event(event_tx, Event::VerificationRequest(announcement));
            // An outgoing flow has no Accept to start the watcher, which
            // sees the other side's ready and starts SAS.
            watch_verification_request(event_tx, verification_tasks, flow, flow_request);
        }
        Err(err) => emit_matrix_error(event_tx, Operation::RequestVerification, err),
    }

    false
}

pub(super) async fn handle_accept_verification(
    event_tx: &EventSender,
    state: &mut WorkerState,
    flow: VerificationFlowId,
) -> bool {
    let WorkerState::Ready { client, .. } = state else {
        emit_invalid_state(
            event_tx,
            Operation::Verification,
            "verification requires an authenticated session",
        );
        return false;
    };

    match matrix::accept_verification(client, &flow).await {
        Ok(event) => {
            emit_event(event_tx, Event::VerificationState(event));
            ensure_verification_watcher(event_tx, state, &flow).await;
        }
        Err(err) => emit_matrix_error(event_tx, Operation::Verification, err),
    }

    false
}

pub(super) async fn handle_cancel_verification(
    event_tx: &EventSender,
    state: &mut WorkerState,
    flow: VerificationFlowId,
) -> bool {
    let WorkerState::Ready { client, .. } = state else {
        emit_invalid_state(
            event_tx,
            Operation::Verification,
            "verification requires an authenticated session",
        );
        return false;
    };

    match matrix::cancel_verification(client, &flow).await {
        Ok(event) => emit_event(event_tx, Event::VerificationState(event)),
        Err(err) => emit_matrix_error(event_tx, Operation::Verification, err),
    }

    false
}

pub(super) async fn handle_confirm_verification_sas(
    event_tx: &EventSender,
    state: &mut WorkerState,
    flow: VerificationFlowId,
) -> bool {
    let WorkerState::Ready { client, .. } = state else {
        emit_invalid_state(
            event_tx,
            Operation::Verification,
            "verification requires an authenticated session",
        );
        return false;
    };

    match matrix::confirm_verification_sas(client, &flow).await {
        Ok(event) => {
            emit_event(event_tx, Event::VerificationState(event));
            ensure_verification_watcher(event_tx, state, &flow).await;
        }
        Err(err) => emit_matrix_error(event_tx, Operation::Verification, err),
    }

    false
}

pub(super) async fn handle_reject_verification_sas(
    event_tx: &EventSender,
    state: &mut WorkerState,
    flow: VerificationFlowId,
) -> bool {
    let WorkerState::Ready { client, .. } = state else {
        emit_invalid_state(
            event_tx,
            Operation::Verification,
            "verification requires an authenticated session",
        );
        return false;
    };

    match matrix::reject_verification_sas(client, &flow).await {
        Ok(event) => emit_event(event_tx, Event::VerificationState(event)),
        Err(err) => emit_matrix_error(event_tx, Operation::Verification, err),
    }

    false
}

pub(super) async fn ensure_verification_watcher(
    event_tx: &EventSender,
    state: &mut WorkerState,
    flow: &VerificationFlowId,
) {
    let WorkerState::Ready {
        client,
        verification_tasks,
        ..
    } = state
    else {
        return;
    };

    let key = verification_task_key(flow);
    if task_is_running(verification_tasks, &key) {
        return;
    }

    if let Ok(request) = matrix::lookup_verification_request(client, flow).await {
        watch_verification_request(event_tx, verification_tasks, flow.clone(), request);
        return;
    }
    let Ok(sas) = matrix::lookup_sas_verification(client, flow).await else {
        return;
    };
    let task = tokio::task::spawn_local(matrix::watch_sas_verification(
        sas,
        flow.clone(),
        event_tx.clone(),
    ));
    verification_tasks.insert(key, task);
}

/// Follows a request's flow until it ends: the watcher of its flow.
pub(super) fn watch_verification_request(
    event_tx: &EventSender,
    verification_tasks: &mut HashMap<(String, String), TokioJoinHandle<()>>,
    flow: VerificationFlowId,
    request: MatrixVerificationRequest,
) {
    let key = verification_task_key(&flow);
    let task = tokio::task::spawn_local(matrix::watch_verification_request(
        request,
        flow,
        event_tx.clone(),
    ));
    verification_tasks.insert(key, task);
}

pub(super) fn verification_task_key(flow: &VerificationFlowId) -> (String, String) {
    (flow.user_id.clone(), flow.flow_id.clone())
}

pub(super) fn task_is_running(
    verification_tasks: &mut HashMap<(String, String), TokioJoinHandle<()>>,
    key: &(String, String),
) -> bool {
    if verification_tasks
        .get(key)
        .is_some_and(|existing_task| !existing_task.is_finished())
    {
        return true;
    }

    verification_tasks.remove(key);
    false
}
