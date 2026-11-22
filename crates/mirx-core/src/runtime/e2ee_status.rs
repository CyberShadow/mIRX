//! What the UI is told of the session's E2EE status: the session's first
//! summary from the server (its entry's or its first contact's), a
//! command's answer, and after that every change, once. The SDK's state
//! streams tick whenever a state is set, equal or not, and from the
//! session's local start on, before the server was reached; their reads
//! are compared with the last summary told.

use std::cell::RefCell;
use std::rc::Rc;

use matrix_sdk::Client;

use super::{EventSender, emit_event, emit_matrix_error};
use crate::api::{E2eeStatus, Event, Operation};
use crate::matrix::{self, BackupServerStatusMode, MatrixCoreError};

/// The last summary the UI was told; `None` until the session's first from
/// the server. Shared by everything in a session that tells one.
#[derive(Clone, Default)]
pub(crate) struct E2eeStatusWatch(Rc<RefCell<Option<E2eeStatus>>>);

impl E2eeStatusWatch {
    /// A summary the server was asked for (a command's answer): always
    /// told, and the new baseline.
    pub(crate) fn report(&self, events: &EventSender, status: E2eeStatus) {
        *self.0.borrow_mut() = Some(status.clone());
        emit_event(events, Event::E2eeStatus(status));
    }

    /// The session's first summary: the server's, or, if asking it failed,
    /// its error and the cached summary, whose server backup is then truly
    /// not checked. Either is the baseline; with none (the cached read
    /// failed too) changes are not told until a command's answer.
    pub(crate) fn report_first(&self, events: &EventSender, first: FirstE2eeStatus) {
        let FirstE2eeStatus {
            fetch_error,
            status,
        } = first;
        if let Some(err) = fetch_error {
            emit_matrix_error(events, Operation::E2eeStatus, err);
        }
        match status {
            Ok(status) => self.report(events, status),
            Err(err) => emit_matrix_error(events, Operation::E2eeStatus, err),
        }
    }

    /// A summary read after a state change, without asking the server:
    /// told only once there is a baseline, and only if it differs from it.
    /// The read cannot know whether the server has a backup: it keeps the
    /// server's last answer.
    pub(crate) fn changed(&self, events: &EventSender, mut status: E2eeStatus) {
        let mut last = self.0.borrow_mut();
        let Some(last) = last.as_mut() else {
            return;
        };
        if status.backup.exists_on_server.is_none() {
            status.backup.exists_on_server = last.backup.exists_on_server;
        }
        if *last != status {
            *last = status.clone();
            emit_event(events, Event::E2eeStatus(status));
        }
    }
}

/// The session's first E2EE summary, read once the server answered a sync.
pub(crate) struct FirstE2eeStatus {
    /// Why the server could not be asked whether it has a backup.
    fetch_error: Option<MatrixCoreError>,
    /// The server's summary, or after a `fetch_error` the cached one.
    status: Result<E2eeStatus, MatrixCoreError>,
}

impl FirstE2eeStatus {
    pub(crate) async fn read(client: &Client) -> Self {
        match matrix::summarize_e2ee_status(client, BackupServerStatusMode::Fetch).await {
            Ok(status) => Self {
                fetch_error: None,
                status: Ok(status),
            },
            Err(err) => Self {
                fetch_error: Some(err),
                status: matrix::summarize_e2ee_status(client, BackupServerStatusMode::Cached).await,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::mpsc;

    use super::{E2eeStatusWatch, FirstE2eeStatus};
    use crate::api::{
        E2eeBackupState, E2eeBackupStatus, E2eeRecoveryState, E2eeSessionState, E2eeStatus, Event,
        Operation,
    };
    use crate::matrix::MatrixCoreError;

    fn status(recovery: E2eeRecoveryState, exists_on_server: Option<bool>) -> E2eeStatus {
        E2eeStatus {
            recovery,
            backup: E2eeBackupStatus {
                state: E2eeBackupState::Enabled,
                enabled_locally: true,
                exists_on_server,
            },
            cross_signing: None,
            session: E2eeSessionState::Verified,
        }
    }

    fn told(events: &mpsc::Receiver<Event>) -> Vec<Event> {
        events.try_iter().collect()
    }

    #[test]
    fn a_change_is_told_once_after_the_baseline() {
        let watch = E2eeStatusWatch::default();
        let (tx, rx) = mpsc::channel();
        let enabled = status(E2eeRecoveryState::Enabled, Some(true));

        // Before the server was reached: nothing.
        watch.changed(&tx, status(E2eeRecoveryState::Enabled, None));
        assert_eq!(told(&rx), []);

        watch.report(&tx, enabled.clone());
        assert_eq!(told(&rx), [Event::E2eeStatus(enabled.clone())]);
        // The same, read without asking the server: it still has its backup.
        watch.changed(&tx, status(E2eeRecoveryState::Enabled, None));
        assert_eq!(told(&rx), []);

        let disabled = status(E2eeRecoveryState::Disabled, Some(true));
        watch.changed(&tx, status(E2eeRecoveryState::Disabled, None));
        assert_eq!(told(&rx), [Event::E2eeStatus(disabled.clone())]);
        watch.changed(&tx, disabled);
        assert_eq!(told(&rx), []);

        // A server's answer is told, changed or not.
        watch.report(&tx, enabled.clone());
        watch.report(&tx, enabled.clone());
        assert_eq!(
            told(&rx),
            [
                Event::E2eeStatus(enabled.clone()),
                Event::E2eeStatus(enabled)
            ]
        );
    }

    #[test]
    fn a_first_status_the_server_could_not_answer_is_the_cached_one() {
        let watch = E2eeStatusWatch::default();
        let (tx, rx) = mpsc::channel();
        let cached = status(E2eeRecoveryState::Enabled, None);

        watch.report_first(
            &tx,
            FirstE2eeStatus {
                fetch_error: Some(MatrixCoreError::new("no backup answer")),
                status: Ok(cached.clone()),
            },
        );
        let first = told(&rx);
        let [Event::Error(err), Event::E2eeStatus(first_status)] = first.as_slice() else {
            panic!("an error, then the cached status: {first:?}");
        };
        assert_eq!(
            (err.operation, err.message.as_str()),
            (Operation::E2eeStatus, "no backup answer")
        );
        assert_eq!(*first_status, cached);
        // It is the baseline.
        watch.changed(&tx, cached);
        assert_eq!(told(&rx), []);
        watch.changed(&tx, status(E2eeRecoveryState::Disabled, None));
        assert_eq!(
            told(&rx),
            [Event::E2eeStatus(status(E2eeRecoveryState::Disabled, None))]
        );
    }
}
