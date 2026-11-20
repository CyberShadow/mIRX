//! mIRC's Reconnect on disconnection and connect retry, on the frame's
//! timer.
//!
//! `ConnectionState::WaitingToReconnect` is the only record that the retry
//! timer is armed: `wait_to_retry` enters it with the timer, and `cancel`
//! and the timer itself leave it.

use std::time::Duration;

use super::connection::AfterSyncLost;
use super::*;

/// After the core reported the connection lost: reconnect, wait to retry,
/// or give up, as the settings say.
pub(super) unsafe fn after_sync_lost(hwnd: HWND) -> Result<(), String> {
    let state = state_mut(hwnd);
    let settings = state.settings.settings().connect.clone();
    match state.reconnect.sync_lost(&settings) {
        AfterSyncLost::ReconnectNow => {
            let line = status_log::reconnecting(own_server(&state.model)?);
            log_status(state, StatusLineKind::Info, line);
            submit_start_sync(state)
        }
        AfterSyncLost::RetryAfter(delay) => wait_to_retry(hwnd, delay),
        AfterSyncLost::GiveUp { attempts } => {
            log_status(
                state,
                StatusLineKind::Info,
                status_log::not_retrying(attempts),
            );
            Ok(())
        }
        AfterSyncLost::Stay => Ok(()),
    }
}

unsafe fn wait_to_retry(hwnd: HWND, delay: Duration) -> Result<(), String> {
    let milliseconds = u32::try_from(delay.as_millis()).expect("a retry delay is at most an hour");
    if SetTimer(hwnd, RECONNECT_TIMER_ID, milliseconds, None) == 0 {
        return Err(last_os_error("SetTimer"));
    }
    let state = state_mut(hwnd);
    state.model.connection = ConnectionState::WaitingToReconnect;
    state.model.status = status_log::retrying_in(delay);
    Ok(())
}

/// The user's Connect, Disconnect or Log Out while a retry is due: no
/// retry comes.
pub(super) unsafe fn cancel(hwnd: HWND) {
    KillTimer(hwnd, RECONNECT_TIMER_ID);
    let state = state_mut(hwnd);
    assert_eq!(
        state.model.connection,
        ConnectionState::WaitingToReconnect,
        "only a due retry is cancelled"
    );
    state.model.connection = ConnectionState::SignedIn;
}

/// `WM_TIMER`: the retry is due.
pub(super) unsafe fn retry(hwnd: HWND) -> Result<(), String> {
    KillTimer(hwnd, RECONNECT_TIMER_ID);
    let state = state_mut(hwnd);
    // `KillTimer` leaves a `WM_TIMER` already posted, so one can still come
    // after the user's Connect or Disconnect cancelled the retry.
    if state.model.connection != ConnectionState::WaitingToReconnect {
        return Ok(());
    }
    // As File > Connect leaves it: `SyncStarted` makes it `Syncing`.
    state.model.connection = ConnectionState::SignedIn;
    let attempt = state.reconnect.retry();
    let line = status_log::connect_retry(attempt, own_server(&state.model)?);
    log_status(state, StatusLineKind::Info, line);
    submit_start_sync(state)?;
    sync_views_and_tree(hwnd)
}
