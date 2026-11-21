//! Messages that could not be decrypted when they arrived: their lines,
//! their late decryption by the SDK's redecryptor, and room key files.

use std::collections::{BTreeMap, BTreeSet};
use std::pin::pin;

use futures_util::StreamExt;
use matrix_sdk::deserialized_responses::{TimelineEvent, TimelineEventKind};
use matrix_sdk::event_cache::{DecryptionRetryRequest, RedecryptorReport};
use matrix_sdk::ruma::events::room::encrypted::OriginalSyncRoomEncryptedEvent;
use matrix_sdk::ruma::serde::Raw;
use matrix_sdk::ruma::{OwnedEventId, OwnedRoomId, RoomId};
use matrix_sdk::{Client, Room};
use tokio_stream::wrappers::errors::BroadcastStreamRecvError;

use super::{
    MatrixCoreError, SenderProfileCache, emit_sync_error,
    room_history_backfill_event_from_timeline_event,
};
use crate::api::{
    CoreErrorEvent, Event, Operation, RetryDecryptionRequest, RoomEventsDecrypted,
    RoomHistoryBackfillEvent,
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
        Ok(Some(RoomHistoryBackfillEvent::Activity(activity))) => {
            emit_event(events, Event::RoomActivity(activity));
        }
        Ok(Some(RoomHistoryBackfillEvent::ActivityUpdate(update))) => {
            emit_event(events, Event::RoomActivityUpdate(update));
        }
        Ok(None) => {}
        Err(err) => emit_sync_error(events, err),
    }
}

async fn undecrypted_sync_event(
    room: &Room,
    event: &Raw<OriginalSyncRoomEncryptedEvent>,
) -> Result<Option<RoomHistoryBackfillEvent>, MatrixCoreError> {
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
        &mut BTreeMap::new(),
        &mut SenderProfileCache::new(room),
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
    let mut reaction_index = BTreeMap::new();
    let mut senders = SenderProfileCache::new(&room);
    let mut events = Vec::with_capacity(decrypted.len());
    for event in &decrypted {
        events.extend(
            room_history_backfill_event_from_timeline_event(
                &room,
                event,
                &redaction_rules,
                &mut reaction_index,
                &mut senders,
            )
            .await?,
        );
    }
    Ok((!events.is_empty()).then(|| RoomEventsDecrypted {
        room_id: room_id.to_string(),
        events,
    }))
}

/// Hands the sessions to the SDK's redecryptor, which tries the room's
/// events of them again (and asks the key backup for their keys); what it
/// decrypts `room_decryption_task` reports.
pub(crate) fn retry_decryption(
    client: &Client,
    request: RetryDecryptionRequest,
) -> Result<(), MatrixCoreError> {
    let room_id = OwnedRoomId::try_from(request.room_id.as_str()).map_err(|err| {
        MatrixCoreError::new(format!("invalid Matrix room ID {}: {err}", request.room_id))
    })?;
    if request.session_ids.is_empty() {
        return Err(MatrixCoreError::new(
            "retrying decryption needs at least one session",
        ));
    }
    client
        .event_cache()
        .request_decryption(DecryptionRetryRequest {
            room_id,
            utd_session_ids: request.session_ids.into_iter().collect(),
            refresh_info_session_ids: BTreeSet::new(),
        });
    Ok(())
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

    use super::super::{backfill_room_history, register_event_handlers};
    use super::{retry_decryption, room_decryption_task};
    use crate::api::{
        BackfillRoomHistoryRequest, Event, HistoryFrom, RetryDecryptionRequest, RoomActivity,
        RoomActivityKind, RoomActivityRelation, RoomActivityUpdateKind, RoomActivityUpdateTarget,
        RoomEventsDecrypted, RoomHistoryBackfillEvent, RoomUnableToDecryptReason,
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
            register_event_handlers(&bob, events_tx.clone(), &RoomSnapshots::default());
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
        /// events before it.
        fn decrypted(&self) -> (RoomEventsDecrypted, Vec<Event>) {
            let mut before = Vec::new();
            loop {
                match self.events.recv_timeout(Duration::from_secs(10)) {
                    Ok(Event::RoomEventsDecrypted(decrypted)) => return (decrypted, before),
                    Ok(event) => before.push(event),
                    Err(err) => panic!("no RoomEventsDecrypted ({err}) after {before:#?}"),
                }
            }
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
            assert_eq!(utd.sender.display_name.as_deref(), Some("Alice"));
            assert_eq!(utd.reason, RoomUnableToDecryptReason::MissingMegolmSession);
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

            let (decrypted, before) = room.decrypted();
            assert!(activities(&before).is_empty(), "{before:#?}");
            assert_eq!(decrypted.room_id, "!secret:example.org");
            assert_eq!(decrypted.events.len(), 1, "{decrypted:#?}");
            let (activity, body) = decrypted_message(&decrypted.events[0]);
            assert_eq!(body, "late");
            assert_eq!(activity.activity_id, "event:$late");
            assert_eq!(activity.activity_id, activities(&utd)[0].activity_id);
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

            let (decrypted, _) = room.decrypted();
            assert_eq!(decrypted.events.len(), 1, "{decrypted:#?}");
            let (activity, body) = decrypted_message(&decrypted.events[0]);
            assert_eq!(body, "old");
            assert_eq!(activity.activity_id, "event:$old");
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

            let (decrypted, _) = room.decrypted();
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

    #[test]
    fn retrying_decryption_checks_the_request() {
        block_on(async {
            let room = EncryptedRoom::new().await;
            let bad_room = retry_decryption(
                &room.bob,
                RetryDecryptionRequest {
                    room_id: "secret".to_owned(),
                    session_ids: vec!["SESSION".to_owned()],
                },
            )
            .expect_err("not a room ID");
            assert!(
                bad_room
                    .to_string()
                    .starts_with("invalid Matrix room ID secret"),
                "{bad_room}"
            );
            let no_session = retry_decryption(
                &room.bob,
                RetryDecryptionRequest {
                    room_id: room.room_id.to_string(),
                    session_ids: Vec::new(),
                },
            )
            .expect_err("no session");
            assert_eq!(
                no_session.to_string(),
                "retrying decryption needs at least one session"
            );
        });
    }
}
