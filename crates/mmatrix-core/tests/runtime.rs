use std::time::Duration;

use mmatrix_core::{
    Command, CoreConfig, Event, LoginRequest, Operation, SendTextRequest, StoredSession, start,
};
use tempfile::TempDir;

fn test_config() -> (TempDir, CoreConfig) {
    let temp_dir = TempDir::new().expect("temporary profile directory");
    let config = CoreConfig {
        profile_dir: temp_dir.path().join("profile"),
        store_passphrase: "test-store-passphrase".to_owned(),
    };

    (temp_dir, config)
}

#[test]
fn startup_and_shutdown_emit_lifecycle_events() {
    let (_temp_dir, config) = test_config();
    let handle = start(config).expect("start core");

    assert_eq!(
        handle
            .events()
            .recv_timeout(Duration::from_secs(2))
            .expect("started event"),
        Event::Started
    );

    handle
        .command_sender()
        .send(Command::Shutdown)
        .expect("send shutdown");

    assert_eq!(
        handle
            .events()
            .recv_timeout(Duration::from_secs(2))
            .expect("stopped event"),
        Event::Stopped
    );

    handle.join().expect("join worker");
}

#[test]
fn invalid_command_ordering_emits_typed_error_events() {
    let (_temp_dir, config) = test_config();
    let handle = start(config).expect("start core");

    assert_eq!(
        handle
            .events()
            .recv_timeout(Duration::from_secs(2))
            .expect("started event"),
        Event::Started
    );

    handle
        .command_sender()
        .send(Command::StartSync)
        .expect("send start sync");
    handle
        .command_sender()
        .send(Command::SendText(SendTextRequest {
            room_id: "!room:example.org".to_owned(),
            body: "hello".to_owned(),
        }))
        .expect("send text");
    handle
        .command_sender()
        .send(Command::Logout)
        .expect("send logout");

    let start_sync_error = handle
        .events()
        .recv_timeout(Duration::from_secs(2))
        .expect("start sync error");
    let send_text_error = handle
        .events()
        .recv_timeout(Duration::from_secs(2))
        .expect("send text error");
    let logout_error = handle
        .events()
        .recv_timeout(Duration::from_secs(2))
        .expect("logout error");

    match start_sync_error {
        Event::Error(err) => assert_eq!(err.operation, Operation::StartSync),
        other => panic!("expected start sync error, got {other:?}"),
    }
    match send_text_error {
        Event::Error(err) => assert_eq!(err.operation, Operation::SendText),
        other => panic!("expected send text error, got {other:?}"),
    }
    match logout_error {
        Event::Error(err) => assert_eq!(err.operation, Operation::Logout),
        other => panic!("expected logout error, got {other:?}"),
    }

    handle
        .command_sender()
        .send(Command::Shutdown)
        .expect("send shutdown");
    assert_eq!(
        handle
            .events()
            .recv_timeout(Duration::from_secs(2))
            .expect("stopped event"),
        Event::Stopped
    );
    handle.join().expect("join worker");
}

#[test]
fn login_store_initialization_failure_emits_typed_error_event() {
    let temp_dir = TempDir::new().expect("temporary profile directory");
    let profile_file = temp_dir.path().join("profile-file");
    std::fs::write(&profile_file, "not a directory").expect("write profile file");
    let config = CoreConfig {
        profile_dir: profile_file,
        store_passphrase: "test-store-passphrase".to_owned(),
    };
    let handle = start(config).expect("start core");

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
            homeserver_url: "https://example.invalid".to_owned(),
            username: "user".to_owned(),
            password: "password".to_owned(),
            device_display_name: "mmatrix runtime test".to_owned(),
        }))
        .expect("send login");

    match handle
        .events()
        .recv_timeout(Duration::from_secs(2))
        .expect("login error")
    {
        Event::Error(err) => {
            assert_eq!(err.operation, Operation::Login);
            assert!(
                err.message.contains("failed to create store directory"),
                "unexpected login error: {}",
                err.message
            );
        }
        other => panic!("expected login error, got {other:?}"),
    }

    handle
        .command_sender()
        .send(Command::Shutdown)
        .expect("send shutdown");
    assert_eq!(
        handle
            .events()
            .recv_timeout(Duration::from_secs(2))
            .expect("stopped event"),
        Event::Stopped
    );
    handle.join().expect("join worker");
}

#[test]
fn stored_session_serializes_and_deserializes_with_existing_fields() {
    let session = StoredSession {
        user_id: "@user:example.org".to_owned(),
        device_id: "DEVICE".to_owned(),
        access_token: "access-token".to_owned(),
        refresh_token: Some("refresh-token".to_owned()),
    };

    let json = serde_json::to_string(&session).expect("serialize stored session");
    assert_eq!(
        json,
        r#"{"user_id":"@user:example.org","device_id":"DEVICE","access_token":"access-token","refresh_token":"refresh-token"}"#
    );

    let restored: StoredSession = serde_json::from_str(&json).expect("deserialize stored session");
    assert_eq!(restored, session);
}

#[test]
fn stored_session_serializes_and_deserializes_without_refresh_token() {
    let session = StoredSession {
        user_id: "@user:example.org".to_owned(),
        device_id: "DEVICE".to_owned(),
        access_token: "access-token".to_owned(),
        refresh_token: None,
    };

    let json = serde_json::to_string(&session).expect("serialize stored session");
    assert_eq!(
        json,
        r#"{"user_id":"@user:example.org","device_id":"DEVICE","access_token":"access-token","refresh_token":null}"#
    );

    let restored: StoredSession = serde_json::from_str(&json).expect("deserialize stored session");
    assert_eq!(restored, session);
}

#[test]
fn stored_session_rejects_unknown_fields() {
    assert!(
        serde_json::from_str::<StoredSession>(
            r#"{"user_id":"@user:example.org","device_id":"DEVICE","access_token":"access-token","refresh_token":null,"unexpected":"value"}"#
        )
        .is_err(),
        "stored session must reject unknown fields"
    );
}

#[test]
fn logged_in_event_serializes_and_deserializes_session_payload() {
    let session = StoredSession {
        user_id: "@user:example.org".to_owned(),
        device_id: "DEVICE".to_owned(),
        access_token: "access-token".to_owned(),
        refresh_token: Some("refresh-token".to_owned()),
    };
    let event = Event::LoggedIn {
        session: session.clone(),
    };

    let json = serde_json::to_string(&event).expect("serialize logged-in event");
    assert_eq!(
        json,
        r#"{"LoggedIn":{"session":{"user_id":"@user:example.org","device_id":"DEVICE","access_token":"access-token","refresh_token":"refresh-token"}}}"#
    );

    let restored: Event = serde_json::from_str(&json).expect("deserialize logged-in event");
    assert_eq!(restored, event);
}

#[test]
fn sync_started_event_round_trips_and_logged_in_no_longer_uses_unit_shape() {
    let sync_started = serde_json::to_string(&Event::SyncStarted).expect("serialize sync started");
    assert_eq!(sync_started, r#""SyncStarted""#);

    let restored: Event = serde_json::from_str(&sync_started).expect("deserialize sync started");
    assert_eq!(restored, Event::SyncStarted);

    assert!(
        serde_json::from_str::<Event>(r#""LoggedIn""#).is_err(),
        "logged-in event must include a session payload"
    );
}
