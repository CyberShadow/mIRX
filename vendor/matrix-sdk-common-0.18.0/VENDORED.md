# matrix-sdk-common 0.18.0, vendored

The crate as published on crates.io (`matrix-sdk-common-0.18.0.crate`,
sha256 `2b9d1e0fee0f090180ef9457034adc547e80de9e6e3eb2e63aaac68a8476f836`,
the checksum `Cargo.lock` had), used through `[patch.crates-io]` in the
workspace `Cargo.toml`. The commit after the one that added it is the only
change to it; `git log -- vendor/` lists them.

## Why

`CrossProcessLockGuard` derives `Clone`, but its `Drop` decrements the
lock's holder count, which only taking the lock increments. matrix-sdk's
event cache clones its store guard into a task for every write to the store
(`event_cache/persistence.rs`, `send_updates_to_store`), so the count
wraps below zero. The lease renewal task runs while the count is not zero,
so it runs forever, renewing the lease every 50 ms and owning a clone of the
lock, and with it the event cache store: its SQLite files stay open for the
life of the process, after the client is gone. mirx deletes a session's
store at Log Out, which Windows refuses while the files are open.

## The fix

A `Clone` for the guard that counts the new holder. Upstream (`main`, after
0.19.1) fixed it by counting the holders as the `Weak` references the guards
hold (`crates/matrix-sdk-common/src/cross_process_lock.rs`: "Every time
`CrossProcessLockGuard` is cloned, `Weak` is cloned, and thus the number of
holders of `CrossProcessLockInner` increases").

## When to remove

When upgrading to a matrix-sdk release that contains that fix: delete this
directory and its line in `[patch.crates-io]` (and, with the last vendored
crate, the section and the workspace's `exclude = ["vendor"]`).
