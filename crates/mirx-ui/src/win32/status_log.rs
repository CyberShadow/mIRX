//! The Status window's log: mIRC's Status window is a scrolling, timestamped
//! log of connection, notice and error lines, kept across disconnects until
//! the user clears it.
//!
//! Builders return line bodies, which `log_status` stores as `* body`. A
//! line holds the text after its `[hh:mm]` timestamp, so a command's echo
//! may print other lines too, such as `-> *Alice* hi`.

use mirx_core::{E2eeStatus, SyncStopReason, VerificationState, VerificationStateEvent};

use super::{
    e2ee_cross_signing_label, e2ee_local_backup_label, e2ee_recovery_state_label,
    e2ee_server_backup_label, format_verification_emojis, verification_actions_text,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum StatusLineKind {
    Info,
    Error,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct StatusLine {
    /// Unique for the log's lifetime, so identical lines stay distinct.
    pub(super) sequence: u64,
    pub(super) timestamp_ms: u64,
    pub(super) kind: StatusLineKind,
    pub(super) text: String,
}

#[derive(Debug, Default)]
pub(super) struct StatusLog {
    lines: Vec<StatusLine>,
    next_sequence: u64,
}

impl StatusLog {
    pub(super) fn push(&mut self, timestamp_ms: u64, kind: StatusLineKind, text: String) {
        self.lines.push(StatusLine {
            sequence: self.next_sequence,
            timestamp_ms,
            kind,
            text,
        });
        self.next_sequence += 1;
    }

    pub(super) fn lines(&self) -> &[StatusLine] {
        &self.lines
    }

    /// `/clear`: the sequence keeps counting, so no later line reuses an
    /// identity.
    pub(super) fn clear(&mut self) {
        self.lines.clear();
    }
}

pub(super) fn connecting(homeserver_url: &str, username: &str) -> String {
    format!("Connecting to {homeserver_url} as {username}")
}

pub(super) fn restoring(homeserver_url: &str, user_id: &str) -> String {
    format!("Restoring session for {user_id} on {homeserver_url}")
}

pub(super) fn logged_in(user_id: &str, device_id: &str) -> String {
    format!("Logged in as {user_id} (device {device_id})")
}

/// Logged at startup without a saved session.
pub(super) const CONNECT_HINT: &str = "Not connected. Use File > Select Server... (Alt+E) or /server to connect to a Matrix homeserver";
pub(super) const SESSION_RESTORED: &str = "Session restored";
pub(super) const SYNC_STARTED: &str = "Syncing with the homeserver";
pub(super) const LOGGED_OUT: &str = "Logged out";
pub(super) const STOPPED: &str = "Disconnected";
pub(super) const E2EE_RECOVERY_SET_UP: &str =
    "E2EE recovery set up; the recovery key was shown in a dialog";
pub(super) const E2EE_RECOVERY_COMPLETE: &str = "E2EE recovery complete";
pub(super) const E2EE_STATUS_UNKNOWN: &str = "E2EE status not yet known";

/// mIRC's `* Disconnected`, with the reason when sync ended by itself.
pub(super) fn disconnected(reason: &SyncStopReason) -> (StatusLineKind, String) {
    match reason {
        SyncStopReason::Requested => (StatusLineKind::Info, "Disconnected".to_owned()),
        SyncStopReason::Failed(message) => {
            (StatusLineKind::Error, format!("Disconnected: {message}"))
        }
    }
}

pub(super) fn synced(joined: usize, invited: usize) -> String {
    format!("Synced rooms: {joined} joined, {invited} invited")
}

pub(super) fn e2ee_status(status: &E2eeStatus) -> Vec<String> {
    vec![
        format!(
            "E2EE: recovery {}, local backup {}, server backup {}, cross-signing {}",
            e2ee_recovery_state_label(status.recovery),
            e2ee_local_backup_label(&status.backup),
            e2ee_server_backup_label(status.backup.exists_on_server),
            e2ee_cross_signing_label(status.cross_signing.as_ref()),
        ),
        "Use /e2ee refresh to check server backup, /e2ee setup to enable recovery, /e2ee recover to restore keys"
            .to_owned(),
    ]
}

pub(super) fn verification(event: &VerificationStateEvent) -> Vec<String> {
    let user = &event.user_id;
    let mut lines = match &event.state {
        VerificationState::Requested => vec![format!(
            "Verification requested by {user} (flow {})",
            event.flow_id
        )],
        VerificationState::Accepted => vec![format!("Verification accepted with {user}")],
        VerificationState::Ready => vec![format!("Verification ready with {user}")],
        VerificationState::SasReady { emojis, decimals } => vec![
            format!("Verification with {user}: compare the emoji or the numbers"),
            format!("Emoji: {}", format_verification_emojis(emojis)),
            format!(
                "Decimals: {}-{}-{}",
                decimals.first, decimals.second, decimals.third
            ),
        ],
        VerificationState::Confirmed => vec![format!("Verification confirmed with {user}")],
        VerificationState::Done => vec![format!("Verification done with {user}: verified")],
        VerificationState::Cancelled { reason, code, .. } => {
            vec![format!(
                "Verification cancelled with {user}: {reason} ({code})"
            )]
        }
    };
    lines.extend(
        verification_actions_text(Some(event)).map(|actions| format!("Actions: {actions}")),
    );
    lines
}

pub(super) fn error(operation_name: &str, message: &str) -> String {
    format!("Error during {operation_name}: {message}")
}

#[cfg(test)]
mod tests {
    use mirx_core::{
        E2eeBackupState, E2eeBackupStatus, E2eeCrossSigningStatus, E2eeRecoveryState,
        VerificationDecimals, VerificationEmoji,
    };

    use super::*;

    #[test]
    fn sequence_keeps_counting_across_clear() {
        let mut log = StatusLog::default();
        log.push(1, StatusLineKind::Info, "one".to_owned());
        log.push(2, StatusLineKind::Error, "two".to_owned());
        assert_eq!(
            log.lines()
                .iter()
                .map(|line| (line.sequence, line.timestamp_ms, line.kind))
                .collect::<Vec<_>>(),
            vec![(0, 1, StatusLineKind::Info), (1, 2, StatusLineKind::Error)]
        );

        log.clear();
        assert!(log.lines().is_empty());
        log.push(3, StatusLineKind::Info, "three".to_owned());
        assert_eq!(log.lines()[0].sequence, 2);
        assert_eq!(log.lines()[0].text, "three");
    }

    #[test]
    fn connection_lines() {
        assert_eq!(
            connecting("https://matrix.example", "alice"),
            "Connecting to https://matrix.example as alice"
        );
        assert_eq!(
            restoring("https://matrix.example", "@alice:example"),
            "Restoring session for @alice:example on https://matrix.example"
        );
        assert_eq!(
            logged_in("@alice:example", "DEVICE"),
            "Logged in as @alice:example (device DEVICE)"
        );
        assert_eq!(synced(3, 1), "Synced rooms: 3 joined, 1 invited");
        assert_eq!(
            disconnected(&SyncStopReason::Requested),
            (StatusLineKind::Info, "Disconnected".to_owned())
        );
        assert_eq!(
            disconnected(&SyncStopReason::Failed(
                "continuous Matrix sync failed: timeout".to_owned()
            )),
            (
                StatusLineKind::Error,
                "Disconnected: continuous Matrix sync failed: timeout".to_owned()
            )
        );
        assert_eq!(
            error("send message", "message send failed"),
            "Error during send message: message send failed"
        );
    }

    #[test]
    fn e2ee_status_lines_state_every_part_and_the_commands() {
        let status = E2eeStatus {
            recovery: E2eeRecoveryState::Disabled,
            backup: E2eeBackupStatus {
                state: E2eeBackupState::Enabled,
                enabled_locally: true,
                exists_on_server: None,
            },
            cross_signing: Some(E2eeCrossSigningStatus {
                has_master: true,
                has_self_signing: true,
                has_user_signing: true,
                complete: true,
            }),
        };
        assert_eq!(
            e2ee_status(&status),
            vec![
                "E2EE: recovery disabled, local backup enabled, server backup not checked, cross-signing complete"
                    .to_owned(),
                "Use /e2ee refresh to check server backup, /e2ee setup to enable recovery, /e2ee recover to restore keys"
                    .to_owned(),
            ]
        );
    }

    fn verification_event(state: VerificationState) -> VerificationStateEvent {
        VerificationStateEvent {
            flow_id: "flow-1".to_owned(),
            user_id: "@alice:example".to_owned(),
            state,
        }
    }

    #[test]
    fn verification_lines_follow_the_flow() {
        assert_eq!(
            verification(&verification_event(VerificationState::Requested)),
            vec![
                "Verification requested by @alice:example (flow flow-1)".to_owned(),
                "Actions: Account > Accept Verification, Account > Cancel Verification, /verify accept, /verify cancel"
                    .to_owned(),
            ]
        );
        assert_eq!(
            verification(&verification_event(VerificationState::SasReady {
                emojis: vec![
                    VerificationEmoji {
                        symbol: "🐶".to_owned(),
                        description: "Dog".to_owned(),
                    },
                    VerificationEmoji {
                        symbol: "🔑".to_owned(),
                        description: "Key".to_owned(),
                    },
                ],
                decimals: VerificationDecimals {
                    first: 1234,
                    second: 5678,
                    third: 9012,
                },
            }))[..3],
            [
                "Verification with @alice:example: compare the emoji or the numbers".to_owned(),
                "Emoji: 🐶 Dog | 🔑 Key".to_owned(),
                "Decimals: 1234-5678-9012".to_owned(),
            ]
        );
        assert_eq!(
            verification(&verification_event(VerificationState::Done)),
            vec!["Verification done with @alice:example: verified".to_owned()]
        );
        assert_eq!(
            verification(&verification_event(VerificationState::Cancelled {
                reason: "user cancelled".to_owned(),
                code: "m.user".to_owned(),
                cancelled_by_us: false,
            })),
            vec!["Verification cancelled with @alice:example: user cancelled (m.user)".to_owned()]
        );
    }
}
