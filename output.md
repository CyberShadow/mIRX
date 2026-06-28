# Execution Report

Output commit: `1e6ed7f Finish room activity fixture and automation migration`

## What changed

- Migrated checked-in fixture streams in:
  - `tests/ui-states/multi-room.json`
  - `tests/ui-states/space-hierarchy.json`
  - `tests/ui-states/verification-error.json`
- Added `tests/ui-states/mixed-room-activity.json` covering:
  - synced sent text
  - topic notice
  - join member notice
  - leave member notice
  - pending local echo
  - failed local echo via `RoomActivityUpdate::MessageFailed`
  - encrypted room metadata
- Updated `crates/mmatrix-ui/tests/automation_driver.rs` to use typed `RoomActivity` / `RoomActivityUpdate` helpers and typed testkit DTO assertions only.
- Added Wine automation coverage for:
  - failed outgoing state using typed failed send-state details
  - remote-echo reconciliation after `MessageSent`
  - `/clear` preserving only the pending row and later reconciling it in place
  - loading the new mixed fixture through the file API
- Updated `crates/mmatrix-ui/tests/mdi_child_reactivation.rs` to use typed sent-message activities.
- Updated `crates/mmatrix-ui/src/automation.rs` command tests to use typed event payloads only.
- Removed old core room DTOs/event variants from:
  - `crates/mmatrix-core/src/api.rs`
  - `crates/mmatrix-core/src/lib.rs`
- Removed remaining downstream legacy references and Win32 compatibility branches from:
  - `crates/mmatrix-ui/src/win32.rs`
  - `crates/mmatrix-core/tests/live_matrix.rs`
- Added a narrow append-only automation command through:
  - `crates/mmatrix-ui/src/automation.rs`
  - `crates/mmatrix-ui/src/win32.rs`
  - `crates/mmatrix-testkit/src/lib.rs`

## Deviation from plan

- The original unit plan assumed the automation surface could inject later synthetic updates into an existing session after `/clear`.
- The actual code only had replacement semantics through `LoadSyntheticState`.
- After escalation, I added a narrow append-only command reusing `LoadEventsArgs` / `LoadEventsResponse` so the `/clear` automation test could prove later in-session `MessageSent` reconciliation without changing replacement behavior.
- This required a protocol bump from `10` to `11`.

## Verification

Ran successfully:

```sh
TMPDIR=$PWD/.tmp-verification nix develop -c cargo fmt --check
TMPDIR=$PWD/.tmp-verification nix develop -c cargo test --workspace
TMPDIR=$PWD/.tmp-verification nix develop -c cargo test --target x86_64-pc-windows-gnu -p mmatrix-ui --test automation_driver
TMPDIR=$PWD/.tmp-verification nix develop -c cargo clippy --workspace --all-targets -- -D warnings
```

Notes:

- The first Windows-target automation-driver attempt failed before test execution with `wine-runner: Xvfb did not publish a display number`.
- I reran the same required command unchanged; the second run passed (`34 passed`).
- Wine emitted non-fatal `Fontconfig` noise and final `X connection ... broken` shutdown messages after the successful run.

## Old core DTO/event removal

- Removed:
  - `TimelineText`
  - `OutgoingTextQueued`
  - `OutgoingTextSent`
  - `OutgoingTextFailed`
  - `RoomStateEntry`
  - `RoomStateEventKind`
  - `RoomStateContent`
  - `RoomTopicState`
  - `RoomMemberState`
  - `Event::TimelineText`
  - `Event::OutgoingTextQueued`
  - `Event::OutgoingTextSent`
  - `Event::OutgoingTextFailed`
  - `Event::RoomState`
- Post-change search:

```sh
rg "TimelineText|OutgoingTextQueued|OutgoingTextSent|OutgoingTextFailed|RoomStateEntry|RoomStateEventKind|RoomStateContent|RoomTopicState|RoomMemberState|Event::RoomState|Event::TimelineText|Event::OutgoingText" crates tests
```

- Result: no hits.

## Blockers

- No remaining code blockers.

## Next unit

- Finalize snapshot script/baseline work deferred by plan:
  - update `scripts/ui-snapshot.sh` expectations for `mixed-room-activity.json`
  - add the matching PNG baseline under `artifacts/ui-snapshots/`
- Handle any documentation follow-up that should mention the now-complete room-activity contract and the append-only automation test hook if that surface is meant to be documented externally.
