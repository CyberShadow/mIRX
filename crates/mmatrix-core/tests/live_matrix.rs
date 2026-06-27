use std::env;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use mmatrix_core::{Command, CoreConfig, Event, LoginRequest, SendTextRequest, start};
use tempfile::TempDir;

fn required_env(name: &str) -> Option<String> {
    match env::var(name) {
        Ok(value) if !value.is_empty() => Some(value),
        _ => None,
    }
}

#[test]
fn live_matrix_login_and_shutdown_is_env_gated() {
    let Some(homeserver_url) = required_env("MMATRIX_HS") else {
        return;
    };
    let Some(username) = required_env("MMATRIX_USER") else {
        return;
    };
    let Some(password) = required_env("MMATRIX_PASS") else {
        return;
    };
    let room_id = required_env("MMATRIX_ROOM");

    let temp_dir = TempDir::new().expect("temporary profile directory");
    let handle = start(CoreConfig {
        profile_dir: temp_dir.path().join("profile"),
        store_passphrase: "live-test-store-passphrase".to_owned(),
    })
    .expect("start core");

    assert_eq!(
        handle
            .events()
            .recv_timeout(Duration::from_secs(2))
            .expect("started event"),
        Event::Started
    );

    handle
        .command_sender()
        .send(Command::Login(LoginRequest {
            homeserver_url,
            username,
            password,
            device_display_name: "mmatrix live test".to_owned(),
        }))
        .expect("send login");

    let login_deadline = Instant::now() + Duration::from_secs(60);
    let mut saw_logged_in = false;
    let mut saw_rooms = false;

    while Instant::now() < login_deadline && (!saw_logged_in || !saw_rooms) {
        match handle
            .events()
            .recv_timeout(Duration::from_secs(2))
            .expect("login event")
        {
            Event::LoggedIn { session } => {
                assert!(
                    !session.user_id.is_empty(),
                    "expected non-empty session user id"
                );
                assert!(
                    !session.device_id.is_empty(),
                    "expected non-empty session device id"
                );
                assert!(
                    !session.access_token.is_empty(),
                    "expected non-empty session access token"
                );
                saw_logged_in = true;
            }
            Event::SessionUpdated { session } => {
                assert!(
                    !session.user_id.is_empty(),
                    "expected non-empty session update user id"
                );
                assert!(
                    !session.device_id.is_empty(),
                    "expected non-empty session update device id"
                );
                assert!(
                    !session.access_token.is_empty(),
                    "expected non-empty session update access token"
                );
            }
            Event::SyncedRooms(_) => saw_rooms = true,
            Event::TimelineText(_) => {}
            Event::Error(err) => panic!(
                "live login failed during {:?}: {}",
                err.operation, err.message
            ),
            other => panic!("unexpected live login event: {other:?}"),
        }
    }

    assert!(saw_logged_in, "expected logged-in event");
    assert!(saw_rooms, "expected room sync event");

    handle
        .command_sender()
        .send(Command::StartSync)
        .expect("send start sync");

    let sync_started_deadline = Instant::now() + Duration::from_secs(30);
    let mut saw_sync_started = false;
    while Instant::now() < sync_started_deadline {
        match handle
            .events()
            .recv_timeout(Duration::from_secs(2))
            .expect("sync start event")
        {
            Event::SyncStarted => {
                saw_sync_started = true;
                break;
            }
            Event::SessionUpdated { session } => {
                assert!(
                    !session.user_id.is_empty(),
                    "expected non-empty session update user id"
                );
                assert!(
                    !session.device_id.is_empty(),
                    "expected non-empty session update device id"
                );
                assert!(
                    !session.access_token.is_empty(),
                    "expected non-empty session update access token"
                );
            }
            Event::TimelineText(_) => {}
            Event::Error(err) => panic!(
                "live start sync failed during {:?}: {}",
                err.operation, err.message
            ),
            other => panic!("unexpected sync start event: {other:?}"),
        }
    }

    assert!(saw_sync_started, "timed out waiting for sync started event");

    handle
        .command_sender()
        .send(Command::StartSync)
        .expect("send duplicate start sync");

    let duplicate_sync_deadline = Instant::now() + Duration::from_secs(15);
    let mut saw_duplicate_sync_error = false;
    while Instant::now() < duplicate_sync_deadline {
        match handle
            .events()
            .recv_timeout(Duration::from_secs(2))
            .expect("duplicate start sync event")
        {
            Event::Error(err) if err.operation == mmatrix_core::Operation::StartSync => {
                assert_eq!(err.message, "continuous sync is already running");
                saw_duplicate_sync_error = true;
                break;
            }
            Event::SessionUpdated { session } => {
                assert!(
                    !session.user_id.is_empty(),
                    "expected non-empty session update user id"
                );
                assert!(
                    !session.device_id.is_empty(),
                    "expected non-empty session update device id"
                );
                assert!(
                    !session.access_token.is_empty(),
                    "expected non-empty session update access token"
                );
            }
            Event::TimelineText(_) => {}
            other => panic!("unexpected duplicate start sync event: {other:?}"),
        }
    }

    assert!(
        saw_duplicate_sync_error,
        "timed out waiting for duplicate start sync error"
    );

    if let Some(room_id) = room_id {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time after unix epoch")
            .as_nanos();
        handle
            .command_sender()
            .send(Command::SendText(SendTextRequest {
                room_id,
                body: format!("mmatrix live test {nonce}"),
            }))
            .expect("send text");
    }

    handle
        .command_sender()
        .send(Command::Logout)
        .expect("send logout");

    let logout_deadline = Instant::now() + Duration::from_secs(30);
    let mut saw_logged_out = false;
    while Instant::now() < logout_deadline {
        match handle
            .events()
            .recv_timeout(Duration::from_secs(2))
            .expect("logout event")
        {
            Event::LoggedOut => {
                saw_logged_out = true;
                break;
            }
            Event::SessionUpdated { session } => {
                assert!(
                    !session.user_id.is_empty(),
                    "expected non-empty session update user id"
                );
                assert!(
                    !session.device_id.is_empty(),
                    "expected non-empty session update device id"
                );
                assert!(
                    !session.access_token.is_empty(),
                    "expected non-empty session update access token"
                );
            }
            Event::TimelineText(_) => {}
            Event::Error(err) => panic!(
                "live post-login command failed during {:?}: {}",
                err.operation, err.message
            ),
            other => panic!("unexpected logout event: {other:?}"),
        }
    }

    assert!(saw_logged_out, "timed out waiting for logged out event");

    handle
        .command_sender()
        .send(Command::Shutdown)
        .expect("send shutdown");

    let stop_deadline = Instant::now() + Duration::from_secs(15);
    while Instant::now() < stop_deadline {
        match handle
            .events()
            .recv_timeout(Duration::from_secs(2))
            .expect("shutdown event")
        {
            Event::Stopped => {
                handle.join().expect("join worker");
                return;
            }
            Event::SessionUpdated { session } => {
                assert!(
                    !session.user_id.is_empty(),
                    "expected non-empty session update user id"
                );
                assert!(
                    !session.device_id.is_empty(),
                    "expected non-empty session update device id"
                );
                assert!(
                    !session.access_token.is_empty(),
                    "expected non-empty session update access token"
                );
            }
            Event::TimelineText(_) => {}
            Event::Error(err) => {
                panic!(
                    "live shutdown failed during {:?}: {}",
                    err.operation, err.message
                )
            }
            other => panic!("unexpected shutdown event: {other:?}"),
        }
    }

    panic!("timed out waiting for stopped event");
}
