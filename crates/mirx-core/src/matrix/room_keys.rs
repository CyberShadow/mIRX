//! Messages that could not be decrypted when they arrived: their lines,
//! their late decryption by the SDK's redecryptor, and room key files.

use std::collections::BTreeMap;

use matrix_sdk::Room;
use matrix_sdk::ruma::events::room::encrypted::OriginalSyncRoomEncryptedEvent;
use matrix_sdk::ruma::serde::Raw;

use super::{
    MatrixCoreError, SenderProfileCache, emit_sync_error,
    room_history_backfill_event_from_timeline_event,
};
use crate::api::{Event, RoomHistoryBackfillEvent};
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

#[cfg(test)]
mod tests {
    use std::sync::mpsc;
    use std::time::Duration;

    use matrix_sdk::Client;
    use matrix_sdk::ruma::events::AnySyncTimelineEvent;
    use matrix_sdk::ruma::serde::Raw;
    use matrix_sdk::ruma::{OwnedRoomId, RoomVersionId, owned_room_id};
    use matrix_sdk::test_utils::mocks::MatrixMockServer;
    use matrix_sdk_base::crypto::types::events::room::encrypted::EncryptedToDeviceEvent;
    use matrix_sdk_test::JoinedRoomBuilder;
    use matrix_sdk_test::event_factory::EventFactory;

    use super::super::register_event_handlers;
    use crate::api::{
        Event, RoomActivity, RoomActivityKind, RoomActivityRelation, RoomUnableToDecryptReason,
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
            register_event_handlers(&bob, events_tx, &RoomSnapshots::default());

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
                "origin_server_ts": 1_700_000_000_000u64,
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
}
