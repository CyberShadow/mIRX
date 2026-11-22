//! Other users' presence from the sync: each response's presence events,
//! and, with the first room snapshot, what the store kept of the room
//! members' presence.

mod common;

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use common::{Harness, alice_member_event};
use matrix_sdk::ruma::serde::Raw;
use matrix_sdk::ruma::{OwnedRoomId, events::presence::PresenceEvent};
use matrix_sdk_test::{JoinedRoomBuilder, SyncResponseBuilder};
use mirx_core::{Command, Event, PresenceChange, PresenceKind};
use serde_json::json;
use wiremock::matchers::{method, path_regex};
use wiremock::{Mock, Request, ResponseTemplate};

const ROOM: &str = "!a:example.org";

fn presence(user_id: &str, presence: &str, status_msg: Option<&str>) -> Raw<PresenceEvent> {
    let mut content = json!({ "presence": presence });
    if let Some(status_msg) = status_msg {
        content["status_msg"] = json!(status_msg);
    }
    Raw::from_json_string(
        json!({ "type": "m.presence", "sender": user_id, "content": content }).to_string(),
    )
    .expect("a raw presence event")
}

fn bob_member_event() -> serde_json::Value {
    json!({
        "type": "m.room.member",
        "state_key": "@bob:example.org",
        "sender": "@bob:example.org",
        "event_id": "$bob-join",
        "origin_server_ts": 2,
        "content": { "membership": "join", "displayname": "bob" },
    })
}

/// `ROOM`, with alice and bob in it.
fn add_room(sync: &mut SyncResponseBuilder) {
    sync.add_joined_room(
        JoinedRoomBuilder::new(&OwnedRoomId::try_from(ROOM).expect("a room ID"))
            .add_state_bulk([alice_member_event(), bob_member_event()].map(|event| {
                Raw::from_json_string(event.to_string()).expect("a raw member event")
            })),
    );
}

/// A session in `ROOM` whose every sync carries `presence`, and the
/// events of its restore: the first room snapshot.
fn restored(presence: Vec<Raw<PresenceEvent>>) -> (Harness, Vec<Event>) {
    Harness::restored_with_sync(|sync| {
        add_room(sync);
        sync.add_presence_bulk(presence);
    })
}

fn change(user_id: &str, kind: PresenceKind, status_msg: Option<&str>) -> PresenceChange {
    PresenceChange {
        user_id: user_id.to_owned(),
        kind,
        status_msg: status_msg.map(str::to_owned),
    }
}

/// What the server's syncs carry: set it to change their presence.
type SyncPresence = Arc<Mutex<Vec<Raw<PresenceEvent>>>>;

/// From now on the server answers every sync with `presence`, and nothing
/// else; the handle changes what the next syncs carry.
fn answer_syncs_with(harness: &Harness, presence: Vec<Raw<PresenceEvent>>) -> SyncPresence {
    let served = SyncPresence::new(Mutex::new(presence));
    let responses = AtomicUsize::new(0);
    let serving = served.clone();
    // A new token each time: the SDK skips a response whose token it has.
    harness.mount(
        Mock::given(method("GET"))
            .and(path_regex(r"^/_matrix/client/v3/sync"))
            .respond_with(move |_: &Request| {
                let mut sync = SyncResponseBuilder::new();
                sync.add_presence_bulk(serving.lock().expect("sync presence").clone());
                let mut body = sync.build_json_sync_response();
                body["next_batch"] = json!(format!(
                    "loop-{}",
                    responses.fetch_add(1, Ordering::Relaxed)
                ));
                ResponseTemplate::new(200).set_body_json(body)
            })
            .with_priority(1),
    );
    served
}

/// Starts the sync: the events of its first response, up to
/// `SyncConnected`, and of the next.
fn first_syncs(harness: &Harness) -> Vec<Event> {
    harness.send(Command::StartSync);
    let mut events = harness.events_until("SyncConnected", |event| *event == Event::SyncConnected);
    let syncs = harness.requests("GET", "/sync").len();
    harness.wait_for_requests("GET", "/sync", syncs + 1);
    events.extend(std::iter::from_fn(|| {
        harness
            .core
            .events()
            .recv_timeout(Duration::from_millis(300))
            .ok()
    }));
    events
}

fn presence_events(events: &[Event]) -> Vec<&Vec<PresenceChange>> {
    events
        .iter()
        .filter_map(|event| match event {
            Event::PresenceChanged(changes) => Some(changes),
            _ => None,
        })
        .collect()
}

fn errors(events: &[Event]) -> Vec<&str> {
    events
        .iter()
        .filter_map(|event| match event {
            Event::Error(error) => Some(error.message.as_str()),
            _ => None,
        })
        .collect()
}

/// Every sync's presence, of members (Bob) and others (Carol). The first
/// snapshot has only the members'.
#[test]
fn sync_presence_is_reported() {
    let sync_presence = || {
        vec![
            presence("@bob:example.org", "unavailable", Some("lunch")),
            // An empty status is none.
            presence("@carol:example.org", "offline", Some("")),
        ]
    };
    let (harness, restore) = restored(sync_presence());
    let bob = change("@bob:example.org", PresenceKind::Unavailable, Some("lunch"));
    let carol = change("@carol:example.org", PresenceKind::Offline, None);
    assert_eq!(
        presence_events(&restore),
        [&vec![bob.clone()]],
        "{restore:#?}"
    );

    answer_syncs_with(&harness, sync_presence());
    let events = first_syncs(&harness);

    let reported = presence_events(&events);
    assert!(!reported.is_empty(), "{events:#?}");
    for changes in reported {
        assert_eq!(*changes, [bob.clone(), carol.clone()], "{events:#?}");
    }

    harness.shut_down();
}

#[test]
fn a_sync_without_presence_reports_none() {
    let (harness, restore) = restored(Vec::new());

    answer_syncs_with(&harness, Vec::new());
    let events = first_syncs(&harness);

    assert_eq!(
        presence_events(&restore),
        Vec::<&Vec<_>>::new(),
        "{restore:#?}"
    );
    assert_eq!(
        presence_events(&events),
        Vec::<&Vec<_>>::new(),
        "{events:#?}"
    );

    harness.shut_down();
}

/// A presence mirx does not know (MSC3026's `busy`) is an error line, from
/// the sync and from the snapshot, which still lists the rooms.
#[test]
fn an_unknown_presence_is_a_sync_error() {
    let sync_presence = || {
        vec![
            presence("@bob:example.org", "busy", None),
            presence("@carol:example.org", "online", None),
        ]
    };
    let (harness, restore) = restored(sync_presence());
    let busy = r#"unknown presence "busy" of @bob:example.org"#;
    assert_eq!(errors(&restore), [busy], "{restore:#?}");
    assert!(
        restore
            .iter()
            .any(|event| matches!(event, Event::SyncedRooms(_))),
        "{restore:#?}"
    );
    assert_eq!(
        presence_events(&restore),
        Vec::<&Vec<_>>::new(),
        "{restore:#?}"
    );

    answer_syncs_with(&harness, sync_presence());
    let events = first_syncs(&harness);

    let errors = errors(&events);
    assert!(!errors.is_empty(), "{events:#?}");
    assert!(errors.iter().all(|error| *error == busy), "{events:#?}");
    let reported = presence_events(&events);
    assert!(!reported.is_empty(), "{events:#?}");
    for changes in reported {
        assert_eq!(
            *changes,
            [change("@carol:example.org", PresenceKind::Online, None)],
            "{events:#?}"
        );
    }

    harness.shut_down();
}

/// After a restart, an incremental sync carries no presence: the first
/// room snapshot says what the store kept, after the room's members.
#[test]
fn the_first_snapshot_carries_stored_presence() {
    let (harness, _) = restored(vec![presence(
        "@bob:example.org",
        "unavailable",
        Some("lunch"),
    )]);
    answer_syncs_with(&harness, Vec::new());

    let (harness, restore) = harness.restarted();

    let members = restore
        .iter()
        .position(|event| matches!(event, Event::RoomMembers(members) if members.room_id == ROOM))
        .unwrap_or_else(|| panic!("no members of {ROOM} in {restore:#?}"));
    let presence = restore
        .iter()
        .position(|event| matches!(event, Event::PresenceChanged(_)))
        .unwrap_or_else(|| panic!("no presence in {restore:#?}"));
    assert!(members < presence, "{restore:#?}");
    assert_eq!(
        presence_events(&restore),
        [&vec![change(
            "@bob:example.org",
            PresenceKind::Unavailable,
            Some("lunch")
        )]],
        "{restore:#?}"
    );
    let events = first_syncs(&harness);
    assert_eq!(
        presence_events(&events),
        Vec::<&Vec<_>>::new(),
        "{events:#?}"
    );

    harness.shut_down();
}

/// The events of a sync task up to its `SyncConnected`.
fn until_connected(harness: &Harness) -> Vec<Event> {
    harness.send(Command::StartSync);
    harness.events_until("SyncConnected", |event| *event == Event::SyncConnected)
}

/// A restart's first sync response says Bob went offline while mirx was
/// away: the UI hears it before `SyncConnected`, not after a store seed
/// that still says he is online.
#[test]
fn the_first_responses_presence_comes_before_sync_connected() {
    let (harness, _) = restored(vec![presence("@bob:example.org", "online", None)]);
    let (harness, restore) = harness.restarted();
    assert_eq!(
        presence_events(&restore),
        [&vec![change(
            "@bob:example.org",
            PresenceKind::Online,
            None
        )]],
        "{restore:#?}"
    );
    answer_syncs_with(
        &harness,
        vec![presence("@bob:example.org", "offline", None)],
    );

    let events = until_connected(&harness);

    let offline = change("@bob:example.org", PresenceKind::Offline, None);
    assert!(
        presence_events(&events).contains(&&vec![offline]),
        "{events:#?}"
    );

    harness.shut_down();
}

/// The same for every sync task: a reconnect's first response's presence
/// comes before its `SyncConnected`.
#[test]
fn a_reconnects_first_presence_comes_before_sync_connected() {
    let (harness, _) = restored(Vec::new());
    let served = answer_syncs_with(&harness, vec![presence("@bob:example.org", "online", None)]);
    until_connected(&harness);
    harness.send(Command::StopSync);
    harness.wait_for("SyncStopped", |event| {
        matches!(event, Event::SyncStopped(_))
    });
    *served.lock().expect("sync presence") = vec![presence("@bob:example.org", "offline", None)];

    let events = until_connected(&harness);

    let offline = change("@bob:example.org", PresenceKind::Offline, None);
    assert!(
        presence_events(&events).contains(&&vec![offline]),
        "{events:#?}"
    );

    harness.shut_down();
}
