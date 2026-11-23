//! Decrypting late, and room key files, through the core's commands
//! against a mock homeserver. The core runs on its own thread, as in the
//! app. How the SDK decrypts late is covered with two clients in
//! `matrix::room_keys::tests`.

mod common;

use common::Harness;
use std::fs;

use mirx_core::{
    Command, CommandId, CoreErrorEvent, Event, ExportRoomKeysRequest, ImportRoomKeysRequest,
    Operation, RetryDecryptionRequest,
};
use tempfile::TempDir;

#[test]
fn retrying_decryption_answers_and_refuses_a_bad_room() {
    let harness = Harness::restored_in_rooms(&["!room:example.org"], &[]);
    // The session's key backup became available: the lines that could not
    // be decrypted are worth another try.
    harness.wait_for("DecryptionRetryWanted", |event| {
        *event == Event::DecryptionRetryWanted
    });

    let retry = harness.send(Command::RetryDecryption(RetryDecryptionRequest {
        room_id: "!room:example.org".to_owned(),
        event_ids: vec!["$secret".to_owned()],
    }));
    // The event cache does not have the event: nothing to report.
    let answer = harness.answer(retry, "the retry's ack");
    assert!(
        !answer
            .iter()
            .any(|event| matches!(event, Event::Error(_) | Event::RoomEventsDecrypted(_))),
        "{answer:#?}"
    );

    let retry = harness.send(Command::RetryDecryption(RetryDecryptionRequest {
        room_id: "room".to_owned(),
        event_ids: vec!["$secret".to_owned()],
    }));
    let answer = harness.answer(retry, "the retry's ack");
    let [.., Event::Error(error)] = &answer[..] else {
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

    let retry = harness.send(Command::RetryDecryption(RetryDecryptionRequest {
        room_id: "!room:example.org".to_owned(),
        event_ids: vec!["$secret".to_owned()],
    }));
    assert_eq!(
        harness.answer(retry, "the retry's ack"),
        [Event::Error(CoreErrorEvent {
            operation: Operation::RetryDecryption,
            room_id: Some("!room:example.org".to_owned()),
            message: "retrying decryption requires an authenticated session".to_owned(),
        })]
    );

    harness.shut_down();
}

impl Harness {
    fn export(&self, path: &str, passphrase: &str) -> Vec<Event> {
        let export = self.send(Command::ExportRoomKeys(ExportRoomKeysRequest {
            path: path.to_owned(),
            passphrase: passphrase.to_owned(),
        }));
        self.answer(export, "the export's ack")
    }

    fn import(&self, path: &str, passphrase: &str) -> Vec<Event> {
        let import = self.send(Command::ImportRoomKeys(ImportRoomKeysRequest {
            path: path.to_owned(),
            passphrase: passphrase.to_owned(),
        }));
        self.answer(import, "the import's ack")
    }

    /// The command `id`'s events up to its ack (not included), without what
    /// the session's tasks report meanwhile (E2EE status, retries).
    fn answer(&self, id: CommandId, what: &str) -> Vec<Event> {
        let mut events = self.events_until(what, |event| *event == Event::CommandHandled(id));
        events.pop();
        events
            .into_iter()
            .filter(|event| !matches!(event, Event::E2eeStatus(_) | Event::DecryptionRetryWanted))
            .collect()
    }
}

fn import_error(message: &str) -> Event {
    Event::Error(CoreErrorEvent {
        operation: Operation::ImportRoomKeys,
        room_id: None,
        message: message.to_owned(),
    })
}

#[test]
fn exported_room_keys_are_a_key_file_that_imports_back() {
    let harness = Harness::restored();
    let dir = TempDir::new().expect("a temporary directory");
    let path = dir.path().join("mirx-keys.txt");
    let path = path.to_str().expect("a UTF-8 path");

    assert_eq!(
        harness.export(path, "correct horse"),
        [Event::RoomKeysExported {
            path: path.to_owned()
        }]
    );
    let file = fs::read_to_string(path).expect("the key file");
    assert!(
        file.starts_with("-----BEGIN MEGOLM SESSION DATA-----"),
        "{file}"
    );
    assert!(
        matrix_sdk_crypto::decrypt_room_key_export(file.as_bytes(), "correct horse").is_ok(),
        "the SDK reads it back"
    );

    // The session holds no room keys yet: none are new.
    assert_eq!(
        harness.import(path, "correct horse"),
        [Event::RoomKeysImported {
            path: path.to_owned(),
            imported: 0,
            total: 0,
        }]
    );

    harness.shut_down();
}

#[test]
fn a_room_key_import_names_what_is_wrong_with_the_file() {
    let harness = Harness::restored();
    let dir = TempDir::new().expect("a temporary directory");
    let keys = dir.path().join("keys.txt");
    let keys = keys.to_str().expect("a UTF-8 path");
    harness.export(keys, "correct horse");
    let notes = dir.path().join("notes.txt");
    fs::write(&notes, "not keys at all").expect("write a file");
    let notes = notes.to_str().expect("a UTF-8 path");
    let missing = dir.path().join("missing.txt");
    let missing = missing.to_str().expect("a UTF-8 path");

    assert_eq!(
        harness.import(keys, "wrong horse"),
        [import_error("wrong passphrase, or the file is damaged")]
    );
    assert_eq!(
        harness.import(notes, "correct horse"),
        [import_error("not a room key file")]
    );
    let answer = harness.import(missing, "correct horse");
    let [Event::Error(error)] = &answer[..] else {
        panic!("not an error: {answer:#?}");
    };
    assert_eq!(error.operation, Operation::ImportRoomKeys);
    assert_eq!(
        error.message,
        fs::File::open(missing)
            .expect_err("the file is missing")
            .to_string()
    );
    assert_eq!(
        harness.import(keys, ""),
        [import_error("a room key file needs a passphrase")]
    );
    assert_eq!(
        harness.export(keys, ""),
        [Event::Error(CoreErrorEvent {
            operation: Operation::ExportRoomKeys,
            room_id: None,
            message: "a room key file needs a passphrase".to_owned(),
        })]
    );

    harness.shut_down();
}

#[test]
fn room_key_files_need_a_session() {
    let harness = Harness::idle();

    for (command, operation, message) in [
        (
            Command::ExportRoomKeys(ExportRoomKeysRequest {
                path: "/keys.txt".to_owned(),
                passphrase: "secret".to_owned(),
            }),
            Operation::ExportRoomKeys,
            "exporting room keys requires an authenticated session",
        ),
        (
            Command::ImportRoomKeys(ImportRoomKeysRequest {
                path: "/keys.txt".to_owned(),
                passphrase: "secret".to_owned(),
            }),
            Operation::ImportRoomKeys,
            "importing room keys requires an authenticated session",
        ),
    ] {
        let id = harness.send(command);
        assert_eq!(
            harness.answer(id, "the ack"),
            [Event::Error(CoreErrorEvent {
                operation,
                room_id: None,
                message: message.to_owned(),
            })]
        );
    }

    harness.shut_down();
}
