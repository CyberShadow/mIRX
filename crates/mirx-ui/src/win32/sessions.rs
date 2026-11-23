//! The Sessions dialog's content (Tools > Sessions...): your sessions as
//! its list shows them, which buttons a selected row allows, its status
//! line, and what Verify... and Rename... send. `sessions_dialog` shows it.
//!
//! Pure code over the core's `UserDevices`.

use mirx_core::{DeviceSummary, Event, Operation, UserDevices};

use super::commands::SlashCommand;
use super::timestamp::{LocalTimeFn, list_date};

pub(super) const READING_TEXT: &str = "Reading your sessions...";
pub(super) const EMPTY_NAME_TEXT: &str = "a session needs a name";

/// The list's rows: Session, Name, Verified, Last seen. This session is in
/// use now, so the server's time for it is stale by design.
pub(super) fn session_rows(devices: &[DeviceSummary], local_time: LocalTimeFn) -> Vec<Vec<String>> {
    devices
        .iter()
        .map(|device| {
            vec![
                device.device_id.clone(),
                device.display_name.clone().unwrap_or_default(),
                if device.verified { "Yes" } else { "No" }.to_owned(),
                match (device.this_session, device.last_seen_ms) {
                    (true, _) => "this session".to_owned(),
                    (false, Some(last_seen_ms)) => list_date(local_time, last_seen_ms),
                    (false, None) => String::new(),
                },
            ]
        })
        .collect()
}

/// The buttons that act on the selected row.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(super) struct SessionActions {
    pub(super) verify: bool,
    pub(super) rename: bool,
    /// Not this session: File > Log Out signs it out.
    pub(super) sign_out: bool,
}

/// `None`: nothing is selected.
pub(super) fn session_actions(device: Option<&DeviceSummary>) -> SessionActions {
    match device {
        None => SessionActions::default(),
        Some(device) => SessionActions {
            verify: !device.verified,
            rename: true,
            sign_out: !device.this_session,
        },
    }
}

/// The count, and what the Status window's E2EE line says when this
/// session is not verified.
pub(super) fn status_text(devices: &UserDevices) -> String {
    let count = match devices.devices.len() {
        1 => "1 session".to_owned(),
        count => format!("{count} sessions"),
    };
    let unverified = devices
        .devices
        .iter()
        .any(|device| device.this_session && !device.verified);
    if unverified {
        format!("{count}; this session is not verified")
    } else {
        count
    }
}

/// Verify... on a row: this session asks your other verified sessions,
/// as Tools > Verify This Session; another asks that session.
pub(super) fn verify_command(own_user_id: &str, device: &DeviceSummary) -> SlashCommand {
    if device.this_session {
        SlashCommand::VerifyStart {
            target: None,
            device_id: None,
        }
    } else {
        SlashCommand::VerifyStart {
            target: Some(own_user_id.to_owned()),
            device_id: Some(device.device_id.clone()),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum RenameOutcome {
    /// The name the session has: nothing to send.
    Unchanged,
    /// Refused, as `/devices rename` cannot send it either.
    Empty,
    Rename(String),
}

/// What Rename... sends for `typed`, trimmed as `/devices rename` trims it.
pub(super) fn rename_outcome(old_name: Option<&str>, typed: &str) -> RenameOutcome {
    let name = typed.trim();
    if name == old_name.unwrap_or_default() {
        RenameOutcome::Unchanged
    } else if name.is_empty() {
        RenameOutcome::Empty
    } else {
        RenameOutcome::Rename(name.to_owned())
    }
}

/// While the dialog is open, what only it shows: the answer to its read of
/// your sessions (a `/devices` typed just before it opened and answered
/// meanwhile is the same list, and fills it instead of printing), and the
/// errors of what it does. Another user's sessions print as ever.
pub(super) fn is_dialog_answer(own_user_id: Option<&str>, event: &Event) -> bool {
    match event {
        Event::UserDevices(devices) => own_user_id == Some(devices.user_id.as_str()),
        Event::Error(error) => matches!(
            error.operation,
            Operation::ListUserDevices | Operation::RenameDevice | Operation::SignOutDevice
        ),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use mirx_core::{CoreErrorEvent, UserIdentityState};

    use super::*;
    use crate::win32::timestamp::utc_local_time;

    fn device(device_id: &str, verified: bool, this_session: bool) -> DeviceSummary {
        DeviceSummary {
            device_id: device_id.to_owned(),
            display_name: None,
            verified,
            this_session,
            last_seen_ms: None,
        }
    }

    fn own_devices(devices: Vec<DeviceSummary>) -> UserDevices {
        UserDevices {
            user_id: "@me:example".to_owned(),
            identity: UserIdentityState::Verified,
            devices,
        }
    }

    #[test]
    fn session_rows_show_this_session_and_local_last_seen() {
        let devices = [
            DeviceSummary {
                display_name: Some("Laptop".to_owned()),
                last_seen_ms: Some(1_795_472_040_000),
                ..device("L4PT0P", true, true)
            },
            DeviceSummary {
                display_name: Some("Element Android".to_owned()),
                last_seen_ms: Some(1_795_472_040_000),
                ..device("PH0NE", false, false)
            },
            device("0LDW3B", false, false),
        ];
        assert_eq!(
            session_rows(&devices, utc_local_time),
            [
                ["L4PT0P", "Laptop", "Yes", "this session"],
                ["PH0NE", "Element Android", "No", "23/11/2026 22:14"],
                ["0LDW3B", "", "No", ""],
            ]
            .map(|row| row.map(str::to_owned).to_vec())
        );
    }

    #[test]
    fn session_actions_follow_the_selected_row() {
        let actions = |verify, rename, sign_out| SessionActions {
            verify,
            rename,
            sign_out,
        };
        assert_eq!(session_actions(None), actions(false, false, false));
        assert_eq!(
            session_actions(Some(&device("L4PT0P", true, true))),
            actions(false, true, false)
        );
        assert_eq!(
            session_actions(Some(&device("L4PT0P", false, true))),
            actions(true, true, false)
        );
        assert_eq!(
            session_actions(Some(&device("PH0NE", false, false))),
            actions(true, true, true)
        );
        assert_eq!(
            session_actions(Some(&device("PH0NE", true, false))),
            actions(false, true, true)
        );
    }

    #[test]
    fn status_text_counts_and_says_when_this_session_is_not_verified() {
        assert_eq!(
            status_text(&own_devices(vec![device("L4PT0P", true, true)])),
            "1 session"
        );
        assert_eq!(
            status_text(&own_devices(vec![
                device("L4PT0P", true, true),
                device("PH0NE", false, false),
                device("0LDW3B", false, false),
            ])),
            "3 sessions"
        );
        assert_eq!(
            status_text(&own_devices(vec![
                device("L4PT0P", false, true),
                device("PH0NE", true, false),
            ])),
            "2 sessions; this session is not verified"
        );
    }

    #[test]
    fn verify_on_this_session_asks_your_other_sessions() {
        assert_eq!(
            verify_command("@me:example", &device("L4PT0P", false, true)),
            SlashCommand::VerifyStart {
                target: None,
                device_id: None,
            }
        );
        assert_eq!(
            verify_command("@me:example", &device("PH0NE", false, false)),
            SlashCommand::VerifyStart {
                target: Some("@me:example".to_owned()),
                device_id: Some("PH0NE".to_owned()),
            }
        );
    }

    #[test]
    fn rename_outcome_refuses_empty_and_skips_unchanged() {
        assert_eq!(
            rename_outcome(Some("Phone"), " Phone "),
            RenameOutcome::Unchanged
        );
        assert_eq!(rename_outcome(None, ""), RenameOutcome::Unchanged);
        assert_eq!(rename_outcome(Some("Phone"), "  "), RenameOutcome::Empty);
        assert_eq!(
            rename_outcome(None, " Old phone "),
            RenameOutcome::Rename("Old phone".to_owned())
        );
        assert_eq!(
            rename_outcome(Some("Phone"), "Old phone"),
            RenameOutcome::Rename("Old phone".to_owned())
        );
    }

    #[test]
    fn the_dialog_takes_your_sessions_and_its_operations_errors() {
        let error = |operation| {
            Event::Error(CoreErrorEvent {
                operation,
                room_id: None,
                message: "failed".to_owned(),
            })
        };
        let own = Some("@me:example");
        assert!(is_dialog_answer(
            own,
            &Event::UserDevices(own_devices(Vec::new()))
        ));
        let bob = UserDevices {
            user_id: "@bob:example".to_owned(),
            ..own_devices(Vec::new())
        };
        assert!(!is_dialog_answer(own, &Event::UserDevices(bob)));
        assert!(!is_dialog_answer(
            None,
            &Event::UserDevices(own_devices(Vec::new()))
        ));
        for operation in [
            Operation::ListUserDevices,
            Operation::RenameDevice,
            Operation::SignOutDevice,
        ] {
            assert!(is_dialog_answer(own, &error(operation)));
        }
        assert!(!is_dialog_answer(
            own,
            &error(Operation::RequestVerification)
        ));
        // The changes are lines in the active window too.
        assert!(!is_dialog_answer(
            own,
            &Event::DeviceSignedOut("PH0NE".to_owned())
        ));
    }
}
