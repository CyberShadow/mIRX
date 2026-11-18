use std::env;
use std::error::Error;
use std::fs;
use std::sync::mpsc::RecvTimeoutError;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use matrix_sdk::{
    Client, Room,
    config::SyncSettings,
    ruma::{
        OwnedEventId, RoomId, UInt,
        api::client::config::get_room_account_data,
        events::{
            RoomAccountDataEventType, fully_read::FullyReadEventContent,
            room::message::RoomMessageEventContent,
        },
    },
};
use mirx_core::{
    Command, CoreConfig, CoreHandle, CreateDirectRoomRequest, EditMessageRequest, Event,
    FetchRoomMediaRequest, LoginRequest, MarkRoomReadRequest, RedactEventRequest, RoomMediaFormat,
    RoomMediaKind, RoomMediaSource, RoomMessagePresentationKind, RoomSummary, SendFileRequest,
    SendMessageRequest, SendReactionRequest, SetRoomTopicRequest, StoredSession, start,
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
        homeserver_url: required_env("MIRX_HS")?,
        username: required_env("MIRX_USER")?,
        password: required_env("MIRX_PASS")?,
        room_id: required_env("MIRX_ROOM")?,
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

fn wait_for_live_room_topic(handle: &CoreHandle, room_id: &str, expected_topic: &str) {
    let deadline = Instant::now() + Duration::from_secs(60);

    while Instant::now() < deadline {
        let event = match handle.events().recv_timeout(Duration::from_secs(2)) {
            Ok(event) => event,
            Err(RecvTimeoutError::Timeout) => continue,
            Err(RecvTimeoutError::Disconnected) => {
                panic!("topic refresh event channel disconnected")
            }
        };

        match event {
            Event::SyncedRooms(rooms)
                if rooms.iter().any(|room| {
                    room.room_id == room_id && room.topic.as_deref() == Some(expected_topic)
                }) =>
            {
                return;
            }
            Event::SyncedRooms(_) => {}
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
                    "live topic refresh failed during {:?}: {}",
                    err.operation, err.message
                )
            }
            other => panic!("unexpected live topic refresh event: {other:?}"),
        }
    }

    panic!("timed out waiting for room {room_id} topic {expected_topic}");
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
        .initial_device_display_name("mirx live fixture")
        .send()
        .await?;
    Ok(client)
}

async fn live_room_from_env_value(client: &Client, room_id: &str) -> matrix_sdk::Result<Room> {
    let room_id = <&RoomId>::try_from(room_id).expect("MIRX_ROOM must be a valid Matrix room id");
    client.sync_once(SyncSettings::default()).await?;
    Ok(client
        .get_room(room_id)
        .expect("MIRX_ROOM must be visible to the fixture client after sync"))
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

async fn live_wait_for_room_event(
    client: &Client,
    room: &Room,
    nonce: u128,
    context: &str,
    predicate: impl Fn(&serde_json::Value) -> bool,
) -> serde_json::Value {
    let deadline = Instant::now() + Duration::from_secs(60);

    while Instant::now() < deadline {
        client
            .sync_once(SyncSettings::default())
            .await
            .expect("sync fixture client while waiting for room event");
        let mut options = matrix_sdk::room::MessagesOptions::backward();
        options.limit = UInt::from(50_u32);
        options.filter.limit = Some(UInt::from(50_u32));
        let response = room
            .messages(options)
            .await
            .expect("fetch room history while waiting for room event");

        for event in response.chunk {
            let value: serde_json::Value = serde_json::from_str(event.raw().json().get())
                .expect("deserialize fixture timeline event as JSON");
            if predicate(&value) {
                return value;
            }
        }
    }

    panic!("timed out waiting for {context} for nonce {nonce}");
}

fn wait_for_live_room_media_loaded(
    handle: &CoreHandle,
    activity_id: &str,
    event_id: &str,
    context: &str,
) -> Vec<u8> {
    let deadline = Instant::now() + Duration::from_secs(60);

    while Instant::now() < deadline {
        match handle
            .events()
            .recv_timeout(Duration::from_secs(2))
            .expect("room media event")
        {
            Event::RoomMedia(media)
                if media.activity_id == activity_id && media.event_id == event_id =>
            {
                match media.kind {
                    RoomMediaKind::Loaded {
                        format: RoomMediaFormat::File,
                        bytes,
                    } => return bytes,
                    other => panic!("{context} returned unexpected room media kind: {other:?}"),
                }
            }
            Event::Error(err) => panic!(
                "{context} failed during {:?}: {}",
                err.operation, err.message
            ),
            _ => {}
        }
    }

    panic!("timed out waiting for {context} room media event {event_id}");
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
    let Some(homeserver_url) = required_env("MIRX_HS") else {
        return;
    };
    let Some(username) = required_env("MIRX_USER") else {
        return;
    };
    let Some(password) = required_env("MIRX_PASS") else {
        return;
    };
    let room_id = required_env("MIRX_ROOM");

    let (_temp_dir, handle) = live_start_core();
    live_send_login(
        &handle,
        homeserver_url,
        username,
        password,
        "mirx live test",
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
            Event::Error(err) if err.operation == mirx_core::Operation::StartSync => {
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
                body: format!("mirx live test {nonce}"),
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
        "mirx live read marker test",
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
        .expect("resolve MIRX_ROOM into a joined room");
    let event_id = runtime
        .block_on(live_direct_text_message_event_id(
            &room,
            format!("mirx live read marker test {}", live_nonce()),
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
fn live_matrix_set_room_topic_refreshes_synced_room_summary_is_env_gated() {
    let Some(env) = live_matrix_env_with_room() else {
        return;
    };

    let (_temp_dir, handle) = live_start_core();
    live_send_login(
        &handle,
        env.homeserver_url,
        env.username,
        env.password,
        "mirx live topic refresh test",
    );
    wait_for_live_login_and_synced_rooms(&handle);

    let topic = format!("mirx live topic refresh {}", live_nonce());
    handle
        .command_sender()
        .send(Command::SetRoomTopic(SetRoomTopicRequest {
            room_id: env.room_id.clone(),
            topic: topic.clone(),
        }))
        .expect("send set room topic");

    wait_for_live_room_topic(&handle, &env.room_id, &topic);
    live_shutdown_and_join(handle);
}

#[test]
fn live_matrix_message_actions_round_trip_is_env_gated() {
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
        "mirx live message actions test",
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
        .expect("resolve MIRX_ROOM into a joined room");
    let nonce = live_nonce();

    let reaction_target = runtime
        .block_on(live_direct_text_message_event_id(
            &room,
            format!("mirx live reaction target {nonce}"),
        ))
        .expect("send reaction target");
    let reaction_key = "+1".to_owned();
    handle
        .command_sender()
        .send(Command::SendReaction(SendReactionRequest {
            room_id: env.room_id.clone(),
            target_event_id: reaction_target.to_string(),
            key: reaction_key.clone(),
        }))
        .expect("send reaction");
    runtime.block_on(live_wait_for_room_event(
        &fixture_client,
        &room,
        nonce,
        "reaction targeting the baseline message",
        |event| {
            event["type"] == "m.reaction"
                && event["content"]["m.relates_to"]["event_id"] == reaction_target.as_str()
                && event["content"]["m.relates_to"]["key"] == reaction_key
        },
    ));

    let edit_target = runtime
        .block_on(live_direct_text_message_event_id(
            &room,
            format!("mirx live edit target {nonce}"),
        ))
        .expect("send edit target");
    let edited_body = format!("mirx live edited body {nonce}");
    handle
        .command_sender()
        .send(Command::EditMessage(EditMessageRequest {
            room_id: env.room_id.clone(),
            target_event_id: edit_target.to_string(),
            presentation: RoomMessagePresentationKind::Text,
            body: edited_body.clone(),
        }))
        .expect("send edit message");
    runtime.block_on(live_wait_for_room_event(
        &fixture_client,
        &room,
        nonce,
        "replacement relation for the edit target",
        |event| {
            event["type"] == "m.room.message"
                && event["content"]["m.relates_to"]["rel_type"] == "m.replace"
                && event["content"]["m.relates_to"]["event_id"] == edit_target.as_str()
                && event["content"]["m.new_content"]["body"] == edited_body
        },
    ));

    let redaction_target = runtime
        .block_on(live_direct_text_message_event_id(
            &room,
            format!("mirx live redaction target {nonce}"),
        ))
        .expect("send redaction target");
    handle
        .command_sender()
        .send(Command::RedactEvent(RedactEventRequest {
            room_id: env.room_id,
            event_id: redaction_target.to_string(),
        }))
        .expect("send redaction");
    runtime.block_on(live_wait_for_room_event(
        &fixture_client,
        &room,
        nonce,
        "redaction event for the redaction target",
        |event| {
            event["type"] == "m.room.redaction"
                && (event["redacts"] == redaction_target.as_str()
                    || event["content"]["redacts"] == redaction_target.as_str())
        },
    ));

    live_shutdown_and_join(handle);
}

#[test]
fn live_matrix_file_send_and_fetch_media_round_trip_is_env_gated() {
    let Some(env) = live_matrix_env_with_room() else {
        return;
    };
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("build live fixture runtime");
    let (temp_dir, handle) = live_start_core();
    live_send_login(
        &handle,
        env.homeserver_url.clone(),
        env.username.clone(),
        env.password.clone(),
        "mirx live file media test",
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
        .expect("resolve MIRX_ROOM into a joined room");
    let nonce = live_nonce();
    let filename = format!("mirx-live-{nonce}.txt");
    let payload = format!("mirx live media test {nonce}\n").into_bytes();
    let path = temp_dir.path().join(&filename);
    fs::write(&path, &payload).expect("write live test file");

    handle
        .command_sender()
        .send(Command::SendFile(SendFileRequest {
            room_id: env.room_id.clone(),
            path,
            filename: Some(filename.clone()),
            content_type: Some("text/plain".to_owned()),
        }))
        .expect("send file");
    let file_event = runtime.block_on(live_wait_for_room_event(
        &fixture_client,
        &room,
        nonce,
        "m.file event for the uploaded file",
        |event| {
            event["type"] == "m.room.message"
                && event["content"]["msgtype"] == "m.file"
                && event["content"]["body"] == filename
        },
    ));
    let event_id = file_event["event_id"]
        .as_str()
        .expect("uploaded file event must include an event id")
        .to_owned();
    let url = file_event["content"]["url"]
        .as_str()
        .expect("uploaded file event must be unencrypted and include an MXC URL")
        .to_owned();
    let activity_id = format!("live-file-{nonce}");
    handle
        .command_sender()
        .send(Command::FetchRoomMedia(FetchRoomMediaRequest {
            room_id: env.room_id.clone(),
            activity_id: activity_id.clone(),
            event_id: event_id.clone(),
            source: RoomMediaSource::Plain { url },
            format: RoomMediaFormat::File,
        }))
        .expect("fetch uploaded room media");
    let bytes = wait_for_live_room_media_loaded(
        &handle,
        &activity_id,
        &event_id,
        "fetch uploaded room media",
    );
    assert_eq!(bytes, payload);

    live_shutdown_and_join(handle);
}

#[test]
fn live_matrix_create_direct_room_metadata_timing_is_env_gated()
-> Result<(), Box<dyn Error + Send + Sync>> {
    let Some(homeserver_url) = required_env("MIRX_HS") else {
        eprintln!(
            "skipping live_matrix_create_direct_room_metadata_timing_is_env_gated: MIRX_HS is not set"
        );
        return Ok(());
    };
    let Some(username) = required_env("MIRX_USER") else {
        eprintln!(
            "skipping live_matrix_create_direct_room_metadata_timing_is_env_gated: MIRX_USER is not set"
        );
        return Ok(());
    };
    let Some(password) = required_env("MIRX_PASS") else {
        eprintln!(
            "skipping live_matrix_create_direct_room_metadata_timing_is_env_gated: MIRX_PASS is not set"
        );
        return Ok(());
    };
    let Some(target) = required_env("MIRX_DM_TARGET") else {
        eprintln!(
            "skipping live_matrix_create_direct_room_metadata_timing_is_env_gated: MIRX_DM_TARGET is not set"
        );
        return Ok(());
    };

    let (_temp_dir, handle) = live_start_core();
    live_send_login(
        &handle,
        homeserver_url,
        username,
        password,
        "mirx live direct room test",
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
