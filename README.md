# mirx

`mirx` is a Windows-only native Win32 Matrix client in Rust. The current prototype includes the typed `mirx-core` facade, the Win32 UI shell, Nix-backed Linux-to-Windows GNU cross-build tooling, Wine smoke coverage, and a DLL import gate for the release artifact.

## Status

- Vertical-slice prototype scaffold is in place.
- `mirx-core` owns the Matrix SDK runtime boundary and has offline tests plus env-gated live Matrix coverage.
- Matrix SDK timeline, state, and send-queue inputs enter `mirx-core`, which normalizes them into typed room activity/state events that Win32 stores and renders.
- Reactions, media, read-state, typing, threads, and verification slices extend that typed boundary instead of adding separate UI-only paths.
- `mirx-ui` is a native Win32 shell with deterministic Wine automation, focused snapshot coverage, and a live login/session-restore/sync/rooms/send slice through `mirx-core`.
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

- `MIRX_HS`
- `MIRX_USER`
- `MIRX_PASS`
- optional `MIRX_ROOM`

Missing or empty `MIRX_HS`, `MIRX_USER`, or `MIRX_PASS` makes the live Matrix tests return early without network access.

Supported live checks:

```sh
MIRX_HS=... MIRX_USER=... MIRX_PASS=... MIRX_ROOM=... \
  nix develop -c cargo test -p mirx-core --test live_matrix

MIRX_HS=... MIRX_USER=... MIRX_PASS=... \
  nix develop -c cargo test -p mirx-core --test live_matrix

MIRX_HS=... MIRX_USER=... MIRX_PASS=... MIRX_ROOM=... \
  nix develop -c cargo test --target x86_64-pc-windows-gnu -p mirx-ui --test automation_driver live_matrix_ui_session
```

`MIRX_ROOM` is optional for the existing login, restore, sync, and logout coverage. It is required for send coverage and for the fully-read marker verification in `live_matrix`. `MIRX_ROOM` must be a room the account has already joined; the live test creates the event-backed message itself before issuing `MarkRoomRead`.

The UI session file still lives at `%LOCALAPPDATA%\mirx\ui-shell\session.json`, but its payload is now DPAPI-protected binary data rather than cleartext JSON. The Matrix SDK SQLite store passphrase is generated per profile and persisted separately in `%LOCALAPPDATA%\mirx\ui-shell\store-passphrase.dpapi`, and refreshed Matrix access/refresh tokens are written back through the same protected session store. The password is never persisted.

The menubar follows mIRC: `File` (Connect, Disconnect, Select Server... (`Alt+E`), Log Out, Exit), `View` (Switchbar, Treebar), `Tools` (E2EE recovery and verification, Options... (`Alt+O`)), `Commands` (prompts for a slash command's argument and runs it), `Window` (Cascade, Tile, Arrange Icons) and `Help` (Help Topics, About mirx). Room, message and nick-list actions are on their right-click menus.

The switchbar has one fixed-width button per window, 120 pixels by default, and wraps them onto as many rows as the frame's width needs, like mIRC's switchbar with Lines set to Auto; the MDI area shrinks to fit. `/switchbar [on|off]` and `/treebar [on|off]`, or `View > Switchbar` and `View > Treebar`, toggle or set whether the switchbar and the tree of windows are shown; without an argument the View menu toggles and the command echoes the current state.

`Tools > Options...` (`Alt+O`) opens mIRC's Options window, a tree of pages: `Connect > Options` (connect on startup), `IRC > Messages` (timestamps on or off and their format, with the tokens listed) and `Display` (switchbar button width, 40 to 400 pixels). OK checks every field: a wrong one shows its page with an error box, and nothing changes until it is fixed; otherwise the settings are saved and take effect at once. Cancel or `Esc` changes nothing.

The Rooms List is mIRC's channels list for a server's room directory. `/list [-s server] [match text]`, `Commands > List rooms...` or `Alt+L` opens it. Without `-s` it lists the user's own homeserver, and the match text is matched by the server against names, topics and aliases. The rooms arrive page by page, up to the first 1000. The caption shows progress: `Rooms List [matrix.org] "rust" - listing...`, then `- 42 rooms`, or `- 1000 of ~5300 rooms` when the cap is hit. Click a column header (Room, Users, Name, Topic) to sort by it, and click it again to reverse; Users sorts the busiest rooms first. Double-click a row or press `Enter` on it to join the room, or to switch to its window if already joined. A new `/list` replaces the listing, and closing the window drops it.

Every join you ask for opens the room's window and makes it active, as `/join` does in mIRC: `/join`, a Rooms List row, `/query` creating a direct room, accepting an invite and `/create`. `/create <name>` creates a private, invite-only, encrypted room. `/create -public #alias [name]` creates a public room listed in the room directory under `#alias:<your server>`; the alias must be on your own server, and the name defaults to the alias.

Connecting works like mIRC's. With a saved session mirx restores it and starts syncing at startup, or, with connect on startup off, restores it and waits for `File > Connect` or `/server`; without one, Select Server opens. Select Server takes a server name such as `matrix.org` (the client URL is discovered through `.well-known`), a homeserver URL, or nothing when the user is a full ID like `@alice:matrix.org`, then logs in and starts syncing. `/server [server]` and `File > Connect` reconnect a disconnected session, restore the saved one, or open Select Server; `/quit` and `File > Disconnect` stop syncing and keep the session; `File > Log Out` ends the session.

Presentation settings live in `%LOCALAPPDATA%\mirx\ui-shell\mirx.ini`, next to the session file, and are saved as soon as they change:

```ini
[fonts]
text=Fixedsys,9
[timestamp]
show=on
format=[HH:nn]
[connect]
startup=on
[switchbar]
show=on
buttons=120
[treebar]
show=on
```

`/font [-b] <size> <font name>` (or `/font` and every window's system-menu `Font...` for the font dialog) sets the one text font used by every window's text area, nick list and editbox; mIRC's default is Fixedsys 9. `/timestamp [on|off|-f format]` shows, toggles or formats line timestamps, which are in local time; the format takes mIRC `$asctime` tokens (`HH H hh h nn n ss s tt TT dd d mm m yy yyyy`, everything else literal). A missing file means the defaults; a file mirx cannot parse stops startup with an error naming the line, rather than being overwritten. The Wine test scripts pin `TZ=UTC` so timestamps are deterministic.

Like mIRC, a character the text font has no glyph for draws in the first fallback font that has one, at the text font's height and on its baseline, in every text area and nick list: Segoe UI, Tahoma, Segoe UI Symbol, Segoe UI Emoji, Noto Emoji, then the Windows script fonts (Nirmala UI, Microsoft YaHei, Yu Gothic, Malgun Gothic and others), whichever are installed. Emoji are monochrome; sequences (ZWJ families, skin tones, flags) draw as their component glyphs, and invisible format characters such as variation selectors are not drawn. The Wine test prefixes get the monochrome Noto Emoji font from the devshell (`MIRX_WINE_FONTS`), so the harness chain is Fixedsys, Tahoma, Noto Emoji.

Replies and edits read like mIRC lines with a dim note. A reply has a grey quote line directly above it, `    > <Alice> are we deploying today? the build looks gr...`: the replied-to line without its timestamp, first line only, cut to 60 characters; a message that is not loaded is fetched from the server, and a redacted one reads `message redacted`. A thread message replying to its own thread's root has no quote. Plain-text reply fallbacks (`> <@alice:example.org> ...`) that older clients send are stripped. An edited message ends with ` (edited)`. The message right-click menu's `Reply to Message` and `Edit Message`, or `/reply <event-id>` and `/edit <event-id>` without text, arm the window's editbox: a navy line under the timeline says `* Replying to <Alice> ... (Esc cancels)`, and the next line typed there (plain, `//literal` or `/me`) is sent as the reply or the new text, while other commands leave it armed. Edit puts the message in the editbox as one would type it (`/me waves` for an emote) and is offered only for your own text and emotes. `Esc` disarms the editbox; for an edit it also clears the old text. `/reply <event-id> <text>` and `/edit <event-id> <text>` still send at once. A reply mentions (`m.mentions`) the replied-to sender and any member it names, so it notifies them. `Up` in the editbox stays mIRC's per-window history; it does not edit the last message.

Reactions show on a dim line under the message that names who reacted, as mIRC lists nicks: `    reactions: 👍 Alice, Bob  heart Carol, Dave, Erin +2`. Each reaction names its first three reactors by nick (a user who is not a member shows by user ID), then counts the rest; reactions keep the order they were first given in, and a long line wraps.

Every room window's window menu has `Send File...`, which opens the standard Open dialog (`Send File to <room>`) and uploads the chosen file to that room, as mIRC's DCC Send dialog does; `/sendfile` without a path opens the same dialog for the active room, and `/sendfile <path>` sends at once. Threads cannot take files yet. A file's type comes from its extension, so pictures, videos and audio arrive as such (`.png` as an image, `.mp4` as a video, `.mp3` as audio); anything else, including `.svg`, is sent as a plain file.

Find is UI-scoped. Status and Thread Find search only loaded text, while Room Find searches loaded text first and can page older room history through backfill. `/find <text>`, every window's system-menu `Find Text...`, and `Ctrl+F` all invoke the same behavior; Find does not use Matrix server-side search.

## Release Measurement

Measured artifact: `target/x86_64-pc-windows-gnu/release/mirx-ui.exe`

- Size: `16195584` bytes (`16MiB`)
- Target/runtime note: this is the Windows GNU release binary, with the current linked `matrix-sdk` E2EE stack and bundled SQLite stack included in the executable build.

DLL imports from `scripts/measure.sh`:

```text
ADVAPI32.dll
KERNEL32.dll
api-ms-win-core-synch-l1-2-0.dll
bcrypt.dll
bcryptprimitives.dll
comctl32.dll
comdlg32.dll
crypt32.dll
gdi32.dll
gdiplus.dll
kernel32.dll
msvcrt.dll
ntdll.dll
ole32.dll
user32.dll
ws2_32.dll
```
