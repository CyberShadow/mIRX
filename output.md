# Full Gate Verification Report

- Worktree: `/home/vladimir/work/.cydo/tasks/25113/worktree`
- Branch under verification: `verification-snapshots`
- Report date: `2026-07-09`
- Verified implementation commit: `be32fdbf465f8c2afb912109b0a546501d272433`
- Additional implementation commit made during this task: `152bdb89ec58eddfeedbff47a8082d958b40c949` (`Refresh render-state snapshot baselines`)

## Evidence Policy

- Authoritative fresh rerun evidence for the disputed final gates lives in `/var/tmp/mmatrix-evidence-rerun-20260709T000000Z`.
- Specifically:
  - `/var/tmp/mmatrix-evidence-rerun-20260709T000000Z/06-automation-driver.log`
  - `/var/tmp/mmatrix-evidence-rerun-20260709T000000Z/07-ui-snapshot-run1.log`
  - `/var/tmp/mmatrix-evidence-rerun-20260709T000000Z/08-ui-snapshot-run2.log`
  - `/var/tmp/mmatrix-evidence-rerun-20260709T000000Z/09-dll-gate.log`
  - `/var/tmp/mmatrix-evidence-rerun-20260709T000000Z/10-measure.log`
- The untracked harness artifacts in `gate-evidence-final-after-harness/` are not authoritative for the final acceptance proof of gates `06` and `07`; review should ignore:
  - `gate-evidence-final-after-harness/06-automation-driver.log`
  - `gate-evidence-final-after-harness/07-ui-snapshot.log`
- Earlier stable gates `01` through `05` were already preserved in the untracked harness evidence and were not rerun during this report repair:
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
   - Fresh authoritative evidence: `/var/tmp/mmatrix-evidence-rerun-20260709T000000Z/06-automation-driver.log`
   - Exact pass count:
     - `running 122 tests`
     - `test result: ok. 122 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 133.26s`

7. `nix develop -c ./scripts/ui-snapshot.sh`
   - PASS
   - Fresh authoritative run 1: `/var/tmp/mmatrix-evidence-rerun-20260709T000000Z/07-ui-snapshot-run1.log`
   - Fresh authoritative run 2: `/var/tmp/mmatrix-evidence-rerun-20260709T000000Z/08-ui-snapshot-run2.log`
   - Exact full case counts:
     - run 1: `41` `PNG` cases
     - run 2: `41` `PNG` cases
   - Proof of two consecutive full passes:
     - run 1 `visual snapshot mismatch` count: `0`
     - run 2 `visual snapshot mismatch` count: `0`

8. `nix develop -c ./scripts/dll-gate.sh`
   - PASS
   - Fresh authoritative evidence: `/var/tmp/mmatrix-evidence-rerun-20260709T000000Z/09-dll-gate.log`
   - Terminal summary:
     - `dll-gate ok: advapi32.dll api-ms-win-core-synch-l1-2-0.dll bcrypt.dll bcryptprimitives.dll comctl32.dll crypt32.dll gdi32.dll gdiplus.dll kernel32.dll msvcrt.dll ntdll.dll ole32.dll user32.dll ws2_32.dll`

9. `nix develop -c ./scripts/measure.sh`
   - PASS
   - Fresh authoritative evidence: `/var/tmp/mmatrix-evidence-rerun-20260709T000000Z/10-measure.log`
   - Terminal summary:
     - `exe: /var/tmp/mmatrix-build/x86_64-pc-windows-gnu/release/mmatrix-ui.exe`
     - `size: 15323648 bytes (15MiB)`

## Snapshot Scope

- The implementation change itself remained baseline-only.
- The accepted baseline refresh commit is `152bdb89ec58eddfeedbff47a8082d958b40c949`.
- No product source files were changed during this report repair.

## Status And Cleanup

- Status at the verified implementation point after commit `152bdb89ec58eddfeedbff47a8082d958b40c949`: clean on `verification-snapshots`
- Clean-worktree confirmation at that point: `yes`
- `/var/tmp` usage:
  - reused `/var/tmp/mmatrix-build`
  - reused `/var/tmp/mmatrix-wineprefix`
  - created fresh report-repair evidence directory `/var/tmp/mmatrix-evidence-rerun-20260709T000000Z`
  - no cleanup performed for those `/var/tmp` directories so the evidence remains inspectable
