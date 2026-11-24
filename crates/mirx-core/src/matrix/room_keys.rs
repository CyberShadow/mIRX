//! Messages that could not be decrypted when they arrived: their lines,
//! their late decryption by the SDK's redecryptor, and room key files.

use std::collections::BTreeSet;
use std::path::PathBuf;
use std::pin::pin;

use futures_util::StreamExt;
use matrix_sdk::deserialized_responses::{TimelineEvent, TimelineEventKind};
use matrix_sdk::encryption::RoomKeyImportError;
use matrix_sdk::event_cache::{DecryptionRetryRequest, RedecryptorReport};
use matrix_sdk::ruma::events::room::encrypted::OriginalSyncRoomEncryptedEvent;
use matrix_sdk::ruma::serde::Raw;
use matrix_sdk::ruma::{OwnedEventId, OwnedRoomId, RoomId};
use matrix_sdk::{Client, Room};
use matrix_sdk_crypto::KeyExportError;
use tokio_stream::wrappers::errors::BroadcastStreamRecvError;

use super::{
    EventBatchLookups, MatrixCoreError, emit_sync_error,
    room_history_backfill_event_from_timeline_event,
};
use crate::api::{
    CoreErrorEvent, Event, ExportRoomKeysRequest, ImportRoomKeysRequest, Operation,
    RetryDecryptionRequest, RoomEventsDecrypted, RoomHistoryBackfillEvent,
};
use crate::runtime::{EventSender, emit_event};

/// A synced event the SDK could not decrypt, as the event backfill would
/// make of it. It is decrypted again first: that gives the reason and the
/// session, which the event handler is not told; it starts the key's
/// download from the backup; and a key that came in the meantime makes it
/// the event it decrypts to.
pub(super) async fn emit_undecrypted_sync_event(
    room: &Room,
    event: Raw<OriginalSyncRoomEncryptedEvent>,
    events: &EventSender,
) {
    match undecrypted_sync_event(room, &event).await {
        Ok(mapped) => {
            for event in mapped {
                emit_event(
                    events,
                    match event {
                        RoomHistoryBackfillEvent::Activity(activity) => {
                            Event::RoomActivity(activity)
                        }
                        RoomHistoryBackfillEvent::ActivityUpdate(update) => {
                            Event::RoomActivityUpdate(*update)
                        }
                    },
                );
            }
        }
        Err(err) => emit_sync_error(events, err),
    }
}

async fn undecrypted_sync_event(
    room: &Room,
    event: &Raw<OriginalSyncRoomEncryptedEvent>,
) -> Result<Vec<RoomHistoryBackfillEvent>, MatrixCoreError> {
    let push_context = room.push_context().await.map_err(|err| {
        MatrixCoreError::new(format!(
            "failed to read the push rules of {}: {err}",
            room.room_id()
        ))
    })?;
    let event = room
        .decrypt_event(event, push_context.as_ref())
        .await
        .map_err(|err| {
            MatrixCoreError::new(format!(
                "failed to decrypt a Matrix event in {}: {err}",
                room.room_id()
            ))
        })?;
    let redaction_rules = room.clone_info().room_version_rules_or_default().redaction;
    room_history_backfill_event_from_timeline_event(
        room,
        &event,
        &redaction_rules,
        &mut EventBatchLookups::new(room),
    )
    .await
}

/// Forwards what the SDK's redecryptor reports: the events it decrypted
/// once their keys came (to-device, the key backup, a key file), as
/// `RoomEventsDecrypted`; and, when it may have missed keys or the backup
/// became available, `DecryptionRetryWanted`, since only the UI knows which
/// lines it shows. It subscribes when first polled, and then waits.
pub(crate) async fn room_decryption_task(client: Client, events: EventSender) {
    let mut reports = pin!(client.event_cache().subscribe_to_decryption_reports());
    while let Some(report) = reports.next().await {
        let event = match report {
            Ok(RedecryptorReport::ResolvedUtds { room_id, events }) => {
                match room_events_decrypted(&client, &room_id, events).await {
                    Ok(Some(decrypted)) => Event::RoomEventsDecrypted(decrypted),
                    Ok(None) => continue,
                    Err(err) => Event::Error(CoreErrorEvent {
                        operation: Operation::RetryDecryption,
                        room_id: Some(room_id.to_string()),
                        message: err.to_string(),
                    }),
                }
            }
            Ok(RedecryptorReport::BackupAvailable | RedecryptorReport::Lagging)
            | Err(BroadcastStreamRecvError::Lagged(_)) => Event::DecryptionRetryWanted,
        };
        if events.send(event).is_err() {
            return;
        }
    }
}

/// The reported events of a room, as backfill would map them, oldest
/// first; `None` when none is left to report.
async fn room_events_decrypted(
    client: &Client,
    room_id: &RoomId,
    event_ids: BTreeSet<OwnedEventId>,
) -> Result<Option<RoomEventsDecrypted>, MatrixCoreError> {
    // The UI shows no room the client does not know.
    let Some(room) = client.get_room(room_id) else {
        return Ok(None);
    };
    let (cache, _handles) = room.event_cache().await.map_err(|err| {
        MatrixCoreError::new(format!(
            "failed to open the event cache of {room_id}: {err}"
        ))
    })?;
    let mut decrypted: Vec<TimelineEvent> = Vec::with_capacity(event_ids.len());
    for event_id in &event_ids {
        let event = cache.find_event(event_id).await.map_err(|err| {
            MatrixCoreError::new(format!("failed to read {event_id} in {room_id}: {err}"))
        })?;
        // The redecryptor reports every event it decrypted, also one it
        // then found in neither the room's events nor the store, which it
        // therefore did not replace (`on_resolved_utds` in matrix-sdk's
        // `event_cache/redecryptor.rs`). Such an event is still the one
        // that could not be decrypted, or missing: nothing to report.
        if let Some(event) =
            event.filter(|event| !matches!(event.kind, TimelineEventKind::UnableToDecrypt { .. }))
        {
            decrypted.push(event);
        }
    }
    decrypted.sort_by_key(|event| event.timestamp());

    let redaction_rules = room.clone_info().room_version_rules_or_default().redaction;
    let mut lookups = EventBatchLookups::new(&room);
    let mut events = Vec::with_capacity(decrypted.len());
    for event in &decrypted {
        events.extend(
            room_history_backfill_event_from_timeline_event(
                &room,
                event,
                &redaction_rules,
                &mut lookups,
            )
            .await?,
        );
    }
    Ok((!events.is_empty()).then(|| RoomEventsDecrypted {
        room_id: room_id.to_string(),
        events,
    }))
}

/// Reads again the room's events the UI shows that could not be decrypted,
/// as they are now: each still in the event cache (no network), oldest
/// first, decrypted or still undecryptable with its current cause; `None`
/// when none is. Their sessions also go to the SDK's redecryptor, which
/// tries the room's other events of them (edits, reactions), and decrypting
/// again asks the key backup for a missing key. An event the cache has
/// decrypted already, the redecryptor reported (`room_decryption_task`).
pub(crate) async fn retry_decryption(
    client: &Client,
    request: RetryDecryptionRequest,
) -> Result<Option<RoomEventsDecrypted>, MatrixCoreError> {
    let room_id = OwnedRoomId::try_from(request.room_id.as_str()).map_err(|err| {
        MatrixCoreError::new(format!("invalid Matrix room ID {}: {err}", request.room_id))
    })?;
    if request.event_ids.is_empty() {
        return Err(MatrixCoreError::new(
            "retrying decryption needs at least one event",
        ));
    }
    let event_ids = request
        .event_ids
        .iter()
        .map(|event_id| {
            OwnedEventId::try_from(event_id.as_str()).map_err(|err| {
                MatrixCoreError::new(format!("invalid Matrix event ID {event_id}: {err}"))
            })
        })
        .collect::<Result<Vec<_>, _>>()?;
    let room = client
        .get_room(&room_id)
        .ok_or_else(|| MatrixCoreError::new(format!("unknown Matrix room: {room_id}")))?;
    let (cache, _handles) = room.event_cache().await.map_err(|err| {
        MatrixCoreError::new(format!(
            "failed to open the event cache of {room_id}: {err}"
        ))
    })?;
    let mut undecrypted: Vec<TimelineEvent> = Vec::with_capacity(event_ids.len());
    let mut utd_session_ids = BTreeSet::new();
    for event_id in &event_ids {
        let event = cache.find_event(event_id).await.map_err(|err| {
            MatrixCoreError::new(format!("failed to read {event_id} in {room_id}: {err}"))
        })?;
        if let Some(event) = event
            && let TimelineEventKind::UnableToDecrypt { utd_info, .. } = &event.kind
        {
            // An Olm event has no session, but can be read again.
            utd_session_ids.extend(utd_info.session_id.clone());
            undecrypted.push(event);
        }
    }
    undecrypted.sort_by_key(|event| event.timestamp());

    client
        .event_cache()
        .request_decryption(DecryptionRetryRequest {
            room_id: room_id.clone(),
            utd_session_ids,
            refresh_info_session_ids: BTreeSet::new(),
        });
    // The cache keeps the failure it first stored (the redecryptor does not
    // refresh it when a retry fails too): decrypt each again for its
    // current one.
    let mut events = Vec::with_capacity(undecrypted.len());
    for event in &undecrypted {
        events.extend(undecrypted_sync_event(&room, event.raw().cast_ref_unchecked()).await?);
    }
    Ok((!events.is_empty()).then(|| RoomEventsDecrypted {
        room_id: room_id.to_string(),
        events,
    }))
}

/// Writes every room key the session holds to `path`, encrypted with the
/// passphrase (the SDK runs the key derivation off the runtime).
pub(crate) async fn export_room_keys(
    client: &Client,
    request: &ExportRoomKeysRequest,
) -> Result<(), MatrixCoreError> {
    refuse_empty_passphrase(&request.passphrase)?;
    client
        .encryption()
        .export_room_keys(PathBuf::from(&request.path), &request.passphrase, |_| true)
        .await
        .map_err(|err| MatrixCoreError::new(err.to_string()))
}

/// Reads the room keys of the file at `path`; returns how many were new
/// and how many it held. The redecryptor decrypts what they unlock.
pub(crate) async fn import_room_keys(
    client: &Client,
    request: &ImportRoomKeysRequest,
) -> Result<(u64, u64), MatrixCoreError> {
    refuse_empty_passphrase(&request.passphrase)?;
    let result = client
        .encryption()
        .import_room_keys(PathBuf::from(&request.path), &request.passphrase)
        .await
        .map_err(|err| MatrixCoreError::new(room_key_import_error_text(err)))?;
    Ok((
        u64::try_from(result.imported_count).expect("a count fits in u64"),
        u64::try_from(result.total_count).expect("a count fits in u64"),
    ))
}

fn refuse_empty_passphrase(passphrase: &str) -> Result<(), MatrixCoreError> {
    if passphrase.is_empty() {
        return Err(MatrixCoreError::new("a room key file needs a passphrase"));
    }
    Ok(())
}

/// What went wrong, as the user can act on it: the file's MAC fails for a
/// wrong passphrase as for a damaged file, and anything unreadable after
/// decryption is no room key file.
fn room_key_import_error_text(err: RoomKeyImportError) -> String {
    match err {
        RoomKeyImportError::Export(KeyExportError::InvalidMac) => {
            "wrong passphrase, or the file is damaged".to_owned()
        }
        RoomKeyImportError::Export(
            KeyExportError::InvalidHeaders
            | KeyExportError::UnsupportedVersion
            | KeyExportError::InvalidUtf8(_)
            | KeyExportError::Json(_)
            | KeyExportError::Decode(_),
        ) => "not a room key file".to_owned(),
        RoomKeyImportError::Export(KeyExportError::Io(err)) | RoomKeyImportError::Io(err) => {
            err.to_string()
        }
        err @ (RoomKeyImportError::SerdeJson(_)
        | RoomKeyImportError::StoreClosed
        | RoomKeyImportError::CryptoStore(_)) => err.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use std::sync::mpsc;
    use std::time::Duration;

    use matrix_sdk::Client;
    use matrix_sdk::ruma::events::{AnySyncTimelineEvent, AnyTimelineEvent};
    use matrix_sdk::ruma::serde::Raw;
    use matrix_sdk::ruma::{OwnedRoomId, RoomVersionId, owned_room_id};
    use matrix_sdk::test_utils::mocks::{MatrixMockServer, RoomMessagesResponseTemplate};
    use matrix_sdk_base::crypto::types::events::room::encrypted::EncryptedToDeviceEvent;
    use matrix_sdk_test::JoinedRoomBuilder;
    use matrix_sdk_test::event_factory::EventFactory;

    use super::super::{backfill_room_history, register_event_handlers, verification_watch};
    use super::{import_room_keys, retry_decryption, room_decryption_task};
    use crate::api::{
        BackfillRoomHistoryRequest, Event, HistoryFrom, ImportRoomKeysRequest,
        RetryDecryptionRequest, RoomActivity, RoomActivityKind, RoomActivityRelation,
        RoomActivityUpdate, RoomActivityUpdateKind, RoomActivityUpdateTarget, RoomEventsDecrypted,
        RoomHistoryBackfillEvent, RoomUnableToDecryptCause,
    };
    use crate::runtime::RoomSnapshots;

    fn block_on<F: std::future::Future>(future: F) -> F::Output {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("build tokio runtime")
            .block_on(future)
    }

    /// An encrypted room of alice, an SDK client that sends, and bob, a
    /// client with mirx's event handlers.
    struct EncryptedRoom {
        server: MatrixMockServer,
        alice: Client,
        bob: Client,
        events: mpsc::Receiver<Event>,
        room_id: OwnedRoomId,
    }

    /// An encrypted event alice sent, and the to-device message that gives
    /// bob its room key.
    struct Sent {
        event: Raw<AnySyncTimelineEvent>,
        room_key: Raw<EncryptedToDeviceEvent>,
    }

    impl EncryptedRoom {
        async fn new() -> Self {
            let server = MatrixMockServer::new().await;
            server.mock_crypto_endpoints_preset().await;
            let (alice, bob) = server.set_up_alice_and_bob_for_encryption().await;
            // As the session does before its first sync.
            bob.event_cache().subscribe().expect("the client is alive");
            let (events_tx, events) = mpsc::channel();
            let (verifications, _) = verification_watch();
            register_event_handlers(
                &bob,
                events_tx.clone(),
                &RoomSnapshots::default(),
                &verifications,
            );
            // As the session does: subscribed before the room's first sync.
            let mut decryption = Box::pin(room_decryption_task(bob.clone(), events_tx));
            assert!(futures_util::poll!(decryption.as_mut()).is_pending());
            tokio::spawn(decryption);

            let room_id = owned_room_id!("!secret:example.org");
            let alice_id = alice.user_id().expect("alice is signed in").to_owned();
            let bob_id = bob.user_id().expect("bob is signed in").to_owned();
            let factory = EventFactory::new().room(&room_id).sender(&alice_id);
            let room = JoinedRoomBuilder::new(&room_id)
                .add_state_event(factory.create(&alice_id, RoomVersionId::V10))
                .add_state_event(factory.room_encryption())
                .add_state_event(factory.member(&alice_id).display_name("Alice"))
                .add_state_event(factory.member(&bob_id).sender(&bob_id));
            for client in [&alice, &bob] {
                server
                    .mock_sync()
                    .ok_and_run(client, |builder| {
                        builder.add_joined_room(room.clone());
                    })
                    .await;
            }
            let room = Self {
                server,
                alice,
                bob,
                events,
                room_id,
            };
            room.drain();
            room
        }

        /// Alice's first message in the room: `content` as `event_type`,
        /// encrypted, as `event_id`. It starts her room key, which she
        /// shares with bob.
        async fn alice_sends_first(
            &self,
            event_id: &str,
            event_type: &str,
            content: serde_json::Value,
        ) -> Sent {
            let alice_id = self.alice.user_id().expect("alice is signed in");
            let (_to_device, room_key) = self.server.mock_capture_put_to_device(alice_id).await;
            let factory = EventFactory::new().room(&self.room_id);
            self.server
                .mock_get_members()
                .ok(vec![
                    factory.member(alice_id).sender(alice_id).into_raw(),
                    factory
                        .member(self.bob.user_id().expect("bob is signed in"))
                        .into_raw(),
                ])
                .mock_once()
                .mount()
                .await;
            let event = self.alice_sends(event_id, event_type, content).await;
            Sent {
                event,
                room_key: room_key.await,
            }
        }

        /// Alice sends again, with the room key she already shared.
        async fn alice_sends(
            &self,
            event_id: &str,
            event_type: &str,
            content: serde_json::Value,
        ) -> Raw<AnySyncTimelineEvent> {
            let alice_id = self.alice.user_id().expect("alice is signed in");
            let event_id = matrix_sdk::ruma::OwnedEventId::try_from(event_id).expect("event id");
            let (event, send) = self
                .server
                .mock_room_send()
                .ok_with_capture(event_id, alice_id);
            let _send = send.mock_once().mount_as_scoped().await;
            self.alice
                .get_room(&self.room_id)
                .expect("alice is in the room")
                .send_raw(event_type, content)
                .await
                .expect("alice sends");
            event.await.expect("the sent event")
        }

        /// An event alice encrypts herself with the room key she already
        /// shared, as clients do that encrypt what the SDK sends in the
        /// clear (Element's reactions).
        async fn alice_encrypts(
            &self,
            event_id: &str,
            event_type: &str,
            content: serde_json::Value,
        ) -> Raw<AnySyncTimelineEvent> {
            let machine = self.alice.olm_machine_for_testing().await;
            let encrypted = machine
                .as_ref()
                .expect("alice has E2EE")
                .encrypt_room_event_raw(
                    &self.room_id,
                    event_type,
                    &Raw::new(&content).expect("content").cast_unchecked(),
                )
                .await
                .expect("alice encrypts");
            Raw::new(&serde_json::json!({
                "type": "m.room.encrypted",
                "event_id": event_id,
                "sender": self.alice.user_id().expect("alice is signed in"),
                "origin_server_ts": matrix_sdk::ruma::MilliSecondsSinceUnixEpoch::now(),
                "content": encrypted.content,
            }))
            .expect("an encrypted event")
            .cast_unchecked()
        }

        /// Bob syncs `events` in the room, after `room_keys` as to-device
        /// messages.
        async fn bob_syncs(
            &self,
            room_keys: &[&Raw<EncryptedToDeviceEvent>],
            events: &[&Raw<AnySyncTimelineEvent>],
        ) {
            self.server
                .mock_sync()
                .ok_and_run(&self.bob, |builder| {
                    for room_key in room_keys {
                        builder.add_to_device_event(
                            room_key.deserialize_as().expect("a to-device event"),
                        );
                    }
                    let mut room = JoinedRoomBuilder::new(&self.room_id);
                    for event in events {
                        room = room.add_timeline_event((*event).clone());
                    }
                    builder.add_joined_room(room);
                })
                .await;
        }

        /// Waits for the next `RoomEventsDecrypted`, and returns it with the
        /// events before it. It comes from the decryption task, which runs
        /// while this waits.
        async fn decrypted(&self) -> (RoomEventsDecrypted, Vec<Event>) {
            let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
            let mut before = Vec::new();
            loop {
                match self.events.try_recv() {
                    Ok(Event::RoomEventsDecrypted(decrypted)) => return (decrypted, before),
                    Ok(event) => before.push(event),
                    Err(mpsc::TryRecvError::Empty) => {
                        assert!(
                            tokio::time::Instant::now() < deadline,
                            "no RoomEventsDecrypted after {before:#?}"
                        );
                        tokio::time::sleep(Duration::from_millis(10)).await;
                    }
                    Err(err) => panic!("no RoomEventsDecrypted ({err}) after {before:#?}"),
                }
            }
        }

        /// Bob syncs alice's notice that she withholds the key of `sent`
        /// from him, with `code` (`m.unverified`, `m.blacklisted`).
        async fn bob_syncs_withheld(&self, sent: &Raw<AnySyncTimelineEvent>, code: &str) {
            let alice_id = self.alice.user_id().expect("alice is signed in");
            let sender_key = self
                .alice
                .encryption()
                .curve25519_key()
                .await
                .expect("alice has E2EE");
            let withheld = serde_json::json!({
                "type": "m.room_key.withheld",
                "sender": alice_id,
                "content": {
                    "algorithm": "m.megolm.v1.aes-sha2",
                    "code": code,
                    "reason": "withheld",
                    "room_id": self.room_id,
                    "session_id": session_id(sent),
                    "sender_key": sender_key.to_base64(),
                    "from_device": self.alice.device_id().expect("alice has a device"),
                },
            });
            self.server
                .mock_sync()
                .ok_and_run(&self.bob, |builder| {
                    builder.add_to_device_event(withheld);
                })
                .await;
        }

        /// The server's answer to bob's query whether he has a key backup.
        async fn mount_backup_exists(&self, exists: bool) {
            let version = self
                .server
                .mock_room_keys_version()
                .expect_any_access_token();
            match exists {
                true => version.exists(),
                false => version.none(),
            }
            .mount()
            .await;
        }

        /// The one line bob shows after a sync, which could not be
        /// decrypted, and why.
        fn utd_cause(&self) -> RoomUnableToDecryptCause {
            let events = self.drain();
            let activities = activities(&events);
            let [activity] = activities[..] else {
                panic!("not one line: {events:#?}");
            };
            let RoomActivityKind::UnableToDecrypt(utd) = &activity.kind else {
                panic!("not a UTD: {activity:#?}");
            };
            utd.cause
        }

        fn drain(&self) -> Vec<Event> {
            std::iter::from_fn(|| self.events.recv_timeout(Duration::from_millis(300)).ok())
                .collect()
        }
    }

    fn activities(events: &[Event]) -> Vec<&RoomActivity> {
        events
            .iter()
            .filter_map(|event| match event {
                Event::RoomActivity(activity) => Some(&**activity),
                _ => None,
            })
            .collect()
    }

    fn session_id(event: &Raw<AnySyncTimelineEvent>) -> String {
        event
            .get_field::<serde_json::Value>("content")
            .expect("content")
            .expect("content")["session_id"]
            .as_str()
            .expect("a megolm session id")
            .to_owned()
    }

    /// `event` with `field` set to `value`, as the server may change it:
    /// Megolm encrypts only the content.
    fn with_field(
        event: &Raw<AnySyncTimelineEvent>,
        field: &str,
        value: serde_json::Value,
    ) -> Raw<AnySyncTimelineEvent> {
        let mut json: serde_json::Value = event.deserialize_as().expect("json");
        json[field] = value;
        Raw::new(&json).expect("an event").cast_unchecked()
    }

    fn text(body: &str) -> serde_json::Value {
        serde_json::json!({ "msgtype": "m.text", "body": body })
    }

    #[test]
    fn a_message_decrypted_at_sync_is_one_message_line() {
        block_on(async {
            let room = EncryptedRoom::new().await;
            let sent = room
                .alice_sends_first("$hello", "m.room.message", text("hello"))
                .await;

            room.bob_syncs(&[&sent.room_key], &[&sent.event]).await;

            let events = room.drain();
            let activities = activities(&events);
            assert_eq!(activities.len(), 1, "{events:#?}");
            let RoomActivityKind::Message(message) = &activities[0].kind else {
                panic!("not a message: {activities:#?}");
            };
            assert_eq!(message.body, "hello");
            assert_eq!(activities[0].activity_id, "event:$hello");
        });
    }

    #[test]
    fn a_message_without_its_key_is_a_line_that_could_not_be_decrypted() {
        block_on(async {
            let room = EncryptedRoom::new().await;
            let sent = room
                .alice_sends_first("$secret", "m.room.message", text("psst"))
                .await;

            room.bob_syncs(&[], &[&sent.event]).await;

            let events = room.drain();
            let activities = activities(&events);
            assert_eq!(activities.len(), 1, "{events:#?}");
            assert_eq!(activities[0].activity_id, "event:$secret");
            assert_eq!(activities[0].relation, None);
            assert!(!activities[0].highlight);
            let RoomActivityKind::UnableToDecrypt(utd) = &activities[0].kind else {
                panic!("not a UTD: {activities:#?}");
            };
            assert_eq!(utd.event_id, "$secret");
            assert_eq!(utd.sender.user_id, "@alice:example.org");
            assert_eq!(utd.sender.nick.as_deref(), Some("Alice"));
            // Sent after this session was made, while bob was in the room:
            // the key may yet come.
            assert_eq!(utd.cause, RoomUnableToDecryptCause::Unknown);
            let session_id = sent
                .event
                .get_field::<serde_json::Value>("content")
                .expect("content")
                .expect("content")["session_id"]
                .as_str()
                .expect("a megolm session id")
                .to_owned();
            assert_eq!(utd.session_id.as_deref(), Some(session_id.as_str()));
        });
    }

    /// The server marks an event sent while bob was not in the room
    /// (MSC4115, also under its unstable name): no key can be expected.
    #[test]
    fn a_message_sent_before_you_joined_says_so() {
        block_on(async {
            let room = EncryptedRoom::new().await;
            let sent = room
                .alice_sends_first("$before", "m.room.message", text("psst"))
                .await;
            for membership in ["membership", "io.element.msc4115.membership"] {
                let event = with_field(
                    &sent.event,
                    "unsigned",
                    serde_json::json!({ membership: "leave" }),
                );

                room.bob_syncs(&[], &[&event]).await;

                assert_eq!(
                    room.utd_cause(),
                    RoomUnableToDecryptCause::SentBeforeYouJoined,
                    "{membership}"
                );
            }
        });
    }

    #[test]
    fn a_message_older_than_this_session_without_a_backup_says_so() {
        block_on(async {
            let room = EncryptedRoom::new().await;
            room.mount_backup_exists(false).await;
            let sent = room
                .alice_sends_first("$old", "m.room.message", text("psst"))
                .await;
            let event = with_field(&sent.event, "origin_server_ts", serde_json::json!(1000));

            room.bob_syncs(&[], &[&event]).await;

            assert_eq!(
                room.utd_cause(),
                RoomUnableToDecryptCause::HistoricalNoBackup
            );
        });
    }

    /// Bob's session is not verified, so it cannot use the backup the
    /// server has: verifying it would read the message.
    #[test]
    fn a_message_older_than_this_session_with_a_backup_says_verify() {
        block_on(async {
            let room = EncryptedRoom::new().await;
            room.mount_backup_exists(true).await;
            let sent = room
                .alice_sends_first("$old", "m.room.message", text("psst"))
                .await;
            let event = with_field(&sent.event, "origin_server_ts", serde_json::json!(1000));

            room.bob_syncs(&[], &[&event]).await;

            assert_eq!(
                room.utd_cause(),
                RoomUnableToDecryptCause::HistoricalUnverified
            );
        });
    }

    #[test]
    fn a_withheld_key_says_the_sender_withheld_it() {
        block_on(async {
            for (code, cause) in [
                ("m.unverified", RoomUnableToDecryptCause::WithheldUnverified),
                ("m.blacklisted", RoomUnableToDecryptCause::WithheldBySender),
            ] {
                let room = EncryptedRoom::new().await;
                let sent = room
                    .alice_sends_first("$withheld", "m.room.message", text("psst"))
                    .await;
                room.bob_syncs_withheld(&sent.event, code).await;

                room.bob_syncs(&[], &[&sent.event]).await;

                assert_eq!(room.utd_cause(), cause, "{code}");
            }
        });
    }

    #[test]
    fn a_thread_message_without_its_key_shows_in_its_thread() {
        block_on(async {
            let room = EncryptedRoom::new().await;
            let sent = room
                .alice_sends_first(
                    "$in-thread",
                    "m.room.message",
                    serde_json::json!({
                        "msgtype": "m.text",
                        "body": "in the thread",
                        "m.relates_to": {
                            "rel_type": "m.thread",
                            "event_id": "$root",
                            "is_falling_back": true,
                            "m.in_reply_to": { "event_id": "$root" },
                        },
                    }),
                )
                .await;

            room.bob_syncs(&[], &[&sent.event]).await;

            let events = room.drain();
            let activities = activities(&events);
            assert_eq!(activities.len(), 1, "{events:#?}");
            assert!(matches!(
                activities[0].kind,
                RoomActivityKind::UnableToDecrypt(_)
            ));
            assert_eq!(
                activities[0].relation,
                Some(RoomActivityRelation::Thread {
                    thread_root_event_id: "$root".to_owned(),
                    in_reply_to_event_id: Some("$root".to_owned()),
                    is_falling_back: true,
                })
            );
        });
    }

    #[test]
    fn an_edit_or_a_reaction_without_its_key_is_no_line() {
        block_on(async {
            let room = EncryptedRoom::new().await;
            let edit = room
                .alice_sends_first(
                    "$edit",
                    "m.room.message",
                    serde_json::json!({
                        "msgtype": "m.text",
                        "body": "* fixed",
                        "m.new_content": { "msgtype": "m.text", "body": "fixed" },
                        "m.relates_to": { "rel_type": "m.replace", "event_id": "$hello" },
                    }),
                )
                .await;
            let reaction = room
                .alice_encrypts(
                    "$reaction",
                    "m.reaction",
                    serde_json::json!({
                        "m.relates_to": {
                            "rel_type": "m.annotation",
                            "event_id": "$hello",
                            "key": "+1",
                        },
                    }),
                )
                .await;

            room.bob_syncs(&[], &[&edit.event, &reaction]).await;

            let events = room.drain();
            assert!(
                !events.iter().any(|event| matches!(
                    event,
                    Event::RoomActivity(_) | Event::RoomActivityUpdate(_)
                )),
                "{events:#?}"
            );
        });
    }

    fn decrypted_message(event: &RoomHistoryBackfillEvent) -> (&RoomActivity, &str) {
        let RoomHistoryBackfillEvent::Activity(activity) = event else {
            panic!("not a line: {event:#?}");
        };
        let RoomActivityKind::Message(message) = &activity.kind else {
            panic!("not a message: {activity:#?}");
        };
        (activity, message.body.as_str())
    }

    #[test]
    fn a_line_that_could_not_be_decrypted_decrypts_when_its_key_comes() {
        block_on(async {
            let room = EncryptedRoom::new().await;
            let sent = room
                .alice_sends_first("$late", "m.room.message", text("late"))
                .await;
            room.bob_syncs(&[], &[&sent.event]).await;
            let utd = room.drain();
            assert!(
                matches!(
                    activities(&utd)[..],
                    [RoomActivity {
                        kind: RoomActivityKind::UnableToDecrypt(_),
                        ..
                    }]
                ),
                "{utd:#?}"
            );

            room.bob_syncs(&[&sent.room_key], &[]).await;

            let (decrypted, before) = room.decrypted().await;
            assert!(activities(&before).is_empty(), "{before:#?}");
            assert_eq!(decrypted.room_id, "!secret:example.org");
            assert_eq!(decrypted.events.len(), 1, "{decrypted:#?}");
            let (activity, body) = decrypted_message(&decrypted.events[0]);
            assert_eq!(body, "late");
            assert_eq!(activity.activity_id, "event:$late");
            assert_eq!(activity.activity_id, activities(&utd)[0].activity_id);
        });
    }

    /// Reactions go in the clear (the SDK's are): one can show before
    /// its message decrypts, which no line shows yet. It comes from the
    /// event cache with the decrypted line.
    #[test]
    fn a_late_decrypted_message_brings_its_cached_reactions() {
        block_on(async {
            let room = EncryptedRoom::new().await;
            let sent = room
                .alice_sends_first("$late", "m.room.message", text("late"))
                .await;
            let reaction: Raw<AnySyncTimelineEvent> = Raw::new(&serde_json::json!({
                "type": "m.reaction",
                "content": {
                    "m.relates_to": { "rel_type": "m.annotation", "event_id": "$late", "key": "👍" }
                },
                "event_id": "$react",
                "sender": room.alice.user_id().expect("alice is signed in"),
                "origin_server_ts": 1700000000500u64
            }))
            .expect("a reaction")
            .cast_unchecked();
            room.bob_syncs(&[], &[&sent.event, &reaction]).await;
            room.drain();

            room.bob_syncs(&[&sent.room_key], &[]).await;

            let (decrypted, _) = room.decrypted().await;
            assert_eq!(decrypted.events.len(), 2, "{decrypted:#?}");
            assert_eq!(decrypted_message(&decrypted.events[0]).1, "late");
            assert!(
                matches!(
                    &decrypted.events[1],
                    RoomHistoryBackfillEvent::ActivityUpdate(update) if matches!(
                        &**update,
                        RoomActivityUpdate {
                            target: RoomActivityUpdateTarget::Event { event_id },
                            kind: RoomActivityUpdateKind::ReactionAdded { reaction_event_id, .. },
                            ..
                        } if event_id == "$late" && reaction_event_id == "$react"
                    )
                ),
                "{decrypted:#?}"
            );
        });
    }

    #[test]
    fn a_backfilled_line_that_could_not_be_decrypted_decrypts_when_its_key_comes() {
        block_on(async {
            let room = EncryptedRoom::new().await;
            let sent = room
                .alice_sends_first("$old", "m.room.message", text("old"))
                .await;
            let mut history: serde_json::Value = sent.event.deserialize_as().expect("json");
            history["room_id"] = serde_json::json!(room.room_id.as_str());
            room.server
                .mock_room_messages()
                .expect_any_access_token()
                .ok(RoomMessagesResponseTemplate::default().events(vec![
                    Raw::<AnyTimelineEvent>::from_json_string(history.to_string())
                        .expect("a room event"),
                ]))
                .mock_once()
                .mount()
                .await;
            let backfill = backfill_room_history(
                &room.bob,
                BackfillRoomHistoryRequest {
                    room_id: room.room_id.to_string(),
                    from: HistoryFrom::Latest,
                    limit: 10,
                },
            )
            .await
            .expect("backfill");
            assert!(
                matches!(
                    &backfill.events[..],
                    [RoomHistoryBackfillEvent::Activity(activity)]
                        if matches!(activity.kind, RoomActivityKind::UnableToDecrypt(_))
                ),
                "{backfill:#?}"
            );

            room.bob_syncs(&[&sent.room_key], &[]).await;

            let (decrypted, _) = room.decrypted().await;
            assert_eq!(decrypted.events.len(), 1, "{decrypted:#?}");
            let (activity, body) = decrypted_message(&decrypted.events[0]);
            assert_eq!(body, "old");
            assert_eq!(activity.activity_id, "event:$old");
        });
    }

    /// A page of history reads this session's context for its lines that
    /// could not be decrypted, as a synced line does.
    #[test]
    fn a_backfilled_message_without_its_key_has_its_cause() {
        block_on(async {
            let room = EncryptedRoom::new().await;
            let sent = room
                .alice_sends_first("$before", "m.room.message", text("psst"))
                .await;
            let mut history: serde_json::Value = sent.event.deserialize_as().expect("json");
            history["room_id"] = serde_json::json!(room.room_id.as_str());
            history["unsigned"] = serde_json::json!({ "membership": "leave" });
            room.server
                .mock_room_messages()
                .expect_any_access_token()
                .ok(RoomMessagesResponseTemplate::default().events(vec![
                    Raw::<AnyTimelineEvent>::from_json_string(history.to_string())
                        .expect("a room event"),
                ]))
                .mock_once()
                .mount()
                .await;

            let backfill = backfill_room_history(
                &room.bob,
                BackfillRoomHistoryRequest {
                    room_id: room.room_id.to_string(),
                    from: HistoryFrom::Latest,
                    limit: 10,
                },
            )
            .await
            .expect("backfill");

            let [RoomHistoryBackfillEvent::Activity(activity)] = &backfill.events[..] else {
                panic!("not one line: {backfill:#?}");
            };
            let RoomActivityKind::UnableToDecrypt(utd) = &activity.kind else {
                panic!("not a UTD: {activity:#?}");
            };
            assert_eq!(utd.cause, RoomUnableToDecryptCause::SentBeforeYouJoined);
        });
    }

    /// The server bundles a message's latest edit with it, encrypted as
    /// sent; the SDK decrypts the bundled edit with the message.
    #[test]
    fn a_backfilled_message_brings_its_decrypted_bundled_edit() {
        block_on(async {
            let room = EncryptedRoom::new().await;
            let typo = room
                .alice_sends_first("$typo", "m.room.message", text("tpyo"))
                .await;
            let fix = room
                .alice_sends(
                    "$fix",
                    "m.room.message",
                    serde_json::json!({
                        "msgtype": "m.text",
                        "body": "* typo",
                        "m.new_content": { "msgtype": "m.text", "body": "typo" },
                        "m.relates_to": { "rel_type": "m.replace", "event_id": "$typo" },
                    }),
                )
                .await;
            room.bob_syncs(&[&typo.room_key], &[]).await;
            room.drain();
            let room_id = serde_json::json!(room.room_id.as_str());
            let mut bundled: serde_json::Value = fix.deserialize_as().expect("json");
            bundled["room_id"] = room_id.clone();
            let mut history: serde_json::Value = typo.event.deserialize_as().expect("json");
            history["room_id"] = room_id;
            history["unsigned"] = serde_json::json!({ "m.relations": { "m.replace": bundled } });
            room.server
                .mock_room_messages()
                .expect_any_access_token()
                .ok(RoomMessagesResponseTemplate::default().events(vec![
                    Raw::<AnyTimelineEvent>::from_json_string(history.to_string())
                        .expect("a room event"),
                ]))
                .mock_once()
                .mount()
                .await;

            let backfill = backfill_room_history(
                &room.bob,
                BackfillRoomHistoryRequest {
                    room_id: room.room_id.to_string(),
                    from: HistoryFrom::Latest,
                    limit: 10,
                },
            )
            .await
            .expect("backfill");

            let [line, RoomHistoryBackfillEvent::ActivityUpdate(edit)] = &backfill.events[..]
            else {
                panic!("not a line and its edit: {backfill:#?}");
            };
            assert_eq!(decrypted_message(line).1, "tpyo");
            assert_eq!(
                edit.target,
                RoomActivityUpdateTarget::Event {
                    event_id: "$typo".to_owned()
                }
            );
            let RoomActivityUpdateKind::MessageReplaced {
                replacement_event_id,
                sender,
                body,
                ..
            } = &edit.kind
            else {
                panic!("not an edit: {edit:#?}");
            };
            assert_eq!(
                (
                    replacement_event_id.as_str(),
                    sender.as_str(),
                    body.as_str()
                ),
                ("$fix", "@alice:example.org", "typo")
            );
        });
    }

    #[test]
    fn an_edit_that_could_not_be_decrypted_applies_when_its_key_comes() {
        block_on(async {
            let room = EncryptedRoom::new().await;
            let edit = room
                .alice_sends_first(
                    "$edit",
                    "m.room.message",
                    serde_json::json!({
                        "msgtype": "m.text",
                        "body": "* fixed",
                        "m.new_content": { "msgtype": "m.text", "body": "fixed" },
                        "m.relates_to": { "rel_type": "m.replace", "event_id": "$typo" },
                    }),
                )
                .await;
            let reaction = room
                .alice_encrypts(
                    "$reaction",
                    "m.reaction",
                    serde_json::json!({
                        "m.relates_to": {
                            "rel_type": "m.annotation",
                            "event_id": "$typo",
                            "key": "+1",
                        },
                    }),
                )
                .await;
            room.bob_syncs(&[], &[&edit.event, &reaction]).await;
            room.drain();

            room.bob_syncs(&[&edit.room_key], &[]).await;

            let (decrypted, _) = room.decrypted().await;
            let kinds = decrypted
                .events
                .iter()
                .map(|event| match event {
                    RoomHistoryBackfillEvent::ActivityUpdate(update) => {
                        assert_eq!(
                            update.target,
                            RoomActivityUpdateTarget::Event {
                                event_id: "$typo".to_owned()
                            }
                        );
                        &update.kind
                    }
                    RoomHistoryBackfillEvent::Activity(activity) => {
                        panic!("an edit or a reaction is no line: {activity:#?}")
                    }
                })
                .collect::<Vec<_>>();
            assert!(
                matches!(
                    kinds[..],
                    [
                        RoomActivityUpdateKind::MessageReplaced { .. },
                        RoomActivityUpdateKind::ReactionAdded { .. }
                    ]
                ),
                "{kinds:#?}"
            );
            let RoomActivityUpdateKind::MessageReplaced { body, .. } = kinds[0] else {
                unreachable!();
            };
            assert_eq!(body, "fixed");
        });
    }

    /// A reaction redacted before bob had its key is gone from his store:
    /// the SDK's event cache replaced it with its redacted form, which
    /// nothing decrypts. So the core need not remember redactions it could
    /// not apply yet.
    #[test]
    fn a_reaction_redacted_before_its_key_comes_is_not_reported() {
        block_on(async {
            let room = EncryptedRoom::new().await;
            let typo = room
                .alice_sends_first("$typo", "m.room.message", text("tpyo"))
                .await;
            let reaction = room
                .alice_encrypts(
                    "$r",
                    "m.reaction",
                    serde_json::json!({
                        "m.relates_to": {
                            "rel_type": "m.annotation",
                            "event_id": "$typo",
                            "key": "+1",
                        },
                    }),
                )
                .await;
            let after = room
                .alice_sends("$after", "m.room.message", text("after"))
                .await;
            room.bob_syncs(&[], &[&typo.event, &reaction, &after]).await;
            room.drain();
            // Room v10: `redacts` is top-level (and in content from v11).
            let redaction: Raw<AnySyncTimelineEvent> = Raw::new(&serde_json::json!({
                "type": "m.room.redaction",
                "event_id": "$unreact",
                "sender": room.alice.user_id().expect("alice is signed in"),
                "origin_server_ts": matrix_sdk::ruma::MilliSecondsSinceUnixEpoch::now(),
                "redacts": "$r",
                "content": { "redacts": "$r" },
            }))
            .expect("a redaction")
            .cast_unchecked();
            room.bob_syncs(&[], &[&redaction]).await;
            let redacted = room.drain();
            assert!(
                redacted.iter().any(|event| matches!(
                    event,
                    Event::RoomActivityUpdate(RoomActivityUpdate {
                        target: RoomActivityUpdateTarget::Event { event_id },
                        kind: RoomActivityUpdateKind::ActivityRedacted { .. },
                        ..
                    }) if event_id == "$r"
                )),
                "{redacted:#?}"
            );

            room.bob_syncs(&[&typo.room_key], &[]).await;

            let (decrypted, _) = room.decrypted().await;
            let lines = decrypted
                .events
                .iter()
                .map(|event| decrypted_message(event).1)
                .collect::<Vec<_>>();
            assert_eq!(lines, ["tpyo", "after"], "{decrypted:#?}");
        });
    }

    #[test]
    fn retrying_decryption_checks_the_request() {
        block_on(async {
            let room = EncryptedRoom::new().await;
            let retry = |room_id: &str, event_ids: &[&str]| {
                retry_decryption(
                    &room.bob,
                    RetryDecryptionRequest {
                        room_id: room_id.to_owned(),
                        event_ids: event_ids.iter().map(|id| (*id).to_owned()).collect(),
                    },
                )
            };
            let bad_room = retry("secret", &["$secret"])
                .await
                .expect_err("not a room ID");
            assert!(
                bad_room
                    .to_string()
                    .starts_with("invalid Matrix room ID secret"),
                "{bad_room}"
            );
            let no_event = retry(room.room_id.as_str(), &[])
                .await
                .expect_err("no event");
            assert_eq!(
                no_event.to_string(),
                "retrying decryption needs at least one event"
            );
            let bad_event = retry(room.room_id.as_str(), &["secret"])
                .await
                .expect_err("not an event ID");
            assert!(
                bad_event
                    .to_string()
                    .starts_with("invalid Matrix event ID secret"),
                "{bad_event}"
            );
            let unknown_room = retry("!elsewhere:example.org", &["$secret"])
                .await
                .expect_err("no such room");
            assert_eq!(
                unknown_room.to_string(),
                "unknown Matrix room: !elsewhere:example.org"
            );
        });
    }

    fn retry_request(room: &EncryptedRoom, event_ids: &[&str]) -> RetryDecryptionRequest {
        RetryDecryptionRequest {
            room_id: room.room_id.to_string(),
            event_ids: event_ids.iter().map(|id| (*id).to_owned()).collect(),
        }
    }

    /// The SDK's event cache keeps the failure it first stored, and no
    /// stream reports a withheld notice: only a retry tells the line's
    /// current cause.
    #[test]
    fn a_retry_reports_a_line_still_undecryptable_with_its_current_cause() {
        block_on(async {
            let room = EncryptedRoom::new().await;
            let sent = room
                .alice_sends_first("$secret", "m.room.message", text("psst"))
                .await;
            room.bob_syncs(&[], &[&sent.event]).await;
            assert_eq!(room.utd_cause(), RoomUnableToDecryptCause::Unknown);
            room.bob_syncs_withheld(&sent.event, "m.unverified").await;
            assert!(activities(&room.drain()).is_empty());

            let retried = retry_decryption(&room.bob, retry_request(&room, &["$secret"]))
                .await
                .expect("retry")
                .expect("the line is still in the cache");

            assert_eq!(retried.room_id, room.room_id.as_str());
            let [RoomHistoryBackfillEvent::Activity(activity)] = &retried.events[..] else {
                panic!("not one line: {retried:#?}");
            };
            assert_eq!(activity.activity_id, "event:$secret");
            let RoomActivityKind::UnableToDecrypt(utd) = &activity.kind else {
                panic!("not a UTD: {activity:#?}");
            };
            assert_eq!(utd.cause, RoomUnableToDecryptCause::WithheldUnverified);
        });
    }

    /// A key that came is used by the retry, whether or not the SDK's
    /// redecryptor got to the line first (then it reported it already, and
    /// the retry skips it). With the mock server it always does, but the
    /// line is never reported still undecryptable.
    #[test]
    fn a_retry_decrypts_a_line_whose_key_came() {
        block_on(async {
            let room = EncryptedRoom::new().await;
            let sent = room
                .alice_sends_first("$late", "m.room.message", text("late"))
                .await;
            room.bob_syncs(&[], &[&sent.event]).await;
            assert_eq!(room.utd_cause(), RoomUnableToDecryptCause::Unknown);
            room.bob_syncs(&[&sent.room_key], &[]).await;

            let retried = retry_decryption(&room.bob, retry_request(&room, &["$late"]))
                .await
                .expect("retry");

            let decrypted = match retried {
                Some(retried) => retried,
                None => room.decrypted().await.0,
            };
            let [event] = &decrypted.events[..] else {
                panic!("not one line: {decrypted:#?}");
            };
            let (activity, body) = decrypted_message(event);
            assert_eq!(body, "late");
            assert_eq!(activity.activity_id, "event:$late");
        });
    }

    #[test]
    fn a_retry_skips_events_the_cache_does_not_have() {
        block_on(async {
            let room = EncryptedRoom::new().await;

            let retried = retry_decryption(&room.bob, retry_request(&room, &["$unknown"]))
                .await
                .expect("retry");

            assert_eq!(retried, None);
        });
    }

    #[test]
    fn an_imported_key_file_decrypts_the_lines_of_its_keys() {
        block_on(async {
            let room = EncryptedRoom::new().await;
            let sent = room
                .alice_sends_first("$filed", "m.room.message", text("filed"))
                .await;
            room.bob_syncs(&[], &[&sent.event]).await;
            room.drain();
            let dir = tempfile::tempdir().expect("a temporary directory");
            let path = dir.path().join("alice-keys.txt");
            room.alice
                .encryption()
                .export_room_keys(path.clone(), "correct horse", |_| true)
                .await
                .expect("alice exports her keys");

            let counts = import_room_keys(
                &room.bob,
                &ImportRoomKeysRequest {
                    path: path.to_str().expect("a UTF-8 path").to_owned(),
                    passphrase: "correct horse".to_owned(),
                },
            )
            .await
            .expect("bob imports them");

            assert_eq!(counts, (1, 1));
            let (decrypted, _) = room.decrypted().await;
            assert_eq!(decrypted_message(&decrypted.events[0]).1, "filed");
        });
    }
}
