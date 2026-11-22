//! The core answers commands while others wait for the server: commands
//! run in lanes, one after another within a lane and side by side across
//! lanes. These tests hang requests on purpose (`Harness::hang`); they
//! live in their own binary because a hung wiremock responder outlives the
//! test in the pooled server.

mod common;

use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use common::Harness;
use mirx_core::{
    BackfillRoomHistoryRequest, Command, EditMessageRequest, Event, HistoryFrom,
    ListPublicRoomsRequest, LoginMethod, LoginRequest, Operation, RestoreSessionRequest,
    RoomMessagePresentationKind, SendFileRequest, SendMessageRequest, SessionAuth, StoredSession,
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

/// Answers after `delay`, and records when each request came.
struct Recorded {
    arrivals: std::sync::Arc<std::sync::Mutex<Vec<(Instant, serde_json::Value)>>>,
    delay: Duration,
}

impl Respond for Recorded {
    fn respond(&self, request: &Request) -> ResponseTemplate {
        self.arrivals
            .lock()
            .expect("arrivals lock")
            .push((Instant::now(), body(request)));
        ResponseTemplate::new(200)
            .set_body_json(json!({}))
            .set_delay(self.delay)
    }
}

#[test]
fn unignoring_then_ignoring_writes_one_after_the_other() {
    let harness = Harness::restored_in_rooms(
        &[ROOM],
        &[json!({
            "type": "m.ignored_user_list",
            "content": { "ignored_users": { "@bob:example.org": {} } },
        })],
    );
    let delay = Duration::from_millis(500);
    let arrivals = std::sync::Arc::default();
    harness.mount(
        Mock::given(method("PUT"))
            .and(path_regex(r"/account_data/m\.ignored_user_list$"))
            .respond_with(Recorded {
                arrivals: std::sync::Arc::clone(&arrivals),
                delay,
            })
            .with_priority(1),
    );
    let mut seen = Seen::new(&harness);

    harness.send(Command::UnignoreUser("@bob:example.org".to_owned()));
    harness.send(Command::IgnoreUser("@bob:example.org".to_owned()));
    seen.until_acks(2);

    let arrivals = arrivals.lock().expect("arrivals lock").clone();
    assert_eq!(arrivals.len(), 2, "{arrivals:#?}");
    let ((first_at, first), (second_at, second)) = (&arrivals[0], &arrivals[1]);
    assert_eq!(first["ignored_users"], json!({}));
    assert_eq!(second["ignored_users"], json!({ "@bob:example.org": {} }));
    // The second write starts after the first was answered.
    assert!(
        second_at.duration_since(*first_at) >= delay,
        "{:?} apart",
        second_at.duration_since(*first_at)
    );

    let events = seen.shut_down();
    assert_eq!(acks(&events), 3, "{events:#?}");
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
    // Not a 5xx, which the SDK retries.
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
