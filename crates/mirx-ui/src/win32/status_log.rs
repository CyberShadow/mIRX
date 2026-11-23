//! The Status window's log: mIRC's Status window is a scrolling, timestamped
//! log of connection, notice and error lines, kept across disconnects until
//! the user clears it.
//!
//! Builders return line bodies, which `log_status` stores as `* body`. A
//! line holds the text after its `[hh:mm]` timestamp, so a command's echo
//! may print other lines too, such as `-> *Alice* hi`.

use mirx_core::{
    E2eeIdentityResetOutcome, E2eeRecoveryState, E2eeSessionState, E2eeStatus,
    VerificationDirection, VerificationRequest, VerificationState, VerificationStateEvent,
};

use super::colors::PaletteIndex;
use super::menus::verification_actions_text;
use super::{
    e2ee_cross_signing_label, e2ee_local_backup_label, e2ee_recovery_state_label,
    e2ee_server_backup_label, e2ee_session_label, format_verification_emojis,
    verification_state_event_from_request,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum StatusLineKind {
    Info,
    Error,
    /// A notify list user came or went: in the Notify colour.
    Notify,
    /// An invite arrived: in the Invite colour.
    Invite,
    /// `/echo`'s line: its text holds mIRC codes, it is in the Normal
    /// colour or `color`, and has a timestamp only with `-t`.
    Echo {
        color: Option<PaletteIndex>,
        timestamp: bool,
    },
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
    /// Moves on every change: the Status window rebuilds its rows when it
    /// differs from the one they were built from.
    revision: u64,
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
        self.revision += 1;
    }

    pub(super) fn revision(&self) -> u64 {
        self.revision
    }

    pub(super) fn lines(&self) -> &[StatusLine] {
        &self.lines
    }

    /// The sequence the next line will have.
    pub(super) fn next_sequence(&self) -> u64 {
        self.next_sequence
    }

    /// `/clear`: the sequence keeps counting, so no later line reuses an
    /// identity.
    pub(super) fn clear(&mut self) {
        self.lines.clear();
        self.revision += 1;
    }

    /// mIRC's window buffer: drops the oldest lines until at most `cap`
    /// remain. Returns whether it dropped any; only then does the revision
    /// move.
    pub(super) fn trim_to(&mut self, cap: usize) -> bool {
        let excess = self.lines.len().saturating_sub(cap);
        if excess == 0 {
            return false;
        }
        self.lines.drain(..excess);
        self.revision += 1;
        true
    }
}

/// The shell refused the tray icon: no tray, so no hiding in it.
pub(super) fn tray_icon_unavailable(error: &str) -> String {
    format!("Tray icon unavailable: {error}")
}

pub(super) fn connecting(homeserver_url: &str, username: &str) -> String {
    format!("Connecting to {homeserver_url} as {username}")
}

pub(super) fn connecting_with_browser(homeserver_url: &str) -> String {
    format!("Connecting to {homeserver_url} to sign in with your browser")
}

pub(super) fn restoring(homeserver_url: &str, user_id: &str) -> String {
    format!("Restoring session for {user_id} on {homeserver_url}")
}

pub(super) const SIGN_IN_WITH_BROWSER: &str =
    "Sign in on the page your browser opened; File > Disconnect cancels";

/// The page the browser was asked to open (a sign-in, an identity reset's
/// approval) as a hotlink, for when the shell opened no browser.
pub(super) fn sign_in_address(url: &str) -> String {
    format!("If no browser opened, go to {url}")
}

pub(super) const SIGN_IN_CANCELLED: &str = "Sign-in cancelled";

pub(super) fn logged_in(user_id: &str, device_id: &str) -> String {
    format!("Logged in as {user_id} (device {device_id})")
}

/// Logged at startup without a saved session.
pub(super) const CONNECT_HINT: &str = "Not connected. Use File > Select Server... (Alt+E) or /server to connect to a Matrix homeserver";
/// Logged at startup with a saved session when Connect on startup is off.
pub(super) const STARTUP_CONNECT_OFF: &str =
    "Connect on startup is off; File > Connect or /server connects";
pub(super) const SESSION_RESTORED: &str = "Session restored";
pub(super) const SYNC_STARTED: &str = "Syncing with the homeserver";
/// The core waits for the key backup and the server.
pub(super) const LOGGING_OUT: &str = "Logging out; File > Disconnect stops it";
pub(super) const LOG_OUT_CANCELLED: &str = "Log Out cancelled; still signed in";
pub(super) const LOGGED_OUT: &str = "Logged out";
pub(super) const STOPPED: &str = "Disconnected";
pub(super) const E2EE_RECOVERY_SET_UP: &str =
    "E2EE recovery set up; the recovery key was shown in a dialog";
pub(super) const E2EE_RECOVERY_COMPLETE: &str = "E2EE recovery complete";
pub(super) const E2EE_STATUS_UNKNOWN: &str = "E2EE status not yet known";
pub(super) const E2EE_RESET_APPROVAL: &str = "To reset your E2EE identity, approve it on the page your browser opened; mirx waits 2 minutes, /e2ee reset cancel stops it";
/// After a reset, or a cancelled one: the reset turned recovery off first.
pub(super) const E2EE_RECOVERY_OFF_AFTER_RESET: &str =
    "Recovery is off: /e2ee setup to set it up again";

/// The answer to `/e2ee reset`. The E2EE status follows, with the state of
/// the key backup.
pub(super) fn e2ee_identity_reset(
    outcome: E2eeIdentityResetOutcome,
) -> [(StatusLineKind, String); 2] {
    let line = match outcome {
        E2eeIdentityResetOutcome::Done => {
            "Your E2EE identity was reset; this session is verified with the new one"
        }
        E2eeIdentityResetOutcome::Cancelled => {
            "E2EE identity reset cancelled: your identity is unchanged, but your key backup was deleted"
        }
    };
    [
        (StatusLineKind::Info, line.to_owned()),
        (
            StatusLineKind::Error,
            E2EE_RECOVERY_OFF_AFTER_RESET.to_owned(),
        ),
    ]
}

/// mIRC's `* Disconnected` when the connection ended by itself; the user's
/// own Disconnect logs `STOPPED`.
pub(super) fn connection_lost(message: &str) -> String {
    format!("Disconnected: {message}")
}

/// mIRC's `* Connect retry #2 server`. `server` is the session's
/// (`own_server`), here and in the other connection lines.
pub(super) fn connect_retry(attempt: u32, server: &str) -> String {
    format!("Connect retry #{attempt} {server}")
}

/// Reconnect on disconnection, at once.
pub(super) fn reconnecting(server: &str) -> String {
    format!("Reconnecting to {server}")
}

pub(super) fn reconnected(server: &str) -> String {
    format!("Reconnected to {server}")
}

pub(super) fn not_retrying(attempts: u32) -> String {
    let retries = if attempts == 1 { "retry" } else { "retries" };
    format!("Not retrying: {attempts} {retries} failed. File > Connect or /server tries again.")
}

/// The status while a retry is due.
pub(super) fn retrying_in(delay: std::time::Duration) -> String {
    format!("disconnected; retrying in {} seconds", delay.as_secs())
}

/// The server no longer accepts the session: no retry can help. Its room
/// keys are still on this computer, and exporting them needs no server.
pub(super) fn session_ended(server: &str, message: &str) -> String {
    format!(
        "Disconnected: {server} signed this session out ({message}). /e2ee export saves its room keys; File > Log Out ends it, then sign in again."
    )
}

/// A space has no window to open on joining; it shows as a treebar folder.
pub(super) fn joined_space(name: &str) -> String {
    format!("Joined space {name}")
}

pub(super) fn synced(joined: usize, invited: usize) -> String {
    format!("Synced rooms: {joined} joined, {invited} invited")
}

/// The E2EE summary, the commands that apply to it, then, when this
/// session needs verifying, a warning saying how.
pub(super) fn e2ee_status(status: &E2eeStatus) -> Vec<(StatusLineKind, String)> {
    let mut lines = vec![(
        StatusLineKind::Info,
        format!(
            "E2EE: recovery {}, local backup {}, server backup {}, cross-signing {}, this session {}",
            e2ee_recovery_state_label(status.recovery),
            e2ee_local_backup_label(&status.backup),
            e2ee_server_backup_label(status.backup.exists_on_server),
            e2ee_cross_signing_label(status.cross_signing.as_ref()),
            e2ee_session_label(status.session),
        ),
    )];
    let commands = [
        (
            status.backup.exists_on_server.is_none(),
            "/e2ee refresh to check server backup",
        ),
        (
            status.recovery == E2eeRecoveryState::Disabled,
            "/e2ee setup to enable recovery",
        ),
        (
            status.recovery == E2eeRecoveryState::Incomplete,
            "/e2ee recover to restore keys",
        ),
    ]
    .into_iter()
    .filter_map(|(applies, command)| applies.then_some(command))
    .collect::<Vec<_>>();
    if !commands.is_empty() {
        lines.push((StatusLineKind::Info, format!("Use {}", commands.join(", "))));
    }
    if session_needs_verifying(status) {
        lines.push((StatusLineKind::Error, SESSION_UNVERIFIED_WARNING.to_owned()));
    }
    lines
}

pub(super) const SESSION_UNVERIFIED_WARNING: &str = "This session is not verified: /verify to verify it from another of your sessions, /e2ee recover with your recovery key, or /e2ee reset if you have neither";

/// Whether to warn that this session is not verified: your identity does
/// not sign it, and this session lacks the self-signing key to sign
/// itself. A session holding that key signs itself (setting up or
/// recovering cross-signing), and is unverified only until the SDK's
/// next query of your keys sees the signature.
fn session_needs_verifying(status: &E2eeStatus) -> bool {
    status.session == E2eeSessionState::Unverified
        && status
            .cross_signing
            .as_ref()
            .is_some_and(|keys| !keys.has_self_signing)
}

/// A new flow: who asked whom, and the actions that answer it. A flow
/// with the own user is between your own sessions.
pub(super) fn verification_request(
    request: &VerificationRequest,
    own_user_id: Option<&str>,
) -> Vec<String> {
    let user = &request.user_id;
    let own = own_user_id == Some(user.as_str());
    let flow = &request.flow_id;
    let line = match (&request.direction, own) {
        (VerificationDirection::Incoming { from_device }, true) => {
            format!("Verification requested by your other session {from_device} (flow {flow})")
        }
        (VerificationDirection::Incoming { .. }, false) => {
            format!("Verification requested by {user} (flow {flow})")
        }
        (VerificationDirection::Outgoing { to_device: None }, true) => {
            "Verification request sent to your other sessions; accept it on one of them".to_owned()
        }
        (VerificationDirection::Outgoing { to_device: None }, false) => {
            format!("Verification request sent to {user}; waiting for them to accept")
        }
        (
            VerificationDirection::Outgoing {
                to_device: Some(device),
            },
            true,
        ) => {
            format!("Verification request sent to your session {device}; accept it there")
        }
        (
            VerificationDirection::Outgoing {
                to_device: Some(device),
            },
            false,
        ) => {
            format!(
                "Verification request sent to {user} device {device}; waiting for them to accept"
            )
        }
    };
    let event = verification_state_event_from_request(request.clone());
    let mut lines = vec![line];
    lines.extend(
        verification_actions_text(Some(&event)).map(|actions| format!("Actions: {actions}")),
    );
    lines
}

pub(super) fn verification(event: &VerificationStateEvent) -> Vec<String> {
    let user = &event.user_id;
    let mut lines = match &event.state {
        VerificationState::Requested => vec![format!(
            "Verification requested by {user} (flow {})",
            event.flow_id
        )],
        VerificationState::Sent => vec![format!(
            "Verification request sent to {user}; waiting for them to accept"
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

/// A room key file is being written: the key derivation takes a moment.
pub(super) fn exporting_room_keys(path: &str) -> String {
    format!("Exporting room keys to {path}...")
}

pub(super) fn importing_room_keys(path: &str) -> String {
    format!("Importing room keys from {path}...")
}

pub(super) fn room_keys_exported(path: &str) -> String {
    format!("Room keys exported to {path}. Keep the file and its passphrase safe.")
}

pub(super) fn room_keys_imported(path: &str, imported: u64, total: u64) -> String {
    let known = total
        .checked_sub(imported)
        .expect("an import cannot add more keys than its file holds");
    match known {
        0 => format!("Imported {imported} of {total} room keys from {path}"),
        known => {
            format!("Imported {imported} of {total} room keys from {path} ({known} already known)")
        }
    }
}

pub(super) fn error(operation_name: &str, message: &str) -> String {
    format!("Error during {operation_name}: {message}")
}

#[cfg(test)]
mod tests {
    use mirx_core::{
        E2eeBackupState, E2eeBackupStatus, E2eeCrossSigningStatus, E2eeSessionState,
        VerificationDecimals, VerificationEmoji,
    };

    use super::*;

    #[test]
    fn status_log_revision_moves_on_push_and_clear() {
        let mut log = StatusLog::default();
        let start = log.revision();
        log.push(1, StatusLineKind::Info, "* one".to_owned());
        let pushed = log.revision();
        assert_ne!(pushed, start);
        assert_eq!(log.revision(), pushed);
        log.clear();
        assert_ne!(log.revision(), pushed);
    }

    #[test]
    fn trimming_drops_the_oldest_lines_and_moves_the_revision_only_then() {
        let mut log = StatusLog::default();
        for (timestamp, text) in [(1, "one"), (2, "two"), (3, "three")] {
            log.push(timestamp, StatusLineKind::Info, text.to_owned());
        }
        let revision = log.revision();
        assert!(!log.trim_to(3));
        assert_eq!(log.revision(), revision);

        assert!(log.trim_to(2));
        assert_ne!(log.revision(), revision);
        assert_eq!(
            log.lines()
                .iter()
                .map(|line| (line.sequence, line.text.as_str()))
                .collect::<Vec<_>>(),
            vec![(1, "two"), (2, "three")]
        );
        log.push(4, StatusLineKind::Info, "four".to_owned());
        assert_eq!(log.lines()[2].sequence, 3);
    }

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
            connect_retry(2, "example.org"),
            "Connect retry #2 example.org"
        );
        assert_eq!(reconnecting("example.org"), "Reconnecting to example.org");
        assert_eq!(reconnected("example.org"), "Reconnected to example.org");
        assert_eq!(
            not_retrying(1),
            "Not retrying: 1 retry failed. File > Connect or /server tries again."
        );
        assert_eq!(
            not_retrying(99),
            "Not retrying: 99 retries failed. File > Connect or /server tries again."
        );
        assert_eq!(
            retrying_in(std::time::Duration::from_secs(10)),
            "disconnected; retrying in 10 seconds"
        );
        assert_eq!(
            connection_lost("connection refused"),
            "Disconnected: connection refused"
        );
        assert_eq!(
            session_ended("example.org", "[401 / M_UNKNOWN_TOKEN] Invalid token"),
            "Disconnected: example.org signed this session out ([401 / M_UNKNOWN_TOKEN] Invalid token). /e2ee export saves its room keys; File > Log Out ends it, then sign in again."
        );
        assert_eq!(
            error("send message", "message send failed"),
            "Error during send message: message send failed"
        );
    }

    #[test]
    fn e2ee_status_lines_state_every_part_and_the_commands() {
        let keys = |has_self_signing| E2eeCrossSigningStatus {
            has_master: has_self_signing,
            has_self_signing,
            has_user_signing: has_self_signing,
            complete: has_self_signing,
        };
        let status = |session, cross_signing| E2eeStatus {
            recovery: E2eeRecoveryState::Disabled,
            backup: E2eeBackupStatus {
                state: E2eeBackupState::Enabled,
                enabled_locally: true,
                exists_on_server: None,
            },
            cross_signing,
            session,
        };
        let info = |line: &str| (StatusLineKind::Info, line.to_owned());
        let commands =
            info("Use /e2ee refresh to check server backup, /e2ee setup to enable recovery");
        assert_eq!(
            e2ee_status(&status(E2eeSessionState::Verified, Some(keys(true)))),
            vec![
                info(
                    "E2EE: recovery disabled, local backup enabled, server backup not checked, cross-signing complete, this session verified"
                ),
                commands.clone(),
            ]
        );

        // Only a session that cannot sign itself, of an account with an
        // identity, needs verifying.
        assert_eq!(
            e2ee_status(&status(E2eeSessionState::Unverified, Some(keys(false)))),
            vec![
                info(
                    "E2EE: recovery disabled, local backup enabled, server backup not checked, cross-signing incomplete, this session not verified"
                ),
                commands.clone(),
                (StatusLineKind::Error, SESSION_UNVERIFIED_WARNING.to_owned()),
            ]
        );
        for (session, cross_signing, label) in [
            // It holds the key: it signs itself, until the next key query
            // sees it.
            (
                E2eeSessionState::Unverified,
                Some(keys(true)),
                "complete, this session not verified",
            ),
            (
                E2eeSessionState::Unverified,
                None,
                "unknown, this session not verified",
            ),
            (
                E2eeSessionState::NoCrossSigning,
                Some(keys(false)),
                "incomplete, this session not verified",
            ),
            (
                E2eeSessionState::Unknown,
                Some(keys(false)),
                "incomplete, this session unknown",
            ),
        ] {
            assert_eq!(
                e2ee_status(&status(session, cross_signing)),
                vec![
                    info(&format!(
                        "E2EE: recovery disabled, local backup enabled, server backup not checked, cross-signing {label}"
                    )),
                    commands.clone(),
                ],
                "{session:?}"
            );
        }
        assert_eq!(
            SESSION_UNVERIFIED_WARNING,
            "This session is not verified: /verify to verify it from another of your sessions, /e2ee recover with your recovery key, or /e2ee reset if you have neither"
        );
    }

    #[test]
    fn the_e2ee_hint_names_only_what_applies() {
        let status = |recovery, exists_on_server| E2eeStatus {
            recovery,
            backup: E2eeBackupStatus {
                state: E2eeBackupState::Enabled,
                enabled_locally: true,
                exists_on_server,
            },
            cross_signing: None,
            session: E2eeSessionState::Verified,
        };
        let hint = |status: &E2eeStatus| {
            e2ee_status(status)
                .into_iter()
                .skip(1)
                .map(|(_, line)| line)
                .collect::<Vec<_>>()
        };
        for (recovery, exists_on_server, expected) in [
            (E2eeRecoveryState::Enabled, Some(true), None),
            (E2eeRecoveryState::Enabled, Some(false), None),
            (E2eeRecoveryState::Unknown, Some(true), None),
            (
                E2eeRecoveryState::Enabled,
                None,
                Some("Use /e2ee refresh to check server backup"),
            ),
            (
                E2eeRecoveryState::Disabled,
                Some(true),
                Some("Use /e2ee setup to enable recovery"),
            ),
            (
                E2eeRecoveryState::Incomplete,
                Some(true),
                Some("Use /e2ee recover to restore keys"),
            ),
            (
                E2eeRecoveryState::Incomplete,
                None,
                Some("Use /e2ee refresh to check server backup, /e2ee recover to restore keys"),
            ),
        ] {
            assert_eq!(
                hint(&status(recovery, exists_on_server)),
                Vec::from_iter(expected.map(str::to_owned)),
                "{recovery:?} {exists_on_server:?}"
            );
        }
    }

    fn verification_event(state: VerificationState) -> VerificationStateEvent {
        VerificationStateEvent {
            flow_id: "flow-1".to_owned(),
            user_id: "@alice:example".to_owned(),
            state,
        }
    }

    #[test]
    fn verification_request_lines_name_who_asked_whom() {
        let request = |user_id: &str, direction| VerificationRequest {
            flow_id: "flow-1".to_owned(),
            user_id: user_id.to_owned(),
            direction,
        };
        let incoming = || VerificationDirection::Incoming {
            from_device: "PHONE".to_owned(),
        };
        let own = Some("@me:example");
        let accept_or_cancel = "Actions: Tools > Accept Verification, Tools > Cancel Verification, /verify accept, /verify cancel";
        let cancel = "Actions: Tools > Cancel Verification, /verify cancel";
        let cases = [
            (
                request("@alice:example", incoming()),
                "Verification requested by @alice:example (flow flow-1)",
                accept_or_cancel,
            ),
            (
                request("@me:example", incoming()),
                "Verification requested by your other session PHONE (flow flow-1)",
                accept_or_cancel,
            ),
            (
                request(
                    "@me:example",
                    VerificationDirection::Outgoing { to_device: None },
                ),
                "Verification request sent to your other sessions; accept it on one of them",
                cancel,
            ),
            (
                request(
                    "@me:example",
                    VerificationDirection::Outgoing {
                        to_device: Some("PHONE".to_owned()),
                    },
                ),
                "Verification request sent to your session PHONE; accept it there",
                cancel,
            ),
            (
                request(
                    "@alice:example",
                    VerificationDirection::Outgoing { to_device: None },
                ),
                "Verification request sent to @alice:example; waiting for them to accept",
                cancel,
            ),
            (
                request(
                    "@alice:example",
                    VerificationDirection::Outgoing {
                        to_device: Some("LAPTOP".to_owned()),
                    },
                ),
                "Verification request sent to @alice:example device LAPTOP; waiting for them to accept",
                cancel,
            ),
        ];
        for (request, line, actions) in cases {
            assert_eq!(
                verification_request(&request, own),
                vec![line.to_owned(), actions.to_owned()],
                "{request:?}"
            );
        }
    }

    #[test]
    fn verification_lines_follow_the_flow() {
        assert_eq!(
            verification(&verification_event(VerificationState::Requested)),
            vec![
                "Verification requested by @alice:example (flow flow-1)".to_owned(),
                "Actions: Tools > Accept Verification, Tools > Cancel Verification, /verify accept, /verify cancel"
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
