//! Decrypting late, and room key files, through the core's commands
//! against a mock homeserver. The core runs on its own thread, as in the
//! app. How the SDK decrypts late is covered with two clients in
//! `matrix::room_keys::tests`.

mod common;

use common::Harness;
use mirx_core::{Command, CoreErrorEvent, Event, Operation, RetryDecryptionRequest};

#[test]
fn retrying_decryption_is_acked_at_once_and_refuses_a_bad_room() {
    let harness = Harness::restored();
    // The session's key backup became available: the lines that could not
    // be decrypted are worth another try.
    harness.wait_for("DecryptionRetryWanted", |event| {
        *event == Event::DecryptionRetryWanted
    });

    harness.send(Command::RetryDecryption(RetryDecryptionRequest {
        room_id: "!room:example.org".to_owned(),
        session_ids: vec!["SESSION".to_owned()],
    }));
    let answer = harness.events_until("the retry's ack", |event| *event == Event::CommandHandled);
    assert!(
        !answer.iter().any(|event| matches!(event, Event::Error(_))),
        "{answer:#?}"
    );

    harness.send(Command::RetryDecryption(RetryDecryptionRequest {
        room_id: "room".to_owned(),
        session_ids: vec!["SESSION".to_owned()],
    }));
    let answer = harness.events_until("the retry's ack", |event| *event == Event::CommandHandled);
    let [.., Event::Error(error), Event::CommandHandled] = &answer[..] else {
        panic!("not an error: {answer:#?}");
    };
    assert_eq!(error.operation, Operation::RetryDecryption);
    assert_eq!(error.room_id.as_deref(), Some("room"));
    assert!(
        error.message.starts_with("invalid Matrix room ID room"),
        "{error:?}"
    );

    harness.shut_down();
}

#[test]
fn retrying_decryption_needs_a_session() {
    let harness = Harness::idle();

    harness.send(Command::RetryDecryption(RetryDecryptionRequest {
        room_id: "!room:example.org".to_owned(),
        session_ids: vec!["SESSION".to_owned()],
    }));
    let answer = harness.events_until("the retry's ack", |event| *event == Event::CommandHandled);
    assert_eq!(
        answer,
        [
            Event::Error(CoreErrorEvent {
                operation: Operation::RetryDecryption,
                room_id: Some("!room:example.org".to_owned()),
                message: "retrying decryption requires an authenticated session".to_owned(),
            }),
            Event::CommandHandled,
        ]
    );

    harness.shut_down();
}
