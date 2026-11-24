//! A server that takes its time, as a big account's homeserver does with
//! the first sync (Synapse computes it for minutes), and what a failed
//! request says about why it failed.

mod common;

use common::Harness;
use mirx_core::{Command, Event, SyncStopReason};

const ROOM: &str = "!a:example.org";

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
