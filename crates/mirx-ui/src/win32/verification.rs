//! Device verification and E2EE status: labels and submissions.

use super::*;

pub(super) fn apply_verification_event(state: &mut WindowState, event: VerificationStateEvent) {
    for line in status_log::verification(&event) {
        log_status(state, StatusLineKind::Info, line);
    }
    state.model.status = verification_status_message(&event);
    state.model.last_verification = Some(event);
}

/// A new flow, in or out, replaces the current one: the UI follows one
/// verification at a time.
pub(super) fn apply_verification_request(state: &mut WindowState, request: VerificationRequest) {
    let lines = status_log::verification_request(&request, state.model.own_user_id());
    for line in lines {
        log_status(state, StatusLineKind::Info, line);
    }
    let event = verification_state_event_from_request(request);
    state.model.status = verification_status_message(&event);
    state.model.last_verification = Some(event);
}

/// The flow's first state: requested by the other side, or sent by this
/// session.
pub(super) fn verification_state_event_from_request(
    request: VerificationRequest,
) -> VerificationStateEvent {
    VerificationStateEvent {
        flow_id: request.flow_id,
        user_id: request.user_id,
        state: match request.direction {
            VerificationDirection::Incoming { .. } => VerificationState::Requested,
            VerificationDirection::Outgoing { .. } => VerificationState::Sent,
        },
    }
}

pub(super) fn verification_state_label(state: &VerificationState) -> &'static str {
    match state {
        VerificationState::Requested => "requested",
        VerificationState::Sent => "sent",
        VerificationState::Accepted => "accepted",
        VerificationState::Ready => "ready",
        VerificationState::SasReady { .. } => "sas ready",
        VerificationState::Confirmed => "confirmed",
        VerificationState::Done => "done",
        VerificationState::Cancelled { .. } => "cancelled",
    }
}

pub(super) fn e2ee_recovery_state_label(state: E2eeRecoveryState) -> &'static str {
    match state {
        E2eeRecoveryState::Unknown => "unknown",
        E2eeRecoveryState::Enabled => "enabled",
        E2eeRecoveryState::Disabled => "disabled",
        E2eeRecoveryState::Incomplete => "incomplete",
    }
}

fn e2ee_backup_state_label(state: E2eeBackupState) -> &'static str {
    match state {
        E2eeBackupState::Unknown => "unknown",
        E2eeBackupState::Creating => "creating",
        E2eeBackupState::Enabling => "enabling",
        E2eeBackupState::Resuming => "resuming",
        E2eeBackupState::Enabled => "enabled",
        E2eeBackupState::Downloading => "downloading",
        E2eeBackupState::Disabling => "disabling",
    }
}

pub(super) fn e2ee_local_backup_label(status: &E2eeBackupStatus) -> &'static str {
    match status.state {
        E2eeBackupState::Creating
        | E2eeBackupState::Enabling
        | E2eeBackupState::Resuming
        | E2eeBackupState::Downloading
        | E2eeBackupState::Disabling => e2ee_backup_state_label(status.state),
        E2eeBackupState::Unknown => "unknown",
        _ if status.enabled_locally => "enabled",
        _ => "disabled",
    }
}

pub(super) fn e2ee_server_backup_label(exists_on_server: Option<bool>) -> &'static str {
    match exists_on_server {
        Some(true) => "exists",
        Some(false) => "missing",
        None => "not checked",
    }
}

pub(super) fn e2ee_backup_health_summary(status: &E2eeStatus) -> &'static str {
    match status.backup.state {
        E2eeBackupState::Creating
        | E2eeBackupState::Enabling
        | E2eeBackupState::Resuming
        | E2eeBackupState::Downloading
        | E2eeBackupState::Disabling => {
            return match status.backup.state {
                E2eeBackupState::Creating => "backup creating",
                E2eeBackupState::Enabling => "backup enabling",
                E2eeBackupState::Resuming => "backup resuming",
                E2eeBackupState::Downloading => "backup downloading",
                E2eeBackupState::Disabling => "backup disabling",
                _ => unreachable!(),
            };
        }
        _ => {}
    }

    if status.backup.state == E2eeBackupState::Enabled
        && status.backup.enabled_locally
        && status.backup.exists_on_server == Some(true)
    {
        "backup healthy"
    } else if status.backup.exists_on_server == Some(false) {
        "backup missing on server"
    } else if !status.backup.enabled_locally {
        "local backup disabled"
    } else if status.backup.exists_on_server.is_none() {
        "server backup not checked"
    } else {
        "backup unknown"
    }
}

/// This session's verification, as the SDK's trust state says.
pub(super) fn e2ee_session_label(session: E2eeSessionState) -> &'static str {
    match session {
        E2eeSessionState::Verified => "verified",
        E2eeSessionState::Unverified | E2eeSessionState::NoCrossSigning => "not verified",
        E2eeSessionState::Unknown => "unknown",
    }
}

pub(super) fn e2ee_cross_signing_label(status: Option<&E2eeCrossSigningStatus>) -> &'static str {
    match status {
        Some(status) if status.complete => "complete",
        Some(_) => "incomplete",
        None => "unknown",
    }
}

fn verification_status_message(event: &VerificationStateEvent) -> String {
    match &event.state {
        VerificationState::Requested => format!("verification requested by {}", event.user_id),
        VerificationState::Sent => format!("verification request sent to {}", event.user_id),
        VerificationState::Accepted => format!("verification accepted with {}", event.user_id),
        VerificationState::Ready => format!("verification ready with {}", event.user_id),
        VerificationState::SasReady { .. } => {
            format!("verification sas ready with {}", event.user_id)
        }
        VerificationState::Confirmed => format!("verification confirmed with {}", event.user_id),
        VerificationState::Done => format!("verification done with {}", event.user_id),
        VerificationState::Cancelled { .. } => {
            format!("verification cancelled with {}", event.user_id)
        }
    }
}

pub(super) fn format_verification_emojis(emojis: &[VerificationEmoji]) -> String {
    emojis
        .iter()
        .map(|emoji| format!("{} {}", emoji.symbol, emoji.description))
        .collect::<Vec<_>>()
        .join(" | ")
}

pub(super) fn verification_action_label(action: VerificationAction) -> &'static str {
    match action {
        VerificationAction::Accept => "accept",
        VerificationAction::Cancel => "cancel",
        VerificationAction::Confirm => "confirm",
        VerificationAction::Reject => "reject",
    }
}

fn current_verification_flow_id(model: &UiModel) -> Result<VerificationFlowId, String> {
    let event = model
        .last_verification
        .as_ref()
        .ok_or_else(|| "no verification flow available".to_owned())?;
    Ok(VerificationFlowId {
        flow_id: event.flow_id.clone(),
        user_id: event.user_id.clone(),
    })
}

fn submit_verification_command(
    state: &mut WindowState,
    command: Command,
    action_name: &'static str,
) -> Result<(), String> {
    {
        let sender = state
            .command_sender
            .as_ref()
            .ok_or_else(|| "core command sender is not available".to_owned())?;
        sender.send(command.clone()).map_err(|err| {
            format!("failed to enqueue {action_name} verification command: {err}")
        })?;
    }
    state.harness.record_command(command);
    Ok(())
}

pub(super) unsafe fn submit_menu_verification_action(
    hwnd: HWND,
    action: VerificationAction,
) -> Result<(), String> {
    {
        let state = state_mut(hwnd);
        submit_menu_verification_action_to_state(state, action)?;
    }
    set_local_command_status(
        hwnd,
        format!(
            "verification {} submitted",
            verification_action_label(action)
        ),
    )
}

fn submit_menu_verification_action_to_state(
    state: &mut WindowState,
    action: VerificationAction,
) -> Result<(), String> {
    if !verification_menu_action_enabled(state.model.last_verification.as_ref(), action) {
        return Err("verification action is not available in the current state".to_owned());
    }

    match action {
        VerificationAction::Accept => submit_accept_verification(state),
        VerificationAction::Cancel => submit_cancel_verification(state),
        VerificationAction::Confirm => submit_confirm_verification_sas(state),
        VerificationAction::Reject => submit_reject_verification_sas(state),
    }
}

fn submit_accept_verification_flow(
    state: &mut WindowState,
    flow: VerificationFlowId,
) -> Result<(), String> {
    submit_verification_command(state, Command::AcceptVerification(flow), "accept")
}

pub(super) fn submit_accept_verification(state: &mut WindowState) -> Result<(), String> {
    let flow = current_verification_flow_id(&state.model)?;
    submit_accept_verification_flow(state, flow)
}

fn submit_cancel_verification_flow(
    state: &mut WindowState,
    flow: VerificationFlowId,
) -> Result<(), String> {
    submit_verification_command(state, Command::CancelVerification(flow), "cancel")
}

pub(super) fn submit_cancel_verification(state: &mut WindowState) -> Result<(), String> {
    let flow = current_verification_flow_id(&state.model)?;
    submit_cancel_verification_flow(state, flow)
}

fn submit_confirm_verification_sas_flow(
    state: &mut WindowState,
    flow: VerificationFlowId,
) -> Result<(), String> {
    submit_verification_command(state, Command::ConfirmVerificationSas(flow), "confirm")
}

pub(super) fn submit_confirm_verification_sas(state: &mut WindowState) -> Result<(), String> {
    let flow = current_verification_flow_id(&state.model)?;
    submit_confirm_verification_sas_flow(state, flow)
}

fn submit_reject_verification_sas_flow(
    state: &mut WindowState,
    flow: VerificationFlowId,
) -> Result<(), String> {
    submit_verification_command(state, Command::RejectVerificationSas(flow), "reject")
}

pub(super) fn submit_reject_verification_sas(state: &mut WindowState) -> Result<(), String> {
    let flow = current_verification_flow_id(&state.model)?;
    submit_reject_verification_sas_flow(state, flow)
}

/// The automation harness's part of this module: what it reads and does
/// here for the protocol.
#[cfg(any(test, feature = "automation"))]
pub(super) mod automation {
    use super::*;

    fn verification_flow_id_from_action_args(
        model: &UiModel,
        args: &VerificationActionArgs,
    ) -> Result<VerificationFlowId, String> {
        match (&args.flow_id, &args.user_id) {
            (Some(flow_id), Some(user_id)) => Ok(VerificationFlowId {
                flow_id: flow_id.clone(),
                user_id: user_id.clone(),
            }),
            (None, None) => current_verification_flow_id(model),
            _ => unreachable!("verification action args must contain both flow_id and user_id"),
        }
    }

    pub(in crate::win32) unsafe fn submit_automation_verification_action(
        hwnd: HWND,
        action: VerificationAction,
        args: VerificationActionArgs,
    ) -> Result<(), String> {
        {
            let state = state_mut(hwnd);
            submit_verification_automation_action_to_state(state, action, args)?;
        }
        set_local_command_status(
            hwnd,
            format!(
                "verification {} submitted",
                verification_action_label(action)
            ),
        )
    }

    pub(in crate::win32) fn submit_verification_automation_action_to_state(
        state: &mut WindowState,
        action: VerificationAction,
        args: VerificationActionArgs,
    ) -> Result<(), String> {
        let flow = verification_flow_id_from_action_args(&state.model, &args)?;
        match action {
            VerificationAction::Accept => submit_accept_verification_flow(state, flow),
            VerificationAction::Cancel => submit_cancel_verification_flow(state, flow),
            VerificationAction::Confirm => submit_confirm_verification_sas_flow(state, flow),
            VerificationAction::Reject => submit_reject_verification_sas_flow(state, flow),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::menus::verification_actions_text;
    use super::automation::*;

    use super::super::test_fixtures::*;
    use super::*;

    #[test]
    fn e2ee_backup_status_labels_distinguish_missing_unknown_and_transitional_states() {
        let missing = e2ee_status(
            E2eeRecoveryState::Enabled,
            E2eeBackupState::Enabled,
            true,
            Some(false),
            Some(true),
        );
        assert!(
            status_log::e2ee_status(&missing)[0]
                .1
                .contains("server backup missing")
        );
        assert_eq!(
            e2ee_backup_health_summary(&missing),
            "backup missing on server"
        );

        let unknown = e2ee_status(
            E2eeRecoveryState::Enabled,
            E2eeBackupState::Enabled,
            true,
            None,
            Some(true),
        );
        assert!(
            status_log::e2ee_status(&unknown)[0]
                .1
                .contains("server backup not checked")
        );
        assert_eq!(
            e2ee_backup_health_summary(&unknown),
            "server backup not checked"
        );

        let transitional = e2ee_status(
            E2eeRecoveryState::Enabled,
            E2eeBackupState::Enabling,
            false,
            None,
            Some(true),
        );
        assert_eq!(e2ee_backup_health_summary(&transitional), "backup enabling");
    }

    fn test_verification_state_event(state: VerificationState) -> VerificationStateEvent {
        VerificationStateEvent {
            flow_id: "flow-current".to_owned(),
            user_id: "@alice:example".to_owned(),
            state,
        }
    }

    fn test_verification_sas_ready_state() -> VerificationState {
        VerificationState::SasReady {
            emojis: vec![VerificationEmoji {
                symbol: "🐶".to_owned(),
                description: "dog".to_owned(),
            }],
            decimals: mirx_core::VerificationDecimals {
                first: 1234,
                second: 5678,
                third: 9012,
            },
        }
    }

    #[test]
    fn verification_menu_action_predicate_matches_state_table() {
        let assert_actions = |last_verification: Option<&VerificationStateEvent>,
                              accept: bool,
                              cancel: bool,
                              confirm: bool,
                              reject: bool| {
            assert_eq!(
                verification_menu_action_enabled(last_verification, VerificationAction::Accept),
                accept
            );
            assert_eq!(
                verification_menu_action_enabled(last_verification, VerificationAction::Cancel),
                cancel
            );
            assert_eq!(
                verification_menu_action_enabled(last_verification, VerificationAction::Confirm),
                confirm
            );
            assert_eq!(
                verification_menu_action_enabled(last_verification, VerificationAction::Reject),
                reject
            );
        };

        let requested = test_verification_state_event(VerificationState::Requested);
        let sent = test_verification_state_event(VerificationState::Sent);
        let accepted = test_verification_state_event(VerificationState::Accepted);
        let ready = test_verification_state_event(VerificationState::Ready);
        let sas_ready = test_verification_state_event(test_verification_sas_ready_state());
        let confirmed = test_verification_state_event(VerificationState::Confirmed);
        let done = test_verification_state_event(VerificationState::Done);
        let cancelled = test_verification_state_event(VerificationState::Cancelled {
            reason: "timed out".to_owned(),
            code: "m.timeout".to_owned(),
            cancelled_by_us: false,
        });

        assert_actions(None, false, false, false, false);
        assert_actions(Some(&requested), true, true, false, false);
        assert_actions(Some(&sent), false, true, false, false);
        assert_actions(Some(&accepted), false, true, false, false);
        assert_actions(Some(&ready), false, true, false, false);
        assert_actions(Some(&sas_ready), false, true, true, true);
        assert_actions(Some(&confirmed), false, true, false, false);
        assert_actions(Some(&done), false, false, false, false);
        assert_actions(Some(&cancelled), false, false, false, false);
    }

    #[test]
    fn verification_actions_text_matches_native_menu_affordance_strings() {
        let requested = test_verification_state_event(VerificationState::Requested);
        let sas_ready = test_verification_state_event(test_verification_sas_ready_state());
        let done = test_verification_state_event(VerificationState::Done);
        let cancelled = test_verification_state_event(VerificationState::Cancelled {
            reason: "timed out".to_owned(),
            code: "m.timeout".to_owned(),
            cancelled_by_us: false,
        });

        assert_eq!(
            verification_actions_text(Some(&requested)),
            Some(
                "Tools > Accept Verification, Tools > Cancel Verification, /verify accept, /verify cancel"
            )
        );
        assert_eq!(
            verification_actions_text(Some(&sas_ready)),
            Some(
                "Tools > Confirm SAS, Tools > Reject SAS, Tools > Cancel Verification, /verify confirm, /verify reject, /verify cancel"
            )
        );
        assert_eq!(
            verification_actions_text(Some(&test_verification_state_event(
                VerificationState::Sent
            ))),
            Some("Tools > Cancel Verification, /verify cancel")
        );
        assert_eq!(verification_actions_text(Some(&done)), None);
        assert_eq!(verification_actions_text(Some(&cancelled)), None);
        assert_eq!(verification_actions_text(None), None);
    }

    #[test]
    fn submit_verification_helpers_enqueue_typed_commands_for_current_flow() {
        let mut state = test_state();
        let handle = start_test_core();
        state.command_sender = Some(handle.command_sender().clone());
        state.model.last_verification = Some(VerificationStateEvent {
            flow_id: "flow-verify-123".to_owned(),
            user_id: "@alice:example".to_owned(),
            state: VerificationState::SasReady {
                emojis: vec![VerificationEmoji {
                    symbol: "🐶".to_owned(),
                    description: "dog".to_owned(),
                }],
                decimals: mirx_core::VerificationDecimals {
                    first: 1234,
                    second: 5678,
                    third: 9012,
                },
            },
        });
        let last_verification = state.model.last_verification.clone();

        submit_accept_verification(&mut state).unwrap();
        submit_cancel_verification(&mut state).unwrap();
        submit_confirm_verification_sas(&mut state).unwrap();
        submit_reject_verification_sas(&mut state).unwrap();

        assert_eq!(state.model.last_verification, last_verification);
        assert_eq!(
            state.harness.commands,
            vec![
                Command::AcceptVerification(VerificationFlowId {
                    flow_id: "flow-verify-123".to_owned(),
                    user_id: "@alice:example".to_owned(),
                }),
                Command::CancelVerification(VerificationFlowId {
                    flow_id: "flow-verify-123".to_owned(),
                    user_id: "@alice:example".to_owned(),
                }),
                Command::ConfirmVerificationSas(VerificationFlowId {
                    flow_id: "flow-verify-123".to_owned(),
                    user_id: "@alice:example".to_owned(),
                }),
                Command::RejectVerificationSas(VerificationFlowId {
                    flow_id: "flow-verify-123".to_owned(),
                    user_id: "@alice:example".to_owned(),
                }),
            ]
        );

        handle
            .command_sender()
            .send(Command::Shutdown)
            .expect("shutdown command must enqueue");
        handle.join().expect("test core must stop cleanly");
    }

    #[test]
    fn submit_menu_verification_action_rejects_disabled_actions_without_recording_commands() {
        let mut state = test_state();
        let handle = start_test_core();
        state.command_sender = Some(handle.command_sender().clone());
        state.model.last_verification =
            Some(test_verification_state_event(VerificationState::Accepted));
        let last_verification = state.model.last_verification.clone();

        assert_eq!(
            submit_menu_verification_action_to_state(&mut state, VerificationAction::Accept),
            Err("verification action is not available in the current state".to_owned())
        );
        assert_eq!(state.model.last_verification, last_verification);
        assert!(state.harness.commands.is_empty());

        handle
            .command_sender()
            .send(Command::Shutdown)
            .expect("shutdown command must enqueue");
        handle.join().expect("test core must stop cleanly");
    }

    #[test]
    fn submit_menu_verification_action_enqueues_typed_commands_for_requested_flow() {
        let mut state = test_state();
        let handle = start_test_core();
        state.command_sender = Some(handle.command_sender().clone());
        state.model.last_verification =
            Some(test_verification_state_event(VerificationState::Requested));
        let last_verification = state.model.last_verification.clone();

        submit_menu_verification_action_to_state(&mut state, VerificationAction::Accept).unwrap();
        submit_menu_verification_action_to_state(&mut state, VerificationAction::Cancel).unwrap();

        assert_eq!(state.model.last_verification, last_verification);
        assert_eq!(
            state.harness.commands,
            vec![
                Command::AcceptVerification(VerificationFlowId {
                    flow_id: "flow-current".to_owned(),
                    user_id: "@alice:example".to_owned(),
                }),
                Command::CancelVerification(VerificationFlowId {
                    flow_id: "flow-current".to_owned(),
                    user_id: "@alice:example".to_owned(),
                }),
            ]
        );

        handle
            .command_sender()
            .send(Command::Shutdown)
            .expect("shutdown command must enqueue");
        handle.join().expect("test core must stop cleanly");
    }

    #[test]
    fn submit_menu_verification_action_enqueues_typed_commands_for_sas_ready_flow() {
        let mut state = test_state();
        let handle = start_test_core();
        state.command_sender = Some(handle.command_sender().clone());
        state.model.last_verification = Some(test_verification_state_event(
            test_verification_sas_ready_state(),
        ));
        let last_verification = state.model.last_verification.clone();

        submit_menu_verification_action_to_state(&mut state, VerificationAction::Confirm).unwrap();
        submit_menu_verification_action_to_state(&mut state, VerificationAction::Reject).unwrap();
        submit_menu_verification_action_to_state(&mut state, VerificationAction::Cancel).unwrap();

        assert_eq!(state.model.last_verification, last_verification);
        assert_eq!(
            state.harness.commands,
            vec![
                Command::ConfirmVerificationSas(VerificationFlowId {
                    flow_id: "flow-current".to_owned(),
                    user_id: "@alice:example".to_owned(),
                }),
                Command::RejectVerificationSas(VerificationFlowId {
                    flow_id: "flow-current".to_owned(),
                    user_id: "@alice:example".to_owned(),
                }),
                Command::CancelVerification(VerificationFlowId {
                    flow_id: "flow-current".to_owned(),
                    user_id: "@alice:example".to_owned(),
                }),
            ]
        );

        handle
            .command_sender()
            .send(Command::Shutdown)
            .expect("shutdown command must enqueue");
        handle.join().expect("test core must stop cleanly");
    }

    #[test]
    fn submit_verification_helpers_still_allow_accept_after_request_has_advanced() {
        let mut state = test_state();
        let handle = start_test_core();
        state.command_sender = Some(handle.command_sender().clone());
        state.model.last_verification =
            Some(test_verification_state_event(VerificationState::Accepted));
        let last_verification = state.model.last_verification.clone();

        submit_accept_verification(&mut state).unwrap();

        assert_eq!(state.model.last_verification, last_verification);
        assert_eq!(
            state.harness.commands,
            vec![Command::AcceptVerification(VerificationFlowId {
                flow_id: "flow-current".to_owned(),
                user_id: "@alice:example".to_owned(),
            })]
        );

        handle
            .command_sender()
            .send(Command::Shutdown)
            .expect("shutdown command must enqueue");
        handle.join().expect("test core must stop cleanly");
    }

    #[test]
    fn automation_verification_actions_use_current_flow_when_args_are_omitted() {
        let mut state = test_state();
        let handle = start_test_core();
        state.command_sender = Some(handle.command_sender().clone());
        state.model.last_verification = Some(VerificationStateEvent {
            flow_id: "flow-current".to_owned(),
            user_id: "@alice:example".to_owned(),
            state: VerificationState::Requested,
        });
        let last_verification = state.model.last_verification.clone();

        submit_verification_automation_action_to_state(
            &mut state,
            VerificationAction::Accept,
            VerificationActionArgs::default(),
        )
        .unwrap();
        submit_verification_automation_action_to_state(
            &mut state,
            VerificationAction::Cancel,
            VerificationActionArgs::default(),
        )
        .unwrap();
        submit_verification_automation_action_to_state(
            &mut state,
            VerificationAction::Confirm,
            VerificationActionArgs::default(),
        )
        .unwrap();
        submit_verification_automation_action_to_state(
            &mut state,
            VerificationAction::Reject,
            VerificationActionArgs::default(),
        )
        .unwrap();

        assert_eq!(state.model.last_verification, last_verification);
        assert_eq!(
            state.harness.commands,
            vec![
                Command::AcceptVerification(VerificationFlowId {
                    flow_id: "flow-current".to_owned(),
                    user_id: "@alice:example".to_owned(),
                }),
                Command::CancelVerification(VerificationFlowId {
                    flow_id: "flow-current".to_owned(),
                    user_id: "@alice:example".to_owned(),
                }),
                Command::ConfirmVerificationSas(VerificationFlowId {
                    flow_id: "flow-current".to_owned(),
                    user_id: "@alice:example".to_owned(),
                }),
                Command::RejectVerificationSas(VerificationFlowId {
                    flow_id: "flow-current".to_owned(),
                    user_id: "@alice:example".to_owned(),
                }),
            ]
        );

        handle
            .command_sender()
            .send(Command::Shutdown)
            .expect("shutdown command must enqueue");
        handle.join().expect("test core must stop cleanly");
    }

    #[test]
    fn automation_verification_actions_use_explicit_flow_args_without_mutating_current_flow() {
        let mut state = test_state();
        let handle = start_test_core();
        state.command_sender = Some(handle.command_sender().clone());
        state.model.last_verification = Some(VerificationStateEvent {
            flow_id: "flow-current".to_owned(),
            user_id: "@alice:example".to_owned(),
            state: VerificationState::SasReady {
                emojis: vec![VerificationEmoji {
                    symbol: "🐶".to_owned(),
                    description: "dog".to_owned(),
                }],
                decimals: mirx_core::VerificationDecimals {
                    first: 1234,
                    second: 5678,
                    third: 9012,
                },
            },
        });
        let last_verification = state.model.last_verification.clone();

        submit_verification_automation_action_to_state(
            &mut state,
            VerificationAction::Accept,
            VerificationActionArgs {
                flow_id: Some("flow-explicit".to_owned()),
                user_id: Some("@bob:example".to_owned()),
            },
        )
        .unwrap();

        assert_eq!(state.model.last_verification, last_verification);
        assert_eq!(
            state.harness.commands,
            vec![Command::AcceptVerification(VerificationFlowId {
                flow_id: "flow-explicit".to_owned(),
                user_id: "@bob:example".to_owned(),
            })]
        );

        handle
            .command_sender()
            .send(Command::Shutdown)
            .expect("shutdown command must enqueue");
        handle.join().expect("test core must stop cleanly");
    }

    #[test]
    fn automation_verification_partial_explicit_args_fail_during_parsing_before_submission() {
        let state = test_state();

        let error = parse_command_request(
            crate::automation::COMMAND_ID_ACCEPT_VERIFICATION,
            br#"{"id":"accept-verification","args":{"flow_id":"flow-1"}}"#,
        )
        .expect_err("partial explicit flow args must fail");

        assert_eq!(
            serde_json::to_value(error).unwrap()["err"]["code"],
            json!("invalid_args")
        );
        assert!(state.harness.commands.is_empty());
    }

    #[test]
    fn submit_verification_helpers_require_current_flow() {
        let mut state = test_state();

        assert_eq!(
            submit_accept_verification(&mut state),
            Err("no verification flow available".to_owned())
        );
        assert_eq!(
            submit_cancel_verification(&mut state),
            Err("no verification flow available".to_owned())
        );
        assert_eq!(
            submit_confirm_verification_sas(&mut state),
            Err("no verification flow available".to_owned())
        );
        assert_eq!(
            submit_reject_verification_sas(&mut state),
            Err("no verification flow available".to_owned())
        );
    }
}
