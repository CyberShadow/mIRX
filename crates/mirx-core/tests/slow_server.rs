//! A server that takes its time, as a big account's homeserver does with
//! the first sync (Synapse computes it for minutes), and what a failed
//! request says about why it failed.

mod common;

use std::time::Duration;

use common::Harness;
use mirx_core::{Command, Event, SyncStopReason};
use serde_json::json;
use wiremock::matchers::{method, path_regex};
use wiremock::{Mock, ResponseTemplate};

const ROOM: &str = "!a:example.org";

/// The sync task's first request asks the server not to wait for news
/// (`timeout=0`), and the server still takes 35 s to answer: longer than
/// the SDK's 30 s request timeout, which bounded it, so the connection
/// was lost and the answer never read.
#[test]
fn a_sync_the_server_takes_long_to_answer_is_waited_for() {
    let harness = Harness::restored_in_rooms(&[ROOM], &[]);
    harness.mount(
        Mock::given(method("GET"))
            .and(path_regex(r"^/_matrix/client/v3/sync$"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(json!({ "next_batch": "slow" }))
                    .set_delay(Duration::from_secs(35)),
            )
            .up_to_n_times(1)
            .with_priority(1),
    );
    harness.send(Command::StartSync);

    let events = harness.events_for(Duration::from_secs(45));
    assert!(
        !events
            .iter()
            .any(|event| matches!(event, Event::SyncStopped(_))),
        "{events:#?}"
    );
    assert!(events.contains(&Event::SyncConnected), "{events:#?}");

    harness.shut_down();
}

/// A request that cannot reach the server says why: the SDK's error says
/// "error sending request for url (...)", its cause only in its source.
#[test]
fn a_lost_connection_says_why() {
    let harness = Harness::restored_in_rooms(&[ROOM], &[]);
    let (harness, _) = harness.restarted_offline();
    harness.send(Command::StartSync);

    let stopped = harness.wait_for("SyncStopped", |event| {
        matches!(event, Event::SyncStopped(_))
    });

    let Event::SyncStopped(SyncStopReason::Lost(reason)) = stopped else {
        panic!("{stopped:?}");
    };
    assert!(
        reason.starts_with("error sending request for url (http://127.0.0.1:"),
        "{reason}"
    );
    assert!(reason.contains("(os error "), "{reason}");

    harness.shut_down();
}
