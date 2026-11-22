# matrix-sdk 0.18.0, vendored

The crate as published on crates.io (`matrix-sdk-0.18.0.crate`, sha256
`7083d580527511ac5d9369e03b9f2b20902e76949f1b3964051f978e4d3756ae`, the
checksum `Cargo.lock` had), used through `[patch.crates-io]` in the
workspace `Cargo.toml`. The commits after the one that added it are the
only changes to it; `git log -- vendor/` lists them.

## 1. The latest events task leaks the client

### Why

matrix-sdk 0.18.0 (and 0.19.1) never drops a `Client` that synced with
the event cache subscribed, so its SQLite stores stay open for the life of
the process. `Client::latest_events` builds `LatestEvents` with
`SendQueue::new(client.clone())`, and `LatestEvents::new` moves that
`SendQueue`, which holds a strong `Client`, into `listen_to_updates_task`.
The task's abort-on-drop handle lives in the client itself: a reference
cycle. mirx deletes a session's store at Log Out, which Windows refuses
while the files are open.

### The fix

Upstream's, from `main` after 0.19.1: `LatestEvents::new` subscribes to
the send queue itself and passes only the `broadcast::Receiver` into the
task, so the `SendQueue` drops when `new` returns
(`crates/matrix-sdk/src/latest_events/mod.rs`, the comment that says
"which would lead to a `Client` leak").

## 2. A stale recovery state overwrites a fresh one

### Why

`Recovery::update_recovery_state` computes the state over several network
requests (secret storage, cross-signing, whether backups are marked
disabled) and then sets it, and several tasks run it at once: the key
query handler, the backup state listener, `recover_and_fix_backup` itself.
A computation that read "backups not enabled" before a recovery enabled
them could finish after the recovery's own update, leaving the state
`Incomplete` for good. mirx's live lane saw it after `/e2ee recover` on a
new device; `runtime::tests::recovering_during_a_key_query_ends_with_recovery_enabled`
reproduces it. Upstream `main` (as of 2025-09) has the same code.

### The fix

mirx's own: `EncryptionData::recovery_state_update`, a lock each update
holds from computing the state to setting it
(`src/encryption/mod.rs`, `src/encryption/recovery/mod.rs`).

## When to remove

When upgrading to a matrix-sdk release that contains both fixes (check the
second against upstream; it is not reported there yet): delete this
directory and its line in `[patch.crates-io]` (and, with the last vendored
crate, the section and the workspace's `exclude = ["vendor"]`).
