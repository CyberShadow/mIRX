# mirx — Vision

> mIRC, but Matrix instead of IRC: mIRC's client experience, Matrix's concepts.

A Matrix chat client for Windows that looks, feels, and weighs like mIRC: a
dense, native, keyboard-driven UI rendered with the actual Win32 common
controls, a tiny resource footprint, and no protocol or encryption code written
from scratch.

## Goals

- **mIRC aesthetic & ergonomics.** Dense, information-rich, native-looking UI.
  A left-hand tree of servers/rooms, MDI room windows, a status/log view, a
  single-line input with history. Keyboard-first. No chrome, no animation, no
  web view.
- **Genuinely native.** Real Win32 common controls (`Comctl32`) — the same
  toolkit mIRC uses — not custom-drawn widgets and not a native-mimicking toolkit
  (no Qt/GTK/Electron/Flutter).
- **Low footprint.** Small binary, low idle RAM, fast cold start. A first-class,
  measured product requirement.
- **Reuse the Matrix ecosystem.** Protocol, sync, storage, and especially E2EE
  come from `matrix-rust-sdk`. We do not hand-roll crypto.
- **Well-behaved E2EE.** Encrypted rooms work out of the box, including the
  verification + recovery flows other Matrix users expect (see E2EE scope).

## Fidelity boundary: mIRC's client, not IRC's protocol

mirx borrows **mIRC the application** — its window model, density, menus,
dialogs, keyboard habits, options layout, and the ergonomics long-time users
know — and applies them to **Matrix concepts, named in Matrix terms**. It does
not import the IRC protocol, or conventions that exist only because of IRC,
into the user-facing surface. A user should never have to know IRC to
understand mirx, and nothing should suggest the program speaks IRC.

Concretely:

- **Name things as Matrix does.** Rooms, spaces, direct messages, members,
  power levels, invites, sessions — not channels, networks, modes, or
  user@host. No UI label, menu, dialog or option page called "IRC".
- **No IRC protocol vocabulary as a user interface.** No `/mode` flag letters
  (`+i`, `+m`, `+o`…) mapped onto room settings, no CTCP, no raw-protocol
  numerics or IRC server concepts. Room settings are changed through dialogs
  and commands that describe the Matrix setting itself.
- **Matrix formatting, not IRC formatting.** Message formatting is Matrix's
  (HTML / Markdown-style body); IRC control codes and the IRC colour palette are
  neither a wire format nor an intermediate representation.
- **Familiar mIRC commands are fine where they mean the same thing in Matrix**
  (`/join`, `/part`, `/msg`, `/query`, `/me`, `/topic`, `/whois`-style lookups,
  aliases, perform on connect), as long as their behaviour and output are honest
  about Matrix semantics.
- **When in doubt, ask "would this confuse someone who has never used IRC?"**
  If yes, prefer the Matrix-native framing while keeping mIRC's look and
  workflow.

## Non-goals (for now)

- Cross-platform. Windows-only by design; we commit fully to Win32. (We keep the
  Matrix/E2EE core cleanly separated from the UI so a port *could* happen later.)
- Voice/video, widgets, stickers, location, polls.
- Scripting engine (mIRC's famous scripting is out of scope for v1; revisit later).
- Bridging / multi-protocol. This is a Matrix client, not an aggregator.

## Tech stack (decided)

| Layer | Choice | Rationale |
|---|---|---|
| Language | **Rust** | The modern Matrix E2EE library is Rust. Same-language host removes the FFI boundary — the biggest integration risk in any other stack. |
| GUI | **Raw Win32 via `windows`/`windows-sys`**, NWG for scaffolding | Real `Comctl32` controls. NWG speeds up standard controls; drop to raw for dense mIRC-style owner-draw areas (tabs/switchbar, log/timeline panes). |
| Matrix | **`matrix-sdk` 0.18+** (matrix-rust-sdk) | Production-ready; backs Element X, Fractal, iamb. Sync, room state, storage. |
| E2EE | **`matrix-sdk-crypto` / vodozemac** via the SDK | Current Rust crypto stack; audited. Cross-signing, SAS verification, recovery, key backup. |
| Async | **Tokio**, isolated to a worker thread | SDK is async; the Win32 message loop is sync. Never share a thread. |
| Storage | SDK SQLite store (`bundled-sqlite`) | Per-profile account/crypto store, statically linked. |
| TLS | **rustls** (SDK default) | Pure-Rust; no OpenSSL DLL to ship. |
| License | **Apache-2.0** | Matches matrix-sdk / matrix-sdk-crypto / vodozemac (all Apache-2.0); compatible with the MIT / MIT-OR-Apache deps (windows-rs, NWG, tokio, rusqlite). |
| Min OS | **Windows 10+** (x64) | Lets Rust use its default, fully-supported `x86_64-pc-windows-msvc` target. |
| Packaging | **Single static `.exe`**, system DLLs only | rustls (no OpenSSL) + bundled SQLite + static CRT. CI gate that fails on any non-system DLL import. |

### Architecture sketch

```
┌────────────────────────────┐        channels         ┌──────────────────────────┐
│  UI thread (main)          │  ── commands ──────────▶ │  Matrix worker thread     │
│  • Win32 message loop      │                          │  • Tokio runtime          │
│  • Comctl32 controls (MDI) │ ◀── events (PostMessage) │  • matrix-sdk client      │
│  • render-only, no SDK     │                          │  • crypto store, sync     │
└────────────────────────────┘                          │  • SQLite storage         │
                                                        └──────────────────────────┘
```

- The UI thread **never** calls SDK objects directly — typed commands out, typed
  events in (posted to the message loop).
- All Matrix/crypto state lives behind an internal **app-service facade**, so SDK
  API churn is contained and the core stays UI-agnostic (and testable, and
  portable later).
- Matrix SDK timeline, state, and send-queue inputs first enter `mirx-core`,
  which normalizes them into typed room activity/state events before Win32 stores
  and renders them.
- Reactions, media, read-state, typing, threads, and verification slices extend
  that typed boundary rather than adding separate UI-only paths.

## UI model (mIRC-faithful)

mIRC is a true **MDI** application: a frame window hosts an `MDICLIENT`, and each
channel/query/status view is an MDI child window. A **treebar** (left-side
`SysTreeView32`) and an optional **switchbar** (toolbar/rebar of buttons) act as
controllers that activate/maximize the MDI children. Maximizing a child makes it
fill the client area — that's what produces the "tabbed full-window" feel without
real tabs.

mirx replicates this exactly:

- **Frame window** — menu, status bar, layout owner.
- **Treebar** (`SysTreeView32`) — hierarchy: **space → room → thread**. Rooms sort
  into their spaces; threads appear as children of their room node. Selecting a
  node activates the corresponding MDI child.
- **MDI client** (`MDICLIENT`) — each room / thread / status view is an MDI child;
  maintain a `{node_id → HWND}` map. Tree selection → `WM_MDIACTIVATE`
  (+`WM_MDIMAXIMIZE` in full-window mode).
- **Switchbar** (optional) — toolbar/rebar buttons mirroring open children, with
  mIRC-style highlight/flash on new activity.

## E2EE scope (v1)

The minimum to be a *well-behaved* encrypted client in 2026 (per the Matrix spec's
"recommended client behaviour", v1.18+) is more than just encrypt/decrypt. Omitting
these makes *other* users see unverified-device warnings, causes room keys to be
withheld, and produces "unable to decrypt" / lost history on relogin.

v1 baseline:

- Persistent SDK crypto store; Olm/Megolm enabled (default).
- **Cross-signing** auto-bootstrap (`auto_enable_cross_signing`).
- **Device verification**: SAS/emoji at minimum (QR can follow).
- **Secret storage / 4S**: set up + unlock via recovery key or passphrase.
- **Server-side key backup**: create/connect/upload/download
  (`auto_enable_backups`, `backup_download_strategy = AfterDecryptionFailure`).
- Clear error states: missing keys, unverified devices, identity reset,
  backup/recovery out of sync.

The SDK's high-level `Recovery` / `Backups` / `Verification` APIs implement the
protocol. **Our work is the UX**: display/save recovery key, passphrase entry,
recovery prompt on new login, verification request/accept/emoji-confirm/cancel,
and backup health indicators.

**Secret storage:** the SDK's encrypted store remains the base account/crypto
store. The Win32 shell persists its session payload via DPAPI and stores the
per-profile SDK SQLite passphrase separately, while password persistence stays
out of scope.

## Development environment (Linux host → Windows target)

Validated empirically (spike, task 25116) on this Linux host:

- **Cross-compile target: `x86_64-pc-windows-gnu`** via nix mingw
  (`pkgsCross.mingwW64.stdenv.cc` + `windows.pthreads`). Needs pthread lib/include
  passed through: `RUSTFLAGS=-L native=$PTHREADS/lib`,
  `CFLAGS_x86_64_pc_windows_gnu=-I$PTHREADS/include` (and CXXFLAGS). `matrix-sdk`
  requires `bundled-sqlite`.
- **Run/test under Wine 11 + Xvfb.** These are required local gates for HWND
  automation and screenshot diffs. Set `WINEDLLOVERRIDES='mscoree,mshtml='`
  (without it, Wine prefix init hangs). Real Windows remains authoritative for
  DPI, keyboard/input behavior, DPAPI, and installer behavior.
- **MSVC parity lane.** `x86_64-pc-windows-msvc` via `cargo-xwin` is the
  production-parity build but is currently network-blocked here (can't reach
  `aka.ms`); reserve it for a real-Windows CI lane.
- Encode the toolchain + flags in a **nix devshell / build script** so this isn't
  rediscovered per developer.

## Testing strategy (layered)

There is no "Playwright for Win32" that runs on Linux; WinAppDriver/UIA need real
Windows. Validated layered approach:

1. **Core (most coverage):** pure-Rust app-service facade, event handling, SDK
   integration, crypto/store flows — `cargo test`, ideally against a test
   homeserver.
2. **UI behavior:** an in-process Win32 message-driven harness under Wine/Xvfb —
   launch app, drive controls via `SendMessageW`/`WM_COMMAND`/`TVM_*`, assert via
   `WM_GETTEXT` and control-state queries. (Prototyped and working.)
3. **Visual smoke:** screenshot diffs under Wine/Xvfb, top-level rendering only.
4. **Real-Windows lane:** MSVC builds, WinAppDriver/UIA accessibility checks,
   installer, DPI/input edge cases, authoritative secret-storage behavior.

## Footprint targets (to confirm against a release build)

- Package / installed size: **< 50 MiB** (one EXE; SDK+TLS+SQLite+crypto dominate).
- Idle private working set after syncing ~10 rooms: **< 100 MiB**.
- Cold start to usable window: **< 500 ms**.

> Note: the spike's 549 MB matrix-sdk binary was an unoptimized **debug** build.
> Release (`opt-level`, LTO, strip, `panic=abort`) must be measured before these
> targets are accepted. Anti-examples: nheko (Qt) ~50–72 MiB; Element (Electron)
> 100–250+ MiB / 200+ MiB RAM.

## Current prototype capability

The current vertical slice already covers login, saved-session restore, room
sync, room listing, typed room activity/state rendering, send, and logout
through the `mirx-core` boundary, with deterministic offline fixtures plus
env-gated live automation coverage. The Win32 shell, room/thread activation,
read-state persistence, token refresh persistence, and session restore path are
already in place; remaining work is richer UX on top of those slices.

## v1 feature set

Dense mIRC-style room/status shell · space/room/thread tree · room timeline
(virtualized) · send/receive typed room messages (`m.text`, `m.notice`,
`m.emote`) · spaces · threads · reaction/redaction/media UX built on the typed
core boundary · **E2EE** (cross-signing, SAS verification, secret-storage
recovery, key backup) · secure credential storage · token refresh persistence.

## Risks

1. **Release footprint** — measure a release build early; the debug binary is huge.
2. **Async/UI boundary** — sync, media, verification must never block the message
   loop (strict thread separation from day one).
3. **Dense native UI** — owner-draw switchbar, virtualized rich-text timeline, DPI
   correctness are real Win32 work. Prototype the timeline pane first (hardest).
4. **SDK API churn** — pin versions; isolate behind the app-service facade.
5. **E2EE UX completeness** — recovery/verification/backup are product flows, not
   one-line SDK calls; scope deliberately for v1.
6. **Wine ≠ Windows** — Wine/Xvfb are useful required local gates, but a
   real-Windows lane stays authoritative for DPI, input, DPAPI, and installer.

## Resolved decisions

| Question | Decision |
|---|---|
| Spaces / threads | In scope; tree = space → room → thread. |
| License | Apache-2.0. |
| E2EE depth | Full well-behaved baseline (cross-signing + verification + recovery + backup). |
| mIRC fidelity | True Win32 MDI + treebar + switchbar. |
| Packaging | Single static `.exe`, system DLLs only; ZIP if ever needed. |
| Secret storage | SDK encrypted store plus DPAPI-protected session and store-passphrase persistence. |
| Min Windows | Windows 10+. |
| Name | `mirx`. |
| Dev loop | Cross-compile `windows-gnu` on Linux + Wine; MSVC on a real-Windows lane. |

## Immediate next step

1. **Keep shell activation synchronized end to end**: MDI children, treebar
   selection, and switchbar state should continue to move together for every
   room, thread, and status activation path.
2. **Keep shell fidelity visible in coverage**: inactive markers, compose focus,
   and constrained-layout snapshots should stay in the automated verification
   lane so regressions are obvious under Wine/Xvfb.
3. **Track real-Windows-only risk explicitly**: keep Wine/Xvfb as the required
   local gate, then use a real-Windows lane for DPI, input, DPAPI, installer,
   and other parity checks that Wine cannot authoritatively close out.
