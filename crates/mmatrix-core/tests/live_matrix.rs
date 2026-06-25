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
            Event::LoggedIn => saw_logged_in = true,
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
