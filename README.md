# mmatrix

`mmatrix` is a Windows-only native Win32 Matrix client in Rust. The current prototype includes the typed `mmatrix-core` facade, the Win32 UI shell, Nix-backed Linux-to-Windows GNU cross-build tooling, Wine smoke coverage, and a DLL import gate for the release artifact.

## Status

- Vertical-slice prototype scaffold is in place.
- `mmatrix-core` owns the Matrix SDK runtime boundary and has offline tests plus env-gated live Matrix coverage.
- `mmatrix-ui` is a native Win32 shell with deterministic Wine automation and a live login/session-restore/sync/rooms/send slice through `mmatrix-core`.
- CI is intended to run the full local check sequence through the Nix devshell.

## Local Commands

The devshell provisions local `.rustup` and `.cargo-home` caches, installs the stable Rust toolchain with `clippy` and `rustfmt`, enables the `x86_64-pc-windows-gnu` target, and exports the MinGW pthread include/lib settings validated by the spike.

```sh
nix develop -c cargo fmt --check
nix develop -c cargo clippy --workspace --all-targets -- -D warnings
nix develop -c cargo test --workspace
nix develop -c ./scripts/cross-build.sh
nix develop -c ./scripts/ui-snapshot.sh
nix develop -c ./scripts/wine-smoke.sh
nix develop -c ./scripts/dll-gate.sh
nix develop -c ./scripts/measure.sh
```

Normal checks remain offline and deterministic. They do not require network access or Matrix credentials.

## Live Matrix Checks

Live Matrix checks are opt-in and use:

- `MMATRIX_HS`
- `MMATRIX_USER`
- `MMATRIX_PASS`
- optional `MMATRIX_ROOM`

Supported live checks:

```sh
MMATRIX_HS=... MMATRIX_USER=... MMATRIX_PASS=... \
  nix develop -c cargo test -p mmatrix-core --test live_matrix

MMATRIX_HS=... MMATRIX_USER=... MMATRIX_PASS=... MMATRIX_ROOM=... \
  nix develop -c cargo test --target x86_64-pc-windows-gnu -p mmatrix-ui --test automation_driver live_matrix_ui_session
```

Omit `MMATRIX_ROOM` to verify login, restore, sync, and logout without send coverage.

The UI session file lives at `%LOCALAPPDATA%\mmatrix\ui-shell\session.json`. For this iteration it stores Matrix access and refresh tokens as local developer cleartext JSON, never the password. DPAPI/Credential Manager integration and token refresh persistence are future work.

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
