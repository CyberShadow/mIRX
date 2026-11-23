//! The core answers commands while others wait for the server: commands
//! run in lanes, one after another within a lane and side by side across
//! lanes. These tests hang requests on purpose (`Harness::hang`); they
//! live in their own binary because a hung wiremock responder outlives the
//! test in the pooled server.

mod common;

use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use common::{Harness, alice_member_event};
use matrix_sdk::ruma::events::AnyStrippedStateEvent;
use matrix_sdk::ruma::serde::Raw;
use matrix_sdk_test::{InvitedRoomBuilder, JoinedRoomBuilder};
use mirx_core::{
    AcceptRoomInviteRequest, BackfillRoomHistoryRequest, Command, CreateDirectRoomRequest,
    EditMessageRequest, Event, HistoryFrom, ListPublicRoomsRequest, LoginMethod, LoginRequest,
    MemberPowerLevelChange, MemberPowerRole, Operation, RestoreSessionRequest,
    RoomMessagePresentationKind, SendFileRequest, SendMessageRequest, SessionAuth,
    SetRoomMemberPowerLevelRequest, StoredSession,
};
use serde_json::json;
use wiremock::matchers::{method, path_regex};
use wiremock::{Mock, Request, Respond, ResponseTemplate};

const ROOM: &str = "!a:example.org";
const OTHER_ROOM: &str = "!b:example.org";

/// The events a test has seen, so it can count the acks among them.
struct Seen<'a> {
    harness: &'a Harness,
    events: Vec<Event>,
}

impl<'a> Seen<'a> {
    fn new(harness: &'a Harness) -> Self {
        Self {
            harness,
            events: Vec::new(),
        }
    }

    /// Reads events up to and including the first that `matches`.
    fn until(&mut self, what: &str, matches: impl Fn(&Event) -> bool) -> &Event {
        let events = self.harness.events_until(what, matches);
        self.events.extend(events);
        self.events.last().expect("the matching event")
    }

    /// Reads events until `count` acks were seen in all.
    fn until_acks(&mut self, count: usize) {
        while self.acks() < count {
            self.until("an ack", |event| *event == Event::CommandHandled);
        }
    }

    fn acks(&self) -> usize {
        self.events
            .iter()
            .filter(|event| **event == Event::CommandHandled)
            .count()
    }

    /// Sends `Shutdown` and reads every event up to `Stopped`.
    fn shut_down(mut self) -> Vec<Event> {
        self.harness.send(Command::Shutdown);
        self.until("Stopped", |event| *event == Event::Stopped);
        self.events
    }
}

fn line(room_id: &str, body: &str) -> Command {
    Command::SendMessage(SendMessageRequest {
        room_id: room_id.to_owned(),
        presentation: RoomMessagePresentationKind::Text,
        body: body.to_owned(),
        styles: Vec::new(),
        thread: None,
        reply: None,
    })
}

fn backfill(room_id: &str) -> Command {
    Command::BackfillRoomHistory(BackfillRoomHistoryRequest {
        room_id: room_id.to_owned(),
        from: HistoryFrom::Latest,
        limit: 20,
    })
}

fn is_backfill_answer(event: &Event) -> bool {
    match event {
        Event::RoomHistoryBackfill(_) => true,
        Event::Error(error) => error.operation == Operation::BackfillRoomHistory,
        _ => false,
    }
}

fn acks(events: &[Event]) -> usize {
    events
        .iter()
        .filter(|event| **event == Event::CommandHandled)
        .count()
}

fn body(request: &Request) -> serde_json::Value {
    serde_json::from_slice(&request.body).expect("a JSON request body")
}

/// A send to `room_id`, with a fresh event ID.
fn mount_sends(harness: &Harness) {
    harness.mount(
        Mock::given(method("PUT"))
            .and(path_regex(r"^/_matrix/client/v3/rooms/.*/send/"))
            .respond_with(|request: &Request| {
                let txn = request.url.path().rsplit('/').next().expect("a txn ID");
                ResponseTemplate::new(200)
                    .set_body_json(json!({ "event_id": format!("$sent-{txn}") }))
            }),
    );
}

fn send_paths(harness: &Harness) -> Vec<(String, serde_json::Value)> {
    harness
        .requests("PUT", "/send/")
        .iter()
        .map(|request| (request.url.path().to_owned(), body(request)))
        .collect()
}

/// Bob's profile, and his presence, which `/whois` asks for after it.
fn mount_bob_profile(harness: &Harness) {
    harness.mount(
        Mock::given(method("GET"))
            .and(path_regex(r"^/_matrix/client/v3/profile/@bob:example.org$"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(json!({ "displayname": "Bob" })),
            ),
    );
    harness.mount(
        Mock::given(method("GET"))
            .and(path_regex(
                r"^/_matrix/client/v3/presence/@bob:example.org/status$",
            ))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(json!({ "presence": "offline" })),
            ),
    );
}

#[test]
fn a_hung_backfill_holds_up_nothing_else() {
    let harness = Harness::restored_in_rooms(&[ROOM], &[]);
    harness.hang("GET", r"^/_matrix/client/v3/rooms/.*/messages");
    mount_bob_profile(&harness);
    harness.mount(
        Mock::given(method("POST"))
            .and(path_regex(r"^/_matrix/client/v3/publicRooms"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(json!({ "chunk": [], "total_room_count_estimate": 0 })),
            ),
    );
    mount_sends(&harness);
    let mut seen = Seen::new(&harness);

    harness.send(backfill(ROOM));
    harness.wait_for_requests("GET", "/messages", 1);

    harness.send(Command::FetchUserProfile("@bob:example.org".to_owned()));
    seen.until("the profile", |event| {
        matches!(event, Event::UserProfile(_))
    });
    seen.until_acks(1);

    harness.send(line(ROOM, "hello"));
    seen.until_acks(2);
    let sends = harness.wait_for_requests("PUT", "/send/", 1);
    assert_eq!(body(&sends[0])["body"], "hello");

    harness.send(Command::ListPublicRooms(ListPublicRoomsRequest {
        server: None,
        filter: None,
        since: None,
        limit: 20,
    }));
    seen.until("the public rooms", |event| {
        matches!(event, Event::PublicRooms(_))
    });
    seen.until_acks(3);

    let events = seen.shut_down();
    assert!(
        !events.iter().any(is_backfill_answer),
        "the hung backfill answered: {events:#?}"
    );
    // The backfill, the profile, the line, the list and the shutdown.
    assert_eq!(acks(&events), 5, "{events:#?}");
    harness.join_stopped();
}

#[test]
fn a_rooms_lines_go_in_order_and_other_rooms_go_meanwhile() {
    let harness = Harness::restored_in_rooms(&[ROOM, OTHER_ROOM], &[]);
    harness.runtime.block_on(async {
        harness
            .server
            .mock_authenticated_media_config()
            .expect_any_access_token()
            .ok_default()
            .mount()
            .await;
    });
    harness.hang("POST", r"^/_matrix/media/v3/upload");
    mount_sends(&harness);
    let dir = tempfile::tempdir().expect("temporary directory");
    let path = dir.path().join("report.txt");
    std::fs::write(&path, b"report").expect("write the file to send");
    let mut seen = Seen::new(&harness);

    harness.send(Command::SendFile(SendFileRequest {
        room_id: ROOM.to_owned(),
        path,
        filename: None,
        content_type: None,
    }));
    harness.wait_for_requests("POST", "/upload", 1);
    harness.send(line(ROOM, "after"));
    harness.send(line(OTHER_ROOM, "b"));

    let sends = harness.wait_for_requests("PUT", "/send/", 1);
    assert!(
        sends[0].url.path().contains(OTHER_ROOM),
        "{:?}",
        sends[0].url
    );
    seen.until_acks(1);
    // "after" waits behind the file in its room's lane.
    assert_eq!(
        send_paths(&harness)
            .iter()
            .filter(|(_, body)| body["body"] == "after")
            .count(),
        0
    );

    let events = seen.shut_down();
    assert_eq!(acks(&events), 4, "{events:#?}");
    harness.join_stopped();
}

#[test]
fn an_edit_goes_before_the_line_typed_after_it() {
    let harness = Harness::restored_in_rooms(&[ROOM], &[]);
    // Not in the sync: the SDK fetches the original to make the edit.
    harness.mount(
        Mock::given(method("GET"))
            .and(path_regex(r"^/_matrix/client/v3/rooms/.*/event/\$orig"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(json!({
                        "type": "m.room.message",
                        "room_id": ROOM,
                        "sender": "@alice:example.org",
                        "event_id": "$orig",
                        "origin_server_ts": 2,
                        "content": { "msgtype": "m.text", "body": "original" },
                    }))
                    .set_delay(Duration::from_millis(500)),
            ),
    );
    mount_sends(&harness);
    let mut seen = Seen::new(&harness);

    harness.send(Command::EditMessage(EditMessageRequest {
        room_id: ROOM.to_owned(),
        target_event_id: "$orig".to_owned(),
        presentation: RoomMessagePresentationKind::Text,
        body: "edited".to_owned(),
        styles: Vec::new(),
    }));
    harness.send(line(ROOM, "after"));
    seen.until_acks(2);

    let sends = harness.wait_for_requests("PUT", "/send/", 2);
    let bodies: Vec<_> = sends.iter().map(body).collect();
    assert_eq!(
        bodies[0]["m.relates_to"]["rel_type"], "m.replace",
        "{bodies:#?}"
    );
    assert_eq!(bodies[1]["body"], "after", "{bodies:#?}");

    let events = seen.shut_down();
    assert_eq!(acks(&events), 3, "{events:#?}");
    harness.join_stopped();
}

/// A whole document as a server keeps it (account data, or a state
/// event): a GET answers the last PUT's body (`404 M_NOT_FOUND` before any,
/// for a document the account does not have yet), and each PUT is answered
/// `put_answer` after `delay`. It records every request, with when it came.
#[derive(Clone)]
struct DocumentServer {
    content: std::sync::Arc<std::sync::Mutex<Option<serde_json::Value>>>,
    requests: std::sync::Arc<std::sync::Mutex<Vec<DocumentRequest>>>,
    put_answer: serde_json::Value,
    delay: Duration,
}

/// A request `DocumentServer` got: when, its method, and a PUT's body.
#[derive(Debug, Clone)]
struct DocumentRequest {
    at: Instant,
    method: &'static str,
    body: serde_json::Value,
}

impl DocumentServer {
    /// Serves the document at the paths that match `path`, above the
    /// harness's catch-all account data mocks.
    fn mount(
        harness: &Harness,
        path: &str,
        content: Option<serde_json::Value>,
        put_answer: serde_json::Value,
        delay: Duration,
    ) -> Self {
        let server = Self {
            content: std::sync::Arc::new(std::sync::Mutex::new(content)),
            requests: std::sync::Arc::default(),
            put_answer,
            delay,
        };
        harness.mount(
            Mock::given(path_regex(path))
                .respond_with(server.clone())
                .with_priority(1),
        );
        server
    }

    /// The account's `m.ignored_user_list`, holding `user_ids`.
    fn ignore_list(harness: &Harness, user_ids: &[&str], delay: Duration) -> Self {
        Self::mount(
            harness,
            r"/account_data/m\.ignored_user_list$",
            Some(ignore_list(user_ids)),
            json!({}),
            delay,
        )
    }

    /// The account's `m.direct`, which it does not have yet.
    fn direct_rooms(harness: &Harness, delay: Duration) -> Self {
        Self::mount(harness, r"/account_data/m\.direct$", None, json!({}), delay)
    }

    /// The requests, in arrival order.
    fn requests(&self) -> Vec<DocumentRequest> {
        self.requests.lock().expect("requests lock").clone()
    }

    /// The bodies of the PUTs, in arrival order.
    fn puts(&self) -> Vec<serde_json::Value> {
        self.requests()
            .into_iter()
            .filter(|request| request.method == "PUT")
            .map(|request| request.body)
            .collect()
    }
}

impl Respond for DocumentServer {
    fn respond(&self, request: &Request) -> ResponseTemplate {
        let mut content = self.content.lock().expect("content lock");
        let mut requests = self.requests.lock().expect("requests lock");
        match request.method.as_str() {
            "GET" => {
                requests.push(DocumentRequest {
                    at: Instant::now(),
                    method: "GET",
                    body: serde_json::Value::Null,
                });
                match &*content {
                    Some(content) => ResponseTemplate::new(200).set_body_json(content),
                    None => ResponseTemplate::new(404).set_body_json(json!({
                        "errcode": "M_NOT_FOUND",
                        "error": "Not found",
                    })),
                }
            }
            "PUT" => {
                let body = body(request);
                requests.push(DocumentRequest {
                    at: Instant::now(),
                    method: "PUT",
                    body: body.clone(),
                });
                *content = Some(body);
                ResponseTemplate::new(200)
                    .set_body_json(&self.put_answer)
                    .set_delay(self.delay)
            }
            other => panic!("unexpected {other} of {}", request.url.path()),
        }
    }
}

fn ignore_list(user_ids: &[&str]) -> serde_json::Value {
    let ignored_users = user_ids
        .iter()
        .map(|user_id| ((*user_id).to_owned(), json!({})))
        .collect::<serde_json::Map<_, _>>();
    json!({ "ignored_users": ignored_users })
}

/// A session whose stored ignore list holds Bob, as the server's.
fn ignoring_bob() -> Harness {
    Harness::restored_in_rooms(
        &[ROOM],
        &[json!({ "type": "m.ignored_user_list", "content": ignore_list(&["@bob:example.org"]) })],
    )
}

#[test]
fn ignoring_two_users_in_a_row_keeps_both() {
    let harness = ignoring_bob();
    let server = DocumentServer::ignore_list(&harness, &["@bob:example.org"], Duration::ZERO);
    let mut seen = Seen::new(&harness);

    // The second comes before any sync echoes the first.
    harness.send(Command::IgnoreUser("@carol:example.org".to_owned()));
    harness.send(Command::IgnoreUser("@dave:example.org".to_owned()));
    seen.until_acks(2);

    assert_eq!(
        server.puts(),
        [
            ignore_list(&["@bob:example.org", "@carol:example.org"]),
            ignore_list(&[
                "@bob:example.org",
                "@carol:example.org",
                "@dave:example.org"
            ]),
        ]
    );
    let events = seen.shut_down();
    assert_eq!(acks(&events), 3, "{events:#?}");
    harness.join_stopped();
}

#[test]
fn unignoring_after_ignoring_keeps_the_others() {
    let harness = ignoring_bob();
    let server = DocumentServer::ignore_list(&harness, &["@bob:example.org"], Duration::ZERO);
    let mut seen = Seen::new(&harness);

    harness.send(Command::IgnoreUser("@carol:example.org".to_owned()));
    harness.send(Command::UnignoreUser("@bob:example.org".to_owned()));
    seen.until_acks(2);

    assert_eq!(
        server.puts(),
        [
            ignore_list(&["@bob:example.org", "@carol:example.org"]),
            ignore_list(&["@carol:example.org"]),
        ]
    );
    let events = seen.shut_down();
    assert_eq!(acks(&events), 3, "{events:#?}");
    harness.join_stopped();
}

#[test]
fn unignoring_then_ignoring_writes_one_after_the_other() {
    let harness = ignoring_bob();
    let delay = Duration::from_millis(500);
    let server = DocumentServer::ignore_list(&harness, &["@bob:example.org"], delay);
    let mut seen = Seen::new(&harness);

    harness.send(Command::UnignoreUser("@bob:example.org".to_owned()));
    harness.send(Command::IgnoreUser("@carol:example.org".to_owned()));
    seen.until_acks(2);

    let requests = server.requests();
    let methods = requests
        .iter()
        .map(|request| request.method)
        .collect::<Vec<_>>();
    assert_eq!(methods, ["GET", "PUT", "GET", "PUT"], "{requests:#?}");
    assert_eq!(requests[1].body, ignore_list(&[]));
    assert_eq!(requests[3].body, ignore_list(&["@carol:example.org"]));
    // The second read-modify-write reads only after the first write was
    // answered: the Account lane runs them one at a time.
    let (first_put_at, second_get_at) = (requests[1].at, requests[2].at);
    assert!(
        second_get_at.duration_since(first_put_at) >= delay,
        "{:?} apart",
        second_get_at.duration_since(first_put_at)
    );

    let events = seen.shut_down();
    assert_eq!(acks(&events), 3, "{events:#?}");
    harness.join_stopped();
}

const DM_BOB: &str = "!dm-bob:example.org";
const DM_CAROL: &str = "!dm-carol:example.org";

/// A session invited to a direct room by each inviter: `(room, inviter)`.
/// The server lets it join them, and answers their members.
fn invited_to_direct_rooms(invites: &[(&'static str, &'static str)]) -> Harness {
    let (harness, _) = Harness::restored_with_sync(|sync| {
        for (room, inviter) in invites {
            let room_id = matrix_sdk::ruma::OwnedRoomId::try_from(*room).expect("a room ID");
            sync.add_invited_room(InvitedRoomBuilder::new(&room_id).add_state_bulk([
                stripped(json!({
                    "type": "m.room.member",
                    "state_key": inviter,
                    "sender": inviter,
                    "content": { "membership": "join" },
                })),
                stripped(json!({
                    "type": "m.room.member",
                    "state_key": "@alice:example.org",
                    "sender": inviter,
                    "content": { "membership": "invite", "is_direct": true },
                })),
            ]));
        }
    });
    let invites = invites.to_vec();
    harness.mount(
        Mock::given(method("POST"))
            .and(path_regex(r"^/_matrix/client/v3/rooms/[^/]+/join$"))
            .respond_with(|request: &Request| {
                ResponseTemplate::new(200).set_body_json(json!({ "room_id": path_room(request) }))
            }),
    );
    harness.mount(
        Mock::given(method("GET"))
            .and(path_regex(r"^/_matrix/client/v3/rooms/[^/]+/members$"))
            .respond_with(move |request: &Request| {
                let room = path_room(request);
                let (_, inviter) = invites
                    .iter()
                    .find(|(invited, _)| *invited == room)
                    .expect("an invited room");
                ResponseTemplate::new(200).set_body_json(json!({ "chunk": [
                    member_join(&room, inviter),
                    member_join(&room, "@alice:example.org"),
                ] }))
            }),
    );
    harness
}

fn stripped(event: serde_json::Value) -> Raw<AnyStrippedStateEvent> {
    Raw::from_json_string(event.to_string()).expect("a stripped state event")
}

/// The room ID in a `/rooms/{room_id}/...` path.
fn path_room(request: &Request) -> String {
    let segment = request.url.path_segments().expect("a path").nth(4);
    segment.expect("a room ID segment").to_owned()
}

fn member_join(room: &str, user_id: &str) -> serde_json::Value {
    json!({
        "type": "m.room.member",
        "room_id": room,
        "state_key": user_id,
        "sender": user_id,
        "event_id": format!("$join-{}", user_id.trim_start_matches('@')),
        "origin_server_ts": 1,
        "content": { "membership": "join" },
    })
}

fn accept(room_id: &str) -> Command {
    Command::AcceptRoomInvite(AcceptRoomInviteRequest {
        room_id: room_id.to_owned(),
    })
}

/// Each read of the document came after the write before it was answered:
/// the read-modify-writes never interleaved.
fn assert_writes_one_after_the_other(requests: &[DocumentRequest], delay: Duration) {
    let methods = requests
        .iter()
        .map(|request| request.method)
        .collect::<Vec<_>>();
    assert_eq!(
        methods,
        ["GET", "PUT"].repeat(requests.len() / 2),
        "{requests:#?}"
    );
    for pair in requests.windows(2).skip(1).step_by(2) {
        let (put_at, get_at) = (pair[0].at, pair[1].at);
        assert!(
            get_at.duration_since(put_at) >= delay,
            "{:?} apart: {requests:#?}",
            get_at.duration_since(put_at)
        );
    }
}

#[test]
fn accepting_two_direct_invites_at_once_keeps_both() {
    let harness = invited_to_direct_rooms(&[
        (DM_BOB, "@bob:example.org"),
        (DM_CAROL, "@carol:example.org"),
    ]);
    let delay = Duration::from_millis(300);
    let server = DocumentServer::direct_rooms(&harness, delay);
    let mut seen = Seen::new(&harness);

    // Two rooms' lanes: the accepts run side by side.
    harness.send(accept(DM_BOB));
    harness.send(accept(DM_CAROL));
    seen.until_acks(2);

    let puts = server.puts();
    assert_eq!(
        puts.last(),
        Some(&json!({
            "@bob:example.org": [DM_BOB],
            "@carol:example.org": [DM_CAROL],
        })),
        "{:#?}",
        server.requests()
    );
    assert_writes_one_after_the_other(&server.requests(), delay);
    let events = seen.shut_down();
    assert!(
        !events.iter().any(|event| matches!(event, Event::Error(_))),
        "{events:#?}"
    );
    harness.join_stopped();
}

#[test]
fn creating_a_direct_room_while_accepting_one_keeps_both() {
    let harness = invited_to_direct_rooms(&[(DM_BOB, "@bob:example.org")]);
    harness.mount(
        Mock::given(method("POST"))
            .and(path_regex(r"^/_matrix/client/v3/createRoom$"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(json!({ "room_id": "!dm-dave:example.org" })),
            ),
    );
    let delay = Duration::from_millis(300);
    let server = DocumentServer::direct_rooms(&harness, delay);
    let mut seen = Seen::new(&harness);

    harness.send(Command::CreateDirectRoom(CreateDirectRoomRequest {
        user_id: "@dave:example.org".to_owned(),
    }));
    harness.send(accept(DM_BOB));
    seen.until_acks(2);

    assert_eq!(
        server.puts().last(),
        Some(&json!({
            "@bob:example.org": [DM_BOB],
            "@dave:example.org": ["!dm-dave:example.org"],
        })),
        "{:#?}",
        server.requests()
    );
    assert_writes_one_after_the_other(&server.requests(), delay);
    let events = seen.shut_down();
    assert!(
        !events.iter().any(|event| matches!(event, Event::Error(_))),
        "{events:#?}"
    );
    harness.join_stopped();
}

fn power_levels(users: serde_json::Value) -> serde_json::Value {
    json!({ "users": users })
}

/// A session in `ROOM` with Bob, Carol and Dave, whose power levels are
/// `content` in the store and on the server; the server keeps them as a
/// `DocumentServer`.
fn ranking_in(content: serde_json::Value, delay: Duration) -> (Harness, DocumentServer) {
    let state = |event: serde_json::Value| {
        Raw::from_json_string(event.to_string()).expect("a raw state event")
    };
    let member = |user_id: &str| {
        state(json!({
            "type": "m.room.member",
            "state_key": user_id,
            "sender": user_id,
            "event_id": format!("$join-{}", user_id.trim_start_matches('@')),
            "origin_server_ts": 2,
            "content": { "membership": "join" },
        }))
    };
    let store_content = content.clone();
    let (harness, _) = Harness::restored_with_sync(move |sync| {
        sync.add_joined_room(
            JoinedRoomBuilder::new(
                &matrix_sdk::ruma::OwnedRoomId::try_from(ROOM).expect("a room ID"),
            )
            .add_state_bulk([
                state(json!({
                    "type": "m.room.create",
                    "state_key": "",
                    "sender": "@alice:example.org",
                    "event_id": "$create",
                    "origin_server_ts": 1,
                    "content": { "creator": "@alice:example.org" },
                })),
                state(json!({
                    "type": "m.room.power_levels",
                    "state_key": "",
                    "sender": "@alice:example.org",
                    "event_id": "$power",
                    "origin_server_ts": 1,
                    "content": store_content,
                })),
                state(alice_member_event()),
                member("@bob:example.org"),
                member("@carol:example.org"),
                member("@dave:example.org"),
            ]),
        );
    });
    // The members snapshot after a write asks for them.
    harness.mount(
        Mock::given(method("GET"))
            .and(path_regex(
                r"^/_matrix/client/v3/rooms/!a:example\.org/members$",
            ))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "chunk": [
                member_join(ROOM, "@alice:example.org"),
                member_join(ROOM, "@bob:example.org"),
                member_join(ROOM, "@carol:example.org"),
                member_join(ROOM, "@dave:example.org"),
            ] }))),
    );
    let server = DocumentServer::mount(
        &harness,
        r"^/_matrix/client/v3/rooms/!a:example\.org/state/m\.room\.power_levels/?$",
        Some(content),
        json!({ "event_id": "$power-update" }),
        delay,
    );
    (harness, server)
}

fn rank(changes: &[(&str, MemberPowerRole)]) -> Command {
    Command::SetRoomMemberPowerLevel(SetRoomMemberPowerLevelRequest {
        room_id: ROOM.to_owned(),
        changes: changes
            .iter()
            .map(|(user_id, role)| MemberPowerLevelChange {
                user_id: (*user_id).to_owned(),
                role: *role,
            })
            .collect(),
    })
}

#[test]
fn two_rank_changes_in_a_row_keep_both() {
    let delay = Duration::from_millis(300);
    let (harness, server) = ranking_in(power_levels(json!({ "@alice:example.org": 100 })), delay);
    let mut seen = Seen::new(&harness);

    // The second comes before any sync echoes the first.
    harness.send(rank(&[("@bob:example.org", MemberPowerRole::Moderator)]));
    harness.send(rank(&[("@carol:example.org", MemberPowerRole::Voiced)]));
    seen.until_acks(2);

    assert_eq!(
        server.puts(),
        [
            power_levels(json!({ "@alice:example.org": 100, "@bob:example.org": 50 })),
            power_levels(json!({
                "@alice:example.org": 100,
                "@bob:example.org": 50,
                "@carol:example.org": 1,
            })),
        ],
        "{:#?}",
        server.requests()
    );
    assert_writes_one_after_the_other(&server.requests(), delay);
    let events = seen.shut_down();
    assert!(
        !events.iter().any(|event| matches!(event, Event::Error(_))),
        "{events:#?}"
    );
    harness.join_stopped();
}

#[test]
fn several_members_change_in_one_write() {
    let (harness, server) = ranking_in(
        power_levels(json!({ "@alice:example.org": 100, "@dave:example.org": 50 })),
        Duration::ZERO,
    );
    let mut seen = Seen::new(&harness);

    harness.send(rank(&[
        ("@bob:example.org", MemberPowerRole::Moderator),
        ("@carol:example.org", MemberPowerRole::Voiced),
        ("@dave:example.org", MemberPowerRole::Default),
    ]));
    seen.until_acks(1);

    let requests = server.requests();
    let methods = requests
        .iter()
        .map(|request| request.method)
        .collect::<Vec<_>>();
    assert_eq!(methods, ["GET", "PUT"], "{requests:#?}");
    assert_eq!(
        requests[1].body,
        power_levels(json!({
            "@alice:example.org": 100,
            "@bob:example.org": 50,
            "@carol:example.org": 1,
        }))
    );
    let events = seen.shut_down();
    assert!(
        !events.iter().any(|event| matches!(event, Event::Error(_))),
        "{events:#?}"
    );
    harness.join_stopped();
}

#[test]
fn logout_cancels_hung_work_and_acks_it() {
    let harness = Harness::restored_in_rooms(&[ROOM], &[]);
    harness.hang("GET", r"^/_matrix/client/v3/rooms/.*/messages");
    harness.runtime.block_on(async {
        harness
            .server
            .mock_logout()
            .expect_any_access_token()
            .ok()
            .mount()
            .await;
    });
    let mut seen = Seen::new(&harness);

    harness.send(backfill(ROOM));
    harness.wait_for_requests("GET", "/messages", 1);
    harness.send(common::log_out_anyway());
    seen.until("LoggedOut", |event| *event == Event::LoggedOut);
    seen.until_acks(2);
    let shape: Vec<_> = seen
        .events
        .iter()
        .filter(|event| matches!(event, Event::CommandHandled | Event::LoggedOut))
        .collect();
    // The backfill's ack, then the logout and its ack.
    assert_eq!(
        shape,
        [
            &Event::CommandHandled,
            &Event::LoggedOut,
            &Event::CommandHandled
        ]
    );
    assert!(
        !seen.events.iter().any(is_backfill_answer),
        "{:#?}",
        seen.events
    );

    harness.send(Command::FetchUserProfile("@bob:example.org".to_owned()));
    let refusal = seen.until("the refusal", |event| matches!(event, Event::Error(_)));
    let Event::Error(refusal) = refusal else {
        unreachable!()
    };
    assert_eq!(
        refusal.message,
        "fetch user profile requires an authenticated session"
    );

    let events = seen.shut_down();
    assert_eq!(acks(&events), 4, "{events:#?}");
    harness.join_stopped();
}

/// What comes for the session while its logout waits for the server goes
/// with it, as its jobs do: an automatic read receipt sent just before the
/// answer is no error after it.
#[test]
fn a_logout_drops_the_commands_that_came_for_its_session() {
    let harness = Harness::restored_in_rooms(&[ROOM], &[]);
    harness.mount(
        Mock::given(method("POST"))
            .and(path_regex(r"^/_matrix/client/v3/logout$"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(json!({}))
                    .set_delay(Duration::from_millis(300)),
            ),
    );
    let mut seen = Seen::new(&harness);

    harness.send(common::log_out_anyway());
    harness.wait_for_requests("POST", "/logout", 1);
    harness.send(Command::MarkRoomRead(mirx_core::MarkRoomReadRequest {
        room_id: ROOM.to_owned(),
        event_id: "$alice-join".to_owned(),
    }));
    harness.send(Command::StartSync);
    seen.until("LoggedOut", |event| *event == Event::LoggedOut);
    seen.until_acks(3);

    let events = seen.shut_down();
    assert!(
        !events.iter().any(|event| matches!(event, Event::Error(_))),
        "{events:#?}"
    );
    assert_eq!(acks(&events), 4, "{events:#?}");
    harness.join_stopped();
}

/// A sign-in sent during a logout is not for the session: it runs once
/// the logout ends.
#[test]
fn a_sign_in_sent_during_a_logout_runs_after_it() {
    let harness = Harness::restored_in_rooms(&[ROOM], &[]);
    harness.mount(
        Mock::given(method("POST"))
            .and(path_regex(r"^/_matrix/client/v3/logout$"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(json!({}))
                    .set_delay(Duration::from_millis(300)),
            ),
    );
    harness.mount(
        Mock::given(method("POST"))
            .and(path_regex(r"^/_matrix/client/v3/login$"))
            .respond_with(ResponseTemplate::new(403).set_body_json(json!({
                "errcode": "M_FORBIDDEN",
                "error": "wrong password",
            }))),
    );
    let mut seen = Seen::new(&harness);

    harness.send(common::log_out_anyway());
    harness.wait_for_requests("POST", "/logout", 1);
    harness.send(password_login(&harness));
    seen.until("LoggedOut", |event| *event == Event::LoggedOut);
    seen.until("the sign-in's error", |event| {
        is_error_of(event, Operation::Login)
    });
    assert_eq!(harness.requests("POST", "/login").len(), 1);

    let events = seen.shut_down();
    assert_eq!(acks(&events), 3, "{events:#?}");
    harness.join_stopped();
}

#[test]
fn every_command_is_acked_once_by_stopped() {
    let harness = Harness::restored_in_rooms(&[ROOM], &[]);
    harness.hang("GET", r"^/_matrix/client/v3/rooms/.*/messages");
    mount_sends(&harness);
    let commands = [
        Command::FetchUserProfile("@bob:example.org".to_owned()),
        backfill(ROOM),
        line(ROOM, "hello"),
        Command::MarkRoomRead(mirx_core::MarkRoomReadRequest {
            room_id: ROOM.to_owned(),
            event_id: "$alice-join".to_owned(),
        }),
        Command::SetDisplayName(mirx_core::SetDisplayNameRequest {
            display_name: "Alice".to_owned(),
        }),
        Command::IgnoreUser("@carol:example.org".to_owned()),
        Command::ListUserDevices("@alice:example.org".to_owned()),
        Command::RefreshE2eeStatus,
        Command::ListPublicRooms(ListPublicRoomsRequest {
            server: None,
            filter: None,
            since: None,
            limit: 20,
        }),
        Command::FetchRoomEvent(mirx_core::FetchRoomEventRequest {
            room_id: ROOM.to_owned(),
            event_id: "$missing".to_owned(),
        }),
        Command::StartSync,
        Command::StopSync,
    ];
    let sent = commands.len();
    for command in commands {
        harness.send(command);
    }
    let mut seen = Seen::new(&harness);
    // Every command but the backfill answers.
    seen.until_acks(sent - 1);

    let events = seen.shut_down();
    assert_eq!(acks(&events), sent + 1, "{events:#?}");
    harness.join_stopped();
}

fn is_error_of(event: &Event, operation: Operation) -> bool {
    matches!(event, Event::Error(error) if error.operation == operation)
}

#[test]
fn a_failed_logout_keeps_the_session_whole() {
    let harness = Harness::restored_in_rooms(&[ROOM], &[]);
    // Not a 5xx, which the client retries for seconds first.
    harness.mount(
        Mock::given(method("POST"))
            .and(path_regex(r"^/_matrix/client/v3/logout$"))
            .respond_with(ResponseTemplate::new(403).set_body_json(json!({
                "errcode": "M_FORBIDDEN",
                "error": "not today",
            }))),
    );
    mount_bob_profile(&harness);
    mount_sends(&harness);
    let mut seen = Seen::new(&harness);
    harness.send(Command::StartSync);
    seen.until("SyncConnected", |event| *event == Event::SyncConnected);

    harness.send(common::log_out_anyway());
    seen.until("LoggingOut", |event| *event == Event::LoggingOut);
    seen.until("the logout error", |event| {
        is_error_of(event, Operation::Logout)
    });
    seen.until_acks(2);
    let syncs = harness.requests("GET", "/sync").len();
    // The sync runs again.
    harness.wait_for_requests("GET", "/sync", syncs + 2);

    harness.send(Command::FetchUserProfile("@bob:example.org".to_owned()));
    seen.until("the profile", |event| {
        matches!(event, Event::UserProfile(_))
    });
    // The send queue's task is alive: the line's local echo comes.
    harness.send(line(ROOM, "still here"));
    seen.until(
        "the local echo",
        |event| matches!(event, Event::RoomActivity(activity) if activity.room_id == ROOM),
    );

    let events = seen.shut_down();
    assert!(
        !events
            .iter()
            .any(|event| matches!(event, Event::LoggedOut | Event::SyncStopped(_))),
        "{events:#?}"
    );
    harness.join_stopped();
}

/// File > Disconnect during a Log Out the server does not answer: the
/// logout stops, the session stays signed in with sync stopped, and the
/// commands that waited for the logout run.
#[test]
fn disconnecting_stops_a_logout_under_way() {
    let harness = Harness::restored_in_rooms(&[ROOM], &[]);
    harness.hang("POST", r"^/_matrix/client/v3/logout$");
    mount_bob_profile(&harness);
    let mut seen = Seen::new(&harness);
    harness.send(Command::StartSync);
    seen.until("SyncConnected", |event| *event == Event::SyncConnected);
    seen.until_acks(1);

    harness.send(common::log_out_anyway());
    seen.until("LoggingOut", |event| *event == Event::LoggingOut);
    harness.wait_for_requests("POST", "/logout", 1);
    // Waits for the logout's end.
    harness.send(Command::FetchUserProfile("@bob:example.org".to_owned()));
    harness.send(Command::StopSync);
    let from = seen.events.len();
    seen.until("LogoutCancelled", |event| *event == Event::LogoutCancelled);
    // The logout's ack, the stop's, then the profile's after its answer.
    seen.until("the profile", |event| {
        matches!(event, Event::UserProfile(_))
    });
    seen.until_acks(4);
    let answers: Vec<_> = seen.events[from..]
        .iter()
        .filter(|event| {
            matches!(
                event,
                Event::LogoutCancelled | Event::CommandHandled | Event::UserProfile(_)
            )
        })
        .collect();
    assert!(
        matches!(
            answers[..],
            [
                Event::LogoutCancelled,
                Event::CommandHandled,
                Event::CommandHandled,
                Event::UserProfile(_),
                Event::CommandHandled,
            ]
        ),
        "{answers:#?}"
    );

    // Sync stays stopped: nothing started it again.
    assert!(
        !seen.events[from..]
            .iter()
            .any(|event| matches!(event, Event::SyncStarted | Event::LoggedOut)),
        "{:#?}",
        seen.events
    );
    // Still in the session: Connect syncs again.
    harness.send(Command::StartSync);
    seen.until("SyncConnected again", |event| {
        *event == Event::SyncConnected
    });

    let events = seen.shut_down();
    assert!(!events.contains(&Event::LoggedOut), "{events:#?}");
    harness.join_stopped();
}

fn password_login(harness: &Harness) -> Command {
    Command::Login(LoginRequest {
        server: harness.server.uri(),
        method: LoginMethod::Password {
            username: "example".to_owned(),
            password: "wordpass".to_owned(),
        },
        device_display_name: "mirx".to_owned(),
        start_sync: false,
    })
}

#[test]
fn cancel_login_stops_a_hung_password_sign_in() {
    let harness = Harness::idle();
    // The first sign-in's request hangs; the next is answered.
    let first = AtomicBool::new(true);
    harness.mount(
        Mock::given(method("POST"))
            .and(path_regex(r"^/_matrix/client/v3/login$"))
            .respond_with(move |_: &Request| {
                if first.swap(false, Ordering::SeqCst) {
                    ResponseTemplate::new(200).set_delay(Duration::from_secs(3600))
                } else {
                    ResponseTemplate::new(200).set_body_json(json!({
                        "user_id": "@example:localhost",
                        "access_token": "abc123",
                        "device_id": "GHTYAJCE",
                    }))
                }
            }),
    );
    let mut seen = Seen::new(&harness);

    harness.send(password_login(&harness));
    harness.wait_for_requests("POST", "/login", 1);
    harness.send(Command::CancelLogin);
    seen.until("LoginCancelled", |event| *event == Event::LoginCancelled);
    // The login's ack, then the cancel's.
    seen.until_acks(2);
    assert!(
        !seen
            .events
            .iter()
            .any(|event| matches!(event, Event::LoggedIn { .. })),
        "{:#?}",
        seen.events
    );

    // Idle again, with nothing left over: the next sign-in goes through.
    harness.send(password_login(&harness));
    seen.until("LoggedIn", |event| matches!(event, Event::LoggedIn { .. }));
    seen.until_acks(3);

    let events = seen.shut_down();
    assert_eq!(acks(&events), 4, "{events:#?}");
    harness.join_stopped();
}

#[test]
fn cancel_login_stops_a_hung_restore_before_it_is_announced() {
    let harness = Harness::idle();
    harness.hang("GET", r"^/_matrix/client/v3/sync");
    let mut seen = Seen::new(&harness);

    harness.send(Command::RestoreSession(RestoreSessionRequest {
        homeserver_url: harness.server.uri(),
        session: StoredSession {
            user_id: "@alice:example.org".to_owned(),
            device_id: "MIRX".to_owned(),
            access_token: "TOKEN".to_owned(),
            refresh_token: None,
            auth: SessionAuth::Matrix,
        },
        start_sync: false,
    }));
    harness.wait_for_requests("GET", "/sync", 1);
    harness.send(Command::CancelLogin);
    seen.until("LoginCancelled", |event| *event == Event::LoginCancelled);
    seen.until_acks(2);

    let events = seen.shut_down();
    assert!(
        !events
            .iter()
            .any(|event| matches!(event, Event::RestoredSession | Event::SyncedRooms(_))),
        "{events:#?}"
    );
    assert_eq!(acks(&events), 3, "{events:#?}");
    harness.join_stopped();
}

#[test]
fn shutdown_never_waits_for_a_sign_in() {
    let harness = Harness::idle();
    harness.hang("POST", r"^/_matrix/client/v3/login$");
    let seen = Seen::new(&harness);

    harness.send(password_login(&harness));
    harness.wait_for_requests("POST", "/login", 1);
    // It waits for the sign-in.
    harness.send(Command::FetchUserProfile("@bob:example.org".to_owned()));
    let events = seen.shut_down();
    // The login's, the profile's and the shutdown's.
    assert_eq!(acks(&events), 3, "{events:#?}");
    harness.join_stopped();
}

#[test]
fn commands_sent_during_a_restore_run_after_it() {
    let harness = Harness::idle();
    harness.mount(
        Mock::given(method("GET"))
            .and(path_regex(r"^/_matrix/client/v3/sync"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(json!({ "next_batch": "s1" }))
                    .set_delay(Duration::from_millis(300)),
            )
            .with_priority(1),
    );
    mount_bob_profile(&harness);
    let mut seen = Seen::new(&harness);

    harness.send(Command::RestoreSession(RestoreSessionRequest {
        homeserver_url: harness.server.uri(),
        session: StoredSession {
            user_id: "@alice:example.org".to_owned(),
            device_id: "MIRX".to_owned(),
            access_token: "TOKEN".to_owned(),
            refresh_token: None,
            auth: SessionAuth::Matrix,
        },
        start_sync: false,
    }));
    harness.send(Command::FetchUserProfile("@bob:example.org".to_owned()));
    seen.until("the profile", |event| {
        matches!(event, Event::UserProfile(_))
    });

    let restored = seen
        .events
        .iter()
        .position(|event| *event == Event::RestoredSession)
        .unwrap_or_else(|| panic!("RestoredSession in {:#?}", seen.events));
    let profile = seen
        .events
        .iter()
        .position(|event| matches!(event, Event::UserProfile(_)))
        .expect("the profile");
    assert!(restored < profile, "{:#?}", seen.events);
    assert!(
        !seen
            .events
            .iter()
            .any(|event| is_error_of(event, Operation::FetchUserProfile)),
        "{:#?}",
        seen.events
    );
    let events = seen.shut_down();
    assert_eq!(acks(&events), 3, "{events:#?}");
    harness.join_stopped();
}
