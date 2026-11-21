use std::collections::HashMap;
use std::io;
use std::rc::Rc;

use matrix_sdk::Client;
use matrix_sdk::encryption::recovery::{IdentityResetHandle, RecoveryError};
use tokio::sync::mpsc::UnboundedReceiver;
use tokio::task::JoinHandle as TokioJoinHandle;

use super::commands::{
    finish_pending_identity_reset, handle_accept_room_invite, handle_accept_user_identity,
    handle_accept_verification, handle_add_highlight_word, handle_backfill_room_history,
    handle_ban_room_member, handle_cancel_e2ee_identity_reset, handle_cancel_verification,
    handle_confirm_verification_sas, handle_create_direct_room, handle_create_room,
    handle_edit_message, handle_fetch_room_event, handle_fetch_room_media,
    handle_fetch_user_profile, handle_ignore_user, handle_invite_room_member, handle_join_room,
    handle_kick_room_member, handle_leave_room, handle_list_public_rooms, handle_list_user_devices,
    handle_mark_room_read, handle_recover_e2ee, handle_redact_event, handle_refresh_e2ee_status,
    handle_reject_room_invite, handle_reject_verification_sas, handle_remove_highlight_word,
    handle_rename_device, handle_request_verification, handle_reset_e2ee_identity,
    handle_send_file, handle_send_message, handle_send_reaction, handle_set_display_name,
    handle_set_room_member_power_level, handle_set_room_notification_mode, handle_set_room_topic,
    handle_setup_e2ee_recovery, handle_sign_out_device, handle_unban_room_member,
    handle_unignore_user,
};
use super::lifecycle::{
    finish_browser_login, handle_cancel_login, handle_login, handle_logout, handle_restore_session,
    handle_start_sync, handle_stop_sync,
};
use super::{EventSender, InitialSnapshot, emit_event};
use crate::api::{Command, CoreConfig, Event};
use crate::loopback::LoopbackRedirect;
use crate::matrix::{BrowserFlow, PausedSendRooms};

pub(super) enum WorkerState {
    NoClient,
    /// A browser sign-in waits for the browser to come back to the
    /// loopback listener.
    AwaitingBrowser(Box<PendingBrowserLogin>),
    Ready {
        client: Client,
        /// Given to every sync task, which emits it if it is pending.
        initial_snapshot: InitialSnapshot,
        /// Shared by the send queue's update task and every sync task.
        paused_sends: PausedSendRooms,
        sync_task: Option<TokioJoinHandle<()>>,
        session_update_task: TokioJoinHandle<()>,
        send_queue_update_task: TokioJoinHandle<()>,
        room_list_update_task: TokioJoinHandle<()>,
        room_read_state_update_task: TokioJoinHandle<()>,
        e2ee_recovery_state_task: TokioJoinHandle<()>,
        e2ee_backup_state_task: TokioJoinHandle<()>,
        e2ee_session_state_task: TokioJoinHandle<()>,
        room_identity_task: TokioJoinHandle<()>,
        verification_tasks: HashMap<(String, String), TokioJoinHandle<()>>,
        /// The one identity reset under way, between the server's request
        /// for authentication and its end.
        identity_reset: Option<PendingIdentityReset>,
    },
}

/// An identity reset waiting for the server to accept its cross-signing
/// upload: approval in the browser, or the password checked.
pub(super) struct PendingIdentityReset {
    pub(super) handle: Rc<IdentityResetHandle>,
    /// `handle.reset(..)`: it asks every half second for up to two
    /// minutes, so it runs beside the worker loop.
    pub(super) task: TokioJoinHandle<Result<(), RecoveryError>>,
    /// `CancelE2eeIdentityReset` was asked. The SDK then ends the task with
    /// `Ok` without uploading, unless an upload just got through: only the
    /// server knows which (see `matrix::settle_identity_reset`).
    pub(super) cancel_requested: bool,
}

pub(super) struct PendingBrowserLogin {
    pub(super) client: Client,
    pub(super) flow: BrowserFlow,
    /// The listener, answering until the browser's request for the
    /// redirect URI; it closes its port when it ends.
    pub(super) redirect: TokioJoinHandle<io::Result<LoopbackRedirect>>,
    pub(super) start_sync: bool,
}

/// What the worker does next.
#[expect(
    clippy::large_enum_variant,
    reason = "a value of one loop iteration, never stored"
)]
pub(super) enum Next {
    Command(Option<Command>),
    Redirect(io::Result<LoopbackRedirect>),
    IdentityReset(Result<(), RecoveryError>),
}

pub(super) async fn run_worker(
    config: CoreConfig,
    mut command_rx: UnboundedReceiver<Command>,
    event_tx: EventSender,
) {
    emit_event(&event_tx, Event::Started);

    let mut state = WorkerState::NoClient;

    loop {
        // A browser sign-in waits for its redirect and for commands at
        // once; both are cancel-safe. The listener task reads the
        // requests, so a command never cuts one short.
        let next = match &mut state {
            WorkerState::AwaitingBrowser(pending) => tokio::select! {
                command = command_rx.recv() => Next::Command(command),
                redirect = &mut pending.redirect => Next::Redirect(
                    redirect.unwrap_or_else(|err| std::panic::resume_unwind(err.into_panic())),
                ),
            },
            // So does an identity reset, for the server's approval.
            WorkerState::Ready {
                identity_reset: Some(pending),
                ..
            } => tokio::select! {
                command = command_rx.recv() => Next::Command(command),
                result = &mut pending.task => Next::IdentityReset(
                    result.unwrap_or_else(|err| std::panic::resume_unwind(err.into_panic())),
                ),
            },
            WorkerState::NoClient
            | WorkerState::Ready {
                identity_reset: None,
                ..
            } => Next::Command(command_rx.recv().await),
        };
        match next {
            Next::Command(None) => break,
            Next::Command(Some(command)) => {
                let stop = handle_command(command, &config, &event_tx, &mut state).await;
                // After every event the command emitted itself; a redirect
                // or a finished identity reset is no command, so no ack.
                emit_event(&event_tx, Event::CommandHandled);
                if stop {
                    break;
                }
            }
            Next::Redirect(redirect) => finish_browser_login(&event_tx, &mut state, redirect).await,
            Next::IdentityReset(result) => {
                finish_pending_identity_reset(&event_tx, &mut state, result).await
            }
        }
    }

    shutdown_state(&mut state).await;
    emit_event(&event_tx, Event::Stopped);
}

pub(super) async fn handle_command(
    command: Command,
    config: &CoreConfig,
    event_tx: &EventSender,
    state: &mut WorkerState,
) -> bool {
    match command {
        Command::Login(request) => handle_login(config, event_tx, state, request).await,
        Command::CancelLogin => handle_cancel_login(event_tx, state).await,
        Command::RestoreSession(request) => {
            handle_restore_session(config, event_tx, state, request).await
        }
        Command::StartSync => handle_start_sync(event_tx, state),
        Command::StopSync => handle_stop_sync(event_tx, state),
        Command::JoinRoom(request) => handle_join_room(event_tx, state, request).await,
        Command::AcceptRoomInvite(request) => {
            handle_accept_room_invite(event_tx, state, request).await
        }
        Command::CreateDirectRoom(request) => {
            handle_create_direct_room(event_tx, state, request).await
        }
        Command::CreateRoom(request) => handle_create_room(event_tx, state, request).await,
        Command::LeaveRoom(request) => handle_leave_room(event_tx, state, request).await,
        Command::KickRoomMember(request) => handle_kick_room_member(event_tx, state, request).await,
        Command::BanRoomMember(request) => handle_ban_room_member(event_tx, state, request).await,
        Command::UnbanRoomMember(request) => {
            handle_unban_room_member(event_tx, state, request).await
        }
        Command::InviteRoomMember(request) => {
            handle_invite_room_member(event_tx, state, request).await
        }
        Command::SetRoomMemberPowerLevel(request) => {
            handle_set_room_member_power_level(event_tx, state, request).await
        }
        Command::SetDisplayName(request) => handle_set_display_name(event_tx, state, request).await,
        Command::RejectRoomInvite(request) => {
            handle_reject_room_invite(event_tx, state, request).await
        }
        Command::SetRoomTopic(request) => handle_set_room_topic(event_tx, state, request).await,
        Command::SetRoomNotificationMode(request) => {
            handle_set_room_notification_mode(event_tx, state, request).await
        }
        Command::AddHighlightWord(word) => handle_add_highlight_word(event_tx, state, word).await,
        Command::RemoveHighlightWord(word) => {
            handle_remove_highlight_word(event_tx, state, word).await
        }
        Command::IgnoreUser(user_id) => handle_ignore_user(event_tx, state, user_id).await,
        Command::UnignoreUser(user_id) => handle_unignore_user(event_tx, state, user_id).await,
        Command::FetchUserProfile(user_id) => {
            handle_fetch_user_profile(event_tx, state, user_id).await
        }
        Command::ListUserDevices(user_id) => {
            handle_list_user_devices(event_tx, state, user_id).await
        }
        Command::AcceptUserIdentity(user_id) => {
            handle_accept_user_identity(event_tx, state, user_id).await
        }
        Command::SignOutDevice(request) => handle_sign_out_device(event_tx, state, request).await,
        Command::RenameDevice(request) => handle_rename_device(event_tx, state, request).await,
        Command::SendMessage(request) => handle_send_message(event_tx, state, request).await,
        Command::SendFile(request) => handle_send_file(event_tx, state, request).await,
        Command::EditMessage(request) => handle_edit_message(event_tx, state, request).await,
        Command::SendReaction(request) => handle_send_reaction(event_tx, state, request).await,
        Command::RedactEvent(request) => handle_redact_event(event_tx, state, request).await,
        Command::MarkRoomRead(request) => handle_mark_room_read(event_tx, state, request).await,
        Command::BackfillRoomHistory(request) => {
            handle_backfill_room_history(event_tx, state, request).await
        }
        Command::FetchRoomMedia(request) => handle_fetch_room_media(event_tx, state, request).await,
        Command::FetchRoomEvent(request) => handle_fetch_room_event(event_tx, state, request).await,
        Command::ListPublicRooms(request) => {
            handle_list_public_rooms(event_tx, state, request).await
        }
        Command::RefreshE2eeStatus => handle_refresh_e2ee_status(event_tx, state).await,
        Command::SetupE2eeRecovery(request) => {
            handle_setup_e2ee_recovery(event_tx, state, &request).await
        }
        Command::RecoverE2ee(request) => handle_recover_e2ee(event_tx, state, &request).await,
        Command::ResetE2eeIdentity(request) => {
            handle_reset_e2ee_identity(event_tx, state, request).await
        }
        Command::CancelE2eeIdentityReset => {
            handle_cancel_e2ee_identity_reset(event_tx, state).await
        }
        Command::RequestVerification(request) => {
            handle_request_verification(event_tx, state, request).await
        }
        Command::AcceptVerification(flow) => {
            handle_accept_verification(event_tx, state, flow).await
        }
        Command::CancelVerification(flow) => {
            handle_cancel_verification(event_tx, state, flow).await
        }
        Command::ConfirmVerificationSas(flow) => {
            handle_confirm_verification_sas(event_tx, state, flow).await
        }
        Command::RejectVerificationSas(flow) => {
            handle_reject_verification_sas(event_tx, state, flow).await
        }
        Command::Logout => handle_logout(event_tx, state).await,
        Command::Shutdown => true,
    }
}

/// Stops every worker task and waits until each is gone. The tasks own
/// `Client` clones, and the SDK store closes its connections on the runtime
/// when the last one drops. A task still queued when `run_worker` returns
/// would instead be dropped with the `LocalSet`, outside the runtime, where
/// that panics.
pub(super) async fn shutdown_state(state: &mut WorkerState) {
    let tasks = match std::mem::replace(state, WorkerState::NoClient) {
        WorkerState::NoClient => return,
        // The listener's task does not own the client, which drops here,
        // on the runtime.
        WorkerState::AwaitingBrowser(pending) => {
            let PendingBrowserLogin { redirect, .. } = *pending;
            redirect.abort();
            if let Err(err) = redirect.await
                && err.is_panic()
            {
                std::panic::resume_unwind(err.into_panic());
            }
            return;
        }
        WorkerState::Ready {
            client: _,
            initial_snapshot: _,
            paused_sends: _,
            sync_task,
            session_update_task,
            send_queue_update_task,
            room_list_update_task,
            room_read_state_update_task,
            e2ee_recovery_state_task,
            e2ee_backup_state_task,
            e2ee_session_state_task,
            room_identity_task,
            verification_tasks,
            identity_reset,
        } => {
            if let Some(pending) = identity_reset {
                pending.task.abort();
                if let Err(err) = pending.task.await
                    && err.is_panic()
                {
                    std::panic::resume_unwind(err.into_panic());
                }
            }
            sync_task
                .into_iter()
                .chain([
                    session_update_task,
                    send_queue_update_task,
                    room_list_update_task,
                    room_read_state_update_task,
                    e2ee_recovery_state_task,
                    e2ee_backup_state_task,
                    e2ee_session_state_task,
                    room_identity_task,
                ])
                .chain(verification_tasks.into_values())
                .collect::<Vec<_>>()
        }
    };
    for task in &tasks {
        task.abort();
    }
    for task in tasks {
        if let Err(err) = task.await
            && err.is_panic()
        {
            std::panic::resume_unwind(err.into_panic());
        }
    }
}

pub(super) fn abort_sync_task(sync_task: &mut Option<TokioJoinHandle<()>>) {
    if let Some(task) = sync_task.take() {
        task.abort();
    }
}

pub(super) fn abort_verification_tasks(
    verification_tasks: &mut HashMap<(String, String), TokioJoinHandle<()>>,
) {
    for (_, task) in verification_tasks.drain() {
        task.abort();
    }
}
