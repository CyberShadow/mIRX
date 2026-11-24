//! What each command is: a lifecycle step for the actor, or a job on a
//! lane. A job's body is the command's handler; it answers on the event
//! channel and leaves its changes to the session as a `Followup`.

use std::future::Future;
use std::rc::Rc;

use futures_util::future::LocalBoxFuture;
use matrix_sdk::authentication::AuthApi;
use matrix_sdk::ruma::RoomOrAliasId;

use super::jobs::{Lane, Start, Work};
use super::session::{Followup, IdentityResetEnd, JobCtx, Session, StartedReset};
use super::{
    EventSender, emit_error, emit_event, emit_invalid_state, emit_matrix_error,
    emit_room_matrix_error,
};
use crate::api::{
    AcceptRoomInviteRequest, AwayChanged, BackfillRoomHistoryRequest, BanRoomMemberRequest,
    Command, CreateDirectRoomRequest, CreateRoomRequest, E2eeIdentityResetOutcome,
    E2eeRecoveryOperation, E2eeRecoverySetupSucceeded, E2eeRecoverySucceeded, EditMessageRequest,
    Event, ExportRoomKeysRequest, FetchRoomEventRequest, FetchRoomMediaRequest,
    ImportRoomKeysRequest, InviteRoomMemberRequest, JoinRoomRequest, KickRoomMemberRequest,
    LeaveRoomRequest, ListPublicRoomsRequest, LoginRequest, LogoutRequest, MarkRoomReadRequest,
    Operation, RecoverE2eeRequest, RedactEventRequest, RejectRoomInviteRequest,
    RenameDeviceRequest, RequestVerificationRequest, ResetE2eeIdentityRequest,
    RestoreSessionRequest, RetryDecryptionRequest, RoomJoined, SendFileRequest, SendMessageRequest,
    SendReactionRequest, SetDisplayNameRequest, SetReadMarkerRequest,
    SetRoomNotificationModeRequest, SetRoomTopicRequest, SetTypingRequest,
    SetupE2eeRecoveryRequest, SignOutDeviceRequest, UnbanRoomMemberRequest,
    UpdateRoomSettingsRequest, VerificationFlowId,
};
use crate::matrix::{self, AwayFlag, BackupServerStatusMode, MatrixCoreError};

/// A command, with every in-session command made its lane job.
pub(super) enum Route {
    Login(LoginRequest),
    RestoreSession(RestoreSessionRequest),
    CancelLogin,
    Logout(LogoutRequest),
    StartSync,
    StopSync,
    Shutdown,
    Lane(LaneCommand),
}

impl Route {
    /// Whether the command is for the session: a logout that ends it
    /// drops the command with it.
    pub(super) fn is_for_the_session(&self) -> bool {
        match self {
            Self::Logout(_) | Self::StartSync | Self::StopSync | Self::Lane(_) => true,
            Self::Login(_) | Self::RestoreSession(_) | Self::CancelLogin => false,
            Self::Shutdown => unreachable!("a shutdown never waits"),
        }
    }
}

/// A command that runs as a job on `lane`, in a session; without one it
/// is refused.
pub(super) struct LaneCommand {
    pub(super) lane: Lane,
    pub(super) refusal: Refusal,
    pub(super) start: Start<Session, Followup>,
}

/// The answer to a command that needs a session, without one.
pub(super) struct Refusal {
    operation: Operation,
    /// The room a room command is about: its refusal shows in the room's
    /// window.
    room_id: Option<String>,
    message: &'static str,
}

impl Refusal {
    fn account(operation: Operation, message: &'static str) -> Self {
        Self {
            operation,
            room_id: None,
            message,
        }
    }

    fn room(operation: Operation, room_id: &str, message: &'static str) -> Self {
        Self {
            operation,
            room_id: Some(room_id.to_owned()),
            message,
        }
    }

    pub(super) fn emit(self, events: &EventSender) {
        emit_error(
            events,
            self.operation,
            self.room_id.as_deref(),
            self.message.to_owned(),
        );
    }
}

/// `job` on `lane`, refused with `refusal` without a session.
fn lane<Fut>(lane: Lane, refusal: Refusal, job: impl FnOnce(JobCtx) -> Fut + 'static) -> Route
where
    Fut: Future<Output = Followup> + 'static,
{
    Route::Lane(LaneCommand {
        lane,
        refusal,
        start: Box::new(move |session: &mut Session| Box::pin(job(session.ctx()))),
    })
}

/// A room's command, in the room's lane: after its earlier sends, writes
/// and membership changes.
fn in_room<Fut>(
    operation: Operation,
    room_id: String,
    message: &'static str,
    job: impl FnOnce(JobCtx) -> Fut + 'static,
) -> Route
where
    Fut: Future<Output = Followup> + 'static,
{
    let refusal = Refusal::room(operation, &room_id, message);
    lane(Lane::Room(room_id), refusal, job)
}

/// Where `command` goes. No `_` arm: a new command decides its lane.
pub(super) fn route(command: Command) -> Route {
    use Operation as Op;
    match command {
        Command::Login(request) => Route::Login(request),
        Command::RestoreSession(request) => Route::RestoreSession(request),
        Command::CancelLogin => Route::CancelLogin,
        Command::Logout(request) => Route::Logout(request),
        Command::StartSync => Route::StartSync,
        Command::StopSync => Route::StopSync,
        Command::Shutdown => Route::Shutdown,

        Command::SendMessage(request) => in_room(
            Op::SendMessage,
            request.room_id.clone(),
            "send message requires an authenticated session",
            |ctx| send_message(ctx, request),
        ),
        Command::SendFile(request) => in_room(
            Op::SendFile,
            request.room_id.clone(),
            "send file requires an authenticated session",
            |ctx| send_file(ctx, request),
        ),
        Command::EditMessage(request) => in_room(
            Op::EditMessage,
            request.room_id.clone(),
            "edit message requires an authenticated session",
            |ctx| edit_message(ctx, request),
        ),
        Command::SendReaction(request) => in_room(
            Op::SendReaction,
            request.room_id.clone(),
            "send reaction requires an authenticated session",
            |ctx| send_reaction(ctx, request),
        ),
        Command::RedactEvent(request) => in_room(
            Op::RedactEvent,
            request.room_id.clone(),
            "redact event requires an authenticated session",
            |ctx| redact_event(ctx, request),
        ),
        Command::SetRoomTopic(request) => in_room(
            Op::SetRoomTopic,
            request.room_id.clone(),
            "set room topic requires an authenticated session",
            |ctx| set_room_topic(ctx, request),
        ),
        // After the room's queued writes: the dialog reads what they wrote.
        Command::FetchRoomSettings(room_id) => in_room(
            Op::FetchRoomSettings,
            room_id.clone(),
            "fetch room settings requires an authenticated session",
            |ctx| fetch_room_settings(ctx, room_id),
        ),
        // Read-modify-write of `m.room.power_levels` (modes and ranks):
        // the room's lane runs mirx's own writes one at a time.
        Command::UpdateRoomSettings(request) => in_room(
            Op::UpdateRoomSettings,
            request.room_id.clone(),
            "update room settings requires an authenticated session",
            |ctx| update_room_settings(ctx, request),
        ),
        Command::SetRoomNotificationMode(request) => in_room(
            Op::SetRoomNotificationMode,
            request.room_id.clone(),
            "set room notification mode requires an authenticated session",
            |ctx| set_room_notification_mode(ctx, request),
        ),
        Command::KickRoomMember(request) => in_room(
            Op::KickRoomMember,
            request.room_id.clone(),
            "kick room member requires an authenticated session",
            |ctx| kick_room_member(ctx, request),
        ),
        Command::BanRoomMember(request) => in_room(
            Op::BanRoomMember,
            request.room_id.clone(),
            "ban room member requires an authenticated session",
            |ctx| ban_room_member(ctx, request),
        ),
        Command::UnbanRoomMember(request) => in_room(
            Op::UnbanRoomMember,
            request.room_id.clone(),
            "unban room member requires an authenticated session",
            |ctx| unban_room_member(ctx, request),
        ),
        Command::InviteRoomMember(request) => in_room(
            Op::InviteRoomMember,
            request.room_id.clone(),
            "invite room member requires an authenticated session",
            |ctx| invite_room_member(ctx, request),
        ),
        // After the room's queued sends and writes.
        Command::LeaveRoom(request) => in_room(
            Op::LeaveRoom,
            request.room_id.clone(),
            "leave room requires an authenticated session",
            |ctx| leave_room(ctx, request),
        ),
        Command::AcceptRoomInvite(request) => in_room(
            Op::AcceptRoomInvite,
            request.room_id.clone(),
            "accept room invitation requires an authenticated session",
            |ctx| accept_room_invite(ctx, request),
        ),
        Command::RejectRoomInvite(request) => in_room(
            Op::RejectRoomInvite,
            request.room_id.clone(),
            "reject room invitation requires an authenticated session",
            |ctx| reject_room_invite(ctx, request),
        ),
        // Its own lane: a receipt never waits for an upload.
        Command::MarkRoomRead(request) => lane(
            Lane::Receipts(request.room_id.clone()),
            Refusal::room(
                Op::MarkRoomRead,
                &request.room_id,
                "mark room read requires an authenticated session",
            ),
            |ctx| mark_room_read(ctx, request),
        ),
        Command::SetReadMarker(request) => lane(
            Lane::Receipts(request.room_id.clone()),
            Refusal::room(
                Op::SetReadMarker,
                &request.room_id,
                "set read marker requires an authenticated session",
            ),
            |ctx| set_read_marker(ctx, request),
        ),

        Command::SetTyping(request) => lane(
            Lane::Typing(request.room_id.clone()),
            Refusal::room(
                Op::SetTyping,
                &request.room_id,
                "send typing notice requires an authenticated session",
            ),
            |ctx| set_typing(ctx, request),
        ),

        Command::SetDisplayName(request) => lane(
            Lane::Account,
            Refusal::account(
                Op::SetDisplayName,
                "set display name requires an authenticated session",
            ),
            |ctx| set_display_name(ctx, request),
        ),
        Command::AddHighlightWord(word) => lane(
            Lane::Account,
            Refusal::account(
                Op::AddHighlightWord,
                "add highlight word requires an authenticated session",
            ),
            |ctx| add_highlight_word(ctx, word),
        ),
        Command::RemoveHighlightWord(word) => lane(
            Lane::Account,
            Refusal::account(
                Op::RemoveHighlightWord,
                "remove highlight word requires an authenticated session",
            ),
            |ctx| remove_highlight_word(ctx, word),
        ),
        // Not through `lane`: the job takes the session's away flag too.
        Command::SetAway(message) => Route::Lane(LaneCommand {
            lane: Lane::Account,
            refusal: Refusal::account(
                Op::SetPresence,
                "set presence requires an authenticated session",
            ),
            start: Box::new(move |session: &mut Session| {
                Box::pin(set_away(session.ctx(), session.away(), message))
            }),
        }),
        Command::IgnoreUser(user_id) => lane(
            Lane::Account,
            Refusal::account(
                Op::IgnoreUser,
                "ignore user requires an authenticated session",
            ),
            |ctx| ignore_user(ctx, user_id),
        ),
        Command::UnignoreUser(user_id) => lane(
            Lane::Account,
            Refusal::account(
                Op::UnignoreUser,
                "unignore user requires an authenticated session",
            ),
            |ctx| unignore_user(ctx, user_id),
        ),

        Command::ListUserDevices(user_id) => lane(
            Lane::Devices,
            Refusal::account(
                Op::ListUserDevices,
                "listing devices requires an authenticated session",
            ),
            |ctx| list_user_devices(ctx, user_id),
        ),
        Command::SignOutDevice(request) => lane(
            Lane::Devices,
            Refusal::account(
                Op::SignOutDevice,
                "signing out a session requires an authenticated session",
            ),
            |ctx| sign_out_device(ctx, request),
        ),
        Command::RenameDevice(request) => lane(
            Lane::Devices,
            Refusal::account(
                Op::RenameDevice,
                "renaming a session requires an authenticated session",
            ),
            |ctx| rename_device(ctx, request),
        ),
        Command::AcceptUserIdentity(user_id) => lane(
            Lane::Devices,
            Refusal::account(
                Op::AcceptUserIdentity,
                "accepting an identity requires an authenticated session",
            ),
            |ctx| accept_user_identity(ctx, user_id),
        ),

        Command::RefreshE2eeStatus => lane(
            Lane::E2ee,
            Refusal::account(
                Op::E2eeStatus,
                "E2EE status requires an authenticated session",
            ),
            refresh_e2ee_status,
        ),
        Command::SetupE2eeRecovery(request) => lane(
            Lane::E2ee,
            Refusal::account(
                Op::SetupE2eeRecovery,
                "E2EE recovery setup requires an authenticated session",
            ),
            |ctx| setup_e2ee_recovery(ctx, request),
        ),
        Command::RecoverE2ee(request) => lane(
            Lane::E2ee,
            Refusal::account(
                Op::RecoverE2ee,
                "E2EE recovery requires an authenticated session",
            ),
            |ctx| recover_e2ee(ctx, request),
        ),
        // Checked at the lane's head: a reset queued behind another sees
        // whether that one is still under way.
        Command::ResetE2eeIdentity(request) => Route::Lane(LaneCommand {
            lane: Lane::E2ee,
            refusal: Refusal::account(
                Op::ResetE2eeIdentity,
                "resetting your E2EE identity requires an authenticated session",
            ),
            start: Box::new(move |session: &mut Session| {
                if session.resetting() {
                    refuse(
                        session.ctx(),
                        Op::ResetE2eeIdentity,
                        "an identity reset is already under way; /e2ee reset cancel stops it",
                    )
                } else {
                    Box::pin(reset_e2ee_identity(session.ctx(), request))
                }
            }),
        }),
        Command::ExportRoomKeys(request) => lane(
            Lane::E2ee,
            Refusal::account(
                Op::ExportRoomKeys,
                "exporting room keys requires an authenticated session",
            ),
            |ctx| export_room_keys(ctx, request),
        ),
        Command::ImportRoomKeys(request) => lane(
            Lane::E2ee,
            Refusal::account(
                Op::ImportRoomKeys,
                "importing room keys requires an authenticated session",
            ),
            |ctx| import_room_keys(ctx, request),
        ),
        Command::CancelE2eeIdentityReset => Route::Lane(LaneCommand {
            lane: Lane::E2ee,
            refusal: Refusal::account(Op::ResetE2eeIdentity, "no identity reset is under way"),
            start: Box::new(cancel_e2ee_identity_reset),
        }),

        Command::RequestVerification(request) => lane(
            Lane::Verification,
            Refusal::account(
                Op::RequestVerification,
                "verification requires an authenticated session",
            ),
            |ctx| request_verification(ctx, request),
        ),
        Command::AcceptVerification(flow) => lane(
            Lane::Verification,
            Refusal::account(
                Op::Verification,
                "verification requires an authenticated session",
            ),
            |ctx| accept_verification(ctx, flow),
        ),
        Command::CancelVerification(flow) => lane(
            Lane::Verification,
            Refusal::account(
                Op::Verification,
                "verification requires an authenticated session",
            ),
            |ctx| cancel_verification(ctx, flow),
        ),
        Command::ConfirmVerificationSas(flow) => lane(
            Lane::Verification,
            Refusal::account(
                Op::Verification,
                "verification requires an authenticated session",
            ),
            |ctx| confirm_verification_sas(ctx, flow),
        ),
        Command::RejectVerificationSas(flow) => lane(
            Lane::Verification,
            Refusal::account(
                Op::Verification,
                "verification requires an authenticated session",
            ),
            |ctx| reject_verification_sas(ctx, flow),
        ),

        Command::FetchRoomMedia(request) => lane(
            Lane::Media,
            Refusal::room(
                Op::FetchRoomMedia,
                &request.room_id,
                "fetch room media requires an authenticated session",
            ),
            |ctx| fetch_room_media(ctx, request),
        ),

        // Reads, and rooms that do not exist yet; the UI matches their
        // answers to what it asked.
        Command::JoinRoom(request) => lane(
            Lane::Unordered,
            Refusal::account(Op::JoinRoom, "join room requires an authenticated session"),
            |ctx| join_room(ctx, request),
        ),
        Command::CreateRoom(request) => lane(
            Lane::Unordered,
            Refusal::account(
                Op::CreateRoom,
                "create room requires an authenticated session",
            ),
            |ctx| create_room(ctx, request),
        ),
        Command::CreateDirectRoom(request) => lane(
            Lane::Unordered,
            Refusal::account(
                Op::CreateDirectRoom,
                "create direct room requires an authenticated session",
            ),
            |ctx| create_direct_room(ctx, request),
        ),
        Command::FetchUserProfile(user_id) => lane(
            Lane::Unordered,
            Refusal::account(
                Op::FetchUserProfile,
                "fetch user profile requires an authenticated session",
            ),
            |ctx| fetch_user_profile(ctx, user_id),
        ),
        Command::BackfillRoomHistory(request) => lane(
            Lane::Unordered,
            Refusal::room(
                Op::BackfillRoomHistory,
                &request.room_id,
                "backfill room history requires an authenticated session",
            ),
            |ctx| backfill_room_history(ctx, request),
        ),
        Command::FetchRoomEvent(request) => lane(
            Lane::Unordered,
            Refusal::room(
                Op::FetchRoomEvent,
                &request.room_id,
                "fetch room event requires an authenticated session",
            ),
            |ctx| fetch_room_event(ctx, request),
        ),
        Command::RetryDecryption(request) => lane(
            Lane::Unordered,
            Refusal::room(
                Op::RetryDecryption,
                &request.room_id,
                "retrying decryption requires an authenticated session",
            ),
            |ctx| retry_decryption(ctx, request),
        ),
        Command::ListPublicRooms(request) => lane(
            Lane::Unordered,
            Refusal::account(
                Op::ListPublicRooms,
                "list public rooms requires an authenticated session",
            ),
            |ctx| list_public_rooms(ctx, request),
        ),
    }
}

async fn join_room(ctx: JobCtx, request: JoinRoomRequest) -> Followup {
    let JobCtx { client, events, .. } = &ctx;
    match matrix::join_room(client, &request).await {
        Ok(room_id) => {
            let alias = <&RoomOrAliasId>::try_from(request.room_or_alias.as_str())
                .expect("the join parsed it")
                .is_room_alias_id()
                .then_some(request.room_or_alias);
            emit_snapshot_then_room_joined(&ctx, Operation::JoinRoom, room_id.into(), alias).await
        }
        Err(err) => emit_matrix_error(events, Operation::JoinRoom, err),
    }

    Followup::Done
}

async fn accept_room_invite(ctx: JobCtx, request: AcceptRoomInviteRequest) -> Followup {
    let JobCtx { client, events, .. } = &ctx;
    match matrix::accept_room_invite(client, &request.room_id).await {
        Ok(()) => {
            emit_snapshot_then_room_joined(&ctx, Operation::AcceptRoomInvite, request.room_id, None)
                .await
        }
        Err(err) => {
            emit_room_matrix_error(events, Operation::AcceptRoomInvite, &request.room_id, err)
        }
    }

    Followup::Done
}

async fn create_direct_room(ctx: JobCtx, request: CreateDirectRoomRequest) -> Followup {
    let JobCtx { client, events, .. } = &ctx;
    match matrix::create_direct_room(client, &request.user_id).await {
        Ok(room_id) => {
            emit_snapshot_then_room_joined(&ctx, Operation::CreateDirectRoom, room_id.into(), None)
                .await
        }
        Err(err) => emit_matrix_error(events, Operation::CreateDirectRoom, err),
    }

    Followup::Done
}

async fn create_room(ctx: JobCtx, request: CreateRoomRequest) -> Followup {
    let JobCtx { client, events, .. } = &ctx;
    match matrix::create_room(client, &request).await {
        Ok(room_id) => {
            emit_snapshot_then_room_joined(&ctx, Operation::CreateRoom, room_id.into(), None).await
        }
        Err(err) => emit_matrix_error(events, Operation::CreateRoom, err),
    }

    Followup::Done
}

/// Emits the room snapshot that lists a room just joined, then
/// `RoomJoined`, so the UI knows the room by the time it opens its window.
/// A failed snapshot is the operation's error about the room, and no
/// `RoomJoined` follows. `alias`: the alias the join named.
async fn emit_snapshot_then_room_joined(
    ctx: &JobCtx,
    operation: Operation,
    room_id: String,
    alias: Option<String>,
) {
    let JobCtx {
        client,
        events,
        snapshots,
        ..
    } = ctx;
    match matrix::emit_room_snapshot(client, snapshots, events).await {
        Ok(()) => emit_event(events, Event::RoomJoined(RoomJoined { room_id, alias })),
        Err(err) => emit_room_matrix_error(events, operation, &room_id, err),
    }
}

async fn leave_room(ctx: JobCtx, request: LeaveRoomRequest) -> Followup {
    let JobCtx { client, events, .. } = &ctx;
    if let Err(err) = matrix::leave_joined_room(client, &request.room_id).await {
        emit_room_matrix_error(events, Operation::LeaveRoom, &request.room_id, err);
        return Followup::Done;
    }

    if let Err(err) = matrix::emit_room_snapshot(client, &ctx.snapshots, events).await {
        emit_room_matrix_error(events, Operation::LeaveRoom, &request.room_id, err);
    }

    Followup::Done
}

async fn kick_room_member(ctx: JobCtx, request: KickRoomMemberRequest) -> Followup {
    let JobCtx { client, events, .. } = &ctx;
    if let Err(err) = matrix::kick_room_member(client, &request).await {
        emit_room_matrix_error(events, Operation::KickRoomMember, &request.room_id, err);
        return Followup::Done;
    }

    if let Err(err) =
        matrix::emit_joined_room_members_snapshot(client, events, &request.room_id).await
    {
        emit_room_matrix_error(events, Operation::KickRoomMember, &request.room_id, err);
    }

    Followup::Done
}

async fn ban_room_member(ctx: JobCtx, request: BanRoomMemberRequest) -> Followup {
    let JobCtx { client, events, .. } = &ctx;
    if let Err(err) = matrix::ban_room_member(client, &request).await {
        emit_room_matrix_error(events, Operation::BanRoomMember, &request.room_id, err);
        return Followup::Done;
    }

    if let Err(err) =
        matrix::emit_joined_room_members_snapshot(client, events, &request.room_id).await
    {
        emit_room_matrix_error(events, Operation::BanRoomMember, &request.room_id, err);
    }

    Followup::Done
}

async fn unban_room_member(ctx: JobCtx, request: UnbanRoomMemberRequest) -> Followup {
    let JobCtx { client, events, .. } = &ctx;
    if let Err(err) = matrix::unban_room_member(client, &request).await {
        emit_room_matrix_error(events, Operation::UnbanRoomMember, &request.room_id, err);
        return Followup::Done;
    }

    if let Err(err) =
        matrix::emit_joined_room_members_snapshot(client, events, &request.room_id).await
    {
        emit_room_matrix_error(events, Operation::UnbanRoomMember, &request.room_id, err);
    }

    Followup::Done
}

async fn invite_room_member(ctx: JobCtx, request: InviteRoomMemberRequest) -> Followup {
    let JobCtx { client, events, .. } = &ctx;
    if let Err(err) = matrix::invite_room_member(client, &request).await {
        emit_room_matrix_error(events, Operation::InviteRoomMember, &request.room_id, err);
        return Followup::Done;
    }

    if let Err(err) =
        matrix::emit_joined_room_members_snapshot(client, events, &request.room_id).await
    {
        emit_room_matrix_error(events, Operation::InviteRoomMember, &request.room_id, err);
    }

    Followup::Done
}

async fn set_display_name(ctx: JobCtx, request: SetDisplayNameRequest) -> Followup {
    let JobCtx { client, events, .. } = &ctx;
    if let Err(err) = matrix::set_display_name(client, &request).await {
        emit_matrix_error(events, Operation::SetDisplayName, err);
    }

    Followup::Done
}

/// Says away in the session's syncs before the server hears it: a sync
/// under way that still said `online` would undo the away on Synapse. A
/// failure says what the syncs said before. The read-back after it only
/// informs: its failure leaves the change in place.
async fn set_away(ctx: JobCtx, away: AwayFlag, message: Option<String>) -> Followup {
    let JobCtx { client, events, .. } = &ctx;
    let was_away = away.replace(message.is_some());
    if let Err(err) = matrix::set_own_presence(client, message.as_deref()).await {
        away.replace(was_away);
        emit_matrix_error(events, Operation::SetPresence, err);
        return Followup::Done;
    }
    match matrix::fetch_own_presence(client).await {
        Ok(seen_as) => emit_event(events, Event::AwayChanged(AwayChanged { message, seen_as })),
        Err(err) => emit_matrix_error(events, Operation::SetPresence, err),
    }

    Followup::Done
}

async fn reject_room_invite(ctx: JobCtx, request: RejectRoomInviteRequest) -> Followup {
    let JobCtx { client, events, .. } = &ctx;
    if let Err(err) = matrix::reject_room_invite(client, &request.room_id).await {
        emit_room_matrix_error(events, Operation::RejectRoomInvite, &request.room_id, err);
        return Followup::Done;
    }

    if let Err(err) = matrix::emit_room_snapshot(client, &ctx.snapshots, events).await {
        emit_room_matrix_error(events, Operation::RejectRoomInvite, &request.room_id, err);
    }

    Followup::Done
}

async fn send_message(ctx: JobCtx, request: SendMessageRequest) -> Followup {
    let JobCtx { client, events, .. } = &ctx;
    let room_id = request.room_id.clone();
    if let Err(err) = matrix::send_message(client, request).await {
        emit_room_matrix_error(events, Operation::SendMessage, &room_id, err);
    }

    Followup::Done
}

async fn send_file(ctx: JobCtx, request: SendFileRequest) -> Followup {
    let JobCtx { client, events, .. } = &ctx;
    let room_id = request.room_id.clone();
    if let Err(err) = matrix::send_file(client, request).await {
        emit_room_matrix_error(events, Operation::SendFile, &room_id, err);
    }

    Followup::Done
}

async fn send_reaction(ctx: JobCtx, request: SendReactionRequest) -> Followup {
    let JobCtx { client, events, .. } = &ctx;
    let room_id = request.room_id.clone();
    if let Err(err) = matrix::send_reaction(client, request).await {
        emit_room_matrix_error(events, Operation::SendReaction, &room_id, err);
    }

    Followup::Done
}

async fn edit_message(ctx: JobCtx, request: EditMessageRequest) -> Followup {
    let JobCtx { client, events, .. } = &ctx;
    let room_id = request.room_id.clone();
    if let Err(err) = matrix::edit_message(client, request).await {
        emit_room_matrix_error(events, Operation::EditMessage, &room_id, err);
    }

    Followup::Done
}

async fn redact_event(ctx: JobCtx, request: RedactEventRequest) -> Followup {
    let JobCtx { client, events, .. } = &ctx;
    let room_id = request.room_id.clone();
    if let Err(err) = matrix::redact_event(client, request).await {
        emit_room_matrix_error(events, Operation::RedactEvent, &room_id, err);
    }

    Followup::Done
}

async fn set_room_topic(ctx: JobCtx, request: SetRoomTopicRequest) -> Followup {
    let JobCtx { client, events, .. } = &ctx;
    let room_id = request.room_id.clone();
    if let Err(err) = matrix::set_room_topic(client, request).await {
        emit_room_matrix_error(events, Operation::SetRoomTopic, &room_id, err);
    }

    Followup::Done
}

async fn fetch_room_settings(ctx: JobCtx, room_id: String) -> Followup {
    let JobCtx { client, events, .. } = &ctx;
    match matrix::fetch_room_settings(client, &room_id).await {
        Ok(settings) => emit_event(events, Event::RoomSettings(settings)),
        Err(err) => emit_room_matrix_error(events, Operation::FetchRoomSettings, &room_id, err),
    }

    Followup::Done
}

async fn update_room_settings(ctx: JobCtx, request: UpdateRoomSettingsRequest) -> Followup {
    let JobCtx { client, events, .. } = &ctx;
    if let Err(err) = matrix::update_room_settings(client, &request).await {
        emit_room_matrix_error(events, Operation::UpdateRoomSettings, &request.room_id, err);
    }

    Followup::Done
}

async fn set_room_notification_mode(
    ctx: JobCtx,
    request: SetRoomNotificationModeRequest,
) -> Followup {
    let JobCtx { client, events, .. } = &ctx;
    let room_id = request.room_id.clone();
    if let Err(err) = matrix::set_room_notification_mode(client, request).await {
        emit_room_matrix_error(events, Operation::SetRoomNotificationMode, &room_id, err);
    }

    Followup::Done
}

async fn add_highlight_word(ctx: JobCtx, word: String) -> Followup {
    let JobCtx { client, events, .. } = &ctx;
    if let Err(err) = matrix::add_highlight_word(client, &word).await {
        emit_matrix_error(events, Operation::AddHighlightWord, err);
    }

    Followup::Done
}

async fn remove_highlight_word(ctx: JobCtx, word: String) -> Followup {
    let JobCtx { client, events, .. } = &ctx;
    if let Err(err) = matrix::remove_highlight_word(client, &word).await {
        emit_matrix_error(events, Operation::RemoveHighlightWord, err);
    }

    Followup::Done
}

async fn ignore_user(ctx: JobCtx, user_id: String) -> Followup {
    let JobCtx { client, events, .. } = &ctx;
    if let Err(err) = matrix::ignore_user(client, &user_id).await {
        emit_matrix_error(events, Operation::IgnoreUser, err);
    }

    Followup::Done
}

async fn unignore_user(ctx: JobCtx, user_id: String) -> Followup {
    let JobCtx { client, events, .. } = &ctx;
    if let Err(err) = matrix::unignore_user(client, &user_id).await {
        emit_matrix_error(events, Operation::UnignoreUser, err);
    }

    Followup::Done
}

async fn fetch_user_profile(ctx: JobCtx, user_id: String) -> Followup {
    let JobCtx { client, events, .. } = &ctx;
    match matrix::fetch_user_profile(client, &user_id).await {
        Ok(profile) => emit_event(events, Event::UserProfile(profile)),
        Err(err) => emit_matrix_error(events, Operation::FetchUserProfile, err),
    }

    Followup::Done
}

async fn mark_room_read(ctx: JobCtx, request: MarkRoomReadRequest) -> Followup {
    let JobCtx { client, events, .. } = &ctx;
    if let Err(err) = matrix::mark_room_read(client, &request).await {
        emit_room_matrix_error(events, Operation::MarkRoomRead, &request.room_id, err);
    }

    Followup::Done
}

async fn set_typing(ctx: JobCtx, request: SetTypingRequest) -> Followup {
    let JobCtx { client, events, .. } = &ctx;
    if let Err(err) = matrix::set_typing(client, &request).await {
        emit_room_matrix_error(events, Operation::SetTyping, &request.room_id, err);
    }

    Followup::Done
}

async fn set_read_marker(ctx: JobCtx, request: SetReadMarkerRequest) -> Followup {
    let JobCtx { client, events, .. } = &ctx;
    if let Err(err) = matrix::set_read_marker(client, &request).await {
        emit_room_matrix_error(events, Operation::SetReadMarker, &request.room_id, err);
    }

    Followup::Done
}

async fn backfill_room_history(ctx: JobCtx, request: BackfillRoomHistoryRequest) -> Followup {
    let JobCtx { client, events, .. } = &ctx;
    let room_id = request.room_id.clone();
    match matrix::backfill_room_history(client, request).await {
        Ok(backfill) => emit_event(events, Event::RoomHistoryBackfill(backfill)),
        Err(err) => emit_room_matrix_error(events, Operation::BackfillRoomHistory, &room_id, err),
    }

    Followup::Done
}

async fn fetch_room_media(ctx: JobCtx, request: FetchRoomMediaRequest) -> Followup {
    let JobCtx { client, events, .. } = &ctx;
    let media = matrix::fetch_room_media(client, request).await;
    emit_event(events, Event::RoomMedia(media));

    Followup::Done
}

async fn fetch_room_event(ctx: JobCtx, request: FetchRoomEventRequest) -> Followup {
    let JobCtx { client, events, .. } = &ctx;
    let fetch = matrix::fetch_room_event(client, request).await;
    emit_event(events, Event::RoomEvent(fetch));

    Followup::Done
}

async fn retry_decryption(ctx: JobCtx, request: RetryDecryptionRequest) -> Followup {
    let JobCtx { client, events, .. } = &ctx;
    let room_id = request.room_id.clone();
    match matrix::retry_decryption(client, request).await {
        Ok(Some(decrypted)) => emit_event(events, Event::RoomEventsDecrypted(decrypted)),
        Ok(None) => {}
        Err(err) => emit_room_matrix_error(events, Operation::RetryDecryption, &room_id, err),
    }

    Followup::Done
}

async fn list_public_rooms(ctx: JobCtx, request: ListPublicRoomsRequest) -> Followup {
    let JobCtx { client, events, .. } = &ctx;
    let page = matrix::list_public_rooms(client, request).await;
    emit_event(events, Event::PublicRooms(page));

    Followup::Done
}

async fn refresh_e2ee_status(ctx: JobCtx) -> Followup {
    let JobCtx {
        client,
        events,
        e2ee_status,
        ..
    } = &ctx;
    match matrix::summarize_e2ee_status(client, BackupServerStatusMode::Fetch).await {
        Ok(status) => e2ee_status.report(events, status),
        Err(err) => emit_matrix_error(events, Operation::E2eeStatus, err),
    }

    Followup::Done
}

async fn recover_e2ee(ctx: JobCtx, request: RecoverE2eeRequest) -> Followup {
    let JobCtx {
        client,
        events,
        e2ee_status,
        ..
    } = &ctx;
    match matrix::recover_e2ee(client, &request).await {
        Ok(status) => {
            emit_event(
                events,
                Event::E2eeRecoverySucceeded(E2eeRecoverySucceeded {
                    operation: E2eeRecoveryOperation::Recover,
                }),
            );
            e2ee_status.report(events, status);
        }
        Err(err) => emit_matrix_error(events, Operation::RecoverE2ee, err),
    }

    Followup::Done
}

async fn export_room_keys(ctx: JobCtx, request: ExportRoomKeysRequest) -> Followup {
    let JobCtx { client, events, .. } = &ctx;
    match matrix::export_room_keys(client, &request).await {
        Ok(()) => emit_event(events, Event::RoomKeysExported { path: request.path }),
        Err(err) => emit_matrix_error(events, Operation::ExportRoomKeys, err),
    }

    Followup::Done
}

async fn import_room_keys(ctx: JobCtx, request: ImportRoomKeysRequest) -> Followup {
    let JobCtx { client, events, .. } = &ctx;
    match matrix::import_room_keys(client, &request).await {
        Ok((imported, total)) => emit_event(
            events,
            Event::RoomKeysImported {
                path: request.path,
                imported,
                total,
            },
        ),
        Err(err) => emit_matrix_error(events, Operation::ImportRoomKeys, err),
    }

    Followup::Done
}

async fn setup_e2ee_recovery(ctx: JobCtx, request: SetupE2eeRecoveryRequest) -> Followup {
    let JobCtx {
        client,
        events,
        e2ee_status,
        ..
    } = &ctx;
    match matrix::setup_e2ee_recovery(client, &request).await {
        Ok((recovery_key, status)) => {
            emit_event(
                events,
                Event::E2eeRecoverySetupSucceeded(E2eeRecoverySetupSucceeded { recovery_key }),
            );
            e2ee_status.report(events, status);
        }
        Err(err) => emit_matrix_error(events, Operation::SetupE2eeRecovery, err),
    }

    Followup::Done
}

async fn list_user_devices(ctx: JobCtx, user_id: String) -> Followup {
    let JobCtx { client, events, .. } = &ctx;
    match matrix::list_user_devices(client, &user_id).await {
        Ok(devices) => emit_event(events, Event::UserDevices(devices)),
        Err(err) => emit_matrix_error(events, Operation::ListUserDevices, err),
    }

    Followup::Done
}

async fn sign_out_device(ctx: JobCtx, request: SignOutDeviceRequest) -> Followup {
    let JobCtx { client, events, .. } = &ctx;
    match matrix::sign_out_device(client, &request.device_id, request.password).await {
        Ok(matrix::SignOutOutcome::SignedOut) => {
            emit_event(events, Event::DeviceSignedOut(request.device_id))
        }
        Ok(matrix::SignOutOutcome::Approval { url }) => emit_event(
            events,
            Event::DeviceSignOutApproval {
                device_id: request.device_id,
                url,
            },
        ),
        Err(err) => emit_matrix_error(events, Operation::SignOutDevice, err),
    }

    Followup::Done
}

async fn rename_device(ctx: JobCtx, request: RenameDeviceRequest) -> Followup {
    let JobCtx { client, events, .. } = &ctx;
    match matrix::rename_device(client, &request.device_id, &request.display_name).await {
        Ok(()) => emit_event(
            events,
            Event::DeviceRenamed {
                device_id: request.device_id,
                display_name: request.display_name,
            },
        ),
        Err(err) => emit_matrix_error(events, Operation::RenameDevice, err),
    }

    Followup::Done
}

async fn accept_user_identity(ctx: JobCtx, user_id: String) -> Followup {
    let JobCtx { client, events, .. } = &ctx;
    match matrix::accept_user_identity(client, &user_id).await {
        Ok(()) => emit_event(events, Event::UserIdentityAccepted(user_id)),
        Err(err) => emit_matrix_error(events, Operation::AcceptUserIdentity, err),
    }

    Followup::Done
}

/// The reset's first step, up to the server's request for
/// authentication; the actor then runs the rest beside it.
async fn reset_e2ee_identity(ctx: JobCtx, request: ResetE2eeIdentityRequest) -> Followup {
    let JobCtx { client, events, .. } = &ctx;
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
        emit_invalid_state(events, Operation::ResetE2eeIdentity, refusal);
        return Followup::Done;
    }

    let handle = match matrix::start_identity_reset(client).await {
        Ok(None) => return end_identity_reset(ctx, IdentityResetEnd::Uploaded).await,
        Ok(Some(handle)) => handle,
        Err(err) => return end_identity_reset(ctx, IdentityResetEnd::Failed(err)).await,
    };
    let auth = match (
        matrix::identity_reset_auth(handle.auth_type()),
        request.password,
    ) {
        (Ok(matrix::IdentityResetAuth::Browser { url }), _) => {
            emit_event(events, Event::E2eeIdentityResetApproval { url });
            None
        }
        (Ok(matrix::IdentityResetAuth::Password { session }), Some(password)) => {
            Some(matrix::password_auth(client, session, password))
        }
        (Ok(matrix::IdentityResetAuth::Password { .. }), None) => {
            let err = matrix::identity_reset_error(&"your server asks for your account password");
            return end_identity_reset(ctx, IdentityResetEnd::Refused(err)).await;
        }
        (Err(err), _) => return end_identity_reset(ctx, IdentityResetEnd::Refused(err)).await,
    };
    Followup::IdentityResetStarted(Box::new(StartedReset { handle, auth }))
}

/// Starts the cancel of the pending reset. Marked at once, not when the
/// job ends: the reset may end first, and its end must be known as
/// cancelled then.
fn cancel_e2ee_identity_reset(session: &mut Session) -> LocalBoxFuture<'static, Followup> {
    let Some(pending) = session.identity_reset() else {
        return refuse(
            session.ctx(),
            Operation::ResetE2eeIdentity,
            "no identity reset is under way",
        );
    };
    pending.cancel_requested = true;
    let handle = Rc::clone(&pending.handle);
    // The SDK sees it after its next refused attempt, within half a
    // second; the reset's end answers.
    Box::pin(async move {
        handle.cancel().await;
        Followup::Done
    })
}

/// The work that answers a reset's end, on the E2EE lane; it owes no ack.
pub(super) fn identity_reset_ended(end: IdentityResetEnd) -> Work<Session, Followup> {
    Work::internal(Box::new(move |session: &mut Session| {
        Box::pin(end_identity_reset(session.ctx(), end))
    }))
}

/// Answers a reset, then reports the E2EE status, which is the truth
/// about this session: the backup and recovery are off even when the
/// identity did not change.
async fn end_identity_reset(ctx: JobCtx, end: IdentityResetEnd) -> Followup {
    let JobCtx {
        client,
        events,
        e2ee_status,
        ..
    } = &ctx;
    match end {
        IdentityResetEnd::Uploaded => emit_identity_reset(events, E2eeIdentityResetOutcome::Done),
        IdentityResetEnd::Cancelled => match matrix::settle_identity_reset(client).await {
            Ok(true) => emit_identity_reset(events, E2eeIdentityResetOutcome::Done),
            Ok(false) => emit_identity_reset(events, E2eeIdentityResetOutcome::Cancelled),
            Err(err) => emit_matrix_error(events, Operation::ResetE2eeIdentity, err),
        },
        IdentityResetEnd::Failed(err) => {
            emit_matrix_error(events, Operation::ResetE2eeIdentity, err);
            if let Err(err) = matrix::settle_identity_reset(client).await {
                emit_matrix_error(events, Operation::ResetE2eeIdentity, err);
            }
        }
        IdentityResetEnd::Refused(err) => {
            emit_error(
                events,
                Operation::ResetE2eeIdentity,
                None,
                format!(
                    "{err}; your key backup and recovery were turned off before the server was asked"
                ),
            );
            if let Err(err) = matrix::settle_identity_reset(client).await {
                emit_matrix_error(events, Operation::ResetE2eeIdentity, err);
            }
        }
    }
    match matrix::summarize_e2ee_status(client, BackupServerStatusMode::Fetch).await {
        Ok(status) => e2ee_status.report(events, status),
        Err(err) => emit_matrix_error(events, Operation::E2eeStatus, err),
    }
    Followup::Done
}

fn emit_identity_reset(events: &EventSender, outcome: E2eeIdentityResetOutcome) {
    emit_event(events, Event::E2eeIdentityReset(outcome));
}

async fn request_verification(ctx: JobCtx, request: RequestVerificationRequest) -> Followup {
    let JobCtx {
        client,
        events,
        verifications,
        ..
    } = &ctx;
    match matrix::request_verification(client, &request).await {
        Ok((announcement, flow)) => {
            emit_event(events, Event::VerificationRequest(announcement.clone()));
            // Its watcher sees the other side's ready and starts SAS.
            verifications.follow(announcement, flow);
        }
        Err(err) => emit_matrix_error(events, Operation::RequestVerification, err),
    }
    Followup::Done
}

/// An answer to a flow: the flow's watcher reports what it changes.
async fn answer_verification<F: Future<Output = Result<(), MatrixCoreError>>>(
    ctx: &JobCtx,
    answer: F,
) -> Followup {
    if let Err(err) = answer.await {
        emit_matrix_error(&ctx.events, Operation::Verification, err);
    }
    Followup::Done
}

async fn accept_verification(ctx: JobCtx, flow: VerificationFlowId) -> Followup {
    answer_verification(&ctx, matrix::accept_verification(&ctx.client, &flow)).await
}

async fn cancel_verification(ctx: JobCtx, flow: VerificationFlowId) -> Followup {
    answer_verification(&ctx, matrix::cancel_verification(&ctx.client, &flow)).await
}

async fn confirm_verification_sas(ctx: JobCtx, flow: VerificationFlowId) -> Followup {
    answer_verification(&ctx, matrix::confirm_verification_sas(&ctx.client, &flow)).await
}

async fn reject_verification_sas(ctx: JobCtx, flow: VerificationFlowId) -> Followup {
    answer_verification(&ctx, matrix::reject_verification_sas(&ctx.client, &flow)).await
}

/// A job that only refuses.
fn refuse(
    ctx: JobCtx,
    operation: Operation,
    message: &'static str,
) -> LocalBoxFuture<'static, Followup> {
    Box::pin(async move {
        emit_invalid_state(&ctx.events, operation, message);
        Followup::Done
    })
}
