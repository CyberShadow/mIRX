//! Device verification and E2EE status: the verifications under way,
//! their labels, and the answers to them.

use super::*;

/// A verification under way, as the core reported it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Verification {
    pub(super) flow_id: String,
    /// The other side: your own user for a verification between your
    /// sessions.
    pub(super) user_id: String,
    /// The other side's session, once known.
    pub(super) device_id: Option<String>,
    /// This session asked.
    pub(super) outgoing: bool,
    pub(super) state: VerificationState,
}

impl Verification {
    fn flow(&self) -> VerificationFlowId {
        VerificationFlowId {
            flow_id: self.flow_id.clone(),
            user_id: self.user_id.clone(),
        }
    }

    /// How it is named in a `/verify` answer: the user, and the session
    /// once known.
    pub(super) fn name(&self) -> String {
        match &self.device_id {
            Some(device_id) => format!("{} {device_id}", self.user_id),
            None => self.user_id.clone(),
        }
    }

    /// How Status lines and the status bar call the other side.
    pub(super) fn label(&self, own_user_id: Option<&str>) -> String {
        let own = own_user_id == Some(self.user_id.as_str());
        match (&self.device_id, own) {
            (Some(device_id), true) => format!("your session {device_id}"),
            (None, true) => "your other sessions".to_owned(),
            (Some(device_id), false) => format!("{} (session {device_id})", self.user_id),
            (None, false) => self.user_id.clone(),
        }
    }

    /// Its state and the other side, as the status bar says them:
    /// `requested by @bob:example (session B0B)`.
    pub(super) fn summary(&self, own_user_id: Option<&str>) -> String {
        let label = self.label(own_user_id);
        match &self.state {
            VerificationState::Requested => format!("requested by {label}"),
            VerificationState::Sent => format!("request sent to {label}"),
            state => format!("{} with {label}", verification_state_label(state)),
        }
    }

    /// `name (state)`, as the lists of flows under way say it.
    fn listed(&self) -> String {
        format!(
            "{} ({})",
            self.name(),
            verification_state_label(&self.state)
        )
    }
}

/// Which flow a `/verify` answer names: a user ID, and the session if
/// given.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct VerificationTarget<'t> {
    pub(super) user_id: &'t str,
    pub(super) device_id: Option<&'t str>,
}

/// The verifications under way, in the order they started. The latest
/// (the last started) is what the Tools items and a `/verify` answer
/// without a name act on: the most recently started, not the most
/// recently changed, so another flow's progress never moves the target
/// between reading the emoji and answering. A flow that starts later is
/// only requested or sent, which Confirm and Reject do not apply to. A
/// flow leaves when it is done or cancelled.
#[derive(Debug, Default)]
pub(super) struct Verifications(Vec<Verification>);

impl Verifications {
    /// A flow the core announced. It is new: the core announces each once.
    fn start(&mut self, request: VerificationRequest) -> &Verification {
        assert!(
            self.position(&request.user_id, &request.flow_id).is_none(),
            "the core announces a flow once: {request:?}"
        );
        let (device_id, outgoing, state) = match request.direction {
            VerificationDirection::Incoming { from_device } => {
                (Some(from_device), false, VerificationState::Requested)
            }
            VerificationDirection::Outgoing { to_device } => {
                (to_device, true, VerificationState::Sent)
            }
        };
        self.0.push(Verification {
            flow_id: request.flow_id,
            user_id: request.user_id,
            device_id,
            outgoing,
            state,
        });
        self.0.last().expect("just pushed")
    }

    /// A state of a flow the core announced; done or cancelled, the flow
    /// leaves. The flow as it is now.
    fn update(&mut self, event: VerificationStateEvent) -> Verification {
        let index = self
            .position(&event.user_id, &event.flow_id)
            .unwrap_or_else(|| panic!("the core reports only flows it announced: {event:?}"));
        let verification = &mut self.0[index];
        if event.device_id.is_some() {
            verification.device_id = event.device_id;
        }
        verification.state = event.state;
        match verification.state {
            VerificationState::Done | VerificationState::Cancelled { .. } => self.0.remove(index),
            _ => verification.clone(),
        }
    }

    fn position(&self, user_id: &str, flow_id: &str) -> Option<usize> {
        self.0.iter().position(|verification| {
            verification.user_id == user_id && verification.flow_id == flow_id
        })
    }

    pub(super) fn latest(&self) -> Option<&Verification> {
        self.0.last()
    }

    fn is_latest(&self, verification: &Verification) -> bool {
        self.latest().is_some_and(|latest| {
            latest.user_id == verification.user_id && latest.flow_id == verification.flow_id
        })
    }

    pub(super) fn iter(&self) -> impl Iterator<Item = &Verification> {
        self.0.iter()
    }

    pub(super) fn len(&self) -> usize {
        self.0.len()
    }

    pub(super) fn clear(&mut self) {
        self.0.clear();
    }

    /// The flow with `user_id` under way: the SDK holds at most one per
    /// user (a second request cancels both).
    fn with_user(&self, user_id: &str) -> Option<&Verification> {
        self.0
            .iter()
            .find(|verification| verification.user_id == user_id)
    }

    /// `name (state), ...` of every flow under way.
    fn listing(&self) -> String {
        self.0
            .iter()
            .map(Verification::listed)
            .collect::<Vec<_>>()
            .join(", ")
    }

    /// The flow `action` answers: the latest without a target. Refused,
    /// with the flows under way, when none matches, several do, or the
    /// action does not apply to the flow's state; never another flow.
    fn answered(
        &self,
        action: VerificationAction,
        target: Option<&VerificationTarget>,
    ) -> Result<&Verification, String> {
        let word = verification_action_label(action);
        let Some(target) = target else {
            let latest = self
                .latest()
                .ok_or_else(|| "no verification is under way".to_owned())?;
            if !verification_action_applies(&latest.state, action) {
                return Err(format!(
                    "{word} does not apply to the latest verification, {}; under way: {}",
                    latest.listed(),
                    self.listing()
                ));
            }
            return Ok(latest);
        };
        let matches = self
            .0
            .iter()
            .filter(|verification| {
                verification.user_id == target.user_id
                    && target.device_id.is_none_or(|device_id| {
                        verification.device_id.as_deref() == Some(device_id)
                    })
            })
            .collect::<Vec<_>>();
        let named = match target.device_id {
            Some(device_id) => format!("{} {device_id}", target.user_id),
            None => target.user_id.to_owned(),
        };
        match matches[..] {
            [] if self.0.is_empty() => Err("no verification is under way".to_owned()),
            [] => Err(format!(
                "no verification with {named} is under way; under way: {}",
                self.listing()
            )),
            [verification] if !verification_action_applies(&verification.state, action) => {
                Err(format!(
                    "{word} does not apply to the verification {}",
                    verification.listed()
                ))
            }
            [verification] => Ok(verification),
            _ => Err(format!(
                "{} verifications with {named} are under way; name the session: {}",
                matches.len(),
                matches
                    .iter()
                    .map(|verification| verification.listed())
                    .collect::<Vec<_>>()
                    .join(", ")
            )),
        }
    }
}

pub(super) fn apply_verification_event(state: &mut WindowState, event: VerificationStateEvent) {
    let verification = state.model.verifications.update(event);
    let own_user_id = state.model.own_user_id().map(str::to_owned);
    let lines = status_log::verification(
        &verification,
        own_user_id.as_deref(),
        state.model.verifications.is_latest(&verification),
    );
    for line in lines {
        log_status(state, StatusLineKind::Info, line);
    }
    state.model.status = verification_status_message(&verification, own_user_id.as_deref());
}

/// A new flow, in or out: it joins those under way, as the latest.
pub(super) fn apply_verification_request(state: &mut WindowState, request: VerificationRequest) {
    let own_user_id = state.model.own_user_id().map(str::to_owned);
    let verification = state.model.verifications.start(request).clone();
    let others = state
        .model
        .verifications
        .iter()
        .filter(|other| other.flow_id != verification.flow_id)
        .map(Verification::listed)
        .collect::<Vec<_>>();
    for line in status_log::verification_request(&verification, own_user_id.as_deref(), &others) {
        log_status(state, StatusLineKind::Info, line);
    }
    state.model.status = verification_status_message(&verification, own_user_id.as_deref());
}

/// Refuses to start a verification with a user who has one under way:
/// the SDK would cancel both.
pub(super) fn refuse_second_verification(model: &UiModel, user_id: &str) -> Result<(), String> {
    match model.verifications.with_user(user_id) {
        Some(verification) => Err(format!(
            "a verification with {} is under way; answer or cancel it first: /verify cancel {}",
            verification.label(model.own_user_id()),
            verification.name()
        )),
        None => Ok(()),
    }
}

pub(super) fn verification_state_label(state: &VerificationState) -> &'static str {
    match state {
        VerificationState::Requested => "requested",
        VerificationState::Sent => "sent",
        VerificationState::Ready => "ready",
        VerificationState::SasReady { .. } => "sas ready",
        VerificationState::Confirmed => "confirmed",
        VerificationState::Done => "done",
        VerificationState::Cancelled { .. } => "cancelled",
    }
}

/// Whether `action` answers a flow in `state`: the Tools items' and the
/// `/verify` answers' one rule.
pub(super) fn verification_action_applies(
    state: &VerificationState,
    action: VerificationAction,
) -> bool {
    matches!(
        (state, action),
        (
            VerificationState::Requested,
            VerificationAction::Accept | VerificationAction::Cancel
        ) | (
            VerificationState::Sent | VerificationState::Ready | VerificationState::Confirmed,
            VerificationAction::Cancel
        ) | (
            VerificationState::SasReady { .. },
            VerificationAction::Cancel | VerificationAction::Confirm | VerificationAction::Reject
        )
    )
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

fn verification_status_message(verification: &Verification, own_user_id: Option<&str>) -> String {
    format!("verification {}", verification.summary(own_user_id))
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

/// Sends `action` for the flow it answers (see `Verifications::answered`).
pub(super) fn submit_verification_answer(
    state: &mut WindowState,
    action: VerificationAction,
    target: Option<&VerificationTarget>,
) -> Result<(), String> {
    let flow = state.model.verifications.answered(action, target)?.flow();
    enqueue_command(state, verification_command(action, flow))
}

fn verification_command(action: VerificationAction, flow: VerificationFlowId) -> Command {
    match action {
        VerificationAction::Accept => Command::AcceptVerification(flow),
        VerificationAction::Cancel => Command::CancelVerification(flow),
        VerificationAction::Confirm => Command::ConfirmVerificationSas(flow),
        VerificationAction::Reject => Command::RejectVerificationSas(flow),
    }
}

/// A Tools item: the latest flow's answer.
pub(super) unsafe fn submit_menu_verification_action(
    hwnd: HWND,
    action: VerificationAction,
) -> Result<(), String> {
    submit_verification_answer(state_mut(hwnd), action, None)?;
    set_local_command_status(
        hwnd,
        format!(
            "verification {} submitted",
            verification_action_label(action)
        ),
    )
}

/// The automation harness's part of this module: what it reads and does
/// here for the protocol.
#[cfg(any(test, feature = "automation"))]
pub(super) mod automation {
    use super::*;

    /// The flow the args name, else the latest, whatever its state: the
    /// harness sends what it is told.
    fn verification_flow_id_from_action_args(
        model: &UiModel,
        args: &VerificationActionArgs,
    ) -> Result<VerificationFlowId, String> {
        match (&args.flow_id, &args.user_id) {
            (Some(flow_id), Some(user_id)) => Ok(VerificationFlowId {
                flow_id: flow_id.clone(),
                user_id: user_id.clone(),
            }),
            (None, None) => model
                .verifications
                .latest()
                .map(Verification::flow)
                .ok_or_else(|| "no verification is under way".to_owned()),
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
        enqueue_command(state, verification_command(action, flow))
    }
}

#[cfg(test)]
mod tests {
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

    fn request(
        flow_id: &str,
        user_id: &str,
        direction: VerificationDirection,
    ) -> VerificationRequest {
        VerificationRequest {
            flow_id: flow_id.to_owned(),
            user_id: user_id.to_owned(),
            direction,
        }
    }

    fn incoming(flow_id: &str, user_id: &str, device: &str) -> VerificationRequest {
        request(
            flow_id,
            user_id,
            VerificationDirection::Incoming {
                from_device: device.to_owned(),
            },
        )
    }

    fn event(flow_id: &str, user_id: &str, state: VerificationState) -> VerificationStateEvent {
        VerificationStateEvent {
            flow_id: flow_id.to_owned(),
            user_id: user_id.to_owned(),
            device_id: None,
            state,
        }
    }

    fn sas_ready() -> VerificationState {
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

    fn cancelled() -> VerificationState {
        VerificationState::Cancelled {
            reason: "timed out".to_owned(),
            code: "m.timeout".to_owned(),
            cancelled_by_us: false,
        }
    }

    fn states(verifications: &Verifications) -> Vec<(&str, &'static str)> {
        verifications
            .iter()
            .map(|verification| {
                (
                    verification.flow_id.as_str(),
                    verification_state_label(&verification.state),
                )
            })
            .collect()
    }

    fn target<'t>(user_id: &'t str, device_id: Option<&'t str>) -> VerificationTarget<'t> {
        VerificationTarget { user_id, device_id }
    }

    /// Bob's SAS, then Carol asks: both are under way, Carol's is the
    /// latest, and Bob's events still update Bob's.
    fn bob_at_sas_then_carol() -> Verifications {
        let mut verifications = Verifications::default();
        verifications.start(incoming("flow-bob", "@bob:example", "B0B"));
        verifications.update(event("flow-bob", "@bob:example", sas_ready()));
        verifications.start(incoming("flow-carol", "@carol:example", "C4R0L"));
        verifications
    }

    #[test]
    fn a_second_flow_joins_the_first_and_each_keeps_its_own_state() {
        let mut verifications = bob_at_sas_then_carol();
        assert_eq!(
            states(&verifications),
            [("flow-bob", "sas ready"), ("flow-carol", "requested")]
        );
        let bob = verifications.update(event(
            "flow-bob",
            "@bob:example",
            VerificationState::Confirmed,
        ));
        assert_eq!(bob.device_id.as_deref(), Some("B0B"));
        assert_eq!(
            states(&verifications),
            [("flow-bob", "confirmed"), ("flow-carol", "requested")]
        );
        assert_eq!(
            verifications.latest().map(|latest| latest.flow_id.as_str()),
            Some("flow-carol"),
            "the latest started, not the latest changed"
        );
    }

    #[test]
    fn done_and_cancelled_flows_leave() {
        let mut verifications = bob_at_sas_then_carol();
        let done = verifications.update(event("flow-bob", "@bob:example", VerificationState::Done));
        assert_eq!(done.state, VerificationState::Done);
        assert_eq!(states(&verifications), [("flow-carol", "requested")]);
        verifications.update(event("flow-carol", "@carol:example", cancelled()));
        assert_eq!(verifications.len(), 0);
    }

    #[test]
    #[should_panic(expected = "the core reports only flows it announced")]
    fn a_state_of_an_unknown_flow_panics() {
        Verifications::default().update(event("flow-x", "@x:example", VerificationState::Ready));
    }

    #[test]
    fn the_session_that_answers_is_kept_once_named() {
        let mut verifications = Verifications::default();
        let sent = verifications
            .start(request(
                "flow-out",
                "@bob:example",
                VerificationDirection::Outgoing { to_device: None },
            ))
            .clone();
        assert_eq!((sent.device_id, sent.outgoing), (None, true));
        let mut ready = event("flow-out", "@bob:example", VerificationState::Ready);
        ready.device_id = Some("B0B".to_owned());
        assert_eq!(verifications.update(ready).name(), "@bob:example B0B");
        // A later event that does not name it keeps it.
        let sas = verifications.update(event("flow-out", "@bob:example", sas_ready()));
        assert_eq!(sas.device_id.as_deref(), Some("B0B"));
    }

    #[test]
    fn labels_and_names_say_user_and_session() {
        let mut verifications = Verifications::default();
        let own = Some("@me:example");
        let label = |verification: &Verification| verification.label(own);
        assert_eq!(
            label(verifications.start(incoming("f1", "@me:example", "PH0NE"))),
            "your session PH0NE"
        );
        assert_eq!(
            label(verifications.start(request(
                "f2",
                "@me2:example",
                VerificationDirection::Outgoing { to_device: None },
            ))),
            "@me2:example"
        );
        let other = verifications.start(incoming("f3", "@bob:example", "B0B"));
        assert_eq!(label(other), "@bob:example (session B0B)");
        assert_eq!(other.name(), "@bob:example B0B");
        let mut to_own = Verifications::default();
        let all_own = to_own.start(request(
            "f4",
            "@me:example",
            VerificationDirection::Outgoing { to_device: None },
        ));
        assert_eq!(label(all_own), "your other sessions");
        assert_eq!(all_own.name(), "@me:example");
    }

    #[test]
    fn an_answer_without_a_name_is_the_latest_started_or_refused() {
        let verifications = bob_at_sas_then_carol();
        let answered = |action| {
            verifications
                .answered(action, None)
                .map(|verification| verification.flow_id.as_str())
        };
        assert_eq!(answered(VerificationAction::Accept), Ok("flow-carol"));
        assert_eq!(answered(VerificationAction::Cancel), Ok("flow-carol"));
        // Bob's emoji are up, but the latest is Carol's request: never
        // redirected to Bob's.
        assert_eq!(
            answered(VerificationAction::Confirm),
            Err("confirm does not apply to the latest verification, @carol:example C4R0L (requested); under way: @bob:example B0B (sas ready), @carol:example C4R0L (requested)".to_owned())
        );
        assert_eq!(
            Verifications::default()
                .answered(VerificationAction::Cancel, None)
                .map(|_| ()),
            Err("no verification is under way".to_owned())
        );
    }

    #[test]
    fn an_answer_names_its_flow_by_user_and_session() {
        let mut verifications = bob_at_sas_then_carol();
        let answered = |verifications: &Verifications, action, user, device| {
            verifications
                .answered(action, Some(&target(user, device)))
                .map(|verification| verification.flow_id.clone())
        };
        let confirm = VerificationAction::Confirm;
        assert_eq!(
            answered(&verifications, confirm, "@bob:example", None),
            Ok("flow-bob".to_owned())
        );
        assert_eq!(
            answered(&verifications, confirm, "@bob:example", Some("B0B")),
            Ok("flow-bob".to_owned())
        );
        assert_eq!(
            answered(&verifications, confirm, "@bob:example", Some("PH0NE")),
            Err("no verification with @bob:example PH0NE is under way; under way: @bob:example B0B (sas ready), @carol:example C4R0L (requested)".to_owned())
        );
        assert_eq!(
            answered(&verifications, confirm, "@carol:example", None),
            Err(
                "confirm does not apply to the verification @carol:example C4R0L (requested)"
                    .to_owned()
            )
        );
        // Two of your own sessions for a moment (the SDK cancels both).
        verifications.start(incoming("flow-phone", "@me:example", "PH0NE"));
        verifications.start(incoming("flow-laptop", "@me:example", "L4PT0P"));
        assert_eq!(
            answered(&verifications, VerificationAction::Cancel, "@me:example", None),
            Err("2 verifications with @me:example are under way; name the session: @me:example PH0NE (requested), @me:example L4PT0P (requested)".to_owned())
        );
        assert_eq!(
            answered(
                &verifications,
                VerificationAction::Cancel,
                "@me:example",
                Some("L4PT0P")
            ),
            Ok("flow-laptop".to_owned())
        );
    }

    #[test]
    fn a_second_verification_with_a_user_is_refused() {
        let mut state = test_state();
        let model = &mut state.model;
        model
            .verifications
            .start(incoming("flow-bob", "@bob:example", "B0B"));
        assert_eq!(
            refuse_second_verification(model, "@bob:example"),
            Err("a verification with @bob:example (session B0B) is under way; answer or cancel it first: /verify cancel @bob:example B0B".to_owned())
        );
        assert_eq!(refuse_second_verification(model, "@carol:example"), Ok(()));
    }

    #[test]
    fn the_action_table_matches_the_states() {
        let applies = |state: &VerificationState| {
            [
                VerificationAction::Accept,
                VerificationAction::Cancel,
                VerificationAction::Confirm,
                VerificationAction::Reject,
            ]
            .map(|action| verification_action_applies(state, action))
        };
        assert_eq!(
            applies(&VerificationState::Requested),
            [true, true, false, false]
        );
        assert_eq!(
            applies(&VerificationState::Sent),
            [false, true, false, false]
        );
        assert_eq!(
            applies(&VerificationState::Ready),
            [false, true, false, false]
        );
        assert_eq!(applies(&sas_ready()), [false, true, true, true]);
        assert_eq!(
            applies(&VerificationState::Confirmed),
            [false, true, false, false]
        );
        assert_eq!(applies(&VerificationState::Done), [false; 4]);
        assert_eq!(applies(&cancelled()), [false; 4]);
    }

    fn with_test_core(test: impl FnOnce(&mut WindowState)) {
        let mut state = test_state();
        let handle = start_test_core();
        state.command_sender = Some(handle.command_sender().clone());
        test(&mut state);
        handle
            .command_sender()
            .send(Command::Shutdown)
            .expect("shutdown command must enqueue");
        handle.join().expect("test core must stop cleanly");
    }

    fn flow(flow_id: &str, user_id: &str) -> VerificationFlowId {
        VerificationFlowId {
            flow_id: flow_id.to_owned(),
            user_id: user_id.to_owned(),
        }
    }

    #[test]
    fn answers_enqueue_the_command_for_their_flow_and_refusals_nothing() {
        with_test_core(|state| {
            state.model.verifications = bob_at_sas_then_carol();
            submit_verification_answer(state, VerificationAction::Accept, None).unwrap();
            submit_verification_answer(
                state,
                VerificationAction::Confirm,
                Some(&target("@bob:example", Some("B0B"))),
            )
            .unwrap();
            submit_verification_answer(
                state,
                VerificationAction::Reject,
                Some(&target("@bob:example", None)),
            )
            .unwrap();
            assert!(submit_verification_answer(state, VerificationAction::Confirm, None).is_err());

            assert_eq!(
                state.harness.commands,
                vec![
                    Command::AcceptVerification(flow("flow-carol", "@carol:example")),
                    Command::ConfirmVerificationSas(flow("flow-bob", "@bob:example")),
                    Command::RejectVerificationSas(flow("flow-bob", "@bob:example")),
                ]
            );
            assert_eq!(
                states(&state.model.verifications).len(),
                2,
                "answers change nothing here"
            );
        });
    }

    #[test]
    fn automation_verification_actions_use_the_latest_flow_when_args_are_omitted() {
        with_test_core(|state| {
            state.model.verifications = bob_at_sas_then_carol();
            for action in [VerificationAction::Accept, VerificationAction::Confirm] {
                submit_verification_automation_action_to_state(
                    state,
                    action,
                    VerificationActionArgs::default(),
                )
                .unwrap();
            }
            assert_eq!(
                state.harness.commands,
                vec![
                    Command::AcceptVerification(flow("flow-carol", "@carol:example")),
                    Command::ConfirmVerificationSas(flow("flow-carol", "@carol:example")),
                ]
            );
        });
    }

    #[test]
    fn automation_verification_actions_use_explicit_flow_args() {
        with_test_core(|state| {
            state.model.verifications = bob_at_sas_then_carol();
            submit_verification_automation_action_to_state(
                state,
                VerificationAction::Accept,
                VerificationActionArgs {
                    flow_id: Some("flow-explicit".to_owned()),
                    user_id: Some("@dave:example".to_owned()),
                },
            )
            .unwrap();
            assert_eq!(
                state.harness.commands,
                vec![Command::AcceptVerification(flow(
                    "flow-explicit",
                    "@dave:example"
                ))]
            );
        });
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
}
