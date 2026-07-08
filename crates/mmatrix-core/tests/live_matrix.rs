use std::env;
use std::error::Error;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use matrix_sdk::{
    Client, Room,
    config::SyncSettings,
    ruma::{
        OwnedEventId, RoomId,
        api::client::config::get_room_account_data,
        events::{
            RoomAccountDataEventType, fully_read::FullyReadEventContent,
            room::message::RoomMessageEventContent,
        },
    },
};
use mmatrix_core::{
    Command, CoreConfig, CoreHandle, CreateDirectRoomRequest, Event, LoginRequest,
    MarkRoomReadRequest, RoomMessagePresentationKind, RoomSummary, SendMessageRequest,
    StoredSession, start,
};
use tempfile::TempDir;

fn required_env(name: &str) -> Option<String> {
    match env::var(name) {
        Ok(value) if !value.is_empty() => Some(value),
        _ => None,
    }
}

struct LiveMatrixEnv {
    homeserver_url: String,
    username: String,
    password: String,
    room_id: String,
}

fn live_matrix_env_with_room() -> Option<LiveMatrixEnv> {
    Some(LiveMatrixEnv {
        homeserver_url: required_env("MMATRIX_HS")?,
        username: required_env("MMATRIX_USER")?,
        password: required_env("MMATRIX_PASS")?,
        room_id: required_env("MMATRIX_ROOM")?,
    })
}

fn assert_non_empty_session(session: &StoredSession, context: &str) {
    assert!(
        !session.user_id.is_empty(),
        "expected non-empty {context} user id"
    );
    assert!(
        !session.device_id.is_empty(),
        "expected non-empty {context} device id"
    );
    assert!(
        !session.access_token.is_empty(),
        "expected non-empty {context} access token"
    );
}

fn live_nonce() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system time after unix epoch")
        .as_nanos()
}

fn live_start_core() -> (TempDir, CoreHandle) {
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

    (temp_dir, handle)
}

fn live_send_login(
    handle: &CoreHandle,
    homeserver_url: String,
    username: String,
    password: String,
    device_display_name: &str,
) {
    handle
        .command_sender()
        .send(Command::Login(LoginRequest {
            homeserver_url,
            username,
            password,
            device_display_name: device_display_name.to_owned(),
        }))
        .expect("send login");
}

fn wait_for_live_login_and_synced_rooms(handle: &CoreHandle) {
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
                assert_non_empty_session(&session, "session");
                saw_logged_in = true;
            }
            Event::SessionUpdated { session } => {
                assert_non_empty_session(&session, "session update");
            }
            Event::SyncedRooms(_) => saw_rooms = true,
            Event::RoomActivity(_)
            | Event::RoomActivityUpdate(_)
            | Event::RoomTyping(_)
            | Event::RoomMembers(_)
            | Event::RoomReadState(_)
            | Event::E2eeStatus(_)
            | Event::E2eeRecoverySucceeded(_) => {}
            Event::Error(err) => panic!(
                "live login failed during {:?}: {}",
                err.operation, err.message
            ),
            other => panic!("unexpected live login event: {other:?}"),
        }
    }

    assert!(saw_logged_in, "expected logged-in event");
    assert!(saw_rooms, "expected room sync event");
}

fn wait_for_live_logged_out(handle: &CoreHandle, context: &str) {
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
                assert_non_empty_session(&session, "session update");
            }
            Event::RoomActivity(_)
            | Event::RoomActivityUpdate(_)
            | Event::RoomTyping(_)
            | Event::RoomMembers(_)
            | Event::RoomReadState(_)
            | Event::E2eeStatus(_)
            | Event::E2eeRecoverySucceeded(_) => {}
            Event::Error(err) => {
                panic!(
                    "{context} failed during {:?}: {}",
                    err.operation, err.message
                )
            }
            other => panic!("unexpected logout event: {other:?}"),
        }
    }

    assert!(saw_logged_out, "timed out waiting for logged out event");
}

fn wait_for_live_synced_rooms(handle: &CoreHandle, context: &str) -> Vec<RoomSummary> {
    let deadline = Instant::now() + Duration::from_secs(60);

    while Instant::now() < deadline {
        match handle
            .events()
            .recv_timeout(Duration::from_secs(2))
            .expect("synced rooms event")
        {
            Event::SyncedRooms(rooms) => return rooms,
            Event::SessionUpdated { session } => {
                assert_non_empty_session(&session, "session update");
            }
            Event::RoomActivity(_)
            | Event::RoomActivityUpdate(_)
            | Event::RoomTyping(_)
            | Event::RoomMembers(_)
            | Event::RoomReadState(_)
            | Event::E2eeStatus(_)
            | Event::E2eeRecoverySucceeded(_) => {}
            Event::Error(err) => {
                panic!(
                    "{context} failed during {:?}: {}",
                    err.operation, err.message
                )
            }
            other => panic!("unexpected {context} event: {other:?}"),
        }
    }

    panic!("timed out waiting for synced rooms during {context}");
}

fn live_shutdown_and_join(handle: CoreHandle) {
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
                assert_non_empty_session(&session, "session update");
            }
            Event::RoomActivity(_)
            | Event::RoomActivityUpdate(_)
            | Event::RoomTyping(_)
            | Event::RoomMembers(_)
            | Event::RoomReadState(_)
            | Event::E2eeStatus(_)
            | Event::E2eeRecoverySucceeded(_) => {}
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

async fn live_fixture_client_from_env_values(
    hs: &str,
    user: &str,
    pass: &str,
) -> Result<Client, Box<dyn Error + Send + Sync>> {
    let client = Client::builder().homeserver_url(hs).build().await?;
    client
        .matrix_auth()
        .login_username(user, pass)
        .initial_device_display_name("mmatrix live fixture")
        .send()
        .await?;
    Ok(client)
}

async fn live_room_from_env_value(client: &Client, room_id: &str) -> matrix_sdk::Result<Room> {
    let room_id =
        <&RoomId>::try_from(room_id).expect("MMATRIX_ROOM must be a valid Matrix room id");
    client.sync_once(SyncSettings::default()).await?;
    Ok(client
        .get_room(room_id)
        .expect("MMATRIX_ROOM must be visible to the fixture client after sync"))
}

async fn live_direct_text_message_event_id(
    room: &Room,
    body: String,
) -> matrix_sdk::Result<OwnedEventId> {
    Ok(room
        .send(RoomMessageEventContent::text_plain(body))
        .await?
        .response
        .event_id)
}

async fn live_get_fully_read_marker(
    client: &Client,
    room: &Room,
) -> matrix_sdk::Result<OwnedEventId> {
    let user_id = client
        .user_id()
        .expect("fixture client must have a user id after login")
        .to_owned();
    let response = client
        .send(get_room_account_data::v3::Request::new(
            user_id,
            room.room_id().to_owned(),
            RoomAccountDataEventType::FullyRead,
        ))
        .await?;
    let content = response
        .account_data
        .deserialize_as_unchecked::<FullyReadEventContent>()
        .expect("m.fully_read account data must deserialize as FullyReadEventContent");
    Ok(content.event_id)
}

fn has_direct_room_for_user(rooms: &[RoomSummary], user_id: &str) -> bool {
    rooms.iter().any(|room| {
        room.is_direct
            && room
                .direct_user_ids
                .iter()
                .any(|direct_user_id| direct_user_id == user_id)
    })
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

    let (_temp_dir, handle) = live_start_core();
    live_send_login(
        &handle,
        homeserver_url,
        username,
        password,
        "mmatrix live test",
    );
    wait_for_live_login_and_synced_rooms(&handle);

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
                assert_non_empty_session(&session, "session update");
            }
            Event::RoomActivity(_)
            | Event::RoomActivityUpdate(_)
            | Event::RoomTyping(_)
            | Event::RoomMembers(_)
            | Event::RoomReadState(_)
            | Event::E2eeStatus(_)
            | Event::E2eeRecoverySucceeded(_) => {}
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
                assert_non_empty_session(&session, "session update");
            }
            Event::RoomActivity(_)
            | Event::RoomActivityUpdate(_)
            | Event::RoomTyping(_)
            | Event::RoomMembers(_)
            | Event::RoomReadState(_)
            | Event::E2eeStatus(_)
            | Event::E2eeRecoverySucceeded(_) => {}
            other => panic!("unexpected duplicate start sync event: {other:?}"),
        }
    }

    assert!(
        saw_duplicate_sync_error,
        "timed out waiting for duplicate start sync error"
    );

    if let Some(room_id) = room_id {
        let nonce = live_nonce();
        handle
            .command_sender()
            .send(Command::SendMessage(SendMessageRequest {
                room_id,
                presentation: RoomMessagePresentationKind::Text,
                body: format!("mmatrix live test {nonce}"),
                thread: None,
                reply: None,
            }))
            .expect("send message");
    }

    handle
        .command_sender()
        .send(Command::Logout)
        .expect("send logout");

    wait_for_live_logged_out(&handle, "live post-login command");
    live_shutdown_and_join(handle);
}

#[test]
fn live_matrix_mark_room_read_updates_fully_read_marker_is_env_gated() {
    let Some(env) = live_matrix_env_with_room() else {
        return;
    };

    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("build live fixture runtime");

    let (_temp_dir, handle) = live_start_core();
    live_send_login(
        &handle,
        env.homeserver_url.clone(),
        env.username.clone(),
        env.password.clone(),
        "mmatrix live read marker test",
    );
    wait_for_live_login_and_synced_rooms(&handle);

    let fixture_client = runtime
        .block_on(live_fixture_client_from_env_values(
            &env.homeserver_url,
            &env.username,
            &env.password,
        ))
        .expect("create fixture client");
    let room = runtime
        .block_on(live_room_from_env_value(&fixture_client, &env.room_id))
        .expect("resolve MMATRIX_ROOM into a joined room");
    let event_id = runtime
        .block_on(live_direct_text_message_event_id(
            &room,
            format!("mmatrix live read marker test {}", live_nonce()),
        ))
        .expect("send fixture direct text message");

    handle
        .command_sender()
        .send(Command::MarkRoomRead(MarkRoomReadRequest {
            room_id: env.room_id.clone(),
            event_id: event_id.to_string(),
        }))
        .expect("send mark room read");
    handle
        .command_sender()
        .send(Command::Logout)
        .expect("send logout");

    wait_for_live_logged_out(&handle, "live mark room read");

    let fully_read_event_id = runtime
        .block_on(live_get_fully_read_marker(&fixture_client, &room))
        .expect("query m.fully_read room account data");
    assert_eq!(fully_read_event_id, event_id);

    live_shutdown_and_join(handle);
}

#[test]
fn live_matrix_create_direct_room_metadata_timing_is_env_gated()
-> Result<(), Box<dyn Error + Send + Sync>> {
    let Some(homeserver_url) = required_env("MMATRIX_HS") else {
        eprintln!(
            "skipping live_matrix_create_direct_room_metadata_timing_is_env_gated: MMATRIX_HS is not set"
        );
        return Ok(());
    };
    let Some(username) = required_env("MMATRIX_USER") else {
        eprintln!(
            "skipping live_matrix_create_direct_room_metadata_timing_is_env_gated: MMATRIX_USER is not set"
        );
        return Ok(());
    };
    let Some(password) = required_env("MMATRIX_PASS") else {
        eprintln!(
            "skipping live_matrix_create_direct_room_metadata_timing_is_env_gated: MMATRIX_PASS is not set"
        );
        return Ok(());
    };
    let Some(target) = required_env("MMATRIX_DM_TARGET") else {
        eprintln!(
            "skipping live_matrix_create_direct_room_metadata_timing_is_env_gated: MMATRIX_DM_TARGET is not set"
        );
        return Ok(());
    };

    let (_temp_dir, handle) = live_start_core();
    live_send_login(
        &handle,
        homeserver_url,
        username,
        password,
        "mmatrix live direct room test",
    );
    wait_for_live_login_and_synced_rooms(&handle);

    handle
        .command_sender()
        .send(Command::CreateDirectRoom(CreateDirectRoomRequest {
            user_id: target.clone(),
        }))
        .expect("send create direct room");

    let first_snapshot = wait_for_live_synced_rooms(&handle, "create direct room first snapshot");
    if !has_direct_room_for_user(&first_snapshot, &target) {
        let second_snapshot =
            wait_for_live_synced_rooms(&handle, "create direct room second snapshot");
        assert!(
            has_direct_room_for_user(&second_snapshot, &target),
            "expected direct room metadata for {target} in second post-command snapshot"
        );
    }

    live_shutdown_and_join(handle);
    Ok(())
}
