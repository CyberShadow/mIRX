# ruma-events 0.34.0, vendored

The crate as published on crates.io (`ruma-events-0.34.0.crate`, sha256
`85d2f90830fc131691349b96a69ff53444eb6c3e8dc7869c77961b43cfaf3344`, the
checksum `Cargo.lock` had; the version matrix-sdk 0.18 locks), used through
`[patch.crates-io]` in the workspace `Cargo.toml`. The commits after the one
that added it are the only changes to it; `git log -- vendor/` lists them.

## 1. One invalid entry fails a whole account data document

### Why

matrix-sdk-base decodes every account data event of a sync with these
types before it stores it, and drops one that does not decode
(`response_processors/account_data/{global,room}.rs`: `warn!` and
`continue`): the store keeps the previous version, or none. Long-lived
accounts carry documents written by years of clients, and one entry that
is not what the spec says lost the whole document, silently and for as long
as it stayed that way:

- `m.direct` (`{user: [room ID]}`): a user whose value is `null` or a
  string, or a list item that is no room ID (an alias, a number): no room
  was a direct room any more.
- `m.tag` (`{"tags": {name: {"order": number}}}`): a tag whose `order` is
  neither a number nor (with `compat-tag-info`) a numeric string, or whose
  info is no object: the room kept its old tags.
- `m.ignored_user_list` (`{"ignored_users": {user ID: {}}}`): a key that is
  no user ID, or a value that is no object: the ignore list stayed stale,
  for mirx and for the SDK.

No ruma `compat-*` feature covers these, and ruma's main branch
(2026-11-23) decodes them the same way. Patching the store instead would
leave the SDK's own readers (DM targets, notable tags, ignored users)
stale while mirx read the raw data; the type's `Deserialize` is the one
place every reader agrees on.

### The fix

- `DirectEventContent`: an entry whose value is not a list is ignored, and
  so is a list item that is not a room ID.
- `TagEventContent`: a tag whose info does not decode has empty info (the
  tag stays, without an order).
- `IgnoredUserListEventContent`: a key that is not a user ID is ignored;
  values are not read (the spec's is an empty object).

A content that is not an object (or `tags`/`ignored_users` that are not)
still fails, as before. Writing a document back (the SDK's `mark_as_dm`,
mirx's `/ignore`) drops the ignored entries: they name no user or room.
Tests: `direct::tests::invalid_entries_are_ignored`,
`tag::tests::invalid_tag_info_is_empty_info`,
`ignored_user_list::tests::invalid_entries_are_ignored`, and mirx-core's
`malformed_account_data_entries_are_ignored_not_their_document`. Run the
crate's tests with
`cargo test --manifest-path vendor/ruma-events-0.34.0/Cargo.toml --features compat-tag-info --lib`
(and a `CARGO_TARGET_DIR` of their own).

## When to remove

When ruma decodes these leniently itself (not reported upstream yet), or
when matrix-sdk stops dropping account data that does not decode. On a
matrix-sdk upgrade that moves to another ruma-events, port the patch or
delete this directory and its line in `[patch.crates-io]`.
