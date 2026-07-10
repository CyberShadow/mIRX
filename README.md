# mmatrix

`mmatrix` is a Windows-only native Win32 Matrix client in Rust. The current prototype includes the typed `mmatrix-core` facade, the Win32 UI shell, Nix-backed Linux-to-Windows GNU cross-build tooling, Wine smoke coverage, and a DLL import gate for the release artifact.

## Status

- Vertical-slice prototype scaffold is in place.
- `mmatrix-core` owns the Matrix SDK runtime boundary and has offline tests plus env-gated live Matrix coverage.
- Matrix SDK timeline, state, and send-queue inputs enter `mmatrix-core`, which normalizes them into typed room activity/state events that Win32 stores and renders.
- Reactions, media, read-state, typing, threads, and verification slices extend that typed boundary instead of adding separate UI-only paths.
- `mmatrix-ui` is a native Win32 shell with deterministic Wine automation, focused snapshot coverage, and a live login/session-restore/sync/rooms/send slice through `mmatrix-core`.
- CI is intended to run the full local check sequence through the Nix devshell.

## Local Commands

The devshell provisions local `.rustup` and `.cargo-home` caches, installs the stable Rust toolchain with `clippy` and `rustfmt`, enables the `x86_64-pc-windows-gnu` target, and exports the MinGW pthread include/lib settings validated by the spike.

```sh
nix develop -c cargo fmt --check
nix develop -c cargo clippy --workspace --all-targets -- -D warnings
nix develop -c cargo test --workspace
nix develop -c ./scripts/cross-build.sh
nix develop -c ./scripts/wine-smoke.sh
nix develop -c ./scripts/windows-ui-tests.sh
nix develop -c ./scripts/ui-snapshot.sh
nix develop -c ./scripts/dll-gate.sh
nix develop -c ./scripts/measure.sh
```

Normal checks remain offline and deterministic. They do not require network access or Matrix credentials.
Wine/Xvfb automation and screenshot diffs are required local gates for the Win32 shell, but real Windows remains authoritative for DPI, keyboard/input behavior, DPAPI, and installer validation.

## Live Matrix Checks

Live Matrix checks are opt-in and use:

- `MMATRIX_HS`
- `MMATRIX_USER`
- `MMATRIX_PASS`
- optional `MMATRIX_ROOM`

Missing or empty `MMATRIX_HS`, `MMATRIX_USER`, or `MMATRIX_PASS` makes the live Matrix tests return early without network access.

Supported live checks:

```sh
MMATRIX_HS=... MMATRIX_USER=... MMATRIX_PASS=... MMATRIX_ROOM=... \
  nix develop -c cargo test -p mmatrix-core --test live_matrix

MMATRIX_HS=... MMATRIX_USER=... MMATRIX_PASS=... \
  nix develop -c cargo test -p mmatrix-core --test live_matrix

MMATRIX_HS=... MMATRIX_USER=... MMATRIX_PASS=... MMATRIX_ROOM=... \
  nix develop -c cargo test --target x86_64-pc-windows-gnu -p mmatrix-ui --test automation_driver live_matrix_ui_session
```

`MMATRIX_ROOM` is optional for the existing login, restore, sync, and logout coverage. It is required for send coverage and for the fully-read marker verification in `live_matrix`. `MMATRIX_ROOM` must be a room the account has already joined; the live test creates the event-backed message itself before issuing `MarkRoomRead`.

The UI session file still lives at `%LOCALAPPDATA%\mmatrix\ui-shell\session.json`, but its payload is now DPAPI-protected binary data rather than cleartext JSON. The Matrix SDK SQLite store passphrase is generated per profile and persisted separately in `%LOCALAPPDATA%\mmatrix\ui-shell\store-passphrase.dpapi`, and refreshed Matrix access/refresh tokens are written back through the same protected session store. The password is never persisted.

Find is UI-scoped. Status and Thread Find search only loaded text, while Room Find searches loaded text first and can page older room history through backfill. `/find <text>`, `Edit > Find...`, and `Ctrl+F` all invoke the same behavior; Find does not use Matrix server-side search.

## Release Measurement

Measured artifact: `target/x86_64-pc-windows-gnu/release/mmatrix-ui.exe`

- Size: `13754880` bytes (`14MiB`)
- Target/runtime note: this is the Windows GNU release binary, with the current linked `matrix-sdk` E2EE stack and bundled SQLite stack included in the executable build.

DLL imports from `scripts/measure.sh`:

```text
ADVAPI32.dll
KERNEL32.dll
api-ms-win-core-synch-l1-2-0.dll
bcrypt.dll
bcryptprimitives.dll
comctl32.dll
crypt32.dll
kernel32.dll
msvcrt.dll
ntdll.dll
user32.dll
ws2_32.dll
```
