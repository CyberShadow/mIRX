# Execution Report

## Scope

Ran the full nine-gate survey at `HEAD` in `/home/vladimir/work/.cydo/tasks/25113/worktree` on branch `verification-snapshots` using the repo-documented devshell invocation from `README.md`: `nix develop -c ...`.

No source, fixture, snapshot, test, or script files were modified. Temporary local log capture was removed before finish.

## Gate Results

1. `nix develop -c cargo fmt --check`
   - PASS
   - Relevant output: none
   - Tracked files modified: no

2. `nix develop -c cargo clippy --workspace --all-targets -- -D warnings`
   - PASS
   - Relevant output: `Finished 'dev' profile [unoptimized + debuginfo] target(s) in 1.90s`
   - Tracked files modified: no

3. `nix develop -c cargo test --workspace`
   - PASS
   - Relevant output: workspace tests all green; notable summaries included `mmatrix_core` 41 passed, `runtime` 33 passed, `api_contract` 35 passed, `mmatrix_ui` unit/non-Windows suites passed
   - Tracked files modified: no

4. `nix develop -c ./scripts/cross-build.sh`
   - PASS
   - Relevant output: `Finished 'release' profile [optimized] target(s) in 4.32s`
   - Tracked files modified: no

5. `nix develop -c ./scripts/wine-smoke.sh`
   - PASS
   - Relevant output: `mmatrix-ui self-test ok`
   - Notes: Wine emitted non-fatal `fixme` / `menubuilder` noise and final `X connection ... broken` shutdown text
   - Tracked files modified: no

6. `nix develop -c cargo test --target x86_64-pc-windows-gnu -p mmatrix-ui --test automation_driver`
   - FAIL
   - Exit status: `101`
   - First failing test: `automation_driver_empty_me_reports_usage_without_sending`
   - Assertion:

```text
assertion `left == right` failed
  left: Some(UiError { operation: "mark_room_read", message: "mark room read requires an authenticated session" })
 right: None
```

   - Full failing set:
     - `automation_driver_empty_me_reports_usage_without_sending`
     - `automation_driver_renders_space_hierarchy_and_preserves_duplicate_tree_selection`
     - `automation_driver_slash_clear_clears_only_active_room_timeline`
     - `automation_driver_slash_clear_in_status_view_is_a_no_op`
     - `automation_driver_slash_status_activates_status_view`
     - `automation_driver_slash_unknown_feedback_is_local`
     - `automation_driver_slash_window_reports_unknown_and_ambiguous_targets`
     - `automation_driver_slash_window_switches_by_room_id_and_display_name`
   - Additional distinct failure detail:

```text
automation_driver_renders_space_hierarchy_and_preserves_duplicate_tree_selection
left:  [mmatrix, rooms: 4, Work, Shared]
right: [mmatrix, rooms: 4, Social, Shared]
```

   - Summary: `test result: FAILED. 40 passed; 8 failed; 0 ignored; 0 measured; 0 filtered out`
   - Tracked files modified: no

7. `nix develop -c ./scripts/ui-snapshot.sh`
   - PASS
   - Relevant output: all snapshot cases passed, including generated checks for `image-message.png`, `mixed-room-activity.png`, and `space-hierarchy.png`
   - Tracked files modified: no

8. `nix develop -c ./scripts/dll-gate.sh`
   - PASS
   - Relevant output: `dll-gate ok: advapi32.dll api-ms-win-core-synch-l1-2-0.dll bcrypt.dll bcryptprimitives.dll comctl32.dll crypt32.dll gdi32.dll kernel32.dll msvcrt.dll ntdll.dll user32.dll ws2_32.dll`
   - Tracked files modified: no

9. `nix develop -c ./scripts/measure.sh`
   - PASS
   - Relevant output:

```text
exe: /home/vladimir/work/.cydo/tasks/25113/worktree/target/x86_64-pc-windows-gnu/release/mmatrix-ui.exe
size: 14667776 bytes (14MiB)
imports:
ADVAPI32.dll
KERNEL32.dll
api-ms-win-core-synch-l1-2-0.dll
bcrypt.dll
bcryptprimitives.dll
comctl32.dll
crypt32.dll
gdi32.dll
kernel32.dll
msvcrt.dll
ntdll.dll
user32.dll
ws2_32.dll
```

   - Tracked files modified: no

## Suspected Failures Check

- `image-message` UI snapshot expectation gap: not present
- Windows `automation_driver` timestamp expectation failure: not present
- `cargo fmt` drift: not present

## Complete Current Red Set

Only one gate is red at current `HEAD`:

- `cargo test --target x86_64-pc-windows-gnu -p mmatrix-ui --test automation_driver`

Observed failure pattern:

- Seven tests fail because an unexpected `mark_room_read requires an authenticated session` `UiError` is present where `None` is expected.
- One test fails because the rendered duplicate-selection navigation path is `Work > Shared` instead of expected `Social > Shared`.

## Final Worktree Status

```text
## verification-snapshots
 M output.md
```
