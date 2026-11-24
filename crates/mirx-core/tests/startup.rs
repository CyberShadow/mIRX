//! A restart: the store has the rooms of the last sync, so the restore
//! shows them at once, offline too, and asks the server nothing until the
//! session syncs.

mod common;

use common::{Harness, alice_member_event};
use matrix_sdk::ruma::OwnedRoomId;
use matrix_sdk::ruma::serde::Raw;
use matrix_sdk_test::{JoinedRoomBuilder, SyncResponseBuilder};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;
use wiremock::matchers::{method, path_regex};
use wiremock::{Mock, Request, ResponseTemplate};

use mirx_core::{Command, E2eeStatus, Event, Operation, RoomMemberRank, SyncStopReason};
use serde_json::json;

const ROOM: &str = "!a:example.org";

/// `ROOM`, named and with a topic, with alice and bob (a moderator) in it.
fn add_room(sync: &mut SyncResponseBuilder) {
    let state = [
        json!({
            "type": "m.room.create",
            "state_key": "",
            "sender": "@alice:example.org",
            "event_id": "$create",
            "origin_server_ts": 0,
            "content": { "creator": "@alice:example.org", "room_version": "10" },
        }),
        json!({
            "type": "m.room.name",
            "state_key": "",
            "sender": "@alice:example.org",
            "event_id": "$name",
            "origin_server_ts": 1,
            "content": { "name": "Lobby" },
        }),
        json!({
            "type": "m.room.topic",
            "state_key": "",
            "sender": "@alice:example.org",
            "event_id": "$topic",
            "origin_server_ts": 2,
            "content": { "topic": "Say hello" },
        }),
        json!({
            "type": "m.room.power_levels",
            "state_key": "",
            "sender": "@alice:example.org",
            "event_id": "$power",
            "origin_server_ts": 3,
            "content": { "users": { "@alice:example.org": 100, "@bob:example.org": 50 } },
        }),
        alice_member_event(),
        json!({
            "type": "m.room.member",
            "state_key": "@bob:example.org",
            "sender": "@bob:example.org",
            "event_id": "$bob-join",
            "origin_server_ts": 4,
            "content": { "membership": "join", "displayname": "bob" },
        }),
    ];
    sync.add_joined_room(
        JoinedRoomBuilder::new(&OwnedRoomId::try_from(ROOM).expect("a room ID")).add_state_bulk(
            state.map(|event| Raw::from_json_string(event.to_string()).expect("a raw event")),
        ),
    );
}

/// A session in `ROOM`, restored once (its store has synced), and the
/// number of requests the server got so far.
fn synced_once() -> (Harness, usize) {
    let (harness, _) = Harness::restored_with_sync(add_room);
    let requests = harness.requests("", "").len();
    (harness, requests)
}

/// The paths of the requests the server got after the first `since`.
fn paths_since(harness: &Harness, since: usize) -> Vec<String> {
    harness
        .requests("", "")
        .into_iter()
        .skip(since)
        .map(|request| format!("{} {}", request.method, request.url.path()))
        .collect()
}

/// What the stored rooms are: the room list, the members with their ranks
/// and the read state of `ROOM`; and no error.
fn assert_stored_rooms(events: &[Event]) {
    let rooms = events
        .iter()
        .find_map(|event| match event {
            Event::SyncedRooms(rooms) => Some(rooms),
            _ => None,
        })
        .unwrap_or_else(|| panic!("no SyncedRooms in {events:#?}"));
    let [room] = rooms.as_slice() else {
        panic!("one room in {events:#?}");
    };
    assert_eq!(
        (
            room.room_id.as_str(),
            room.display_name.as_str(),
            room.topic.as_deref()
        ),
        (ROOM, "Lobby", Some("Say hello")),
        "{events:#?}"
    );
    let members = events
        .iter()
        .find_map(|event| match event {
            Event::RoomMembers(members) if members.room_id == ROOM => Some(members),
            _ => None,
        })
        .unwrap_or_else(|| panic!("no members of {ROOM} in {events:#?}"));
    let mut ranks = members
        .members
        .iter()
        .map(|member| (member.user_id.as_str(), member.rank))
        .collect::<Vec<_>>();
    ranks.sort_by_key(|(user_id, _)| *user_id);
    assert_eq!(
        ranks,
        [
            ("@alice:example.org", RoomMemberRank::Administrator),
            ("@bob:example.org", RoomMemberRank::Moderator),
        ],
        "{events:#?}"
    );
    assert!(
        events
            .iter()
            .any(|event| matches!(event, Event::RoomReadState(state) if state.room_id == ROOM)),
        "{events:#?}"
    );
    assert!(
        !events.iter().any(|event| matches!(event, Event::Error(_))),
        "{events:#?}"
    );
}

/// What the SDK asks by itself when a session is restored: recovery
/// reads whether secret storage is set up. Not mirx's to stop; offline it
/// fails quietly.
fn is_the_sdks_own_request(path: &str) -> bool {
    path.ends_with("/account_data/m.secret_storage.default_key")
}

#[test]
fn a_restart_reads_the_rooms_from_the_store_and_asks_the_server_nothing() {
    let (harness, since) = synced_once();

    let (harness, restore) = harness.restarted();

    assert_eq!(
        restore.first(),
        Some(&Event::RestoredSession),
        "{restore:#?}"
    );
    assert_stored_rooms(&restore);
    assert!(
        !restore
            .iter()
            .any(|event| matches!(event, Event::E2eeStatus(_))),
        "{restore:#?}"
    );
    let asked = paths_since(&harness, since);
    assert_eq!(
        asked
            .iter()
            .filter(|path| !is_the_sdks_own_request(path))
            .collect::<Vec<_>>(),
        Vec::<&String>::new(),
        "every request after the restart: {asked:#?}"
    );

    harness.shut_down();
}

#[test]
fn a_restart_offline_shows_the_rooms_and_loses_only_the_sync() {
    let (harness, _) = synced_once();

    let (harness, restore) = harness.restarted_offline();

    assert_eq!(
        restore.first(),
        Some(&Event::RestoredSession),
        "{restore:#?}"
    );
    assert_stored_rooms(&restore);
    harness.send(Command::StartSync);
    let events = harness.events_until("SyncStopped", |event| {
        matches!(event, Event::SyncStopped(_))
    });
    // Background events of the restore may come first.
    assert!(events.contains(&Event::SyncStarted), "{events:#?}");
    assert!(
        matches!(
            events.last(),
            Some(Event::SyncStopped(SyncStopReason::Lost(_)))
        ),
        "{events:#?}"
    );
    assert!(
        !events.iter().any(|event| matches!(
            event,
            Event::Error(error)
                if matches!(error.operation, Operation::RestoreSession | Operation::E2eeStatus)
        )),
        "{events:#?}"
    );

    harness.shut_down();
}

#[test]
fn a_restarted_session_reads_its_e2ee_status_once_connected() {
    let (harness, since) = synced_once();
    let (harness, _) = harness.restarted();

    harness.send(Command::StartSync);
    let mut events = harness.events_until("SyncConnected", |event| *event == Event::SyncConnected);
    // A second response.
    let syncs = paths_since(&harness, since)
        .iter()
        .filter(|path| path.ends_with("/sync"))
        .count();
    harness.wait_for_requests("GET", "/sync", syncs + 1);
    harness.send(Command::RefreshE2eeStatus);
    events.extend(harness.events_until("the status's ack", |event| {
        matches!(event, Event::CommandHandled(_))
    }));

    let started = events
        .iter()
        .position(|event| *event == Event::SyncStarted)
        .unwrap_or_else(|| panic!("no SyncStarted in {events:#?}"));
    let connected = events
        .iter()
        .position(|event| *event == Event::SyncConnected)
        .expect("SyncConnected");
    // The server's statuses: the first contact's, then the refresh's.
    let fetched = events
        .iter()
        .enumerate()
        .filter(|(_, event)| {
            matches!(event, Event::E2eeStatus(status) if status.backup.exists_on_server.is_some())
        })
        .map(|(index, _)| index)
        .collect::<Vec<_>>();
    let [first, refreshed] = fetched.as_slice() else {
        panic!("two fetched E2EE statuses in {events:#?}");
    };
    assert!(started < *first && *first < connected, "{events:#?}");
    assert!(connected < *refreshed, "{events:#?}");
    assert!(
        !events.iter().any(|event| matches!(event, Event::Error(_))),
        "{events:#?}"
    );

    harness.shut_down();
}

#[test]
fn a_first_restore_lists_the_rooms_once_after_announcing_the_session() {
    // The restore's sync carries account data whose handlers re-list the
    // rooms: they come before the session is announced, and its first
    // room list already has what they changed.
    let (harness, restore) = Harness::restored_with_sync(|sync| {
        add_room(sync);
        sync.add_custom_global_account_data(json!({
            "type": "m.direct",
            "content": { "@bob:example.org": [ROOM] },
        }));
        sync.add_custom_global_account_data(json!({
            "type": "m.push_rules",
            "content": { "global": {} },
        }));
    });

    let announced = restore
        .iter()
        .position(|event| *event == Event::RestoredSession)
        .unwrap_or_else(|| panic!("no RestoredSession in {restore:#?}"));
    let listed = restore
        .iter()
        .enumerate()
        .filter(|(_, event)| matches!(event, Event::SyncedRooms(_)))
        .map(|(index, _)| index)
        .collect::<Vec<_>>();
    let [listed] = listed.as_slice() else {
        panic!("one SyncedRooms in {restore:#?}");
    };
    assert!(announced < *listed, "{restore:#?}");
    let Event::SyncedRooms(rooms) = &restore[*listed] else {
        unreachable!()
    };
    assert_eq!(
        rooms
            .iter()
            .map(|room| (room.room_id.as_str(), room.is_direct))
            .collect::<Vec<_>>(),
        [(ROOM, true)],
        "{restore:#?}"
    );

    harness.shut_down();
}

/// Long enough for the SDK's state streams, which tick within
/// milliseconds of the restore and of the first sync.
const QUIET: Duration = Duration::from_secs(2);

/// The E2EE statuses among `events`.
fn e2ee_statuses(events: &[Event]) -> Vec<&E2eeStatus> {
    events
        .iter()
        .filter_map(|event| match event {
            Event::E2eeStatus(status) => Some(status),
            _ => None,
        })
        .collect()
}

#[test]
fn a_restarted_session_tells_its_e2ee_status_once() {
    let (harness, _) = synced_once();
    let (harness, mut events) = harness.restarted();

    harness.send(Command::StartSync);
    events.extend(harness.events_until("SyncConnected", |event| *event == Event::SyncConnected));
    events.extend(harness.events_for(QUIET));

    // The first contact's, from the server; none before it, none after.
    let statuses = e2ee_statuses(&events);
    let [status] = statuses.as_slice() else {
        panic!("one E2EE status in {events:#?}");
    };
    assert!(status.backup.exists_on_server.is_some(), "{events:#?}");
    let position = |wanted: &Event| {
        events
            .iter()
            .position(|event| event == wanted)
            .unwrap_or_else(|| panic!("no {wanted:?} in {events:#?}"))
    };
    let told = events
        .iter()
        .position(|event| matches!(event, Event::E2eeStatus(_)))
        .expect("the status");
    assert!(
        position(&Event::SyncStarted) < told && told < position(&Event::SyncConnected),
        "{events:#?}"
    );

    harness.shut_down();
}

#[test]
fn a_restart_not_connecting_tells_no_e2ee_status() {
    let (harness, _) = synced_once();
    let (harness, mut events) = harness.restarted();

    events.extend(harness.events_for(QUIET));

    assert_eq!(
        e2ee_statuses(&events),
        Vec::<&E2eeStatus>::new(),
        "{events:#?}"
    );

    harness.shut_down();
}

#[test]
fn a_first_restore_tells_its_e2ee_status_once() {
    let (harness, mut events) = Harness::restored_with_sync(add_room);

    harness.send(Command::StartSync);
    events.extend(harness.events_until("SyncConnected", |event| *event == Event::SyncConnected));
    events.extend(harness.events_for(QUIET));

    // The entry's, from the server: its sync got through.
    let statuses = e2ee_statuses(&events);
    let [status] = statuses.as_slice() else {
        panic!("one E2EE status in {events:#?}");
    };
    assert!(status.backup.exists_on_server.is_some(), "{events:#?}");

    harness.shut_down();
}

/// A restart's first sync response, what happened while mirx was away,
/// reaches the UI before `SyncConnected`, which tells it apart from live
/// lines (the UI's catch-up: no tips, one flash at the connect).
#[test]
fn the_first_responses_lines_come_before_sync_connected() {
    let (harness, _) = synced_once();
    let (harness, _) = harness.restarted();
    let responses = AtomicUsize::new(0);
    // A new token each time: the SDK skips a response whose token it has.
    harness.mount(
        Mock::given(method("GET"))
            .and(path_regex(r"^/_matrix/client/v3/sync"))
            .respond_with(move |_: &Request| {
                let message = json!({
                    "type": "m.room.message",
                    "sender": "@bob:example.org",
                    "event_id": "$while-away",
                    "origin_server_ts": 5,
                    "content": { "msgtype": "m.text", "body": "alice: you there?" },
                });
                let mut sync = SyncResponseBuilder::new();
                sync.add_joined_room(
                    JoinedRoomBuilder::new(&OwnedRoomId::try_from(ROOM).expect("a room ID"))
                        .add_timeline_event(
                            Raw::from_json_string(message.to_string()).expect("a raw event"),
                        ),
                );
                let mut body = sync.build_json_sync_response();
                body["next_batch"] = json!(format!(
                    "away-{}",
                    responses.fetch_add(1, Ordering::Relaxed)
                ));
                ResponseTemplate::new(200).set_body_json(body)
            })
            .with_priority(1),
    );

    harness.send(Command::StartSync);
    let events = harness.events_until("SyncConnected", |event| *event == Event::SyncConnected);

    assert!(
        events.iter().any(|event| matches!(
            event,
            Event::RoomActivity(activity) if activity.activity_id == "event:$while-away"
        )),
        "{events:#?}"
    );

    harness.shut_down();
}
