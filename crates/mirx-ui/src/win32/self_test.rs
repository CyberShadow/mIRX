//! `--self-test`: drive the real frame through automation and check it.

use super::*;

pub(super) unsafe fn run_self_test(hwnd: HWND) -> Result<(), String> {
    let edit = compose_edit(hwnd);
    let expected_text = "self-test message";
    let expected = wide_null(expected_text);
    SendMessageW(edit, WM_SETTEXT, 0, expected.as_ptr() as LPARAM);

    let actual = read_window_text(edit);
    if actual.is_empty() {
        return Err("self-test failed: WM_GETTEXTLENGTH returned no text".to_owned());
    }
    if actual != expected_text {
        return Err(format!(
            "self-test failed: expected edit text \"{expected_text}\", got \"{actual}\""
        ));
    }

    let compose_text = send_automation_request(
        hwnd,
        COMMAND_ID_DUMP_COMPOSE_TEXT,
        json!({"id":"compose-text","args":{}}),
    )?;
    if compose_text["ok"] != json!(true) || compose_text["json"] != json!({"text": expected_text}) {
        return Err(format!(
            "self-test failed: unexpected compose-text response {compose_text}"
        ));
    }

    let ping = send_automation_request(hwnd, COMMAND_ID_PING, json!({"id":"ping","args":{}}))?;
    if ping["ok"] != json!(true) || ping["json"] != json!({"pong": true}) {
        return Err(format!("self-test failed: unexpected ping response {ping}"));
    }

    let events = vec![
        Event::Started,
        Event::SyncedRooms(vec![
            RoomSummary {
                room_id: "!one:example".to_owned(),
                display_name: "One".to_owned(),
                topic: None,
                membership: RoomMembership::Join,
                encrypted: false,
                is_space: false,
                is_direct: false,
                direct_user_ids: Vec::new(),
                tags: Vec::new(),
                space_children: Vec::new(),
                notification_mode: Some(RoomNotificationMode::AllMessages),
                canonical_alias: None,
                alt_aliases: Vec::new(),
            },
            RoomSummary {
                room_id: "!two:example".to_owned(),
                display_name: "Two".to_owned(),
                topic: None,
                membership: RoomMembership::Join,
                encrypted: true,
                is_space: false,
                is_direct: false,
                direct_user_ids: Vec::new(),
                tags: Vec::new(),
                space_children: Vec::new(),
                notification_mode: Some(RoomNotificationMode::AllMessages),
                canonical_alias: None,
                alt_aliases: Vec::new(),
            },
        ]),
        Event::RoomActivity(Box::new(RoomActivity {
            room_id: "!one:example".to_owned(),
            activity_id: "event:$one-first".to_owned(),
            timestamp_ms: 1_700_000_000_000,
            highlight: false,
            relation: None,
            kind: RoomActivityKind::Message(RoomMessageActivity {
                event_id: Some("$one-first".to_owned()),
                transaction_id: None,
                sender: RoomUserProfile {
                    user_id: "@alice:example".to_owned(),
                    display_name: None,
                    display_name_ambiguous: false,
                },
                presentation: RoomMessagePresentationKind::Text,
                body: "first line".to_owned(),
                styles: Vec::new(),
                send_state: RoomMessageSendState::Sent,
            }),
        })),
        Event::RoomActivity(Box::new(RoomActivity {
            room_id: "!one:example".to_owned(),
            activity_id: "event:$one-second".to_owned(),
            timestamp_ms: 1_700_000_060_000,
            highlight: false,
            relation: None,
            kind: RoomActivityKind::Message(RoomMessageActivity {
                event_id: Some("$one-second".to_owned()),
                transaction_id: None,
                sender: RoomUserProfile {
                    user_id: "@alice:example".to_owned(),
                    display_name: None,
                    display_name_ambiguous: false,
                },
                presentation: RoomMessagePresentationKind::Text,
                body: "second line".to_owned(),
                styles: Vec::new(),
                send_state: RoomMessageSendState::Sent,
            }),
        })),
        Event::RoomActivity(Box::new(RoomActivity {
            room_id: "!two:example".to_owned(),
            activity_id: "event:$two-third".to_owned(),
            timestamp_ms: 1_700_000_120_000,
            highlight: false,
            relation: None,
            kind: RoomActivityKind::Message(RoomMessageActivity {
                event_id: Some("$two-third".to_owned()),
                transaction_id: None,
                sender: RoomUserProfile {
                    user_id: "@bob:example".to_owned(),
                    display_name: None,
                    display_name_ambiguous: false,
                },
                presentation: RoomMessagePresentationKind::Text,
                body: "third line".to_owned(),
                styles: Vec::new(),
                send_state: RoomMessageSendState::Sent,
            }),
        })),
    ];
    let load_response = send_automation_request(
        hwnd,
        COMMAND_ID_LOAD_SYNTHETIC_STATE,
        json!({"id":"load","args":{"events":events}}),
    )?;
    if load_response["ok"] != json!(true)
        || load_response["json"] != json!({"loaded": true, "rooms": 2, "timeline_events": 3})
    {
        return Err(format!(
            "self-test failed: unexpected synthetic-load response {load_response}"
        ));
    }

    let mut idle = false;
    for _ in 0..8 {
        let wait_idle = send_automation_request(
            hwnd,
            COMMAND_ID_WAIT_IDLE,
            json!({"id":"idle","args":{"timeout_ms":10}}),
        )?;
        if wait_idle["ok"] != json!(true) {
            return Err(format!(
                "self-test failed: wait-idle returned error {wait_idle}"
            ));
        }
        if wait_idle["json"]["idle"] == json!(true) {
            idle = true;
            break;
        }
    }
    if !idle {
        return Err("self-test failed: WAIT_IDLE never reported idle".to_owned());
    }

    let timeline = send_automation_request(
        hwnd,
        COMMAND_ID_DUMP_ROOM_ACTIVITY,
        json!({"id":"timeline","args":{"room_id":"!one:example"}}),
    )?;
    if timeline["ok"] != json!(true)
        || timeline["json"]
            != json!({
                "room_id":"!one:example",
                "entries":[
                    {
                        "room_id":"!one:example",
                        "activity_id":"event:$one-first",
                        "timestamp_ms":1700000000000u64,
                        "highlight":false,
                        "reactions":[],
                        "relation":null,
                        "kind":{
                            "type":"message",
                            "event_id":"$one-first",
                            "transaction_id":null,
                            "sender":"@alice:example",
                            "presentation":"text",
                            "body":"first line",
                            "send_state":{"state":"sent"}
                        }
                    },
                    {
                        "room_id":"!one:example",
                        "activity_id":"event:$one-second",
                        "timestamp_ms":1700000060000u64,
                        "highlight":false,
                        "reactions":[],
                        "relation":null,
                        "kind":{
                            "type":"message",
                            "event_id":"$one-second",
                            "transaction_id":null,
                            "sender":"@alice:example",
                            "presentation":"text",
                            "body":"second line",
                            "send_state":{"state":"sent"}
                        }
                    }
                ]
            })
    {
        return Err(format!(
            "self-test failed: unexpected dump-room-activity response {timeline}"
        ));
    }

    println!("{}", mirx_core::ui_self_test_line());
    Ok(())
}
