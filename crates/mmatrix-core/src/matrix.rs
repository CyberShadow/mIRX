use std::error::Error;
use std::fmt::{self, Display, Formatter};

use matrix_sdk::{
    Client, LoopCtrl, Room, SessionMeta, SessionTokens,
    authentication::matrix::MatrixSession,
    config::SyncSettings,
    encryption::{BackupDownloadStrategy, EncryptionSettings},
    ruma::{
        OwnedDeviceId, OwnedUserId,
        events::{
            AnyMessageLikeEventContent,
            room::message::{MessageType, OriginalSyncRoomMessageEvent, RoomMessageEventContent},
        },
    },
    store::RoomLoadSettings,
};
use tokio::task::JoinHandle;

use crate::api::{
    CoreConfig, Event, LoginRequest, Operation, RoomSummary, SendTextRequest, StoredSession,
    TimelineText,
};
use crate::runtime::{EventSender, emit_event};

#[derive(Debug)]
pub(crate) struct MatrixCoreError {
    message: String,
}

impl MatrixCoreError {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl Display for MatrixCoreError {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl Error for MatrixCoreError {}

pub(crate) async fn build_client(
    config: &CoreConfig,
    homeserver_url: &str,
) -> Result<Client, MatrixCoreError> {
    let store_path = config.profile_dir.join("matrix-store");
    std::fs::create_dir_all(&store_path)
        .map_err(|err| MatrixCoreError::new(format!("failed to create store directory: {err}")))?;

    Client::builder()
        .homeserver_url(homeserver_url)
        .sqlite_store(&store_path, Some(config.store_passphrase.as_str()))
        .with_encryption_settings(encryption_settings())
        .build()
        .await
        .map_err(|err| MatrixCoreError::new(format!("failed to build Matrix client: {err}")))
}

pub(crate) fn encryption_settings() -> EncryptionSettings {
    EncryptionSettings {
        auto_enable_cross_signing: true,
        auto_enable_backups: true,
        backup_download_strategy: BackupDownloadStrategy::AfterDecryptionFailure,
    }
}

pub(crate) async fn restore_session(
    client: &Client,
    dto: StoredSession,
) -> Result<(), MatrixCoreError> {
    let session = MatrixSession {
        meta: SessionMeta {
            user_id: OwnedUserId::try_from(dto.user_id)
                .map_err(|err| MatrixCoreError::new(format!("invalid session user id: {err}")))?,
            device_id: OwnedDeviceId::try_from(dto.device_id)
                .map_err(|err| MatrixCoreError::new(format!("invalid session device id: {err}")))?,
        },
        tokens: SessionTokens {
            access_token: dto.access_token,
            refresh_token: dto.refresh_token,
        },
    };

    client
        .matrix_auth()
        .restore_session(session, RoomLoadSettings::default())
        .await
        .map_err(|err| MatrixCoreError::new(format!("failed to restore Matrix session: {err}")))
}

pub(crate) async fn login(client: &Client, request: &LoginRequest) -> Result<(), MatrixCoreError> {
    client
        .matrix_auth()
        .login_username(&request.username, &request.password)
        .initial_device_display_name(&request.device_display_name)
        .request_refresh_token()
        .send()
        .await
        .map_err(|err| MatrixCoreError::new(format!("failed to log in to Matrix: {err}")))?;

    Ok(())
}

pub(crate) async fn initial_room_sync(
    client: &Client,
) -> Result<Vec<RoomSummary>, MatrixCoreError> {
    client
        .sync_once(SyncSettings::default())
        .await
        .map_err(|err| MatrixCoreError::new(format!("initial Matrix sync failed: {err}")))?;

    summarize_joined_rooms(client).await
}

pub(crate) async fn summarize_joined_rooms(
    client: &Client,
) -> Result<Vec<RoomSummary>, MatrixCoreError> {
    let mut rooms = Vec::new();

    for room in client.joined_rooms() {
        let display_name = room.display_name().await.map_err(|err| {
            MatrixCoreError::new(format!("failed to read room display name: {err}"))
        })?;
        let encrypted = room.latest_encryption_state().await.map_err(|err| {
            MatrixCoreError::new(format!("failed to read room encryption state: {err}"))
        })?;

        rooms.push(RoomSummary {
            room_id: room.room_id().to_string(),
            display_name: display_name.to_string(),
            encrypted: encrypted.is_encrypted(),
        });
    }

    Ok(rooms)
}

pub(crate) fn register_event_handlers(client: &Client, events: EventSender) {
    client.add_event_handler(move |event: OriginalSyncRoomMessageEvent, room: Room| {
        let events = events.clone();
        async move {
            if let MessageType::Text(text) = &event.content.msgtype {
                emit_event(
                    &events,
                    Event::TimelineText(TimelineText {
                        room_id: room.room_id().to_string(),
                        body: text.body.to_string(),
                    }),
                );
            }
        }
    });
}

pub(crate) fn start_sync_task(client: Client, events: EventSender) -> JoinHandle<()> {
    tokio::task::spawn_local(async move {
        if let Err(err) = client
            .sync_with_callback(SyncSettings::default(), |_| async { LoopCtrl::Continue })
            .await
        {
            emit_event(
                &events,
                Event::Error(crate::api::CoreErrorEvent {
                    operation: Operation::StartSync,
                    message: format!("continuous Matrix sync failed: {err}"),
                }),
            );
        }
    })
}

pub(crate) async fn send_text(
    client: &Client,
    request: SendTextRequest,
) -> Result<(), MatrixCoreError> {
    let room = client
        .joined_rooms()
        .into_iter()
        .find(|room| room.room_id().as_str() == request.room_id)
        .ok_or_else(|| {
            MatrixCoreError::new(format!("joined room not found: {}", request.room_id))
        })?;

    let content = RoomMessageEventContent::text_plain(request.body);

    room.send_queue()
        .send(AnyMessageLikeEventContent::RoomMessage(content))
        .await
        .map_err(|err| MatrixCoreError::new(format!("failed to send Matrix text: {err}")))?;

    Ok(())
}

pub(crate) async fn logout(client: Client) -> Result<(), MatrixCoreError> {
    client
        .logout()
        .await
        .map_err(|err| MatrixCoreError::new(format!("failed to log out of Matrix: {err}")))
}
