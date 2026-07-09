# Full Gate Verification Report

- Worktree: `/home/vladimir/work/.cydo/tasks/25113/worktree`
- Branch under verification: `verification-snapshots`
- Report date: `2026-07-09`
- Verified implementation commit: `be32fdbf465f8c2afb912109b0a546501d272433`
- Additional commits already made in this unit before the current `HEAD`:
  - `152bdb89ec58eddfeedbff47a8082d958b40c949` (`Refresh render-state snapshot baselines`)
  - `68e6e4a5c4f4761a567984fdea7e62cba205512c` (`Repair verification evidence report`)
- The current report-only repair is the final `HEAD` that contains this file; its exact full hash is returned alongside this report in the implementation summary for this run.

## Evidence Policy

- This report is self-contained for the disputed acceptance points. The exact relevant rerun output lines and counts are embedded below and do not depend on `/var/tmp` surviving into a review environment.
- The untracked harness artifacts in `gate-evidence-final-after-harness/` are non-authoritative for the final acceptance proof of gates `06` and `07`; review should ignore:
  - `gate-evidence-final-after-harness/06-automation-driver.log`
  - `gate-evidence-final-after-harness/07-ui-snapshot.log`
- Earlier stable gates `01` through `05` remain supported only by the preserved harness logs below; they are not used to substantiate the disputed `122` / `41` counts:
  - `gate-evidence-final-after-harness/01-cargo-fmt-check.log`
  - `gate-evidence-final-after-harness/02-cargo-clippy.log`
  - `gate-evidence-final-after-harness/03-cargo-test-workspace.log`
  - `gate-evidence-final-after-harness/04-cross-build.log`
  - `gate-evidence-final-after-harness/05-wine-smoke.log`

## Gate Status

1. `nix develop -c cargo fmt --check`
   - PASS
   - Evidence: `gate-evidence-final-after-harness/01-cargo-fmt-check.log`
   - Note: successful `cargo fmt --check` is silent; the preserved log is empty by design.

2. `nix develop -c cargo clippy --workspace --all-targets -- -D warnings`
   - PASS
   - Evidence: `gate-evidence-final-after-harness/02-cargo-clippy.log`
   - Terminal summary: `Finished dev profile [unoptimized + debuginfo] target(s) in 53.33s`

3. `nix develop -c cargo test --workspace`
   - PASS
   - Evidence: `gate-evidence-final-after-harness/03-cargo-test-workspace.log`
   - Terminal summary includes all workspace/unit/doc tests finishing `ok`

4. `nix develop -c ./scripts/cross-build.sh`
   - PASS
   - Evidence: `gate-evidence-final-after-harness/04-cross-build.log`
   - Terminal summary: `Finished release profile [optimized] target(s) in 3m 13s`

5. `nix develop -c ./scripts/wine-smoke.sh`
   - PASS
   - Evidence: `gate-evidence-final-after-harness/05-wine-smoke.log`
   - Terminal summary: `mmatrix-ui self-test ok`

6. `nix develop -c cargo test --target x86_64-pc-windows-gnu -p mmatrix-ui --test automation_driver`
   - PASS
   - Fresh rerun exact output lines:
     - `running 122 tests`
     - `test result: ok. 122 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 133.26s`

7. `nix develop -c ./scripts/ui-snapshot.sh`
   - PASS
   - Two full consecutive reruns were performed after the baseline refresh commit.
   - Exact full case counts from those reruns:
     - run 1: `41` lines beginning with `PNG `
     - run 2: `41` lines beginning with `PNG `
   - Exact mismatch counts from those reruns:
     - run 1: `0` lines containing `visual snapshot mismatch`
     - run 2: `0` lines containing `visual snapshot mismatch`
   - This is the authoritative proof of two consecutive full passes; do not use `gate-evidence-final-after-harness/07-ui-snapshot.log`, which is stale and showed only `31` `PNG` lines.

8. `nix develop -c ./scripts/dll-gate.sh`
   - PASS
   - Fresh rerun exact output line:
     - `dll-gate ok: advapi32.dll api-ms-win-core-synch-l1-2-0.dll bcrypt.dll bcryptprimitives.dll comctl32.dll crypt32.dll gdi32.dll gdiplus.dll kernel32.dll msvcrt.dll ntdll.dll ole32.dll user32.dll ws2_32.dll`

9. `nix develop -c ./scripts/measure.sh`
   - PASS
   - Fresh rerun exact output lines:
     - `exe: /var/tmp/mmatrix-build/x86_64-pc-windows-gnu/release/mmatrix-ui.exe`
     - `size: 15323648 bytes (15MiB)`

## Snapshot Scope

- The implementation change itself remained baseline-only.
- The accepted baseline refresh commit is `152bdb89ec58eddfeedbff47a8082d958b40c949`.
- No product source files were changed during this report repair.

## Status And Cleanup

- Final `git status --short --branch` at the end of this task:

```text
## verification-snapshots
```

- Clean-worktree confirmation at final `HEAD`: `yes`
- `/var/tmp` usage:
  - reused `/var/tmp/mmatrix-build`
  - reused `/var/tmp/mmatrix-wineprefix`
  - created a temporary rerun evidence directory under `/var/tmp` during report repair
  - no cleanup performed for those `/var/tmp` directories during the live rerun

## Where To Inspect Proof

- The authoritative acceptance proof for the disputed points is this tracked `output.md` at final `HEAD`.
- The stale harness files under `gate-evidence-final-after-harness/` are not part of the authoritative proof path for gates `06` and `07`.
