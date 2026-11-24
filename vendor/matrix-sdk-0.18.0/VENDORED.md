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

## 3. `max_retry_time` caps one delay, not the retrying

### Why

`RequestConfig::max_retry_time` is documented as "a time limit for how
long a request should be retried", the time-based variant of
`retry_limit`. Since the move to `backon` (0.11, #4916) it is passed to
`with_max_delay`, the cap of one backoff step, so it bounds nothing: a
request the server answers with a transient error (a 5xx but 520, or a
429) is retried for the fixed 15 minutes of total delay. `retry_limit`,
the only other bound, also turns on retries of network failures, which
hides a dropped connection for retries of 30 s timeouts. mirx bounds the
transient retrying of its client's requests to seconds, so an outage
behind a proxy (502, 503) fails a sync or a command, as an unreachable
server does. Upstream `main` (as of 2025-09) has the same code. Nothing
else in the SDK sets `max_retry_time` (only a test that reads it back),
so no other caller changes.

### The fix

mirx's own: `max_retry_time` sets the backoff's `with_total_delay`
(`src/http_client/native.rs`); the 60 s cap of one delay stays. A
server's `Retry-After` still sets the delay of each retry.

## 4. Accepting a direct invite drops another direct room

### Why

Joining a room the user was invited to as a direct room (`Room::join`,
`Client::join_room_by_id`) ends in `finish_join_room`, which marks it
direct with `Room::set_is_direct(true)`. That read the account's
`m.direct` from the store, added the room, and wrote the whole map back,
without the `mark_as_dm_lock` that `Account::mark_as_dm` (`create_dm`'s)
holds. The store only learns a write when a sync echoes it, so two joins
close together each wrote "store + its own room": the second dropped the
first, and the room stopped being direct for the account in every client.
mirx accepts invites in per-room lanes, side by side; and a `create_dm`
racing an accept lost whichever wrote first.
`mirx-core/tests/concurrency.rs`
(`accepting_two_direct_invites_at_once_keeps_both`,
`creating_a_direct_room_while_accepting_one_keeps_both`) reproduces both.
Upstream `main` (as of 2025-09) has the same code; not reported there yet.

### The fix

mirx's own, shaped as `mark_as_dm`: `set_is_direct` fetches `m.direct`
from the server (`fetch_account_data_static`) and holds
`mark_as_dm_lock` from that read to the write (`src/room/mod.rs`). The
members it may ask the server for are read before taking the lock.
Another client writing `m.direct` between the read and the write can
still be lost (account data has no compare-and-swap).

## 5. A sync's request timeout cannot be set

### Why

`Client::sync_once` bounds the `/sync` request to the client's request
timeout (30 s by default) plus the long poll's `timeout`, and nothing
else sets it: a first sync with the default 30 s long poll gives up after
60 s, and a `timeout=0` request after 30 s. A homeserver computes a big
account's first sync, or the catching up after a long absence, for
minutes (Synapse keeps computing after the client hangs up and caches the
answer). mirx's first run against a real account timed out its first
syncs twice ("error sending request for url (...)") before an attempt
got the cached answer. Raising the client's request timeout would bound
every other request by minutes too. Upstream `main` (as of 2025-09) has
the same code.

### The fix

mirx's own: `SyncSettings::request_timeout`, the bound of the whole
`/sync` request, which replaces "request timeout + long poll" when set
(`src/config/sync.rs`, `src/client/mod.rs` `sync_once`). Unset, nothing
changes.

## 6. A request refused with a token already refreshed refreshes again

### Why

`SendRequest::into_future` refreshes the access token on every
`M_UNKNOWN_TOKEN`. `refresh_token_lock` makes requests refused *while* a
refresh is under way wait for it, but a request sent with the old token
and refused *after* that refresh finished (a long-poll sync answers last)
starts another one: the tokens rotate again seconds after they were
issued, and every rotation is one more save the application must not
lose (Synapse's refresh tokens are single-use). Upstream `main` (as of
2025-09) has the same code.

### The fix

mirx's own: the request remembers the access token it was sent with;
refused, it retries with the client's token if a refresh has replaced
that one, and refreshes only otherwise (`src/client/futures.rs`).

## When to remove

When upgrading to a matrix-sdk release that contains all six fixes
(check the second to the sixth against upstream; they are not
reported there yet): delete this directory and its line in `[patch.crates-io]` (and,
with the last vendored crate, the section and the workspace's
`exclude = ["vendor"]`).
