use std::sync::mpsc::{self, TryRecvError};
use std::time::Duration;

use tokio::sync::mpsc::unbounded_channel;

use super::core::Core;
use super::{AccountLists, EventSender, RoomSnapshot, RoomSnapshots};
use crate::api::CoreConfig;
use crate::api::{
    AcceptRoomInviteRequest, AwayChanged, BanRoomMemberRequest, Command, CreateDirectRoomRequest,
    CreateRoomRequest, CreateRoomVisibility, Event, HighlightWord, JoinRoomRequest,
    KickRoomMemberRequest, LoginMethod, LoginRequest, Operation, PresenceKind,
    RestoreSessionRequest, RoomJoined, RoomMemberRank, RoomMemberSummary, RoomMembers,
    RoomMembership, RoomNotificationMode, RoomReadState, RoomSummary, SyncStopReason,
};
use crate::api::{
    RoomActivityKind, RoomActivityUpdate, RoomActivityUpdateKind, RoomActivityUpdateTarget,
    RoomMessagePresentationKind, SendMessageRequest, SetTypingRequest,
};
use matrix_sdk::{
    ruma::{
        OwnedEventId, OwnedRoomId,
        events::{AnySyncStateEvent, room::member::RoomMemberEvent},
        serde::Raw,
    },
    test_utils::mocks::MatrixMockServer,
};
use matrix_sdk_test::{InvitedRoomBuilder, JoinedRoomBuilder};

fn room_id() -> OwnedRoomId {
    OwnedRoomId::try_from("!room:example.org").expect("valid room id")
}

fn joined_members() -> Vec<Raw<RoomMemberEvent>> {
    [
        serde_json::json!({
            "type": "m.room.member", "state_key": "@example:localhost",
            "sender": "@example:localhost", "event_id": "$actor:localhost",
            "origin_server_ts": 1u64,
            "content": { "membership": "join", "displayname": "Actor" }
        }),
        serde_json::json!({
            "type": "m.room.member", "state_key": "@target:example.org",
            "sender": "@target:example.org", "event_id": "$target:example.org",
            "origin_server_ts": 2u64,
            "content": { "membership": "join", "displayname": "Target" }
        }),
    ]
    .into_iter()
    .map(|event| Raw::from_json_string(event.to_string()).expect("raw member event"))
    .collect()
}

fn joined_member_state() -> Vec<Raw<AnySyncStateEvent>> {
    [
        serde_json::json!({
            "type": "m.room.create", "state_key": "", "sender": "@example:localhost",
            "event_id": "$create:localhost", "origin_server_ts": 0u64,
            "content": { "creator": "@example:localhost" }
        }),
        serde_json::json!({
            "type": "m.room.member", "state_key": "@example:localhost",
            "sender": "@example:localhost", "event_id": "$actor:localhost",
            "origin_server_ts": 1u64,
            "content": { "membership": "join", "displayname": "Actor" }
        }),
        serde_json::json!({
            "type": "m.room.member", "state_key": "@target:example.org",
            "sender": "@target:example.org", "event_id": "$target:example.org",
            "origin_server_ts": 2u64,
            "content": { "membership": "join", "displayname": "Target" }
        }),
    ]
    .into_iter()
    .map(|event| Raw::from_json_string(event.to_string()).expect("raw state event"))
    .collect()
}

/// Runs `command` on `core` until its ack.
async fn run(core: &mut Core, command: Command) {
    assert!(core.run_until_idle(command).await.is_continue());
}

async fn assert_ready_moderation_success(command: Command, path: &str, mock_ban: bool) {
    let mock_server = MatrixMockServer::new().await;
    let client = mock_server.client_builder().build().await;
    mock_server
        .sync_room(
            &client,
            JoinedRoomBuilder::new(room_id().as_ref()).add_state_bulk(joined_member_state()),
        )
        .await;
    mock_server
        .mock_get_members()
        .ok(joined_members())
        .mock_once()
        .mount()
        .await;
    if mock_ban {
        mock_server.mock_ban_user().ok().expect(1).mount().await;
    } else {
        mock_server.mock_kick_user().ok().expect(1).mount().await;
    }

    let (event_tx, event_rx) = mpsc::channel::<Event>();
    let mut core = Core::for_client(client, event_tx.clone());
    run(&mut core, command).await;

    let mut answers = drain_events(&event_rx).into_iter();
    match answers.next().expect("authoritative room members snapshot") {
        Event::RoomMembers(snapshot) => {
            assert_eq!(snapshot.room_id, "!room:example.org");
            assert!(
                snapshot
                    .members
                    .iter()
                    .any(|member| member.user_id == "@target:example.org")
            );
        }
        other => panic!("expected room members snapshot, got {other:?}"),
    }
    assert_eq!(answers.next(), None);
    assert_eq!(
        mock_server
            .received_requests()
            .await
            .expect("request recording enabled")
            .iter()
            .filter(|request| request.url.path().ends_with(path))
            .count(),
        1
    );
    core.stop().await;
}

#[test]
fn ready_kick_submits_once_then_emits_authoritative_members_snapshot() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("build tokio runtime");
    tokio::task::LocalSet::new().block_on(&runtime, async {
        assert_ready_moderation_success(
            Command::KickRoomMember(KickRoomMemberRequest {
                room_id: "!room:example.org".to_owned(),
                user_id: "@target:example.org".to_owned(),
                reason: None,
            }),
            "/kick",
            false,
        )
        .await;
    });
}

#[test]
fn ready_ban_submits_once_then_emits_authoritative_members_snapshot() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("build tokio runtime");
    tokio::task::LocalSet::new().block_on(&runtime, async {
        assert_ready_moderation_success(
            Command::BanRoomMember(BanRoomMemberRequest {
                room_id: "!room:example.org".to_owned(),
                user_id: "@target:example.org".to_owned(),
                reason: None,
            }),
            "/ban",
            true,
        )
        .await;
    });
}

async fn assert_ready_moderation_rejection(command: Command, operation: Operation, path: &str) {
    let mock_server = MatrixMockServer::new().await;
    let client = mock_server.client_builder().build().await;
    let room = mock_server
        .sync_room(
            &client,
            JoinedRoomBuilder::new(room_id().as_ref()).add_state_bulk(joined_member_state()),
        )
        .await;
    assert!(
        room.members_no_sync(matrix_sdk::RoomMemberships::JOIN)
            .await
            .expect("cached members")
            .iter()
            .any(|member| member.user_id().as_str() == "@target:example.org")
    );

    let (event_tx, event_rx) = mpsc::channel::<Event>();
    let mut core = Core::for_client(client, event_tx.clone());
    run(&mut core, command).await;

    let mut answers = drain_events(&event_rx).into_iter();
    match answers.next().expect("moderation rejection") {
        Event::Error(error) => assert_eq!(error.operation, operation),
        other => panic!("expected moderation error, got {other:?}"),
    }
    assert_eq!(answers.next(), None);
    assert_eq!(
        mock_server
            .received_requests()
            .await
            .expect("request recording enabled")
            .iter()
            .filter(|request| request.url.path().ends_with(path))
            .count(),
        1
    );
    assert!(
        room.members_no_sync(matrix_sdk::RoomMemberships::JOIN)
            .await
            .expect("cached members after rejection")
            .iter()
            .any(|member| member.user_id().as_str() == "@target:example.org")
    );
    core.stop().await;
}

#[test]
fn ready_kick_rejection_emits_error_without_snapshot_or_optimistic_removal() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("build tokio runtime");
    tokio::task::LocalSet::new().block_on(&runtime, async {
        assert_ready_moderation_rejection(
            Command::KickRoomMember(KickRoomMemberRequest {
                room_id: "!room:example.org".to_owned(),
                user_id: "@target:example.org".to_owned(),
                reason: None,
            }),
            Operation::KickRoomMember,
            "/kick",
        )
        .await;
    });
}

#[test]
fn ready_ban_rejection_emits_error_without_snapshot_or_optimistic_removal() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("build tokio runtime");
    tokio::task::LocalSet::new().block_on(&runtime, async {
        assert_ready_moderation_rejection(
            Command::BanRoomMember(BanRoomMemberRequest {
                room_id: "!room:example.org".to_owned(),
                user_id: "@target:example.org".to_owned(),
                reason: None,
            }),
            Operation::BanRoomMember,
            "/ban",
        )
        .await;
    });
}

fn test_config() -> CoreConfig {
    CoreConfig {
        profile_dir: std::env::temp_dir().join("mirx-core-runtime-test"),
        store_passphrase: "test-store-passphrase".to_owned(),
    }
}

/// Mounts `POST /join/<target>` answering with `room_id`; `target` is
/// as it appears in the URL, percent-encoded.
async fn mock_join(mock_server: &MatrixMockServer, target: &str, room_id: &str) {
    wiremock::Mock::given(wiremock::matchers::method("POST"))
        .and(wiremock::matchers::path(format!(
            "/_matrix/client/v3/join/{target}"
        )))
        .respond_with(
            wiremock::ResponseTemplate::new(200)
                .set_body_json(serde_json::json!({ "room_id": room_id })),
        )
        .expect(1)
        .mount(mock_server.server())
        .await;
}

/// Runs `command` against a ready client and asserts that the room
/// snapshot listing `room_id` comes first, then `RoomJoined`.
async fn assert_join_emits_snapshot_then_room_joined(
    mock_server: &MatrixMockServer,
    client: matrix_sdk::Client,
    command: Command,
    room_id: &str,
) {
    // The snapshot reads each room's encryption state.
    mock_server
        .mock_room_state_encryption()
        .plain()
        .mount()
        .await;
    let (event_tx, event_rx) = mpsc::channel::<Event>();
    let mut core = Core::for_client(client, event_tx.clone());
    run(&mut core, command).await;

    let events = drain_events(&event_rx);
    match events.as_slice() {
        [Event::SyncedRooms(rooms), Event::RoomJoined(joined)] => {
            assert!(
                rooms.iter().any(|room| room.room_id == room_id),
                "the snapshot must list the joined room: {rooms:?}"
            );
            assert_eq!(
                joined,
                &RoomJoined {
                    room_id: room_id.to_owned()
                }
            );
        }
        other => panic!("expected SyncedRooms then RoomJoined, got {other:?}"),
    }
    mock_server.server().verify().await;
    core.stop().await;
}

#[test]
fn join_room_emits_snapshot_then_room_joined_with_the_alias_room_id() {
    run_local(async {
        let mock_server = MatrixMockServer::new().await;
        let client = mock_server.client_builder().build().await;
        mock_join(&mock_server, "%23ops:example.org", "!ops:example.org").await;

        assert_join_emits_snapshot_then_room_joined(
            &mock_server,
            client,
            Command::JoinRoom(JoinRoomRequest {
                room_or_alias: "#ops:example.org".to_owned(),
                via: Vec::new(),
            }),
            "!ops:example.org",
        )
        .await;
    });
}

#[test]
fn join_room_passes_via_servers() {
    run_local(async {
        let mock_server = MatrixMockServer::new().await;
        let client = mock_server.client_builder().build().await;
        wiremock::Mock::given(wiremock::matchers::method("POST"))
            .and(wiremock::matchers::path(
                "/_matrix/client/v3/join/!ops:other.example",
            ))
            .and(wiremock::matchers::query_param("via", "other.example"))
            .respond_with(
                wiremock::ResponseTemplate::new(200)
                    .set_body_json(serde_json::json!({ "room_id": "!ops:other.example" })),
            )
            .expect(1)
            .mount(mock_server.server())
            .await;

        assert_join_emits_snapshot_then_room_joined(
            &mock_server,
            client,
            Command::JoinRoom(JoinRoomRequest {
                room_or_alias: "!ops:other.example".to_owned(),
                via: vec!["other.example".to_owned()],
            }),
            "!ops:other.example",
        )
        .await;
    });
}

#[test]
fn join_room_rejects_an_invalid_via_server_without_a_request() {
    run_local(async {
        let mock_server = MatrixMockServer::new().await;
        let client = mock_server.client_builder().build().await;
        let (event_tx, event_rx) = mpsc::channel::<Event>();
        let mut core = Core::for_client(client, event_tx.clone());
        let command = Command::JoinRoom(JoinRoomRequest {
            room_or_alias: "!ops:other.example".to_owned(),
            via: vec!["not a server".to_owned()],
        });
        run(&mut core, command).await;

        match drain_events(&event_rx).as_slice() {
            [Event::Error(error)] => {
                assert_eq!(error.operation, Operation::JoinRoom);
                assert!(
                    error
                        .message
                        .contains("invalid Matrix server name not a server"),
                    "{}",
                    error.message
                );
            }
            other => panic!("expected one join error, got {other:?}"),
        }
        assert!(
            mock_server
                .received_requests()
                .await
                .expect("request recording enabled")
                .is_empty()
        );
        core.stop().await;
    });
}

#[test]
fn failed_join_room_emits_the_error_and_no_room_joined() {
    run_local(async {
        let mock_server = MatrixMockServer::new().await;
        let client = mock_server.client_builder().build().await;
        wiremock::Mock::given(wiremock::matchers::method("POST"))
            .and(wiremock::matchers::path(
                "/_matrix/client/v3/join/%23gone:example.org",
            ))
            .respond_with(wiremock::ResponseTemplate::new(404).set_body_json(
                serde_json::json!({ "errcode": "M_NOT_FOUND", "error": "No such room" }),
            ))
            .mount(mock_server.server())
            .await;
        let (event_tx, event_rx) = mpsc::channel::<Event>();
        let mut core = Core::for_client(client, event_tx.clone());
        let command = Command::JoinRoom(JoinRoomRequest {
            room_or_alias: "#gone:example.org".to_owned(),
            via: Vec::new(),
        });
        run(&mut core, command).await;

        match drain_events(&event_rx).as_slice() {
            [Event::Error(error)] => {
                assert_eq!(error.operation, Operation::JoinRoom);
                // A join may name an alias: it has no room yet.
                assert_eq!(error.room_id, None);
            }
            other => panic!("expected one join error, got {other:?}"),
        }
        core.stop().await;
    });
}

#[test]
fn kick_failure_error_carries_the_room() {
    run_local(async {
        let mock_server = MatrixMockServer::new().await;
        let client = mock_server.client_builder().build().await;
        mock_server.sync_joined_room(&client, &room_id()).await;
        wiremock::Mock::given(wiremock::matchers::method("POST"))
            .and(wiremock::matchers::path_regex(
                r"^/_matrix/client/v3/rooms/.*/kick$",
            ))
            .respond_with(wiremock::ResponseTemplate::new(403).set_body_json(
                serde_json::json!({ "errcode": "M_FORBIDDEN", "error": "not an op" }),
            ))
            .expect(1)
            .mount(mock_server.server())
            .await;
        let (event_tx, event_rx) = mpsc::channel::<Event>();
        let mut core = Core::for_client(client, event_tx.clone());
        let command = Command::KickRoomMember(KickRoomMemberRequest {
            room_id: room_id().to_string(),
            user_id: "@bob:example.org".to_owned(),
            reason: None,
        });
        run(&mut core, command).await;

        match drain_events(&event_rx).as_slice() {
            [Event::Error(error)] => {
                assert_eq!(error.operation, Operation::KickRoomMember);
                assert_eq!(error.room_id.as_deref(), Some("!room:example.org"));
                assert!(error.message.contains("not an op"), "{}", error.message);
            }
            other => panic!("expected one kick error, got {other:?}"),
        }
        core.stop().await;
    });
}

#[test]
fn create_direct_room_emits_snapshot_then_room_joined() {
    run_local(async {
        let mock_server = MatrixMockServer::new().await;
        let client = mock_server.client_builder().build().await;
        mock_server.mock_create_room().ok().expect(1).mount().await;

        assert_join_emits_snapshot_then_room_joined(
            &mock_server,
            client,
            Command::CreateDirectRoom(CreateDirectRoomRequest {
                user_id: "@friend:example.org".to_owned(),
            }),
            "!room:example.org",
        )
        .await;
    });
}

#[test]
fn create_room_emits_snapshot_then_room_joined() {
    run_local(async {
        let mock_server = MatrixMockServer::new().await;
        let client = mock_server.client_builder().build().await;
        mock_server.mock_create_room().ok().expect(1).mount().await;

        assert_join_emits_snapshot_then_room_joined(
            &mock_server,
            client,
            Command::CreateRoom(CreateRoomRequest {
                name: "Team room".to_owned(),
                visibility: CreateRoomVisibility::Private,
            }),
            "!room:example.org",
        )
        .await;
    });
}

#[test]
fn accept_room_invite_emits_snapshot_then_room_joined() {
    run_local(async {
        let mock_server = MatrixMockServer::new().await;
        let client = mock_server.client_builder().build().await;
        mock_server
            .sync_room(&client, InvitedRoomBuilder::new(room_id().as_ref()))
            .await;
        mock_server
            .mock_room_join(room_id().as_ref())
            .ok()
            .expect(1)
            .mount()
            .await;

        assert_join_emits_snapshot_then_room_joined(
            &mock_server,
            client,
            Command::AcceptRoomInvite(AcceptRoomInviteRequest {
                room_id: room_id().to_string(),
            }),
            room_id().as_str(),
        )
        .await;
    });
}

fn room_summary(room_id: &str, display_name: &str, membership: RoomMembership) -> RoomSummary {
    RoomSummary {
        room_id: room_id.to_owned(),
        display_name: display_name.to_owned(),
        topic: None,
        membership,
        encrypted: true,
        is_space: false,
        is_direct: false,
        direct_user_ids: Vec::new(),
        tags: Vec::new(),
        space_children: Vec::new(),
        notification_mode: match membership {
            RoomMembership::Join => Some(RoomNotificationMode::AllMessages),
            RoomMembership::Invite => None,
        },
        canonical_alias: None,
        alt_aliases: Vec::new(),
    }
}

fn room_read_state(
    room_id: &str,
    latest_active_event_id: &str,
    fully_read_event_id: &str,
) -> RoomReadState {
    RoomReadState {
        room_id: room_id.to_owned(),
        latest_active_event_id: Some(latest_active_event_id.to_owned()),
        fully_read_event_id: Some(fully_read_event_id.to_owned()),
        fully_read_timestamp_ms: None,
        marked_unread: true,
        num_unread: 2,
        num_notifications: 1,
        num_mentions: 0,
    }
}

fn room_members(room_id: &str, members: &[(&str, &str)]) -> RoomMembers {
    RoomMembers {
        room_id: room_id.to_owned(),
        members: members
            .iter()
            .map(|(user_id, nick)| RoomMemberSummary {
                user_id: (*user_id).to_owned(),
                nick: (*nick).to_owned(),
                nick_ambiguous: false,
                is_own_user: false,
                rank: RoomMemberRank::Member,
                can_kick: false,
                can_ban: false,
            })
            .collect(),
    }
}

#[test]
fn duplicate_start_sync_emits_typed_error_event() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("build tokio runtime");
    let local = tokio::task::LocalSet::new();

    local.block_on(&runtime, async {
        let client = matrix_sdk::Client::builder()
            .homeserver_url("http://localhost")
            .build()
            .await
            .expect("build matrix client");
        let (event_tx, event_rx) = mpsc::channel::<Event>();
        let mut core = Core::for_client(client, event_tx.clone());

        run(&mut core, Command::StartSync).await;
        // The first sync task has not run yet: it still syncs.
        run(&mut core, Command::StartSync).await;

        match drain_events(&event_rx).as_slice() {
            [Event::SyncStarted, Event::Error(err)] => {
                assert_eq!(err.operation, Operation::StartSync);
                assert_eq!(err.message, "continuous sync is already running");
            }
            other => panic!("expected duplicate sync error, got {other:?}"),
        }
        core.stop().await;
    });
}

#[test]
fn initial_room_members_and_read_states_follow_joined_synced_rooms() {
    let (event_tx, event_rx) = mpsc::channel::<Event>();
    let rooms = vec![
        room_summary("!ops:example.org", "Ops", RoomMembership::Join),
        room_summary("!eng:example.org", "Eng", RoomMembership::Join),
        room_summary("!invite:example.org", "Invite", RoomMembership::Invite),
    ];
    let room_members = vec![
        room_members(
            "!ops:example.org",
            &[("@alice:example.org", "Alice"), ("@bob:example.org", "bob")],
        ),
        room_members("!eng:example.org", &[("@carol:example.org", "Carol")]),
        room_members("!invite:example.org", &[("@dave:example.org", "Dave")]),
    ];
    let read_states = vec![
        room_read_state(
            "!ops:example.org",
            "$latest-ops:example.org",
            "$fully-read-ops:example.org",
        ),
        room_read_state(
            "!eng:example.org",
            "$latest-eng:example.org",
            "$fully-read-eng:example.org",
        ),
        room_read_state(
            "!invite:example.org",
            "$latest-invite:example.org",
            "$fully-read-invite:example.org",
        ),
    ];

    let snapshots = RoomSnapshots::default();
    RoomSnapshot {
        start: snapshots.start(),
        rooms: rooms.clone(),
        room_members: room_members.clone(),
        room_read_states: read_states.clone(),
        account_lists: AccountLists {
            highlight_words: Ok(vec![HighlightWord {
                word: "deploy".to_owned(),
                highlights: true,
            }]),
            ignored_users: Ok(vec!["@spam:example.org".to_owned()]),
        },
    }
    .emit(&snapshots, &event_tx as &EventSender);

    assert_eq!(
        event_rx
            .recv_timeout(Duration::from_secs(2))
            .expect("synced rooms event"),
        Event::SyncedRooms(rooms)
    );
    assert_eq!(
        event_rx
            .recv_timeout(Duration::from_secs(2))
            .expect("highlight words event"),
        Event::HighlightWords(vec![HighlightWord {
            word: "deploy".to_owned(),
            highlights: true,
        }])
    );
    assert_eq!(
        event_rx
            .recv_timeout(Duration::from_secs(2))
            .expect("ignored users event"),
        Event::IgnoredUsers(vec!["@spam:example.org".to_owned()])
    );
    for members in room_members.into_iter().take(2) {
        assert_eq!(
            event_rx
                .recv_timeout(Duration::from_secs(2))
                .expect("room members event"),
            Event::RoomMembers(members)
        );
    }
    for read_state in read_states.into_iter().take(2) {
        assert_eq!(
            event_rx
                .recv_timeout(Duration::from_secs(2))
                .expect("room read state event"),
            Event::RoomReadState(read_state)
        );
    }
    assert_eq!(
        event_rx.try_recv(),
        Err(TryRecvError::Empty),
        "initial snapshot must only emit synced rooms, highlight words, room members, and room read states"
    );
}

#[test]
fn shutdown_state_drops_every_task_inside_the_runtime() {
    /// Owned by a task like a `Client` clone: dropping it needs the
    /// runtime, as the SDK store's connection pool does.
    struct NeedsRuntimeOnDrop(std::rc::Rc<std::cell::Cell<bool>>);
    impl Drop for NeedsRuntimeOnDrop {
        fn drop(&mut self) {
            tokio::runtime::Handle::current();
            self.0.set(true);
        }
    }

    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("build tokio runtime");
    let local = tokio::task::LocalSet::new();
    let dropped = std::rc::Rc::new(std::cell::Cell::new(false));

    local.block_on(&runtime, async {
        let client = matrix_sdk::Client::builder()
            .homeserver_url("http://localhost")
            .build()
            .await
            .expect("build matrix client");
        let guard = NeedsRuntimeOnDrop(dropped.clone());
        let (event_tx, _event_rx) = mpsc::channel::<Event>();
        let mut core = Core::for_client(client, event_tx);
        let flow = crate::api::VerificationFlowId {
            user_id: "@alice:example.org".to_owned(),
            flow_id: "txn-123".to_owned(),
        };
        core.session().watch(
            &flow,
            Box::pin(async move {
                let _guard = guard;
                std::future::pending::<()>().await
            }),
        );

        core.stop().await;

        assert!(dropped.get(), "the task is gone when shutdown returns");
    });
    drop(local);
}

#[test]
fn worker_stops_when_command_channel_closes() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("build tokio runtime");
    let local = tokio::task::LocalSet::new();

    local.block_on(&runtime, async {
        let (command_tx, command_rx) = unbounded_channel();
        let (event_tx, event_rx) = mpsc::channel::<Event>();
        let config = CoreConfig {
            profile_dir: std::env::temp_dir().join("mirx-core-runtime-test"),
            store_passphrase: "test-store-passphrase".to_owned(),
        };

        drop(command_tx);
        Core::new(config, event_tx).run(command_rx).await;

        assert_eq!(
            event_rx
                .recv_timeout(Duration::from_secs(2))
                .expect("started event"),
            Event::Started
        );
        assert_eq!(
            event_rx
                .recv_timeout(Duration::from_secs(2))
                .expect("stopped event"),
            Event::Stopped
        );
    });
}

fn run_local(test: impl std::future::Future<Output = ()>) {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("build tokio runtime");
    tokio::task::LocalSet::new().block_on(&runtime, test);
}

async fn offline_client() -> matrix_sdk::Client {
    matrix_sdk::Client::builder()
        .homeserver_url("http://localhost")
        .build()
        .await
        .expect("build matrix client")
}

/// The answers emitted so far. These tests are about what commands
/// answer; the integration tests pin their acks, which this skips.
fn drain_events(event_rx: &mpsc::Receiver<Event>) -> Vec<Event> {
    event_rx
        .try_iter()
        .filter(|event| *event != Event::CommandHandled)
        .collect()
}

/// Waits for the next answer (not an ack) while letting the `LocalSet`'s
/// tasks run; a blocking `recv_timeout` would starve the sync task.
async fn next_event(event_rx: &mpsc::Receiver<Event>) -> Event {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    loop {
        match event_rx.try_recv() {
            Ok(Event::CommandHandled) => {}
            Ok(event) => return event,
            Err(TryRecvError::Empty) => {
                assert!(
                    tokio::time::Instant::now() < deadline,
                    "timed out waiting for a core event"
                );
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
            Err(TryRecvError::Disconnected) => panic!("event channel closed"),
        }
    }
}

fn assert_stop_sync_error(event_rx: &mpsc::Receiver<Event>, message: &str) {
    match drain_events(event_rx).as_slice() {
        [Event::Error(err)] => {
            assert_eq!(err.operation, Operation::StopSync);
            assert_eq!(err.message, message);
        }
        other => panic!("expected one stop sync error, got {other:?}"),
    }
}

#[test]
fn stop_sync_aborts_running_sync_and_emits_sync_stopped() {
    run_local(async {
        let (event_tx, event_rx) = mpsc::channel::<Event>();
        let mut core = Core::for_client(offline_client().await, event_tx.clone());
        run(&mut core, Command::StartSync).await;
        assert_eq!(drain_events(&event_rx), vec![Event::SyncStarted]);

        run(&mut core, Command::StopSync).await;

        assert_eq!(
            drain_events(&event_rx),
            vec![Event::SyncStopped(SyncStopReason::Requested)]
        );
        assert!(!core.session().sync_running());
        core.stop().await;
    });
}

#[test]
fn stop_sync_without_running_sync_emits_typed_error() {
    run_local(async {
        let (event_tx, event_rx) = mpsc::channel::<Event>();
        let client =
            matrix_sdk::test_utils::client::MockClientBuilder::new(Some(&closed_port_url()))
                .build()
                .await;
        let mut core = Core::for_client(client, event_tx.clone());

        run(&mut core, Command::StopSync).await;
        assert_stop_sync_error(&event_rx, "sync is not running");

        // A sync that ended by itself.
        run(&mut core, Command::StartSync).await;
        assert_eq!(next_event(&event_rx).await, Event::SyncStarted);
        assert!(matches!(
            next_event(&event_rx).await,
            Event::SyncStopped(SyncStopReason::Lost(_))
        ));

        run(&mut core, Command::StopSync).await;
        assert_stop_sync_error(&event_rx, "sync is not running");
        core.stop().await;
    });
}

#[test]
fn stop_sync_without_session_emits_invalid_state() {
    run_local(async {
        let (event_tx, event_rx) = mpsc::channel::<Event>();
        let mut core = Core::new(test_config(), event_tx);

        run(&mut core, Command::StopSync).await;

        assert_stop_sync_error(&event_rx, "stop sync requires an authenticated session");
        core.stop().await;
    });
}

#[test]
fn start_sync_after_stop_sync_restarts() {
    run_local(async {
        let (event_tx, event_rx) = mpsc::channel::<Event>();
        let mut core = Core::for_client(offline_client().await, event_tx.clone());

        run(&mut core, Command::StartSync).await;
        run(&mut core, Command::StopSync).await;
        run(&mut core, Command::StartSync).await;

        assert_eq!(
            drain_events(&event_rx),
            vec![
                Event::SyncStarted,
                Event::SyncStopped(SyncStopReason::Requested),
                Event::SyncStarted,
            ]
        );
        assert!(core.session().sync_running());
        core.stop().await;
    });
}

#[test]
fn sync_with_an_unknown_token_stops_with_session_ended() {
    run_local(async {
        let mock_server = MatrixMockServer::new().await;
        // No refresh token: the SDK would try a refresh first.
        let client = mock_server.client_builder().build().await;
        mock_server
            .mock_sync()
            .error_unknown_token(false)
            .mount()
            .await;
        let (event_tx, event_rx) = mpsc::channel::<Event>();
        let mut core = Core::for_client(client, event_tx.clone());

        run(&mut core, Command::StartSync).await;

        assert_eq!(next_event(&event_rx).await, Event::SyncStarted);
        match next_event(&event_rx).await {
            Event::SyncStopped(SyncStopReason::SessionEnded(message)) => {
                assert!(message.contains("M_UNKNOWN_TOKEN"), "{message}");
            }
            other => panic!("expected a session-ended sync stop, got {other:?}"),
        }
        assert_eq!(drain_events(&event_rx), Vec::new());
        core.stop().await;
    });
}

#[test]
fn sync_without_a_server_stops_with_lost_and_never_connects() {
    run_local(async {
        let client =
            matrix_sdk::test_utils::client::MockClientBuilder::new(Some(&closed_port_url()))
                .build()
                .await;
        let (event_tx, event_rx) = mpsc::channel::<Event>();
        let mut core = Core::for_client(client, event_tx.clone());

        run(&mut core, Command::StartSync).await;

        assert_eq!(next_event(&event_rx).await, Event::SyncStarted);
        match next_event(&event_rx).await {
            Event::SyncStopped(SyncStopReason::Lost(message)) => {
                assert!(message.contains("error sending request"), "{message}");
            }
            other => panic!("expected a lost sync stop, got {other:?}"),
        }
        assert_eq!(drain_events(&event_rx), Vec::new());
        core.stop().await;
    });
}

#[test]
fn sync_with_another_server_error_stops_with_lost() {
    run_local(async {
        let mock_server = MatrixMockServer::new().await;
        let client = mock_server.client_builder().build().await;
        // A 4xx is not retried; a 5xx would be, for minutes.
        mock_server
            .mock_sync()
            .respond_with(
                wiremock::ResponseTemplate::new(400).set_body_json(serde_json::json!({
                    "errcode": "M_UNKNOWN",
                    "error": "bad sync",
                })),
            )
            .mount()
            .await;
        let (event_tx, event_rx) = mpsc::channel::<Event>();
        let mut core = Core::for_client(client, event_tx.clone());

        run(&mut core, Command::StartSync).await;

        assert_eq!(next_event(&event_rx).await, Event::SyncStarted);
        match next_event(&event_rx).await {
            Event::SyncStopped(SyncStopReason::Lost(message)) => {
                assert!(message.contains("bad sync"), "{message}");
            }
            other => panic!("expected a lost sync stop, got {other:?}"),
        }
        assert_eq!(drain_events(&event_rx), Vec::new());
        core.stop().await;
    });
}

#[test]
fn sync_announces_the_first_response_once() {
    run_local(async {
        let mock_server = MatrixMockServer::new().await;
        let client = mock_server.client_builder().build().await;
        mock_server.mock_sync().ok(|_| {}).mount().await;
        let (event_tx, event_rx) = mpsc::channel::<Event>();
        let mut core = Core::for_client(client, event_tx.clone());

        run(&mut core, Command::StartSync).await;

        assert_eq!(next_event(&event_rx).await, Event::SyncStarted);
        assert_eq!(next_event(&event_rx).await, Event::SyncConnected);
        wait_for_sync_requests(&mock_server, 3).await;
        assert_eq!(drain_events(&event_rx), Vec::new());
        core.stop().await;
    });
}

#[test]
fn sync_requests_reuse_the_token_and_say_no_presence() {
    run_local(async {
        let mock_server = MatrixMockServer::new().await;
        let client = mock_server.client_builder().build().await;
        mock_server.mock_sync().ok(|_| {}).mount().await;
        let (event_tx, event_rx) = mpsc::channel::<Event>();
        let mut core = Core::for_client(client, event_tx.clone());

        run(&mut core, Command::StartSync).await;
        assert_eq!(next_event(&event_rx).await, Event::SyncStarted);
        wait_for_sync_requests(&mock_server, 3).await;

        let queries = sync_queries(&mock_server).await;
        // The first reuses no token (none is stored), every later one the
        // previous response's `next_batch`.
        assert_eq!(query_value(&queries[0], "since"), None);
        for query in &queries[1..] {
            assert!(query_value(query, "since").is_some(), "{query:?}");
        }
        // Online is the default, which goes without the parameter.
        for query in &queries {
            assert_eq!(query_value(query, "set_presence"), None, "{query:?}");
        }
        core.stop().await;
    });
}

const OWN_PRESENCE_PATH: &str = "/_matrix/client/v3/presence/@example:localhost/status";

/// Answers `PUT` of the own presence with `put_status`, and `GET` with
/// `{"presence": seen_as}`.
async fn mock_own_presence(mock_server: &MatrixMockServer, put_status: u16, seen_as: &str) {
    let put = wiremock::ResponseTemplate::new(put_status);
    let put = if put_status == 200 {
        put.set_body_json(serde_json::json!({}))
    } else {
        put.set_body_json(serde_json::json!({
            "errcode": "M_BAD_JSON",
            "error": "no presence for you",
        }))
    };
    wiremock::Mock::given(wiremock::matchers::method("PUT"))
        .and(wiremock::matchers::path(OWN_PRESENCE_PATH))
        .respond_with(put)
        .mount(mock_server.server())
        .await;
    wiremock::Mock::given(wiremock::matchers::method("GET"))
        .and(wiremock::matchers::path(OWN_PRESENCE_PATH))
        .respond_with(
            wiremock::ResponseTemplate::new(200)
                .set_body_json(serde_json::json!({ "presence": seen_as })),
        )
        .mount(mock_server.server())
        .await;
}

/// The bodies of every own presence `PUT` so far, in order.
async fn presence_puts(mock_server: &MatrixMockServer) -> Vec<serde_json::Value> {
    mock_server
        .server()
        .received_requests()
        .await
        .expect("the mock server records requests")
        .into_iter()
        .filter(|request| {
            request.method == wiremock::http::Method::PUT && request.url.path() == OWN_PRESENCE_PATH
        })
        .map(|request| serde_json::from_slice(&request.body).expect("a JSON presence body"))
        .collect()
}

#[test]
fn set_away_puts_presence_and_reports_what_the_server_sees() {
    run_local(async {
        let mock_server = MatrixMockServer::new().await;
        let client = mock_server.client_builder().build().await;
        mock_own_presence(&mock_server, 200, "unavailable").await;
        let (event_tx, event_rx) = mpsc::channel::<Event>();
        let mut core = Core::for_client(client, event_tx.clone());

        run(&mut core, Command::SetAway(Some("lunch".to_owned()))).await;
        assert_eq!(
            drain_events(&event_rx),
            vec![Event::AwayChanged(AwayChanged {
                message: Some("lunch".to_owned()),
                seen_as: PresenceKind::Unavailable,
            })]
        );

        run(&mut core, Command::SetAway(None)).await;
        assert_eq!(
            drain_events(&event_rx),
            vec![Event::AwayChanged(AwayChanged {
                message: None,
                seen_as: PresenceKind::Unavailable,
            })]
        );
        assert_eq!(
            presence_puts(&mock_server).await,
            vec![
                serde_json::json!({ "presence": "unavailable", "status_msg": "lunch" }),
                serde_json::json!({ "presence": "online" }),
            ]
        );
        core.stop().await;
    });
}

#[test]
fn a_server_without_presence_reports_you_offline() {
    run_local(async {
        let mock_server = MatrixMockServer::new().await;
        let client = mock_server.client_builder().build().await;
        // Synapse without presence: a silent 200, then offline for all.
        mock_own_presence(&mock_server, 200, "offline").await;
        let (event_tx, event_rx) = mpsc::channel::<Event>();
        let mut core = Core::for_client(client, event_tx.clone());

        run(&mut core, Command::SetAway(Some("lunch".to_owned()))).await;

        assert_eq!(
            drain_events(&event_rx),
            vec![Event::AwayChanged(AwayChanged {
                message: Some("lunch".to_owned()),
                seen_as: PresenceKind::Offline,
            })]
        );
        core.stop().await;
    });
}

#[test]
fn away_makes_the_sync_loop_say_unavailable() {
    run_local(async {
        let mock_server = MatrixMockServer::new().await;
        let client = mock_server.client_builder().build().await;
        mock_server.mock_sync().ok(|_| {}).mount().await;
        mock_own_presence(&mock_server, 200, "unavailable").await;
        let (event_tx, event_rx) = mpsc::channel::<Event>();
        let mut core = Core::for_client(client, event_tx.clone());
        let presence_of = |queries: &[Vec<(String, String)>]| -> Vec<Option<String>> {
            queries
                .iter()
                .map(|query| query_value(query, "set_presence").map(str::to_owned))
                .collect()
        };

        run(&mut core, Command::StartSync).await;
        wait_for_sync_requests(&mock_server, 2).await;
        let before_away = sync_queries(&mock_server).await.len();
        run(&mut core, Command::SetAway(Some("lunch".to_owned()))).await;
        let after_away = sync_queries(&mock_server).await.len();
        wait_for_sync_requests(&mock_server, after_away + 2).await;
        let queries = sync_queries(&mock_server).await;
        assert!(
            presence_of(&queries[..before_away])
                .iter()
                .all(Option::is_none),
            "{queries:?}"
        );
        // The sync under way while the server heard of it may say either.
        assert!(
            presence_of(&queries[after_away..])
                .iter()
                .all(|presence| presence.as_deref() == Some("unavailable")),
            "{queries:?}"
        );

        // Away outlasts the sync task: the session keeps it.
        run(&mut core, Command::StopSync).await;
        let before_restart = sync_queries(&mock_server).await.len();
        run(&mut core, Command::StartSync).await;
        wait_for_sync_requests(&mock_server, before_restart + 2).await;
        let queries = sync_queries(&mock_server).await;
        assert!(
            presence_of(&queries[before_restart..])
                .iter()
                .all(|presence| presence.as_deref() == Some("unavailable")),
            "{queries:?}"
        );

        run(&mut core, Command::SetAway(None)).await;
        let after_back = sync_queries(&mock_server).await.len();
        wait_for_sync_requests(&mock_server, after_back + 2).await;
        let queries = sync_queries(&mock_server).await;
        assert!(
            presence_of(&queries[after_back..])
                .iter()
                .all(Option::is_none),
            "{queries:?}"
        );
        drain_events(&event_rx);
        core.stop().await;
    });
}

#[test]
fn a_failed_away_leaves_the_syncs_present() {
    run_local(async {
        let mock_server = MatrixMockServer::new().await;
        let client = mock_server.client_builder().build().await;
        mock_server.mock_sync().ok(|_| {}).mount().await;
        // A 4xx: the SDK retries a 5xx for minutes.
        mock_own_presence(&mock_server, 400, "online").await;
        let (event_tx, event_rx) = mpsc::channel::<Event>();
        let mut core = Core::for_client(client, event_tx.clone());

        run(&mut core, Command::SetAway(Some("lunch".to_owned()))).await;
        match drain_events(&event_rx).as_slice() {
            [Event::Error(err)] => {
                assert_eq!(err.operation, Operation::SetPresence);
                assert!(err.message.contains("no presence for you"), "{err:?}");
            }
            other => panic!("expected a set presence error, got {other:?}"),
        }

        // Started after the failure: an away left in place would show.
        run(&mut core, Command::StartSync).await;
        wait_for_sync_requests(&mock_server, 2).await;
        for query in sync_queries(&mock_server).await {
            assert_eq!(query_value(&query, "set_presence"), None, "{query:?}");
        }
        core.stop().await;
    });
}

#[test]
fn set_typing_tells_the_room_and_stops_once() {
    run_local(async {
        let mock_server = MatrixMockServer::new().await;
        let client = mock_server.client_builder().build().await;
        mock_server
            .sync_room(&client, JoinedRoomBuilder::new(room_id().as_ref()))
            .await;
        let typing_path = format!(
            "/_matrix/client/v3/rooms/{}/typing/@example:localhost",
            room_id()
        );
        wiremock::Mock::given(wiremock::matchers::method("PUT"))
            .and(wiremock::matchers::path(typing_path.clone()))
            .respond_with(wiremock::ResponseTemplate::new(200).set_body_json(serde_json::json!({})))
            .mount(mock_server.server())
            .await;
        let (event_tx, event_rx) = mpsc::channel::<Event>();
        let mut core = Core::for_client(client, event_tx.clone());
        let typing = |typing| {
            Command::SetTyping(SetTypingRequest {
                room_id: room_id().to_string(),
                typing,
            })
        };

        // The SDK keeps them apart: a second `true` within 3 s and a
        // second stop send nothing.
        for command in [typing(true), typing(true), typing(false), typing(false)] {
            run(&mut core, command).await;
        }

        assert_eq!(drain_events(&event_rx), Vec::new());
        let bodies: Vec<serde_json::Value> = mock_server
            .server()
            .received_requests()
            .await
            .expect("the mock server records requests")
            .into_iter()
            .filter(|request| request.url.path() == typing_path)
            .map(|request| serde_json::from_slice(&request.body).expect("a JSON typing body"))
            .collect();
        assert_eq!(
            bodies,
            [
                serde_json::json!({ "typing": true, "timeout": 4000 }),
                serde_json::json!({ "typing": false }),
            ]
        );
        core.stop().await;
    });
}

#[test]
fn set_typing_in_an_unknown_room_is_the_rooms_error() {
    run_local(async {
        let (event_tx, event_rx) = mpsc::channel::<Event>();
        let mut core = Core::for_client(offline_client().await, event_tx.clone());

        run(
            &mut core,
            Command::SetTyping(SetTypingRequest {
                room_id: "!gone:example.org".to_owned(),
                typing: true,
            }),
        )
        .await;

        match drain_events(&event_rx).as_slice() {
            [Event::Error(err)] => {
                assert_eq!(err.operation, Operation::SetTyping);
                assert_eq!(err.room_id.as_deref(), Some("!gone:example.org"));
                assert_eq!(err.message, "joined room not found: !gone:example.org");
            }
            other => panic!("expected a typing error, got {other:?}"),
        }
        core.stop().await;
    });
}

/// The query of every `/sync` request so far, in order.
async fn sync_queries(mock_server: &MatrixMockServer) -> Vec<Vec<(String, String)>> {
    mock_server
        .server()
        .received_requests()
        .await
        .expect("the mock server records requests")
        .into_iter()
        .filter(|request| request.url.path().ends_with("/sync"))
        .map(|request| request.url.query_pairs().into_owned().collect())
        .collect()
}

fn query_value<'a>(query: &'a [(String, String)], key: &str) -> Option<&'a str> {
    query
        .iter()
        .find(|(name, _)| name == key)
        .map(|(_, value)| value.as_str())
}

/// A URL whose port refuses connections: the network is down.
fn closed_port_url() -> String {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind a local port");
    let address = listener
        .local_addr()
        .expect("a bound listener has an address");
    drop(listener);
    format!("http://{address}")
}

async fn wait_for_sync_requests(mock_server: &MatrixMockServer, count: usize) {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    loop {
        let syncs = mock_server
            .server()
            .received_requests()
            .await
            .expect("the mock server records requests")
            .into_iter()
            .filter(|request| request.url.path().ends_with("/sync"))
            .count();
        if syncs >= count {
            return;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "timed out waiting for {count} sync requests, saw {syncs}"
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

fn login_request(mock_server: &MatrixMockServer, start_sync: bool) -> LoginRequest {
    LoginRequest {
        server: mock_server.uri(),
        method: LoginMethod::Password {
            username: "example".to_owned(),
            password: "wordpass".to_owned(),
        },
        device_display_name: "mirx".to_owned(),
        start_sync,
    }
}

async fn mock_login_server() -> MatrixMockServer {
    let mock_server = MatrixMockServer::new().await;
    mock_server.mock_versions().ok().mount().await;
    mock_server.mock_login().ok().mount().await;
    mock_server.mock_sync().ok(|_| {}).mount().await;
    mock_server
}

/// Runs `handle_login` and returns the events it emitted up to its
/// return; the sync task, if any, has not run yet.
async fn login_events(
    mock_server: &MatrixMockServer,
    start_sync: bool,
) -> (tempfile::TempDir, Core, Vec<Event>) {
    let profile = tempfile::TempDir::new().expect("temporary profile directory");
    let config = CoreConfig {
        profile_dir: profile.path().join("profile"),
        store_passphrase: "test-store-passphrase".to_owned(),
    };
    let (event_tx, event_rx) = mpsc::channel::<Event>();
    let mut core = Core::new(config.clone(), event_tx.clone());

    run(
        &mut core,
        Command::Login(login_request(mock_server, start_sync)),
    )
    .await;

    (profile, core, drain_events(&event_rx))
}

fn event_position(events: &[Event], matches: impl Fn(&Event) -> bool) -> Option<usize> {
    events.iter().position(matches)
}

#[test]
fn login_with_start_sync_emits_logged_in_with_resolved_homeserver_then_sync_started() {
    run_local(async {
        let mock_server = mock_login_server().await;
        let (_profile, mut core, events) = login_events(&mock_server, true).await;

        // The SDK normalises the discovered URL with a trailing slash.
        let resolved = format!("{}/", mock_server.uri());
        let logged_in = event_position(&events, |event| {
            matches!(
                event,
                Event::LoggedIn { homeserver_url, .. } if *homeserver_url == resolved
            )
        })
        .unwrap_or_else(|| panic!("LoggedIn at {resolved} in {events:?}"));
        let synced_rooms = event_position(&events, |event| matches!(event, Event::SyncedRooms(_)))
            .unwrap_or_else(|| panic!("SyncedRooms in {events:?}"));
        assert_eq!(
            event_position(&events, |event| *event == Event::SyncStarted),
            Some(events.len() - 1),
            "{events:?}"
        );
        assert!(logged_in < synced_rooms);
        // No `m.push_rules` from the mock server: the server defaults
        // have no highlight words.
        assert_eq!(
            events.get(synced_rooms + 1),
            Some(&Event::HighlightWords(Vec::new())),
            "{events:?}"
        );
        assert!(core.session().sync_running());
        core.stop().await;
    });
}

/// The snapshot came with the initial sync: the sync task only
/// announces the connection.
#[test]
fn a_session_that_starts_online_gets_its_rooms_once() {
    run_local(async {
        let mock_server = mock_login_server().await;
        let profile = tempfile::TempDir::new().expect("temporary profile directory");
        let config = CoreConfig {
            profile_dir: profile.path().join("profile"),
            store_passphrase: "test-store-passphrase".to_owned(),
        };
        let (event_tx, event_rx) = mpsc::channel::<Event>();
        let mut core = Core::new(config.clone(), event_tx.clone());
        run(&mut core, Command::Login(login_request(&mock_server, true))).await;

        let events = settled_events(&event_rx).await;
        let snapshot_events = |events: &[Event]| {
            events
                .iter()
                .filter(|event| matches!(event, Event::HighlightWords(_)))
                .count()
        };
        assert_eq!(snapshot_events(&events), 1, "{events:?}");
        let sync_started = event_position(&events, |event| *event == Event::SyncStarted);
        let sync_connected = event_position(&events, |event| *event == Event::SyncConnected);
        assert!(
            sync_started.is_some() && sync_started < sync_connected,
            "{events:?}"
        );
        core.stop().await;
    });
}

#[test]
fn login_without_start_sync_leaves_sync_idle() {
    run_local(async {
        let mock_server = mock_login_server().await;
        let (_profile, mut core, events) = login_events(&mock_server, false).await;

        assert!(
            events
                .iter()
                .any(|event| matches!(event, Event::LoggedIn { .. })),
            "{events:?}"
        );
        assert!(!events.contains(&Event::SyncStarted), "{events:?}");
        assert!(!core.session().sync_running());
        core.stop().await;
    });
}

/// The server's answer when it no longer knows a token.
fn unknown_token(error: &str) -> wiremock::ResponseTemplate {
    wiremock::ResponseTemplate::new(401).set_body_json(serde_json::json!({
        "errcode": "M_UNKNOWN_TOKEN",
        "error": error,
        "soft_logout": false,
    }))
}

/// The events but the E2EE status, which the session's start reports
/// as it comes.
fn without_e2ee_status(events: Vec<Event>) -> Vec<Event> {
    events
        .into_iter()
        .filter(|event| !matches!(event, Event::E2eeStatus(_)))
        .collect()
}

async fn mock_logout(mock_server: &MatrixMockServer, answer: wiremock::ResponseTemplate) {
    wiremock::Mock::given(wiremock::matchers::method("POST"))
        .and(wiremock::matchers::path("/_matrix/client/v3/logout"))
        .respond_with(answer)
        .mount(mock_server.server())
        .await;
}

/// The server refuses the refresh token: it ended the session.
async fn mock_dead_refresh(mock_server: &MatrixMockServer) {
    wiremock::Mock::given(wiremock::matchers::method("POST"))
        .and(wiremock::matchers::path("/_matrix/client/v3/refresh"))
        .respond_with(unknown_token("Invalid refresh token"))
        .expect(1..)
        .mount(mock_server.server())
        .await;
}

/// A core signed in with a password, without sync; the server's login
/// answer has a refresh token if `refresh_token`.
async fn signed_in_core(
    mock_server: &MatrixMockServer,
    refresh_token: bool,
) -> (tempfile::TempDir, Core, mpsc::Receiver<Event>) {
    mock_server.mock_versions().ok().mount().await;
    let mut login = (*matrix_sdk_test::test_json::LOGIN).clone();
    if refresh_token {
        login["refresh_token"] = serde_json::json!("RT1");
    }
    mock_server
        .mock_login()
        .respond_with(wiremock::ResponseTemplate::new(200).set_body_json(login))
        .mount()
        .await;
    mock_server.mock_sync().ok(|_| {}).mount().await;
    let profile = tempfile::TempDir::new().expect("temporary profile directory");
    let config = CoreConfig {
        profile_dir: profile.path().join("profile"),
        store_passphrase: "test-store-passphrase".to_owned(),
    };
    let (event_tx, event_rx) = mpsc::channel::<Event>();
    let mut core = Core::new(config, event_tx);
    run(&mut core, Command::Login(login_request(mock_server, false))).await;
    let events = drain_events(&event_rx);
    assert!(
        events
            .iter()
            .any(|event| matches!(event, Event::LoggedIn { .. })),
        "{events:?}"
    );
    (profile, core, event_rx)
}

/// Logging out asks the server to end the session: one it already ended
/// is logged out.
#[test]
fn logging_out_a_session_the_server_ended_logs_out() {
    run_local(async {
        let mock_server = MatrixMockServer::new().await;
        let (_profile, mut core, event_rx) = signed_in_core(&mock_server, false).await;
        mock_logout(&mock_server, unknown_token("Invalid token")).await;

        run(&mut core, Command::Logout).await;

        assert_eq!(
            without_e2ee_status(drain_events(&event_rx)),
            vec![Event::LoggedOut]
        );
        core.stop().await;
    });
}

/// The SDK tries the refresh token on the logout's 401; the server refuses
/// it too.
#[test]
fn logging_out_with_a_dead_refresh_token_logs_out() {
    run_local(async {
        let mock_server = MatrixMockServer::new().await;
        let (_profile, mut core, event_rx) = signed_in_core(&mock_server, true).await;
        mock_logout(&mock_server, unknown_token("Invalid token")).await;
        mock_dead_refresh(&mock_server).await;

        run(&mut core, Command::Logout).await;

        assert_eq!(
            without_e2ee_status(drain_events(&event_rx)),
            vec![Event::LoggedOut]
        );
        core.stop().await;
    });
}

/// Signed out elsewhere, a session with a refresh token gets a 401 on its
/// sync, and a 401 on the refresh the SDK tries: the session ended, so no
/// reconnect could help.
#[test]
fn sync_with_a_dead_refresh_token_stops_with_session_ended() {
    run_local(async {
        let mock_server = MatrixMockServer::new().await;
        let (_profile, mut core, event_rx) = signed_in_core(&mock_server, true).await;
        // Ahead of the sign-in's mock.
        wiremock::Mock::given(wiremock::matchers::method("GET"))
            .and(wiremock::matchers::path("/_matrix/client/v3/sync"))
            .respond_with(unknown_token("Invalid token"))
            .with_priority(1)
            .mount(mock_server.server())
            .await;
        mock_dead_refresh(&mock_server).await;

        run(&mut core, Command::StartSync).await;

        let stopped = loop {
            match next_event(&event_rx).await {
                Event::E2eeStatus(_) | Event::SyncStarted => {}
                event => break event,
            }
        };
        match stopped {
            Event::SyncStopped(SyncStopReason::SessionEnded(message)) => {
                assert!(message.contains("Invalid refresh token"), "{message}");
            }
            other => panic!("expected a session-ended sync stop, got {other:?}"),
        }
        core.stop().await;
    });
}

/// The mock login's user and Bob, joined, as state or member events.
fn logged_in_user_room_members<T>() -> Vec<Raw<T>> {
    ["@cheeky_monkey:matrix.org", "@bob:example.org"]
        .into_iter()
        .enumerate()
        .map(|(index, user_id)| {
            serde_json::json!({
                "type": "m.room.member", "state_key": user_id, "sender": user_id,
                "event_id": format!("$member-{index}:example.org"),
                "origin_server_ts": 1u64, "content": { "membership": "join" }
            })
        })
        .map(|member| Raw::from_json_string(member.to_string()).expect("raw member event"))
        .collect()
}

fn bob_message(event_id: &str) -> Raw<matrix_sdk::ruma::events::AnySyncTimelineEvent> {
    Raw::from_json_string(
        serde_json::json!({
            "type": "m.room.message",
            "content": { "msgtype": "m.text", "body": "unread" },
            "event_id": event_id,
            "sender": "@bob:example.org",
            "origin_server_ts": 1700000000000u64
        })
        .to_string(),
    )
    .expect("raw timeline event")
}

/// The events emitted until now and in the next half second, while the
/// session's tasks (and the SDK's event cache) run.
async fn settled_events(event_rx: &mpsc::Receiver<Event>) -> Vec<Event> {
    tokio::time::sleep(Duration::from_millis(500)).await;
    drain_events(event_rx)
}

fn read_states_of<'a>(events: &'a [Event], room_id: &str) -> Vec<&'a RoomReadState> {
    events
        .iter()
        .filter_map(|event| match event {
            Event::RoomReadState(read_state) if read_state.room_id == room_id => Some(read_state),
            _ => None,
        })
        .collect()
}

/// mirx started before the network was up: the first sync that gets
/// through brings the whole room snapshot, before `SyncConnected`, and
/// later reconnections do not repeat it.
#[test]
fn a_session_that_starts_offline_gets_its_rooms_at_the_first_sync_that_connects() {
    run_local(async {
        let mock_server = MatrixMockServer::new().await;
        mock_server.mock_versions().ok().mount().await;
        mock_server.mock_login().ok().mount().await;
        mock_server
            .mock_get_members()
            .expect_any_access_token()
            .ok(logged_in_user_room_members())
            .mount()
            .await;
        mock_server
            .mock_room_state_encryption()
            .expect_any_access_token()
            .plain()
            .mount()
            .await;
        let failing_sync = mock_server
            .mock_sync()
            .respond_with(
                wiremock::ResponseTemplate::new(400)
                    .set_body_json(serde_json::json!({ "errcode": "M_UNKNOWN", "error": "down" })),
            )
            .mount_as_scoped()
            .await;
        let profile = tempfile::TempDir::new().expect("temporary profile directory");
        let config = CoreConfig {
            profile_dir: profile.path().join("profile"),
            store_passphrase: "test-store-passphrase".to_owned(),
        };
        let (event_tx, event_rx) = mpsc::channel::<Event>();
        let mut core = Core::new(config.clone(), event_tx.clone());
        run(&mut core, Command::Login(login_request(&mock_server, true))).await;
        let events = settled_events(&event_rx).await;
        assert!(
            events.iter().any(|event| matches!(
                event,
                Event::Error(error) if error.operation == Operation::Login
            )),
            "{events:?}"
        );
        assert!(
            events
                .iter()
                .any(|event| matches!(event, Event::SyncStopped(SyncStopReason::Lost(_)))),
            "{events:?}"
        );
        assert!(
            !events.iter().any(|event| matches!(
                event,
                Event::SyncedRooms(_) | Event::HighlightWords(_) | Event::SyncConnected
            )),
            "{events:?}"
        );

        drop(failing_sync);
        let room_id = room_id();
        mock_server
            .mock_sync()
            .ok(|builder| {
                builder.add_joined_room(
                    JoinedRoomBuilder::new(room_id.as_ref())
                        .add_state_bulk(logged_in_user_room_members()),
                );
            })
            .mount()
            .await;
        run(&mut core, Command::StartSync).await;
        let events = settled_events(&event_rx).await;
        let position = |name: &str, matches: &dyn Fn(&Event) -> bool| {
            event_position(&events, matches).unwrap_or_else(|| panic!("{name} in {events:?}"))
        };
        let sync_started = position("SyncStarted", &|event| *event == Event::SyncStarted);
        let synced_rooms = position("SyncedRooms", &|event| {
            matches!(event, Event::SyncedRooms(rooms)
                if rooms.iter().any(|room| room.room_id == room_id.as_str()))
        });
        let highlight_words = position("HighlightWords", &|event| {
            matches!(event, Event::HighlightWords(_))
        });
        let ignored_users = position("IgnoredUsers", &|event| {
            matches!(event, Event::IgnoredUsers(_))
        });
        let room_members = position(
            "RoomMembers",
            &|event| matches!(event, Event::RoomMembers(members) if members.room_id == room_id.as_str()),
        );
        let sync_connected = position("SyncConnected", &|event| *event == Event::SyncConnected);
        assert!(
            sync_started < synced_rooms
                && synced_rooms < highlight_words
                && highlight_words < ignored_users
                && ignored_users < room_members
                && room_members < sync_connected,
            "{events:?}"
        );

        run(&mut core, Command::StopSync).await;
        run(&mut core, Command::StartSync).await;
        let events = settled_events(&event_rx).await;
        assert!(events.contains(&Event::SyncConnected), "{events:?}");
        assert!(
            !events
                .iter()
                .any(|event| matches!(event, Event::HighlightWords(_) | Event::IgnoredUsers(_))),
            "{events:?}"
        );
        core.stop().await;
    });
}

/// A signed-in session (no sync yet) in a joined room, whose sends
/// `send` answers.
async fn session_in_a_room(
    mock_server: &MatrixMockServer,
    config: &CoreConfig,
    event_tx: &EventSender,
) -> Core {
    mock_server.mock_versions().ok().mount().await;
    mock_server.mock_login().ok().mount().await;
    mock_server
        .mock_room_state_encryption()
        .expect_any_access_token()
        .plain()
        .mount()
        .await;
    mock_server
        .mock_get_members()
        .expect_any_access_token()
        .ok(logged_in_user_room_members())
        .mount()
        .await;
    mock_server
        .mock_sync()
        .ok(|builder| {
            builder.add_joined_room(
                JoinedRoomBuilder::new(room_id().as_ref())
                    .add_state_bulk(logged_in_user_room_members()),
            );
        })
        .mount()
        .await;
    let mut core = Core::new(config.clone(), event_tx.clone());
    run(&mut core, Command::Login(login_request(mock_server, false))).await;
    core
}

fn send_line(body: &str) -> Command {
    Command::SendMessage(SendMessageRequest {
        room_id: room_id().to_string(),
        presentation: RoomMessagePresentationKind::Text,
        body: body.to_owned(),
        styles: Vec::new(),
        thread: None,
        reply: None,
    })
}

/// Waits for the event `matches` picks, collecting every event until
/// then.
async fn events_until(
    event_rx: &mpsc::Receiver<Event>,
    matches: impl Fn(&Event) -> bool,
) -> Vec<Event> {
    let mut events = Vec::new();
    loop {
        let event = next_event(event_rx).await;
        let done = matches(&event);
        events.push(event);
        if done {
            return events;
        }
    }
}

fn message_update(event: &Event) -> Option<(&str, &RoomActivityUpdateKind)> {
    match event {
        Event::RoomActivityUpdate(RoomActivityUpdate {
            target: RoomActivityUpdateTarget::Transaction { transaction_id },
            kind,
            ..
        }) => Some((transaction_id, kind)),
        _ => None,
    }
}

fn local_echo_transaction(events: &[Event], body: &str) -> String {
    events
        .iter()
        .find_map(|event| match event {
            Event::RoomActivity(activity) => match &activity.kind {
                RoomActivityKind::Message(message) if message.body == body => {
                    message.transaction_id.clone()
                }
                _ => None,
            },
            _ => None,
        })
        .unwrap_or_else(|| panic!("the local echo of {body:?} in {events:?}"))
}

/// A line typed while the server is unreachable waits in the queue, is
/// no error, and goes by itself after the next sync response.
#[test]
fn a_send_the_network_stopped_goes_after_the_next_sync_response() {
    run_local(async {
        let mock_server = MatrixMockServer::new().await;
        let profile = tempfile::TempDir::new().expect("temporary profile directory");
        let config = CoreConfig {
            profile_dir: profile.path().join("profile"),
            store_passphrase: "test-store-passphrase".to_owned(),
        };
        let (event_tx, event_rx) = mpsc::channel::<Event>();
        let mut core = session_in_a_room(&mock_server, &config, &event_tx).await;
        // The session's own events, the E2EE status's included.
        settled_events(&event_rx).await;
        let unavailable = mock_server
            .mock_room_send()
            .respond_with(wiremock::ResponseTemplate::new(503))
            .mount_as_scoped()
            .await;

        run(&mut core, send_line("hello")).await;
        let events = events_until(&event_rx, |event| {
            matches!(
                message_update(event),
                Some((_, RoomActivityUpdateKind::MessageFailed { .. }))
            )
        })
        .await;
        let transaction_id = local_echo_transaction(&events, "hello");
        assert!(
            matches!(
                message_update(events.last().expect("the failure")),
                Some((id, RoomActivityUpdateKind::MessageFailed { recoverable: true, .. }))
                    if id == transaction_id
            ),
            "{events:?}"
        );
        assert!(
            !events.iter().any(|event| matches!(event, Event::Error(_))),
            "{events:?}"
        );

        drop(unavailable);
        mock_server
            .mock_room_send()
            .ok(OwnedEventId::try_from("$sent:example.org").expect("valid event id"))
            .mount()
            .await;
        // Nothing resumes the queue but a sync response.
        let events = settled_events(&event_rx).await;
        assert!(
            !events.iter().any(|event| message_update(event).is_some()),
            "{events:?}"
        );

        run(&mut core, Command::StartSync).await;
        let events = events_until(&event_rx, |event| message_update(event).is_some()).await;
        assert_eq!(
            message_update(events.last().expect("the send")),
            Some((
                transaction_id.as_str(),
                &RoomActivityUpdateKind::MessageSent {
                    event_id: "$sent:example.org".to_owned()
                }
            )),
            "{events:?}"
        );
        core.stop().await;
    });
}

/// A send the server refused is final, and does not hold up the room's
/// next line.
#[test]
fn a_refused_send_fails_and_the_next_line_goes() {
    run_local(async {
        let mock_server = MatrixMockServer::new().await;
        let profile = tempfile::TempDir::new().expect("temporary profile directory");
        let config = CoreConfig {
            profile_dir: profile.path().join("profile"),
            store_passphrase: "test-store-passphrase".to_owned(),
        };
        let (event_tx, event_rx) = mpsc::channel::<Event>();
        let mut core = session_in_a_room(&mock_server, &config, &event_tx).await;
        // The session's own events, the E2EE status's included.
        settled_events(&event_rx).await;
        let refused = mock_server
            .mock_room_send()
            .respond_with(wiremock::ResponseTemplate::new(403).set_body_json(
                serde_json::json!({ "errcode": "M_FORBIDDEN", "error": "not here" }),
            ))
            .mount_as_scoped()
            .await;

        run(&mut core, send_line("refused")).await;
        let events = events_until(&event_rx, |event| matches!(event, Event::Error(_))).await;
        let transaction_id = local_echo_transaction(&events, "refused");
        assert!(
            events.iter().any(|event| matches!(
                message_update(event),
                Some((id, RoomActivityUpdateKind::MessageFailed { recoverable: false, .. }))
                    if id == transaction_id
            )),
            "{events:?}"
        );
        assert!(
            matches!(
                events.last(),
                Some(Event::Error(error)) if error.operation == Operation::SendMessage
            ),
            "{events:?}"
        );

        drop(refused);
        mock_server
            .mock_room_send()
            .ok(OwnedEventId::try_from("$next:example.org").expect("valid event id"))
            .mount()
            .await;
        run(&mut core, send_line("next")).await;
        let events = events_until(&event_rx, |event| {
            matches!(
                message_update(event),
                Some((_, RoomActivityUpdateKind::MessageSent { .. }))
            )
        })
        .await;
        let next = local_echo_transaction(&events, "next");
        assert_eq!(
            message_update(events.last().expect("the send")),
            Some((
                next.as_str(),
                &RoomActivityUpdateKind::MessageSent {
                    event_id: "$next:example.org".to_owned()
                }
            )),
            "{events:?}"
        );
        core.stop().await;
    });
}

/// The SDK's event cache counts unread messages, those of the session's
/// first sync included, and keeps the counts in its store
/// (`RoomInfo.read_receipts`): a restored session reports them before
/// any new event arrives, so the treebar and switchbar colours come back
/// after a restart.
#[test]
fn restore_reports_stored_unread_counts_without_new_events() {
    run_local(async {
        let mock_server = MatrixMockServer::new().await;
        mock_server.mock_versions().ok().mount().await;
        mock_server.mock_login().ok().mount().await;
        mock_server
            .mock_room_state_encryption()
            .expect_any_access_token()
            .plain()
            .mount()
            .await;
        mock_server
            .mock_get_members()
            .expect_any_access_token()
            .ok(logged_in_user_room_members())
            .mount()
            .await;
        let room_id = room_id();
        let first_sync = mock_server
            .mock_sync()
            .ok(|builder| {
                builder.add_joined_room(
                    JoinedRoomBuilder::new(room_id.as_ref())
                        .add_state_bulk(logged_in_user_room_members())
                        .add_timeline_event(bob_message("$one:example.org"))
                        .add_timeline_event(bob_message("$two:example.org")),
                );
            })
            .mount_as_scoped()
            .await;
        let profile = tempfile::TempDir::new().expect("temporary profile directory");
        let config = CoreConfig {
            profile_dir: profile.path().join("profile"),
            store_passphrase: "test-store-passphrase".to_owned(),
        };
        let (event_tx, event_rx) = mpsc::channel::<Event>();
        let mut core = Core::new(config.clone(), event_tx.clone());
        run(
            &mut core,
            Command::Login(login_request(&mock_server, false)),
        )
        .await;
        let events = settled_events(&event_rx).await;
        let stored = (*read_states_of(&events, room_id.as_str())
            .last()
            .unwrap_or_else(|| panic!("RoomReadState in {events:?}")))
        .clone();
        assert_eq!(
            (
                stored.num_unread,
                stored.num_notifications,
                stored.num_mentions
            ),
            (2, 2, 0),
            "{events:?}"
        );
        let (session, homeserver_url) = events
            .iter()
            .find_map(|event| match event {
                Event::LoggedIn {
                    session,
                    homeserver_url,
                } => Some((session.clone(), homeserver_url.clone())),
                _ => None,
            })
            .expect("LoggedIn");
        core.stop().await;
        drop(first_sync);
        let mut core = Core::new(config.clone(), event_tx.clone());

        mock_server
            .mock_sync()
            .ok(|builder| {
                builder.add_joined_room(JoinedRoomBuilder::new(room_id.as_ref()));
            })
            .mount()
            .await;
        run(
            &mut core,
            Command::RestoreSession(RestoreSessionRequest {
                homeserver_url,
                session,
                start_sync: false,
            }),
        )
        .await;
        let events = settled_events(&event_rx).await;
        let synced_rooms = event_position(&events, |event| matches!(event, Event::SyncedRooms(_)))
            .unwrap_or_else(|| panic!("SyncedRooms in {events:?}"));
        let restored = read_states_of(&events[synced_rooms..], room_id.as_str());
        assert!(!restored.is_empty(), "{events:?}");
        assert!(
            restored.iter().all(|read_state| **read_state == stored),
            "{events:?}"
        );
        core.stop().await;
    });
}
